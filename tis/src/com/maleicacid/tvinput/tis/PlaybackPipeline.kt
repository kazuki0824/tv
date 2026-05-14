package com.maleicacid.tvinput.tis

import android.content.AttributionSource
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.media.MediaCodec
import android.media.MediaFormat
import android.media.tv.tuner.Tuner
import android.media.tv.tuner.filter.AvSettings
import android.media.tv.tuner.filter.Filter
import android.media.tv.tuner.filter.FilterCallback
import android.media.tv.tuner.filter.FilterEvent
import android.media.tv.tuner.filter.MediaEvent
import android.media.tv.tuner.filter.PesEvent
import android.media.tv.tuner.filter.PesSettings
import android.media.tv.tuner.filter.TsFilterConfiguration
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.common.LogTags
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class PlaybackPipeline(
    private val inputId: String,
    private val sessionId: String,
    private val attributionSource: AttributionSource? = null,
) : AutoCloseable {
    @Volatile private var playbackExecutorThread: Thread? = null
    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-playback-$inputId").also { thread ->
            thread.isDaemon = true
            playbackExecutorThread = thread
        }
    }
    private val mainHandler = Handler(Looper.getMainLooper())
    private var surface: Surface? = null
    private var streamVolume: Float = 1.0f
    private var playbackGeneration: Long = 0L
    private var onVideoAvailable: () -> Unit = {}
    private var onVideoUnavailable: (PlaybackUnavailable) -> Unit = {}
    private var onVideoFormatDiscovered: (VideoFormatInfo) -> Unit = {}
    private var onSubtitlePes: (String, ByteArray, Long) -> Unit = { _, _, _ -> }
    private var videoFilter: Filter? = null
    private var audioFilter: Filter? = null
    private var subtitleFilter: Filter? = null
    private var nextAvFilterToken: Long = 1L
    private var currentVideoFilterToken: Long = -1L
    private var currentAudioFilterToken: Long = -1L
    private var currentSubtitleFilterToken: Long = -1L
    private var videoDecoder: VideoDecoderPipeline? = null
    private var audioDecoder: AudioDecoderPipeline? = null
    private val videoAvailableNotified = AtomicBoolean(false)
    private val syncClock = PlaybackSyncClock()
    private var audioWriteErrors: Int = 0
    private var audioPartialWrites: Int = 0
    private var oversizedSamplesDropped: Int = 0
    private var malformedSamplesDropped: Int = 0
    private var decoderBackpressureDrops: Int = 0
    private var audioPtsFallbackSamples: Int = 0
    private var videoPtsFallbackSamples: Int = 0
    private val released = AtomicBoolean(false)

    enum class PlaybackUnavailableReason {
        SURFACE_DETACHED, SURFACE_NOT_SET, VIDEO_FILTER_NOT_STARTED, AUDIO_FILTER_NOT_STARTED,
        VIDEO_OUTPUT_RENDER_FAILED, VIDEO_CODEC_ERROR, CODEC_CONFIG_TIMEOUT, FIRST_FRAME_TIMEOUT, UNSUPPORTED_VIDEO_STREAM,
        UNSUPPORTED_AUDIO_STREAM, AUDIO_UNAVAILABLE, CAS_NO_KEY, UNKNOWN,
    }

    data class PlaybackUnavailable(
        val reason: PlaybackUnavailableReason,
        val detail: String = "",
        val generation: Long = 0L,
    )

    data class StartResult(
        /** 現行世代の初回フレーム描画後だけ真になる。filter start だけでは映像開始扱いにしない。 */
        val startedVideo: Boolean,
        val startedAudio: Boolean,
        val diagnostics: List<String> = emptyList(),
        val firstFramePending: Boolean = false,
    )

    data class AudioSwitchResult(
        val switchedAudio: Boolean,
        val diagnostics: List<String> = emptyList(),
    )

    data class VideoFormatInfo(
        val streamType: Int,
        val mime: String,
        val width: Int,
        val height: Int,
    )

    private enum class VideoCodecKind(val streamType: Int, val mime: String) {
        MPEG2(0x02, MediaFormat.MIMETYPE_VIDEO_MPEG2),
        AVC(0x1b, MediaFormat.MIMETYPE_VIDEO_AVC);

        companion object {
            fun fromStreamType(streamType: Int): VideoCodecKind? = values().firstOrNull { it.streamType == streamType }
        }
    }

    private enum class AudioCodecKind(val streamType: Int, val mime: String) {
        MPEG1(0x03, MediaFormat.MIMETYPE_AUDIO_MPEG),
        MPEG2(0x04, MediaFormat.MIMETYPE_AUDIO_MPEG),
        AAC_ADTS(0x0f, MediaFormat.MIMETYPE_AUDIO_AAC);

        companion object {
            fun fromStreamType(streamType: Int): AudioCodecKind? = values().firstOrNull { it.streamType == streamType }
        }
    }

    private data class MediaSample(val bytes: ByteArray, val presentationTimeUs: Long, val timestampFallbackUsed: Boolean, val isAudio: Boolean)

    private fun <T> runOnPlaybackExecutorBlocking(action: () -> T): T {
        if (Thread.currentThread() == playbackExecutorThread) return action()
        val future = executor.submit(Callable<T> { action() })
        return try {
            future.get()
        } catch (e: InterruptedException) {
            Thread.currentThread().interrupt()
            throw RuntimeException("playback executor interrupted", e)
        } catch (e: ExecutionException) {
            val cause = e.cause ?: e
            when (cause) {
                is RuntimeException -> throw cause
                is Error -> throw cause
                else -> throw RuntimeException(cause)
            }
        }
    }

    private fun enqueuePlaybackAction(action: () -> Unit) {
        if (released.get()) return
        if (Thread.currentThread() == playbackExecutorThread) {
            action()
            return
        }
        runCatching {
            executor.execute {
                if (!released.get()) action()
            }
        }
    }

    fun setCallbacks(onAvailable: () -> Unit, onUnavailable: (PlaybackUnavailable) -> Unit) {
        runOnPlaybackExecutorBlocking {
            onVideoAvailable = onAvailable
            onVideoUnavailable = onUnavailable
        }
    }

    fun setOnVideoFormatDiscoveredCallback(callback: (VideoFormatInfo) -> Unit) {
        runOnPlaybackExecutorBlocking { onVideoFormatDiscovered = callback }
    }

    fun setOnSubtitlePesCallback(callback: (String, ByteArray, Long) -> Unit) {
        runOnPlaybackExecutorBlocking { onSubtitlePes = callback }
    }

    fun reportUnavailable(reason: PlaybackUnavailableReason, detail: String = "") {
        enqueuePlaybackAction { emitUnavailable(reason, detail) }
    }

    fun setVolume(volume: Float) {
        enqueuePlaybackAction { setVolumeOnPlaybackExecutor(volume) }
    }

    private fun setVolumeOnPlaybackExecutor(volume: Float) {
        streamVolume = volume.coerceIn(0.0f, 1.0f)
        audioDecoder?.setVolume(streamVolume)
    }

    fun setSurface(newSurface: Surface?) {
        enqueuePlaybackAction { setSurfaceOnPlaybackExecutor(newSurface) }
    }

    private fun setSurfaceOnPlaybackExecutor(newSurface: Surface?) {
        surface = newSurface
        if (newSurface == null) emitUnavailable(PlaybackUnavailableReason.SURFACE_DETACHED)
    }

    fun start(
        tuner: Tuner,
        channel: TunerController.ResolvedChannel,
        selection: TunerController.AvStreamSelection,
    ): StartResult = runOnPlaybackExecutorBlocking {
        startOnPlaybackExecutor(tuner, channel, selection)
    }

    private fun startOnPlaybackExecutor(
        tuner: Tuner,
        channel: TunerController.ResolvedChannel,
        selection: TunerController.AvStreamSelection,
    ): StartResult {
        stopOnPlaybackExecutor()
        val startGeneration = ++playbackGeneration
        val currentSurface = surface
        if (currentSurface == null || !currentSurface.isValid) {
            emitUnavailable(PlaybackUnavailableReason.SURFACE_NOT_SET, "有効な Surface がありません")
            return StartResult(false, false, listOf("surface未設定"))
        }
        val video = selection.video ?: run {
            emitUnavailable(PlaybackUnavailableReason.UNSUPPORTED_VIDEO_STREAM, "video PID が PMT から選択できません service=${selection.serviceKey}")
            return StartResult(false, false, listOf("video PID 未検出"))
        }
        val videoKind = VideoCodecKind.fromStreamType(video.streamType) ?: run {
            emitUnavailable(PlaybackUnavailableReason.UNSUPPORTED_VIDEO_STREAM, "未対応 video stream_type=0x${video.streamType.toString(16)}")
            return StartResult(false, false, listOf("video stream_type 未対応"))
        }
        val audio = selection.audio
        val audioKind = audio?.let { stream -> AudioCodecKind.fromStreamType(stream.streamType) }
        if (audio != null && audioKind == null) {
            Log.w(LogTags.TIS, "未対応 audio stream_type=0x${audio.streamType.toString(16)} のため video-only として開始します")
        }

        val diagnostics = mutableListOf<String>()
        val audioExpected = audio != null && audioKind != null
        syncClock.reset(if (audioExpected) PlaybackSyncClock.Mode.AUDIO_MASTER else PlaybackSyncClock.Mode.VIDEO_MASTER)
        val videoDecoderLocal = VideoDecoderPipeline(videoKind, currentSurface, syncClock, startGeneration, ::markFirstFrameRendered) { reason, detail -> emitUnavailable(reason, detail) }
        videoDecoder = videoDecoderLocal
        if (audioExpected) {
            audioDecoder = AudioDecoderPipeline(audioKind!!, streamVolume, syncClock) { reason, detail -> logAudioUnavailable(reason, detail) }
        }

        val openedVideo = createAndStartAvFilter(tuner, video, isAudio = false).getOrElse { error ->
            emitUnavailable(PlaybackUnavailableReason.VIDEO_FILTER_NOT_STARTED, error.message.orEmpty())
            diagnostics += "video filter start failed: ${error.message}"
            stopOnPlaybackExecutor()
            return StartResult(false, false, diagnostics)
        }
        videoFilter = openedVideo
        diagnostics += "videoPid=${video.elementaryPid}"
        diagnostics += "videoCodec=$videoKind"

        var audioStarted = false
        if (audio != null && audioKind != null) {
            val openedAudio = createAndStartAvFilter(tuner, audio, isAudio = true)
                .onFailure { error ->
                    logAudioUnavailable(PlaybackUnavailableReason.AUDIO_FILTER_NOT_STARTED, error.message.orEmpty())
                    diagnostics += "audio filter start failed; continuing video-only: ${error.message}"
                    syncClock.fallbackToVideoMaster("audio filter start failed")
                }
                .getOrNull()
            if (openedAudio != null) {
                audioFilter = openedAudio
                audioStarted = true
                diagnostics += "audioPid=${audio.elementaryPid}"
                diagnostics += "audioCodec=$audioKind"
            } else {
                audioDecoder?.close()
                audioDecoder = null
            }
        } else {
            diagnostics += "audio=absent-or-unsupported-video-only"
        }
        val subtitle = selection.subtitle
        if (subtitle != null) {
            val openedSubtitle = createAndStartSubtitlePesFilter(tuner, subtitle)
                .onFailure { error -> diagnostics += "subtitle PES filter start failed: ${error.message}" }
                .getOrNull()
            if (openedSubtitle != null) {
                subtitleFilter = openedSubtitle
                diagnostics += "subtitlePid=${subtitle.elementaryPid}"
            }
        }
        diagnostics += "service=${selection.serviceKey}"
        diagnostics += "channel=${channel.displayNumber}"
        diagnostics += "volume=$streamVolume"
        diagnostics += "surfaceAttached=${currentSurface.isValid}"
        scheduleFirstFrameTimeout(startGeneration)
        Log.i(LogTags.TIS, "AV filter と遅延 decoder を開始しました inputId=$inputId sessionId=$sessionId firstFramePending=true ${diagnostics.joinToString(" ")}")
        return StartResult(startedVideo = false, startedAudio = audioStarted, diagnostics = diagnostics, firstFramePending = true)
    }

    fun switchAudio(
        tuner: Tuner,
        selection: TunerController.AvStreamSelection,
    ): AudioSwitchResult = runOnPlaybackExecutorBlocking {
        switchAudioOnPlaybackExecutor(tuner, selection)
    }

    private fun switchAudioOnPlaybackExecutor(
        tuner: Tuner,
        selection: TunerController.AvStreamSelection,
    ): AudioSwitchResult {
        val audio = selection.audio ?: return AudioSwitchResult(false, listOf("audio PID 未検出"))
        val audioKind = AudioCodecKind.fromStreamType(audio.streamType) ?: run {
            emitUnavailable(PlaybackUnavailableReason.UNSUPPORTED_AUDIO_STREAM, "未対応 audio stream_type=0x${audio.streamType.toString(16)}")
            return AudioSwitchResult(false, listOf("audio stream_type 未対応"))
        }

        val previousFilter = audioFilter
        val previousDecoder = audioDecoder
        val previousAudioToken = currentAudioFilterToken
        val newDecoder = AudioDecoderPipeline(audioKind, streamVolume, syncClock) { reason, detail -> logAudioUnavailable(reason, detail) }
        audioDecoder = newDecoder

        val openedAudio = createAndStartAvFilter(tuner, audio, isAudio = true).getOrElse { error ->
            currentAudioFilterToken = previousAudioToken
            audioDecoder = previousDecoder
            newDecoder.close()
            logAudioUnavailable(PlaybackUnavailableReason.AUDIO_FILTER_NOT_STARTED, error.message.orEmpty())
            return AudioSwitchResult(false, listOf("audio filter start failed: ${error.message}"))
        }

        audioFilter = openedAudio
        closeFilter(previousFilter)
        previousDecoder?.close()
        val diagnostics = listOf(
            "audioPid=${audio.elementaryPid}",
            "audioCodec=$audioKind",
            "service=${selection.serviceKey}",
        )
        syncClock.reset(PlaybackSyncClock.Mode.AUDIO_MASTER)
        Log.i(LogTags.TIS, "audio track を切り替えました inputId=$inputId sessionId=$sessionId ${diagnostics.joinToString(" ")}")
        return AudioSwitchResult(true, diagnostics)
    }

    private fun createAndStartAvFilter(tuner: Tuner, stream: AribElementaryStream, isAudio: Boolean): Result<Filter> = runCatching {
        val pid = stream.elementaryPid
        require(pid in 0..0x1fff) { "PID が範囲外です: $pid" }
        val subtype = if (isAudio) Filter.SUBTYPE_AUDIO else Filter.SUBTYPE_VIDEO
        val filterGeneration = playbackGeneration
        val filterToken = nextAvFilterToken++
        if (isAudio) currentAudioFilterToken = filterToken else currentVideoFilterToken = filterToken
        val targetAudioDecoder = audioDecoder
        val targetVideoDecoder = videoDecoder
        fun tokenMatches(): Boolean = if (isAudio) currentAudioFilterToken == filterToken else currentVideoFilterToken == filterToken
        val filter = tuner.openFilter(Filter.TYPE_TS, subtype, AV_FILTER_BUFFER_BYTES, executor, object : FilterCallback {
            override fun onFilterEvent(filter: Filter, events: Array<FilterEvent>) {
                runCatching {
                    if (filterGeneration != playbackGeneration || !tokenMatches()) return
                    for (event in events.filterIsInstance<MediaEvent>()) {
                        if (filterGeneration != playbackGeneration || !tokenMatches()) {
                            releaseMediaEvent(event)
                            continue
                        }
                        val sample = try {
                            sampleFromEvent(event, isAudio)
                        } finally {
                            releaseMediaEvent(event)
                        } ?: continue
                        if (filterGeneration != playbackGeneration || !tokenMatches()) continue
                        if (isAudio) targetAudioDecoder?.queue(sample) else targetVideoDecoder?.queue(sample)
                    }
                }.onFailure { error ->
                    if (isAudio) {
                        logAudioUnavailable(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, error.message.orEmpty())
                        syncClock.fallbackToVideoMaster("audio MediaEvent callback failure")
                    } else {
                        emitUnavailable(PlaybackUnavailableReason.VIDEO_CODEC_ERROR, error.message.orEmpty())
                    }
                }
            }
            override fun onFilterStatusChanged(filter: Filter, status: Int) {
                Log.d(LogTags.TIS, "AV filter 状態 inputId=$inputId pid=$pid isAudio=$isAudio status=$status")
            }
        }) ?: error("openFilter が null を返しました pid=$pid isAudio=$isAudio")
        val settingsBuilder = AvSettings.builder(Filter.TYPE_TS, isAudio).setPassthrough(false)
        if (isAudio) {
            settingsBuilder.setAudioStreamType(mapAudioStreamType(stream.streamType))
        } else {
            settingsBuilder.setVideoStreamType(mapVideoStreamType(stream.streamType))
        }
        val config = TsFilterConfiguration.builder().setTpid(pid).setSettings(settingsBuilder.build()).build()
        val configureResult = filter.configure(config)
        if (configureResult != Tuner.RESULT_SUCCESS) {
            if (isAudio && currentAudioFilterToken == filterToken) currentAudioFilterToken = -1L
            if (!isAudio && currentVideoFilterToken == filterToken) currentVideoFilterToken = -1L
            closeFilter(filter)
            error("AV filter configure failed result=$configureResult pid=$pid isAudio=$isAudio")
        }
        val startResult = filter.start()
        if (startResult != Tuner.RESULT_SUCCESS) {
            if (isAudio && currentAudioFilterToken == filterToken) currentAudioFilterToken = -1L
            if (!isAudio && currentVideoFilterToken == filterToken) currentVideoFilterToken = -1L
            closeFilter(filter)
            error("AV filter start failed result=$startResult pid=$pid isAudio=$isAudio")
        }
        filter
    }

    private fun createAndStartSubtitlePesFilter(tuner: Tuner, stream: AribElementaryStream): Result<Filter> = runCatching {
        val pid = stream.elementaryPid
        require(pid in 0..0x1fff) { "PID が範囲外です: $pid" }
        require(TunerController.isCaptionStream(stream)) { "字幕ではない stream を subtitle filter に接続しません pid=$pid" }
        val filterGeneration = playbackGeneration
        val filterToken = nextAvFilterToken++
        currentSubtitleFilterToken = filterToken
        fun tokenMatches(): Boolean = currentSubtitleFilterToken == filterToken
        val trackId = TunerController.trackIdForSubtitleStream(stream)
        val filter = tuner.openFilter(Filter.TYPE_TS, Filter.SUBTYPE_PES, SUBTITLE_FILTER_BUFFER_BYTES, executor, object : FilterCallback {
            override fun onFilterEvent(filter: Filter, events: Array<FilterEvent>) {
                runCatching {
                    if (filterGeneration != playbackGeneration || !tokenMatches()) return
                    for (event in events.filterIsInstance<PesEvent>()) {
                        if (filterGeneration != playbackGeneration || !tokenMatches()) continue
                        val dataLength = event.dataLength
                        if (dataLength <= 0 || dataLength > MAX_SUBTITLE_PES_BYTES) continue
                        val buffer = ByteArray(dataLength)
                        val read = filter.read(buffer, 0, dataLength.toLong())
                        if (read <= 0) continue
                        val payload = if (read == buffer.size) buffer else buffer.copyOf(read)
                        onSubtitlePes(trackId, payload, ARIBCC_PTS_NOPTS_MILLIS)
                    }
                }.onFailure { error ->
                    Log.w(LogTags.TIS, "字幕PES filter callback に失敗しました inputId=$inputId pid=$pid", error)
                }
            }
            override fun onFilterStatusChanged(filter: Filter, status: Int) {
                Log.d(LogTags.TIS, "字幕PES filter 状態 inputId=$inputId pid=$pid status=$status")
            }
        }) ?: error("openFilter が null を返しました subtitle pid=$pid")
        val settings = PesSettings.builder(Filter.TYPE_TS)
            .setStreamId(PES_STREAM_ID_PRIVATE_STREAM_1)
            .build()
        val config = TsFilterConfiguration.builder().setTpid(pid).setSettings(settings).build()
        val configureResult = filter.configure(config)
        if (configureResult != Tuner.RESULT_SUCCESS) {
            if (currentSubtitleFilterToken == filterToken) currentSubtitleFilterToken = -1L
            closeFilter(filter)
            error("subtitle PES filter configure failed result=$configureResult pid=$pid")
        }
        val startResult = filter.start()
        if (startResult != Tuner.RESULT_SUCCESS) {
            if (currentSubtitleFilterToken == filterToken) currentSubtitleFilterToken = -1L
            closeFilter(filter)
            error("subtitle PES filter start failed result=$startResult pid=$pid")
        }
        filter
    }

    private fun markFirstFrameRendered(generation: Long) {
        // MediaCodec frame-rendered コールバック は playback executor ではなく mainHandler へ届く。
        // コールバック thread から playbackGeneration / surface を読んだり first-frame state を変更したりせず、
        // playback executor へ直列化する。これにより既存 generation の frame notification が
        // current state を進めることを防ぐ。
        enqueuePlaybackAction { markFirstFrameRenderedOnPlaybackExecutor(generation) }
    }

    private fun markFirstFrameRenderedOnPlaybackExecutor(generation: Long) {
        if (generation != playbackGeneration) return
        if (surface?.isValid == true && videoAvailableNotified.compareAndSet(false, true)) onVideoAvailable()
    }

    private fun scheduleFirstFrameTimeout(generation: Long) {
        mainHandler.postDelayed({
            enqueuePlaybackAction {
                if (shouldTriggerFirstFrameTimeoutForTest(generation, playbackGeneration, videoAvailableNotified.get())) {
                    emitUnavailable(PlaybackUnavailableReason.FIRST_FRAME_TIMEOUT, "first frame timeout ${FIRST_FRAME_TIMEOUT_MS}ms")
                    stopOnPlaybackExecutor()
                }
            }
        }, FIRST_FRAME_TIMEOUT_MS)
    }

    private fun logAudioUnavailable(reason: PlaybackUnavailableReason, detail: String) {
        Log.w(LogTags.TIS, "audio を利用できませんが video は継続します inputId=$inputId sessionId=$sessionId reason=$reason detail=$detail generation=$playbackGeneration")
    }

    fun currentPlaybackGenerationForTest(): Long = playbackGeneration

    fun audioMasterFallbackCountForDiagnostic(): Int = syncClock.audioFallbackCountForDiagnostic()
    fun audioWriteErrorsForDiagnostic(): Int = audioWriteErrors
    fun audioPartialWritesForDiagnostic(): Int = audioPartialWrites
    fun oversizedSamplesDroppedForDiagnostic(): Int = oversizedSamplesDropped
    fun malformedSamplesDroppedForDiagnostic(): Int = malformedSamplesDropped
    fun decoderBackpressureDropsForDiagnostic(): Int = decoderBackpressureDrops
    fun audioPtsFallbackSamplesForDiagnostic(): Int = audioPtsFallbackSamples
    fun videoPtsFallbackSamplesForDiagnostic(): Int = videoPtsFallbackSamples

    fun simulateFirstFrameRenderedForTest(generation: Long) {
        markFirstFrameRendered(generation)
    }


    private fun releaseMediaEvent(event: MediaEvent) {
        runCatching { event.release() }.onFailure { Log.w(LogTags.TIS, "MediaEvent の release に失敗しました", it) }
    }

    private fun sampleFromEvent(event: MediaEvent, isAudio: Boolean): MediaSample? {
        val offset = event.offset
        val length = event.dataLength
        if (offset < 0L || length <= 0L) {
            malformedSamplesDropped++
            Log.w(LogTags.TIS, "MediaEvent の offset/length が不正なため破棄します offset=$offset length=$length malformed=$malformedSamplesDropped")
            return null
        }
        val end = offset + length
        if (end < offset) {
            malformedSamplesDropped++
            Log.w(LogTags.TIS, "MediaEvent の offset+length が overflow したため破棄します offset=$offset length=$length malformed=$malformedSamplesDropped")
            return null
        }
        if (length > MEDIA_EVENT_SAMPLE_MAX_BYTES) {
            oversizedSamplesDropped++
            Log.w(LogTags.TIS, "MediaEvent sample が上限を超えたため allocation 前に破棄します length=$length max=$MEDIA_EVENT_SAMPLE_MAX_BYTES oversized=$oversizedSamplesDropped")
            return null
        }
        if (event.isSecureMemory) {
            if (isAudio) {
                logAudioUnavailable(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "audio secure MediaEvent は clear playback 対象外です")
                syncClock.fallbackToVideoMaster("audio secure MediaEvent")
            } else {
                emitUnavailable(PlaybackUnavailableReason.VIDEO_CODEC_ERROR, "video secure MediaEvent は clear playback 対象外です")
            }
            return null
        }
        val block = event.linearBlock ?: return null
        val bytes = runCatching {
            val mapped = block.map().duplicate()
            val capacity = mapped.capacity().toLong()
            if (end > capacity) {
                malformedSamplesDropped++
                Log.w(LogTags.TIS, "MediaEvent が LinearBlock 範囲外のため破棄します offset=$offset length=$length capacity=$capacity malformed=$malformedSamplesDropped")
                return null
            }
            val start = offset.toInt()
            val sampleLength = length.toInt()
            mapped.position(start)
            mapped.limit(start + sampleLength)
            ByteArray(sampleLength).also { mapped.get(it) }
        }.onFailure { Log.w(LogTags.TIS, "MediaEvent LinearBlock の map に失敗しました", it) }.getOrNull() ?: return null
        val normalized = PesTimestampNormalizer.toPresentationUs(if (event.isPtsPresent) event.pts else null)
        return MediaSample(bytes, normalized.presentationUs, normalized.fallbackUsed, isAudio)
    }

    private abstract inner class DecoderPipeline : AutoCloseable {
        protected var codec: MediaCodec? = null
        private val configBytes = ByteArrayOutputStream()
        private val bufferInfo = MediaCodec.BufferInfo()
        private val pendingSamples = java.util.ArrayDeque<MediaSample>()

        fun queue(sample: MediaSample) {
            try {
                queueInternal(sample)
            } catch (e: RuntimeException) {
                Log.w(LogTags.TIS, "decoder queue/drain に失敗しました", e)
                onDecoderFailure(e)
                close()
            }
        }

        private fun queueInternal(sample: MediaSample) {
            var decoder = codec
            if (decoder == null) {
                configBytes.write(sample.bytes)
                decoder = configureFromBufferedHeader(configBytes.toByteArray())
                if (decoder == null) {
                    if (configBytes.size() > CODEC_CONFIG_MAX_BYTES) {
                        onCodecConfigTimeout()
                        configBytes.reset()
                    }
                    return
                }
                codec = decoder
            }
            enqueuePendingSample(sample)
            drainPendingInput(decoder)
            drain(decoder)
        }

        private fun enqueuePendingSample(sample: MediaSample) {
            if (pendingSamples.size >= DECODER_PENDING_SAMPLE_LIMIT) {
                pendingSamples.removeFirst()
                decoderBackpressureDrops++
                Log.w(LogTags.TIS, "decoder pending queue が満杯のため最古 sample を破棄します limit=$DECODER_PENDING_SAMPLE_LIMIT drops=$decoderBackpressureDrops isAudio=${sample.isAudio}")
            }
            pendingSamples.addLast(sample)
        }

        private fun drainPendingInput(decoder: MediaCodec) {
            while (!pendingSamples.isEmpty()) {
                val inputIndex = decoder.dequeueInputBuffer(CODEC_DEQUEUE_TIMEOUT_US)
                if (inputIndex < 0) return
                val sample = pendingSamples.removeFirst()
                if (sample.timestampFallbackUsed) {
                    if (sample.isAudio) audioPtsFallbackSamples++ else videoPtsFallbackSamples++
                }
                val input = decoder.getInputBuffer(inputIndex)
                if (input == null) {
                    decoder.queueInputBuffer(inputIndex, 0, 0, sample.presentationTimeUs, 0)
                    continue
                }
                input.clear()
                if (shouldDropOversizedSampleForTest(sample.bytes.size, input.remaining())) {
                    oversizedSamplesDropped++
                    Log.w(LogTags.TIS, "decoder input buffer に収まらない sample を破棄します sampleBytes=${sample.bytes.size} capacity=${input.remaining()} isAudio=${sample.isAudio} dropped=$oversizedSamplesDropped")
                    decoder.queueInputBuffer(inputIndex, 0, 0, sample.presentationTimeUs, 0)
                    continue
                }
                input.put(sample.bytes)
                decoder.queueInputBuffer(inputIndex, 0, sample.bytes.size, sample.presentationTimeUs, 0)
            }
        }

        protected abstract fun configureFromBufferedHeader(bytes: ByteArray): MediaCodec?
        protected open fun onOutputFormatChanged(format: MediaFormat) = Unit
        protected abstract fun releaseOutput(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo)
        protected abstract fun onDecoderFailure(error: RuntimeException)
        protected abstract fun onCodecConfigTimeout()

        private fun drain(decoder: MediaCodec) {
            while (true) {
                when (val outputIndex = decoder.dequeueOutputBuffer(bufferInfo, 0)) {
                    MediaCodec.INFO_TRY_AGAIN_LATER -> return
                    MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> onOutputFormatChanged(decoder.outputFormat)
                    MediaCodec.INFO_OUTPUT_BUFFERS_CHANGED -> Unit
                    else -> if (outputIndex >= 0) releaseOutput(decoder, outputIndex, bufferInfo) else return
                }
            }
        }

        override fun close() {
            pendingSamples.clear()
            val decoder = codec
            codec = null
            if (decoder != null) {
                runCatching { decoder.stop() }.onFailure { Log.w(LogTags.TIS, "decoder stop に失敗しました", it) }
                runCatching { decoder.release() }.onFailure { Log.w(LogTags.TIS, "decoder release に失敗しました", it) }
            }
        }
    }

    private inner class VideoDecoderPipeline(
        private val kind: VideoCodecKind,
        private val outputSurface: Surface,
        private val syncClock: PlaybackSyncClock,
        private val generation: Long,
        private val firstFrameRendered: (Long) -> Unit,
        private val errorSink: (PlaybackUnavailableReason, String) -> Unit,
    ) : DecoderPipeline() {
        override fun configureFromBufferedHeader(bytes: ByteArray): MediaCodec? {
            val format = when (kind) {
                VideoCodecKind.MPEG2 -> EsHeaderParser.mpeg2VideoFormat(bytes)
                VideoCodecKind.AVC -> EsHeaderParser.avcVideoFormat(bytes)
            } ?: return null
            onVideoFormatDiscovered(
                VideoFormatInfo(
                    streamType = kind.streamType,
                    mime = kind.mime,
                    width = getIntegerOrDefault(format, MediaFormat.KEY_WIDTH, 0),
                    height = getIntegerOrDefault(format, MediaFormat.KEY_HEIGHT, 0),
                ),
            )
            val decoder = MediaCodec.createDecoderByType(kind.mime)
            try {
                decoder.configure(format, outputSurface, null, 0)
                decoder.setOnFrameRenderedListener({ _, _, _ -> firstFrameRendered(generation) }, mainHandler)
                decoder.start()
                return decoder
            } catch (e: RuntimeException) {
                runCatching { decoder.release() }
                throw e
            }
        }

        override fun onDecoderFailure(error: RuntimeException) {
            errorSink(PlaybackUnavailableReason.VIDEO_CODEC_ERROR, error.message.orEmpty())
        }

        override fun onCodecConfigTimeout() {
            errorSink(PlaybackUnavailableReason.CODEC_CONFIG_TIMEOUT, "video decoder 構成に必要な ES header が見つかりません")
        }

        override fun releaseOutput(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            if (info.size <= 0) {
                codec.releaseOutputBuffer(index, false)
                return
            }
            if (!outputSurface.isValid) {
                codec.releaseOutputBuffer(index, false)
                errorSink(PlaybackUnavailableReason.VIDEO_OUTPUT_RENDER_FAILED, "video output Surface が無効です")
                return
            }
            val renderAtNs = syncClock.renderTimeNs(info.presentationTimeUs)
            val lateNs = System.nanoTime() - renderAtNs
            if (lateNs > VIDEO_DROP_LATE_NS) {
                codec.releaseOutputBuffer(index, false)
            } else {
                codec.releaseOutputBuffer(index, renderAtNs)
            }
        }
    }

    private inner class AudioDecoderPipeline(
        private val kind: AudioCodecKind,
        initialVolume: Float,
        private val syncClock: PlaybackSyncClock,
        private val errorSink: (PlaybackUnavailableReason, String) -> Unit,
    ) : DecoderPipeline() {
        private var sink: AudioSink? = null
        private var volume: Float = initialVolume
        private var outputSampleRate: Int = DEFAULT_AUDIO_SAMPLE_RATE
        private var outputChannels: Int = DEFAULT_AUDIO_CHANNEL_COUNT

        fun setVolume(value: Float) {
            volume = value
            sink?.let { audioSink ->
                runCatching { audioSink.setVolume(value) }
                    .onFailure { Log.w(LogTags.TIS, "AudioTrack volume 設定に失敗しました", it) }
            }
        }

        override fun configureFromBufferedHeader(bytes: ByteArray): MediaCodec? {
            val format = when (kind) {
                AudioCodecKind.AAC_ADTS -> EsHeaderParser.adtsAacFormat(bytes)
                AudioCodecKind.MPEG1, AudioCodecKind.MPEG2 -> EsHeaderParser.mpegAudioFormat(bytes)
            } ?: return null
            outputSampleRate = getIntegerOrDefault(format, MediaFormat.KEY_SAMPLE_RATE, DEFAULT_AUDIO_SAMPLE_RATE)
            outputChannels = getIntegerOrDefault(format, MediaFormat.KEY_CHANNEL_COUNT, DEFAULT_AUDIO_CHANNEL_COUNT)
            val decoder = MediaCodec.createDecoderByType(kind.mime)
            try {
                decoder.configure(format, null, null, 0)
                decoder.start()
                recreateTrack()
                return decoder
            } catch (e: RuntimeException) {
                runCatching { decoder.release() }
                throw e
            }
        }

        override fun onDecoderFailure(error: RuntimeException) {
            errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, error.message.orEmpty())
            syncClock.fallbackToVideoMaster("audio decoder failure")
        }

        override fun onCodecConfigTimeout() {
            errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "audio decoder 構成に必要な ES header が見つかりません")
            syncClock.fallbackToVideoMaster("audio codec config timeout")
        }

        override fun onOutputFormatChanged(format: MediaFormat) {
            outputSampleRate = getIntegerOrDefault(format, MediaFormat.KEY_SAMPLE_RATE, outputSampleRate)
            outputChannels = getIntegerOrDefault(format, MediaFormat.KEY_CHANNEL_COUNT, outputChannels)
            recreateTrack()
        }

        override fun releaseOutput(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            if (info.size > 0) {
                val output = codec.getOutputBuffer(index)
                if (output != null) {
                    output.position(info.offset)
                    output.limit(info.offset + info.size)
                    syncClock.anchorAudio(info.presentationTimeUs)
                    val writeResult = writeFully(ensureSink(), output, info.size)
                    if (writeResult < 0) {
                        onAudioWriteError(writeResult)
                    }
                } else {
                    errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "audio output buffer が null です")
                }
            }
            codec.releaseOutputBuffer(index, false)
        }

        private fun ensureSink(): AudioSink = sink ?: recreateTrack()

        private fun writeFully(audioSink: AudioSink, output: ByteBuffer, size: Int): Int {
            var remaining = size
            var lastResult = 0
            var consecutiveZeroWrites = 0
            while (remaining > 0) {
                val written = audioSink.write(output, remaining)
                lastResult = written
                when {
                    written < 0 -> return written
                    written == 0 -> {
                        audioPartialWrites++
                        consecutiveZeroWrites++
                        if (consecutiveZeroWrites > MAX_ZERO_AUDIO_WRITE_RETRIES) {
                            return AUDIO_WRITE_STALLED
                        }
                        Thread.yield()
                    }
                    written > remaining -> {
                        audioPartialWrites++
                        return AUDIO_WRITE_INVALID_COUNT
                    }
                    else -> {
                        consecutiveZeroWrites = 0
                        if (written < remaining) audioPartialWrites++
                        remaining -= written
                    }
                }
            }
            return lastResult
        }

        private fun recreateTrack(): AudioSink {
            sink?.let { runCatching { it.release() } }
            val channelMask = if (outputChannels <= 1) AudioFormat.CHANNEL_OUT_MONO else AudioFormat.CHANNEL_OUT_STEREO
            val minBuffer = AudioTrack.getMinBufferSize(outputSampleRate, channelMask, AudioFormat.ENCODING_PCM_16BIT).coerceAtLeast(32 * 1024)
            val builder = AudioTrack.Builder()
                .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build())
                .setAudioFormat(AudioFormat.Builder().setSampleRate(outputSampleRate).setChannelMask(channelMask).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
                .setBufferSizeInBytes(minBuffer)
                .setTransferMode(AudioTrack.MODE_STREAM)
            // Android 14 system SDK tree によって、AudioTrack.Builder が compile time に
            // setAttributionSource を公開しているかが異なる。AttributionSource は保持し、
            // reflection による補助適用を行う。この境界は DESIGN_JA.md で固定する。
            attributionSource?.let { source ->
                runCatching {
                    AudioTrack.Builder::class.java
                        .getMethod("setAttributionSource", AttributionSource::class.java)
                        .invoke(builder, source)
                }.onFailure { Log.w(LogTags.TIS, "AudioTrack attributionSource 設定に失敗しました", it) }
            }
            val created = builder.build()
            created.setVolume(volume)
            val newSink = AndroidAudioSink(created)
            newSink.play()
            sink = newSink
            return newSink
        }

        private fun onAudioWriteError(errorCode: Int) {
            audioWriteErrors++
            errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "AudioTrack.write error=$errorCode")
            syncClock.fallbackToVideoMaster("audio write error=$errorCode")
            sink?.let { runCatching { it.release() } }
            sink = null
        }

        override fun close() {
            super.close()
            sink?.let { runCatching { it.release() } }
            sink = null
        }
    }

    private class PlaybackSyncClock {
        enum class Mode { AUDIO_MASTER, VIDEO_MASTER }
        private var mode: Mode = Mode.VIDEO_MASTER
        private var basePresentationUs: Long? = null
        private var baseSystemNs: Long? = null
        private var audioFallbackDeadlineNs: Long = 0L
        private var audioFallbackCount: Int = 0

        fun reset(nextMode: Mode = Mode.VIDEO_MASTER) {
            mode = nextMode
            basePresentationUs = null
            baseSystemNs = null
            audioFallbackDeadlineNs = if (nextMode == Mode.AUDIO_MASTER) System.nanoTime() + AUDIO_MASTER_WAIT_NS else 0L
        }

        fun anchorAudio(presentationUs: Long) {
            if (mode == Mode.AUDIO_MASTER && (basePresentationUs == null || baseSystemNs == null)) {
                basePresentationUs = presentationUs
                baseSystemNs = System.nanoTime()
            }
        }

        fun fallbackToVideoMaster(reason: String) {
            if (mode == Mode.AUDIO_MASTER) {
                Log.w(LogTags.TIS, "audio master から video master へ fallback します reason=$reason")
                mode = Mode.VIDEO_MASTER
                basePresentationUs = null
                baseSystemNs = null
                audioFallbackDeadlineNs = 0L
                audioFallbackCount++
            }
        }

        fun audioFallbackCountForDiagnostic(): Int = audioFallbackCount

        private fun anchorVideoIfNeeded(presentationUs: Long) {
            if (basePresentationUs == null || baseSystemNs == null) {
                basePresentationUs = presentationUs
                baseSystemNs = System.nanoTime()
            }
        }

        fun renderTimeNs(presentationUs: Long): Long {
            if (mode == Mode.AUDIO_MASTER && (basePresentationUs == null || baseSystemNs == null)) {
                if (audioFallbackDeadlineNs != 0L && System.nanoTime() >= audioFallbackDeadlineNs) {
                    fallbackToVideoMaster("audio anchor timeout")
                } else {
                    return System.nanoTime() + VIDEO_HOLD_BEFORE_AUDIO_NS
                }
            }
            if (mode == Mode.VIDEO_MASTER) anchorVideoIfNeeded(presentationUs)
            val baseUs = basePresentationUs ?: presentationUs
            val baseNs = baseSystemNs ?: System.nanoTime()
            return baseNs + (presentationUs - baseUs) * 1_000L
        }
    }

    data class NormalizedPresentationTime(val presentationUs: Long, val fallbackUsed: Boolean)
    enum class MediaEventBoundsDecision { ACCEPT, MALFORMED, OVERSIZED, OUT_OF_BOUNDS }

    private object PesTimestampNormalizer {
        fun toPresentationUs(pts90k: Long?): NormalizedPresentationTime {
            if (pts90k == null || pts90k < 0) return NormalizedPresentationTime(System.nanoTime() / 1000L, true)
            return NormalizedPresentationTime((pts90k * 1_000_000L) / 90_000L, false)
        }
    }

    private object EsHeaderParser {
        private val sampleRates = intArrayOf(96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350)

        fun mpeg2VideoFormat(bytes: ByteArray): MediaFormat? {
            val pos = findStartCode(bytes, 0xb3) ?: return null
            if (pos + 7 >= bytes.size) return null
            val width = ((bytes[pos + 4].toInt() and 0xff) shl 4) or ((bytes[pos + 5].toInt() and 0xf0) ushr 4)
            val height = ((bytes[pos + 5].toInt() and 0x0f) shl 8) or (bytes[pos + 6].toInt() and 0xff)
            return MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_MPEG2, width.coerceAtLeast(1), height.coerceAtLeast(1))
        }

        fun avcVideoFormat(bytes: ByteArray): MediaFormat? {
            val sps = findNal(bytes, 7) ?: return null
            val pps = findNal(bytes, 8) ?: return null
            val dimensions = parseAvcSpsDimensions(sps) ?: return null
            return MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, dimensions.width, dimensions.height).apply {
                setByteBuffer("csd-0", ByteBuffer.wrap(sps))
                setByteBuffer("csd-1", ByteBuffer.wrap(pps))
            }
        }


        data class VideoDimensions(val width: Int, val height: Int)

        fun parseAvcSpsDimensionsForTest(spsWithStartCode: ByteArray): VideoDimensions? = parseAvcSpsDimensions(spsWithStartCode)

        private fun parseAvcSpsDimensions(spsWithStartCode: ByteArray): VideoDimensions? = runCatching {
            val rbsp = nalRbspPayload(spsWithStartCode)
            val bits = BitReader(rbsp)
            val profileIdc = bits.readBits(8)
            bits.readBits(8) // 制約フラグと予約ビット
            bits.readBits(8) // レベル識別子
            bits.readUE() // SPS 識別子
            var chromaFormatIdc = 1
            if (profileIdc in setOf(100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135)) {
                chromaFormatIdc = bits.readUE()
                if (chromaFormatIdc == 3) bits.readBit()
                bits.readUE() // 輝度ビット深度補正値
                bits.readUE() // 色差ビット深度補正値
                bits.readBit() // 量子化 bypass フラグ
                val scalingMatrixPresent = bits.readBit() == 1
                if (scalingMatrixPresent) {
                    val count = if (chromaFormatIdc == 3) 12 else 8
                    repeat(count) { index ->
                        if (bits.readBit() == 1) skipScalingList(bits, if (index < 6) 16 else 64)
                    }
                }
            }
            bits.readUE() // frame_num 上限補正値
            val picOrderCntType = bits.readUE()
            if (picOrderCntType == 0) {
                bits.readUE()
            } else if (picOrderCntType == 1) {
                bits.readBit()
                bits.readSE()
                bits.readSE()
                repeat(bits.readUE()) { bits.readSE() }
            }
            bits.readUE() // 参照フレーム数上限
            bits.readBit() // frame_num 欠落許可フラグ
            val picWidthInMbsMinus1 = bits.readUE()
            val picHeightInMapUnitsMinus1 = bits.readUE()
            val frameMbsOnlyFlag = bits.readBit()
            if (frameMbsOnlyFlag == 0) bits.readBit()
            bits.readBit() // 8x8 直接推定フラグ
            var cropLeft = 0
            var cropRight = 0
            var cropTop = 0
            var cropBottom = 0
            if (bits.readBit() == 1) {
                cropLeft = bits.readUE()
                cropRight = bits.readUE()
                cropTop = bits.readUE()
                cropBottom = bits.readUE()
            }
            val frameMbsFactor = 2 - frameMbsOnlyFlag
            val cropUnitX = if (chromaFormatIdc == 0) 1 else if (chromaFormatIdc == 3) 1 else 2
            val cropUnitY = when (chromaFormatIdc) {
                0 -> frameMbsFactor
                1 -> 2 * frameMbsFactor
                2 -> frameMbsFactor
                else -> frameMbsFactor
            }
            val width = ((picWidthInMbsMinus1 + 1) * 16) - (cropLeft + cropRight) * cropUnitX
            val height = ((picHeightInMapUnitsMinus1 + 1) * 16 * frameMbsFactor) - (cropTop + cropBottom) * cropUnitY
            VideoDimensions(width.coerceAtLeast(1), height.coerceAtLeast(1))
        }.getOrNull()

        private fun nalRbspPayload(nalWithStartCode: ByteArray): ByteArray {
            val start = when {
                nalWithStartCode.size >= 5 && nalWithStartCode[0] == 0.toByte() && nalWithStartCode[1] == 0.toByte() && nalWithStartCode[2] == 0.toByte() && nalWithStartCode[3] == 1.toByte() -> 5
                nalWithStartCode.size >= 4 && nalWithStartCode[0] == 0.toByte() && nalWithStartCode[1] == 0.toByte() && nalWithStartCode[2] == 1.toByte() -> 4
                else -> 1
            }
            val out = ArrayList<Byte>(nalWithStartCode.size)
            var zeros = 0
            for (i in start until nalWithStartCode.size) {
                val b = nalWithStartCode[i]
                if (zeros >= 2 && b == 0x03.toByte()) {
                    zeros = 0
                    continue
                }
                out += b
                zeros = if (b == 0.toByte()) zeros + 1 else 0
            }
            return out.toByteArray()
        }

        private fun skipScalingList(bits: BitReader, size: Int) {
            var lastScale = 8
            var nextScale = 8
            repeat(size) {
                if (nextScale != 0) {
                    val delta = bits.readSE()
                    nextScale = (lastScale + delta + 256) % 256
                }
                lastScale = if (nextScale == 0) lastScale else nextScale
            }
        }

        private class BitReader(private val bytes: ByteArray) {
            private var bitOffset = 0
            fun readBit(): Int = readBits(1)
            fun readBits(count: Int): Int {
                var value = 0
                repeat(count) {
                    val byteIndex = bitOffset / 8
                    require(byteIndex < bytes.size) { "SPS bitstream ended" }
                    val bitIndex = 7 - (bitOffset % 8)
                    value = (value shl 1) or ((bytes[byteIndex].toInt() ushr bitIndex) and 1)
                    bitOffset++
                }
                return value
            }
            fun readUE(): Int {
                var zeros = 0
                while (readBit() == 0) zeros++
                return if (zeros == 0) 0 else ((1 shl zeros) - 1) + readBits(zeros)
            }
            fun readSE(): Int {
                val codeNum = readUE()
                val value = (codeNum + 1) / 2
                return if (codeNum % 2 == 0) -value else value
            }
        }
        fun adtsAacFormat(bytes: ByteArray): MediaFormat? {
            val header = findAdtsHeader(bytes) ?: return null
            val sampleRate = sampleRates.getOrNull(header.frequencyIndex) ?: return null
            val asc0 = ((2 shl 3) or (header.frequencyIndex ushr 1)).toByte()
            val asc1 = (((header.frequencyIndex and 1) shl 7) or (header.channelConfig shl 3)).toByte()
            return MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_AAC, sampleRate, header.channelConfig.coerceAtLeast(1)).apply {
                setInteger(MediaFormat.KEY_IS_ADTS, 1)
                setByteBuffer("csd-0", ByteBuffer.wrap(byteArrayOf(asc0, asc1)))
            }
        }

        fun mpegAudioFormat(bytes: ByteArray): MediaFormat? {
            val offset = (0 until bytes.size - 3).firstOrNull { i ->
                (bytes[i].toInt() and 0xff) == 0xff && (bytes[i + 1].toInt() and 0xe0) == 0xe0
            } ?: return null
            val b2 = bytes[offset + 2].toInt() and 0xff
            val b3 = bytes[offset + 3].toInt() and 0xff
            val version = (bytes[offset + 1].toInt() ushr 3) and 0x03
            val sampleRateIndex = (b2 ushr 2) and 0x03
            val channelMode = (b3 ushr 6) and 0x03
            val base = when (sampleRateIndex) { 0 -> 44100; 1 -> 48000; 2 -> 32000; else -> return null }
            val sampleRate = when (version) { 3 -> base; 2 -> base / 2; 0 -> base / 4; else -> base }
            val channels = if (channelMode == 3) 1 else 2
            return MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_MPEG, sampleRate, channels)
        }

        private data class AdtsHeader(val frequencyIndex: Int, val channelConfig: Int)

        private fun findAdtsHeader(bytes: ByteArray): AdtsHeader? {
            for (i in 0 until bytes.size - 7) {
                if ((bytes[i].toInt() and 0xff) == 0xff && (bytes[i + 1].toInt() and 0xf0) == 0xf0) {
                    val freqIndex = (bytes[i + 2].toInt() ushr 2) and 0x0f
                    val channelConfig = ((bytes[i + 2].toInt() and 0x01) shl 2) or ((bytes[i + 3].toInt() ushr 6) and 0x03)
                    return AdtsHeader(freqIndex, channelConfig)
                }
            }
            return null
        }

        private fun findStartCode(bytes: ByteArray, code: Int): Int? {
            for (i in 0 until bytes.size - 4) {
                if (bytes[i] == 0.toByte() && bytes[i + 1] == 0.toByte() && bytes[i + 2] == 1.toByte() && (bytes[i + 3].toInt() and 0xff) == code) return i
            }
            return null
        }

        private fun findNal(bytes: ByteArray, nalType: Int): ByteArray? {
            var i = 0
            while (i < bytes.size - 4) {
                val prefixLength = when {
                    i + 4 < bytes.size && bytes[i] == 0.toByte() && bytes[i + 1] == 0.toByte() && bytes[i + 2] == 0.toByte() && bytes[i + 3] == 1.toByte() -> 4
                    bytes[i] == 0.toByte() && bytes[i + 1] == 0.toByte() && bytes[i + 2] == 1.toByte() -> 3
                    else -> { i++; continue }
                }
                val start = i
                val nalHeader = bytes[i + prefixLength].toInt() and 0x1f
                i += prefixLength + 1
                while (i < bytes.size - 4 && !isStartCode(bytes, i)) i++
                if (nalHeader == nalType) return bytes.copyOfRange(start, i)
            }
            return null
        }

        private fun isStartCode(bytes: ByteArray, i: Int): Boolean =
            i + 3 < bytes.size && bytes[i] == 0.toByte() && bytes[i + 1] == 0.toByte() &&
                (bytes[i + 2] == 1.toByte() || (i + 4 < bytes.size && bytes[i + 2] == 0.toByte() && bytes[i + 3] == 1.toByte()))
    }

    private fun getIntegerOrDefault(format: MediaFormat, key: String, defaultValue: Int): Int =
        if (format.containsKey(key)) format.getInteger(key) else defaultValue

    private fun mapVideoStreamType(streamType: Int): Int = when (streamType) {
        0x02 -> AvSettings.VIDEO_STREAM_TYPE_MPEG2
        0x1b -> AvSettings.VIDEO_STREAM_TYPE_AVC
        else -> AvSettings.VIDEO_STREAM_TYPE_UNDEFINED
    }

    private fun mapAudioStreamType(streamType: Int): Int = when (streamType) {
        0x03 -> AvSettings.AUDIO_STREAM_TYPE_MPEG1
        0x04 -> AvSettings.AUDIO_STREAM_TYPE_MPEG2
        0x0f -> AvSettings.AUDIO_STREAM_TYPE_AAC_ADTS
        else -> AvSettings.AUDIO_STREAM_TYPE_UNDEFINED
    }

    fun stop() {
        runOnPlaybackExecutorBlocking { stopOnPlaybackExecutor() }
    }

    private fun stopOnPlaybackExecutor() {
        playbackGeneration++
        currentVideoFilterToken = -1L
        currentAudioFilterToken = -1L
        currentSubtitleFilterToken = -1L
        videoAvailableNotified.set(false)
        closeFilter(videoFilter)
        closeFilter(audioFilter)
        closeFilter(subtitleFilter)
        videoFilter = null
        audioFilter = null
        subtitleFilter = null
        videoDecoder?.close()
        audioDecoder?.close()
        videoDecoder = null
        audioDecoder = null
        syncClock.reset()
    }

    private fun closeFilter(filter: Filter?) {
        if (filter == null) return
        runCatching { filter.stop() }.onFailure { Log.w(LogTags.TIS, "AV filter stop に失敗しました", it) }
        runCatching { filter.close() }.onFailure { Log.w(LogTags.TIS, "AV filter close に失敗しました", it) }
    }

    private fun emitUnavailable(reason: PlaybackUnavailableReason, detail: String = "") {
        Log.w(LogTags.TIS, "映像を利用できません inputId=$inputId sessionId=$sessionId reason=$reason detail=$detail generation=$playbackGeneration")
        onVideoUnavailable(PlaybackUnavailable(reason, detail, playbackGeneration))
    }

    fun release() {
        if (!released.compareAndSet(false, true)) return
        runOnPlaybackExecutorBlocking { stopOnPlaybackExecutor() }
        executor.shutdownNow()
    }

    override fun close() = release()

    companion object {
        fun h264DimensionsForTest(spsWithStartCode: ByteArray): Pair<Int, Int>? =
            EsHeaderParser.parseAvcSpsDimensionsForTest(spsWithStartCode)?.let { it.width to it.height }

        fun acceptsFirstFrameForTest(
            callbackGeneration: Long,
            currentGeneration: Long,
            surfaceValid: Boolean,
            alreadyNotified: Boolean,
        ): Boolean = callbackGeneration == currentGeneration && surfaceValid && !alreadyNotified

        fun shouldTriggerFirstFrameTimeoutForTest(
            timeoutGeneration: Long,
            currentGeneration: Long,
            alreadyNotified: Boolean,
        ): Boolean = timeoutGeneration == currentGeneration && !alreadyNotified

        fun syncModeForTest(audioExpected: Boolean): String = if (audioExpected) "AUDIO_MASTER" else "VIDEO_MASTER"

        fun shouldHoldVideoBeforeAudioAnchorForTest(
            audioExpected: Boolean,
            audioAnchored: Boolean,
            fallbackDeadlineReached: Boolean,
        ): Boolean = audioExpected && !audioAnchored && !fallbackDeadlineReached

        fun unavailableReasonForMediaEventCallbackFailureForTest(isAudio: Boolean): PlaybackUnavailableReason =
            if (isAudio) PlaybackUnavailableReason.AUDIO_UNAVAILABLE else PlaybackUnavailableReason.VIDEO_CODEC_ERROR

        fun normalizedPresentationTimeForTest(pts90k: Long?): NormalizedPresentationTime = PesTimestampNormalizer.toPresentationUs(pts90k)

        fun isAudioWriteErrorForTest(writeResult: Int): Boolean = writeResult < 0

        fun shouldDropOversizedSampleForTest(sampleSize: Int, inputRemaining: Int): Boolean = sampleSize > inputRemaining

        fun mediaEventBoundsDecisionForTest(offset: Long, dataLength: Long, mappedCapacity: Long): MediaEventBoundsDecision {
            if (offset < 0L || dataLength <= 0L) return MediaEventBoundsDecision.MALFORMED
            val end = offset + dataLength
            if (end < offset) return MediaEventBoundsDecision.MALFORMED
            if (dataLength > MEDIA_EVENT_SAMPLE_MAX_BYTES) return MediaEventBoundsDecision.OVERSIZED
            if (end > mappedCapacity) return MediaEventBoundsDecision.OUT_OF_BOUNDS
            return MediaEventBoundsDecision.ACCEPT
        }

        fun queuedInputSizeForSampleForTest(sampleSize: Int, inputRemaining: Int): Int =
            if (shouldDropOversizedSampleForTest(sampleSize, inputRemaining)) 0 else sampleSize

        fun simulateAudioWriteFullyForTest(writeResults: IntArray, size: Int): Pair<Int, Int> {
            var remaining = size
            var consecutiveZeroWrites = 0
            var partialWrites = 0
            var index = 0
            var lastResult = 0
            while (remaining > 0) {
                val written = if (index < writeResults.size) writeResults[index++] else 0
                lastResult = written
                when {
                    written < 0 -> return written to partialWrites
                    written == 0 -> {
                        partialWrites++
                        consecutiveZeroWrites++
                        if (consecutiveZeroWrites > MAX_ZERO_AUDIO_WRITE_RETRIES) {
                            return AUDIO_WRITE_STALLED to partialWrites
                        }
                    }
                    written > remaining -> {
                        partialWrites++
                        return AUDIO_WRITE_INVALID_COUNT to partialWrites
                    }
                    else -> {
                        consecutiveZeroWrites = 0
                        if (written < remaining) partialWrites++
                        remaining -= written
                    }
                }
            }
            return lastResult to partialWrites
        }

        fun normalizedAudioStreamTypeForTest(streamType: Int): Int = mapAudioStreamType(streamType)

        fun isSupportedAudioStreamTypeForTest(streamType: Int): Boolean = AudioCodecKind.fromStreamType(streamType) != null

        fun videoFormatInfoForTest(streamType: Int, spsWithStartCode: ByteArray): VideoFormatInfo? {
            val dimensions = h264DimensionsForTest(spsWithStartCode) ?: return null
            return VideoFormatInfo(streamType, MediaFormat.MIMETYPE_VIDEO_AVC, dimensions.first, dimensions.second)
        }

        private const val AV_FILTER_BUFFER_BYTES = 16 * 1024 * 1024L
        private const val SUBTITLE_FILTER_BUFFER_BYTES = 256 * 1024L
        private const val MAX_SUBTITLE_PES_BYTES = 256 * 1024
        private const val PES_STREAM_ID_PRIVATE_STREAM_1 = 0xbd
        private const val ARIBCC_PTS_NOPTS_MILLIS = Long.MIN_VALUE
        private const val CODEC_DEQUEUE_TIMEOUT_US = 0L
        private const val CODEC_CONFIG_MAX_BYTES = 512 * 1024
        private const val MEDIA_EVENT_SAMPLE_MAX_BYTES = 1024L * 1024L
        private const val DECODER_PENDING_SAMPLE_LIMIT = 32
        private const val DEFAULT_VIDEO_WIDTH = 1920
        private const val DEFAULT_VIDEO_HEIGHT = 1080
        private const val DEFAULT_AUDIO_SAMPLE_RATE = 48_000
        private const val DEFAULT_AUDIO_CHANNEL_COUNT = 2
        private const val VIDEO_DROP_LATE_NS = 500_000_000L
        private const val MAX_ZERO_AUDIO_WRITE_RETRIES = 8
        private const val AUDIO_WRITE_STALLED = -11
        private const val AUDIO_WRITE_INVALID_COUNT = -12
        private const val FIRST_FRAME_TIMEOUT_MS = 10_000L
        private const val AUDIO_MASTER_WAIT_NS = 2_000_000_000L
        private const val VIDEO_HOLD_BEFORE_AUDIO_NS = 50_000_000L
    }
}
