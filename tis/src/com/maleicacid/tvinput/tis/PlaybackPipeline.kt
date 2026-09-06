package com.maleicacid.tvinput.tis

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.media.MediaCodec
import android.media.MediaFormat
import android.media.MediaSync
import android.media.MediaTimestamp
import android.media.PlaybackParams
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
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.common.CaptionTimestamp
import com.maleicacid.tvinput.common.LogTags
import com.maleicacid.tvinput.common.PesPts90k
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class PlaybackPipeline(
    private val inputId: String,
    private val sessionId: String?,
    private val sessionContext: Context? = null,
) : AutoCloseable {
    @Volatile private var playbackExecutorThread: Thread? = null
    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-playback-$inputId").also { thread ->
            thread.isDaemon = true
            playbackExecutorThread = thread
        }
    }
    private val mainHandler = Handler(Looper.getMainLooper())
    private val codecCallbackThread = HandlerThread("maleicacid-codec-$inputId").apply { start() }
    private val codecCallbackHandler = Handler(codecCallbackThread.looper)
    private var surface: Surface? = null
    private var streamVolume: Float = 1.0f
    private var playbackGeneration: Long = 0L
    private var onVideoAvailable: (Long) -> Unit = {}
    private var onVideoUnavailable: (PlaybackUnavailable) -> Unit = {}
    private var onVideoFormatDiscovered: (Long, VideoFormatInfo) -> Unit = { _, _ -> }
    private var onSubtitlePes: (Long, String, ByteArray, CaptionTimestamp) -> Unit = { _, _, _, _ -> }
    private var onVideoOnlyFallbackRestarted: (VideoOnlyFallbackRestart) -> Unit = {}
    private var videoFilter: Filter? = null
    private var audioFilter: Filter? = null
    private var subtitleFilter: Filter? = null
    private var superimposeFilter: Filter? = null
    private var videoDecoder: VideoDecoderPipeline? = null
    private var audioDecoder: AudioDecoderPipeline? = null
    private var mediaSync: MediaSync? = null
    private var mediaSyncInputSurface: Surface? = null
    private var audioTrack: AudioTrack? = null
    private var mediaSyncStarted = false
    private var mediaSyncSurfaceFailed = false
    private var videoInputQueued = false
    private var audioInputQueued = false
    private var videoPathExpected = false
    private var audioPathExpected = false
    private var activeChannel: TunerController.ResolvedChannel? = null
    private var activeTuner: Tuner? = null
    private var activeSelection: TunerController.AvStreamSelection? = null
    private var waitingAvailabilityArm: AvailabilityArm? = null
    private var nextAvailabilityArmSequence: Long = 1L
    private var videoAvailabilityMode: VideoAvailabilityMode? = null
    private val ptsEpochCoordinator = PtsEpochCoordinator()
    private val outstandingAudioOutputs = linkedMapOf<Int, AudioOutput>()
    private var nextAudioBufferId = 1
    private var audioOutputBackpressureStartedAtMs: Long? = null
    private val videoAvailableNotified = AtomicBoolean(false)
    private var oversizedSamplesDropped: Int = 0
    private var malformedSamplesDropped: Int = 0
    private var decoderBackpressureDrops: Int = 0
    private var subtitleMissingPtsSamples: Int = 0
    private val released = AtomicBoolean(false)

    private enum class VideoAvailabilityMode {
        MEDIA_SYNC_FINAL_OUTPUT_EXACT,
        MEDIA_CODEC_TO_MEDIASYNC_INPUT_COMPAT,
    }

    enum class PlaybackUnavailableReason {
        SURFACE_DETACHED, SURFACE_NOT_SET, VIDEO_FILTER_NOT_STARTED, AUDIO_FILTER_NOT_STARTED,
        VIDEO_OUTPUT_RENDER_FAILED, VIDEO_CODEC_ERROR, CODEC_CONFIG_TIMEOUT, FIRST_FRAME_TIMEOUT, UNSUPPORTED_VIDEO_STREAM,
        UNSUPPORTED_AUDIO_STREAM, AUDIO_UNAVAILABLE, INVALID_MEDIA_TIMESTAMP, CAS_NO_KEY, UNKNOWN,
    }

    data class PlaybackUnavailable(
        val reason: PlaybackUnavailableReason,
        val detail: String = "",
        val generation: Long = 0L,
    )

    data class StartResult(
        val startedVideo: Boolean,
        val startedAudio: Boolean,
        val diagnostics: List<String> = emptyList(),
        val firstFramePending: Boolean = false,
        val generation: Long = -1L,
    ) {
        companion object {
            internal fun failedAfterRestart(generation: Long, diagnostics: List<String>) = StartResult(
                startedVideo = false,
                startedAudio = false,
                diagnostics = diagnostics,
                generation = generation,
            )
        }
    }

    data class AudioSwitchResult(
        val switchedAudio: Boolean,
        val diagnostics: List<String> = emptyList(),
        val firstFramePending: Boolean = false,
        val generation: Long = -1L,
    )

    data class VideoOnlyFallbackRestart(
        val originGeneration: Long,
        val result: StartResult,
    )

    enum class DualMonoPresentation { MAIN, SUB, MAIN_SUB }

    data class VideoFormatInfo(
        val streamType: Int,
        val mime: String,
        val width: Int,
        val height: Int,
        val displayAspectRatio: Double? = null,
    )

    private enum class VideoCodecKind(val streamType: Int, val mime: String) {
        MPEG2(0x02, MediaFormat.MIMETYPE_VIDEO_MPEG2),
        AVC(0x1b, MediaFormat.MIMETYPE_VIDEO_AVC),
        HEVC(0x24, MediaFormat.MIMETYPE_VIDEO_HEVC);

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

    private data class MediaSample(
        val event: MediaEvent,
        val block: MediaCodec.LinearBlock,
        val offset: Int,
        val size: Int,
        val presentationTimeUs: Long,
        val isAudio: Boolean,
    )

    private data class AvailabilityArm(
        val generation: Long,
        val armSequence: Long,
        val armedAtNanoTime: Long,
    )

    private data class AudioOutput(
        val codec: MediaCodec,
        val index: Int,
        val frame: MediaCodec.OutputFrame,
        val block: MediaCodec.LinearBlock,
        val bytes: ByteBuffer,
        val size: Int,
        val presentationTimeUs: Long,
    )

    private data class OutputPcmFormat(
        val sampleRate: Int,
        val channelCount: Int,
        val channelMask: Int,
    )

    data class MediaClockSnapshot(
        val mediaTimeUs: Long,
        val nanoTime: Long,
        val clockRate: Float,
    )

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

    fun setCallbacks(onAvailable: (Long) -> Unit, onUnavailable: (PlaybackUnavailable) -> Unit) {
        runOnPlaybackExecutorBlocking {
            onVideoAvailable = onAvailable
            onVideoUnavailable = onUnavailable
        }
    }

    fun setOnVideoFormatDiscoveredCallback(callback: (Long, VideoFormatInfo) -> Unit) {
        runOnPlaybackExecutorBlocking { onVideoFormatDiscovered = callback }
    }

    fun setOnSubtitlePesCallback(callback: (Long, String, ByteArray, CaptionTimestamp) -> Unit) {
        runOnPlaybackExecutorBlocking { onSubtitlePes = callback }
    }

    fun setOnVideoOnlyFallbackRestartedCallback(callback: (VideoOnlyFallbackRestart) -> Unit) {
        runOnPlaybackExecutorBlocking { onVideoOnlyFallbackRestarted = callback }
    }

    fun reportUnavailable(reason: PlaybackUnavailableReason, detail: String = "") {
        enqueuePlaybackAction { emitUnavailable(reason, detail) }
    }

    fun setVolume(volume: Float) {
        enqueuePlaybackAction { setVolumeOnPlaybackExecutor(volume) }
    }

    fun setDualMonoPresentation(presentation: DualMonoPresentation): Boolean = runOnPlaybackExecutorBlocking {
        audioDecoder?.setDualMonoPresentation(presentation) ?: false
    }

    private fun setVolumeOnPlaybackExecutor(volume: Float) {
        streamVolume = volume.coerceIn(0.0f, 1.0f)
        audioDecoder?.setVolume(streamVolume)
    }

    fun setSurface(newSurface: Surface?) {
        enqueuePlaybackAction { setSurfaceOnPlaybackExecutor(newSurface) }
    }

    private fun setSurfaceOnPlaybackExecutor(newSurface: Surface?) {
        if (surface !== newSurface && mediaSync != null) stopOnPlaybackExecutor()
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
        val audioOnly = channel.serviceType == SERVICE_TYPE_DIGITAL_AUDIO
        activeChannel = channel
        activeTuner = tuner
        activeSelection = selection
        val video = selection.video
        val videoKind = video?.let { VideoCodecKind.fromStreamType(it.streamType) }
        val audio = selection.audio
        val audioKind = audio?.let { stream -> AudioCodecKind.fromStreamType(stream.streamType) }
        if (!audioOnly && (video == null || videoKind == null)) {
            emitUnavailable(PlaybackUnavailableReason.UNSUPPORTED_VIDEO_STREAM, "audio-video service の対応video PIDがありません service=${selection.serviceKey}")
            return StartResult.failedAfterRestart(startGeneration, listOf("SERVICE_TYPE_PMT_MISMATCH"))
        }
        if (audioOnly && (audio == null || audioKind == null)) {
            emitUnavailable(PlaybackUnavailableReason.UNSUPPORTED_AUDIO_STREAM, "audio-only service の対応audio PIDがありません service=${selection.serviceKey}")
            return StartResult.failedAfterRestart(startGeneration, listOf("SERVICE_TYPE_PMT_MISMATCH"))
        }
        if (video != null && (currentSurface == null || !currentSurface.isValid)) {
            emitUnavailable(PlaybackUnavailableReason.SURFACE_NOT_SET, "有効な Surface がありません")
            return StartResult.failedAfterRestart(startGeneration, listOf("surface未設定"))
        }
        if (audio != null && audioKind == null) {
            Log.w(LogTags.TIS, "未対応 audio stream_type=0x${audio.streamType.toString(16)} のため video-only として開始します")
        }

        val diagnostics = mutableListOf<String>()
        val audioExpected = audio != null && audioKind != null
        videoPathExpected = video != null && videoKind != null
        audioPathExpected = audioExpected
        ptsEpochCoordinator.reset()
        val sync = createMediaSync(currentSurface?.takeIf { videoPathExpected }, startGeneration)
            ?: return StartResult.failedAfterRestart(startGeneration, listOf("MediaSync初期化失敗"))
        val videoDecoderLocal = if (video != null && videoKind != null) {
            VideoDecoderPipeline(videoKind, requireNotNull(mediaSyncInputSurface), startGeneration) { reason, detail ->
                emitUnavailableForGeneration(startGeneration, reason, detail)
            }.also { videoDecoder = it }
        } else {
            null
        }
        if (audioExpected) {
            audioDecoder = AudioDecoderPipeline(audioKind!!, selection.audioComponentType ?: requireNotNull(audio).componentType, selection.dualMonoPresentation, streamVolume, startGeneration) { reason, detail ->
                if (startGeneration == playbackGeneration) handleAudioFailure(reason, detail, audioOnly)
            }
        }

        if (video != null && videoDecoderLocal != null) {
            val openedVideo = createAndStartAvFilter(tuner, video, isAudio = false).getOrElse { error ->
                emitUnavailable(PlaybackUnavailableReason.VIDEO_FILTER_NOT_STARTED, error.message.orEmpty())
                diagnostics += "video filter start failed: ${error.message}"
                stopOnPlaybackExecutor()
                return StartResult.failedAfterRestart(startGeneration, diagnostics)
            }
            videoFilter = openedVideo
            diagnostics += "videoPid=${video.elementaryPid}"
            diagnostics += "videoCodec=$videoKind"
        }

        var audioStarted = false
        if (audio != null && audioKind != null) {
            val openedAudio = createAndStartAvFilter(tuner, audio, isAudio = true)
                .onFailure { error ->
                    logAudioUnavailable(PlaybackUnavailableReason.AUDIO_FILTER_NOT_STARTED, error.message.orEmpty())
                    diagnostics += "audio filter start failed; continuing video-only: ${error.message}"
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
                audioPathExpected = false
                audioInputQueued = false
                if (audioOnly) {
                    emitUnavailable(PlaybackUnavailableReason.AUDIO_FILTER_NOT_STARTED, "audio-only serviceのaudio filterを開始できません")
                    stopOnPlaybackExecutor()
                    return StartResult.failedAfterRestart(startGeneration, diagnostics)
                }
            }
        } else {
            diagnostics += "audio=absent-or-unsupported-video-only"
        }
        val subtitle = selection.subtitle
        if (subtitle != null) {
            val trackId = TunerSelectionPolicy.trackIdForSubtitle(subtitle, selection.subtitleLanguageId ?: 1)
            val openedSubtitle = createAndStartCaptionPesFilter(tuner, subtitle, trackId, superimpose = false)
                .onFailure { error -> diagnostics += "subtitle PES filter start failed: ${error.message}" }
                .getOrNull()
            if (openedSubtitle != null) {
                subtitleFilter = openedSubtitle
                diagnostics += "subtitlePid=${subtitle.elementaryPid}"
            }
        }
        selection.superimpose?.let { superimpose ->
            val trackId = TunerSelectionPolicy.trackIdForSuperimpose(superimpose)
            val openedSuperimpose = createAndStartCaptionPesFilter(tuner, superimpose, trackId, superimpose = true)
                .onFailure { error -> diagnostics += "superimpose PES filter start failed: ${error.message}" }
                .getOrNull()
            if (openedSuperimpose != null) {
                superimposeFilter = openedSuperimpose
                diagnostics += "superimposePid=${superimpose.elementaryPid}"
            }
        }
        diagnostics += "service=${selection.serviceKey}"
        diagnostics += "channel=${channel.displayNumber}"
        diagnostics += "volume=$streamVolume"
        diagnostics += "surfaceAttached=${currentSurface?.isValid == true}"
        if (videoPathExpected) scheduleFirstFrameTimeout(startGeneration)
        Log.i(LogTags.TIS, "AV filter とblock model decoderを開始しました inputId=$inputId sessionId=$sessionId mediaSync=${sync.hashCode()} ${diagnostics.joinToString(" ")}")
        return StartResult(
            startedVideo = false,
            startedAudio = audioStarted,
            diagnostics = diagnostics,
            firstFramePending = videoPathExpected,
            generation = startGeneration,
        )
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
        val channel = activeChannel ?: return AudioSwitchResult(false, listOf("current channel 未確定"))
        val audio = selection.audio ?: return AudioSwitchResult(false, listOf("audio PID 未検出"))
        AudioCodecKind.fromStreamType(audio.streamType) ?: run {
            emitUnavailable(PlaybackUnavailableReason.UNSUPPORTED_AUDIO_STREAM, "未対応 audio stream_type=0x${audio.streamType.toString(16)}")
            return AudioSwitchResult(false, listOf("audio stream_type 未対応"))
        }
        val restarted = startOnPlaybackExecutor(tuner, channel, selection)
        return AudioSwitchResult(
            switchedAudio = restarted.startedAudio,
            diagnostics = restarted.diagnostics + "MEDIASYNC_GENERATION_RECREATED_FOR_AUDIO_SWITCH",
            firstFramePending = restarted.firstFramePending,
            generation = restarted.generation,
        )
    }

    private fun createAndStartAvFilter(tuner: Tuner, stream: AribElementaryStream, isAudio: Boolean): Result<Filter> = runCatching {
        val pid = stream.elementaryPid
        val pidValue = pid.value
        val subtype = if (isAudio) Filter.SUBTYPE_AUDIO else Filter.SUBTYPE_VIDEO
        val filterGeneration = playbackGeneration
        val targetAudioDecoder = audioDecoder
        val targetVideoDecoder = videoDecoder
        fun sourceIsCurrent(filter: Filter): Boolean =
            filterGeneration == playbackGeneration && (if (isAudio) audioFilter else videoFilter) === filter
        val filter = tuner.openFilter(Filter.TYPE_TS, subtype, AV_FILTER_BUFFER_BYTES, executor, object : FilterCallback {
            override fun onFilterEvent(filter: Filter, events: Array<FilterEvent>) {
                runCatching {
                    if (!sourceIsCurrent(filter)) return
                    for (event in events.filterIsInstance<MediaEvent>()) {
                        if (!sourceIsCurrent(filter)) {
                            releaseMediaEvent(event)
                            continue
                        }
                        val sample = sampleFromEvent(event, isAudio)
                        if (sample == null) {
                            releaseMediaEvent(event)
                            continue
                        }
                        if (!sourceIsCurrent(filter)) {
                            releaseMediaEvent(event)
                            continue
                        }
                        val target = if (isAudio) targetAudioDecoder else targetVideoDecoder
                        if (target == null) {
                            releaseMediaEvent(event)
                            continue
                        }
                        target.queue(sample)
                    }
                }.onFailure { error ->
                    if (isAudio) {
                        logAudioUnavailable(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, error.message.orEmpty())
                    } else {
                        emitUnavailableForGeneration(filterGeneration, PlaybackUnavailableReason.VIDEO_CODEC_ERROR, error.message.orEmpty())
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
        val config = TsFilterConfiguration.builder().setTpid(pidValue).setSettings(settingsBuilder.build()).build()
        val configureResult = filter.configure(config)
        if (configureResult != Tuner.RESULT_SUCCESS) {
            closeFilter(filter)
            error("AV filter configure failed result=$configureResult pid=$pid isAudio=$isAudio")
        }
        if (isAudio) audioFilter = filter else videoFilter = filter
        val startResult = filter.start()
        if (startResult != Tuner.RESULT_SUCCESS) {
            if (isAudio && audioFilter === filter) audioFilter = null
            if (!isAudio && videoFilter === filter) videoFilter = null
            closeFilter(filter)
            error("AV filter start failed result=$startResult pid=$pid isAudio=$isAudio")
        }
        filter
    }

    private fun createAndStartCaptionPesFilter(
        tuner: Tuner,
        stream: AribElementaryStream,
        trackId: String,
        superimpose: Boolean,
    ): Result<Filter> = runCatching {
        val pid = stream.elementaryPid
        val pidValue = pid.value
        require(if (superimpose) TunerSelectionPolicy.isSuperimposeStream(stream) else TunerSelectionPolicy.isCaptionStream(stream)) {
            "caption kindとstreamが一致しません pid=$pid superimpose=$superimpose"
        }
        val filterGeneration = playbackGeneration
        fun sourceIsCurrent(filter: Filter): Boolean =
            filterGeneration == playbackGeneration &&
                (if (superimpose) superimposeFilter === filter else subtitleFilter === filter)
        val filter = tuner.openFilter(Filter.TYPE_TS, Filter.SUBTYPE_PES, SUBTITLE_FILTER_BUFFER_BYTES, executor, object : FilterCallback {
            override fun onFilterEvent(filter: Filter, events: Array<FilterEvent>) {
                runCatching {
                    if (!sourceIsCurrent(filter)) return
                    for (event in events.filterIsInstance<PesEvent>()) {
                        if (!sourceIsCurrent(filter)) continue
                        val dataLength = event.dataLength
                        if (dataLength <= 0 || dataLength > MAX_SUBTITLE_PES_BYTES) continue
                        val buffer = ByteArray(dataLength)
                        val read = filter.read(buffer, 0, dataLength.toLong())
                        if (read <= 0) continue
                        val pes = if (read == buffer.size) buffer else buffer.copyOf(read)
                        val captionSample = captionSampleFromPes(pes, superimpose) ?: continue
                        if (!sourceIsCurrent(filter)) continue
                        onSubtitlePes(
                            filterGeneration,
                            trackId,
                            captionSample.payload,
                            captionTimestampFrom(captionSample.pts90k, superimpose),
                        )
                    }
                }.onFailure { error ->
                    Log.w(LogTags.TIS, "caption PES filter callback に失敗しました inputId=$inputId pid=$pid superimpose=$superimpose", error)
                }
            }
            override fun onFilterStatusChanged(filter: Filter, status: Int) {
                Log.d(LogTags.TIS, "caption PES filter 状態 inputId=$inputId pid=$pid superimpose=$superimpose status=$status")
            }
        }) ?: error("openFilter が null を返しました caption pid=$pid")
        val settings = PesSettings.builder(Filter.TYPE_TS)
            .setStreamId(if (superimpose) PES_STREAM_ID_PRIVATE_STREAM_2 else PES_STREAM_ID_PRIVATE_STREAM_1)
            .build()
        val config = TsFilterConfiguration.builder().setTpid(pidValue).setSettings(settings).build()
        val configureResult = filter.configure(config)
        if (configureResult != Tuner.RESULT_SUCCESS) {
            closeFilter(filter)
            error("caption PES filter configure failed result=$configureResult pid=$pid")
        }
        if (superimpose) superimposeFilter = filter else subtitleFilter = filter
        val startResult = filter.start()
        if (startResult != Tuner.RESULT_SUCCESS) {
            if (superimpose && superimposeFilter === filter) superimposeFilter = null
            if (!superimpose && subtitleFilter === filter) subtitleFilter = null
            closeFilter(filter)
            error("caption PES filter start failed result=$startResult pid=$pid")
        }
        filter
    }

    private fun createMediaSync(outputSurface: Surface?, generation: Long): MediaSync? = runCatching {
        val sync = MediaSync()
        nextAvailabilityArmSequence = 1L
        sync.setCallback(object : MediaSync.Callback() {
            override fun onAudioBufferConsumed(sync: MediaSync, audioBuffer: ByteBuffer, bufferId: Int) {
                enqueuePlaybackAction { onAudioBufferConsumedOnPlaybackExecutor(sync, generation, bufferId) }
            }
        }, codecCallbackHandler)
        sync.setOnErrorListener(MediaSync.OnErrorListener { callbackSync, what, extra ->
            enqueuePlaybackAction { handleMediaSyncError(callbackSync, generation, what, extra) }
        }, codecCallbackHandler)
        if (outputSurface != null) {
            sync.setSurface(outputSurface)
            mediaSyncInputSurface = sync.createInputSurface()
        }
        mediaSync = sync
        mediaSyncSurfaceFailed = false
        if (outputSurface != null) armVideoAvailability(sync, generation)
        sync
    }.onFailure { error ->
        Log.w(LogTags.TIS, "MediaSync 初期化に失敗しました inputId=$inputId generation=$generation", error)
        stopOnPlaybackExecutor()
        emitUnavailable(PlaybackUnavailableReason.VIDEO_CODEC_ERROR, error.message.orEmpty())
    }.getOrNull()

    private fun allocateAvailabilityArmSequence(): Long {
        val armSequence = nextAvailabilityArmSequence
        check(armSequence > 0L && armSequence < Long.MAX_VALUE) { "MediaSync availability arm sequence exhausted" }
        nextAvailabilityArmSequence = armSequence + 1L
        return armSequence
    }

    private fun armVideoAvailability(sync: MediaSync, generation: Long) {
        val arm = AvailabilityArm(
            generation = generation,
            armSequence = allocateAvailabilityArmSequence(),
            armedAtNanoTime = System.nanoTime(),
        )
        waitingAvailabilityArm = arm
        videoAvailableNotified.set(false)
        val exactArmed = MediaSyncFirstOutputBridge.isAvailable() && MediaSyncFirstOutputBridge.arm(
            sync = sync,
            armSequence = arm.armSequence,
            handler = codecCallbackHandler,
        ) { callbackSync, armSequence ->
            enqueuePlaybackAction { commitVideoAvailability(callbackSync, generation, armSequence) }
        }
        videoAvailabilityMode = if (exactArmed) VideoAvailabilityMode.MEDIA_SYNC_FINAL_OUTPUT_EXACT else VideoAvailabilityMode.MEDIA_CODEC_TO_MEDIASYNC_INPUT_COMPAT
        Log.i(LogTags.TIS, "video availability mode=$videoAvailabilityMode inputId=$inputId generation=$generation armSequence=${arm.armSequence}")
    }

    private fun commitVideoAvailability(sync: MediaSync, generation: Long, armSequence: Long) {
        if (videoAvailabilityMode != VideoAvailabilityMode.MEDIA_SYNC_FINAL_OUTPUT_EXACT) return
        val arm = waitingAvailabilityArm ?: return
        if (sync !== mediaSync || generation != playbackGeneration || arm.generation != generation || arm.armSequence != armSequence || mediaSyncSurfaceFailed || surface?.isValid != true) return
        waitingAvailabilityArm = null
        if (videoAvailableNotified.compareAndSet(false, true)) onVideoAvailable(generation)
    }

    private fun commitCompatibilityVideoAvailability(generation: Long, frameNanoTime: Long) {
        if (videoAvailabilityMode != VideoAvailabilityMode.MEDIA_CODEC_TO_MEDIASYNC_INPUT_COMPAT) return
        val arm = waitingAvailabilityArm ?: return
        if (generation != playbackGeneration || arm.generation != generation || frameNanoTime < arm.armedAtNanoTime || mediaSyncSurfaceFailed || surface?.isValid != true) return
        waitingAvailabilityArm = null
        if (videoAvailableNotified.compareAndSet(false, true)) onVideoAvailable(generation)
    }

    private fun onAudioBufferConsumedOnPlaybackExecutor(sync: MediaSync, generation: Long, bufferId: Int) {
        val output = outstandingAudioOutputs.remove(bufferId) ?: return
        runCatching { output.codec.releaseOutputBuffer(output.index, false) }
            .onFailure { Log.w(LogTags.TIS, "consumed audio outputのreleaseに失敗しました bufferId=$bufferId", it) }
        audioOutputBackpressureStartedAtMs = null
        if (sync !== mediaSync || generation != playbackGeneration) return
    }

    private fun handleMediaSyncError(sync: MediaSync, generation: Long, what: Int, extra: Int) {
        if (sync !== mediaSync || generation != playbackGeneration) return
        when (what) {
            MediaSync.MEDIASYNC_ERROR_SURFACE_FAIL -> {
                if (!videoPathExpected) return
                mediaSyncSurfaceFailed = true
                waitingAvailabilityArm = null
                emitUnavailable(PlaybackUnavailableReason.VIDEO_OUTPUT_RENDER_FAILED, "MEDIASYNC_ERROR_SURFACE_FAIL extra=$extra")
            }
            MediaSync.MEDIASYNC_ERROR_AUDIOTRACK_FAIL -> {
                if (!audioPathExpected) return
                handleAudioFailure(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "MEDIASYNC_ERROR_AUDIOTRACK_FAIL extra=$extra", activeChannel?.serviceType == SERVICE_TYPE_DIGITAL_AUDIO)
            }
            else -> emitUnavailable(PlaybackUnavailableReason.UNKNOWN, "MediaSync error what=$what extra=$extra")
        }
    }

    private fun handleAudioFailure(reason: PlaybackUnavailableReason, detail: String, audioOnly: Boolean) {
        val originGeneration = playbackGeneration
        val restartTuner = activeTuner
        val restartChannel = activeChannel
        val restartSelection = activeSelection
        releaseOutstandingAudioOutputs()
        audioDecoder?.close()
        audioDecoder = null
        runCatching { audioTrack?.release() }
        audioTrack = null
        audioPathExpected = false
        audioInputQueued = false
        if (audioOnly) {
            emitUnavailable(reason, detail)
            stopOnPlaybackExecutor()
            return
        }
        logAudioUnavailable(reason, "$detail; recreating MediaSync generation for video-only fallback")
        if (restartTuner == null || restartChannel == null || restartSelection?.video == null) {
            emitUnavailable(reason, "$detail; video-only restart context is unavailable")
            stopOnPlaybackExecutor()
            return
        }
        val restarted = startOnPlaybackExecutor(restartTuner, restartChannel, restartSelection.copy(audio = null))
        onVideoOnlyFallbackRestarted(VideoOnlyFallbackRestart(originGeneration, restarted))
        if (restarted.generation < 0L || (!restarted.firstFramePending && !restarted.startedVideo)) emitUnavailable(reason, "$detail; video-only generation restart failed")
    }

    private fun onCompressedInputQueued(sample: MediaSample) {
        if (sample.isAudio) audioInputQueued = true else videoInputQueued = true
        maybeStartMediaSync()
    }

    private fun maybeStartMediaSync() {
        val sync = mediaSync ?: return
        if (mediaSyncStarted) return
        if (videoPathExpected && !videoInputQueued) return
        if (audioPathExpected && !audioInputQueued) return
        if (audioPathExpected && audioTrack == null) return
        runCatching { sync.setPlaybackParams(PlaybackParams().setSpeed(1.0f)) }
            .onSuccess { mediaSyncStarted = true }
            .onFailure { error ->
                val reason = if (videoPathExpected) PlaybackUnavailableReason.VIDEO_CODEC_ERROR else PlaybackUnavailableReason.AUDIO_UNAVAILABLE
                emitUnavailable(reason, "MediaSync playback start failed: ${error.message}")
            }
    }

    fun currentMediaClockSnapshot(): MediaClockSnapshot? = runOnPlaybackExecutorBlocking {
        val timestamp: MediaTimestamp = mediaSync?.timestamp ?: return@runOnPlaybackExecutorBlocking null
        MediaClockSnapshot(timestamp.anchorMediaTimeUs, timestamp.anchorSytemNanoTime, timestamp.mediaClockRate)
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
    fun oversizedSamplesDroppedForDiagnostic(): Int = oversizedSamplesDropped
    fun malformedSamplesDroppedForDiagnostic(): Int = malformedSamplesDropped
    fun decoderBackpressureDropsForDiagnostic(): Int = decoderBackpressureDrops
    fun subtitleMissingPtsSamplesForDiagnostic(): Int = subtitleMissingPtsSamples

    fun simulateFirstFrameRenderedForTest(generation: Long) {
        enqueuePlaybackAction {
            val sync = mediaSync ?: return@enqueuePlaybackAction
            val armSequence = waitingAvailabilityArm?.armSequence ?: return@enqueuePlaybackAction
            commitVideoAvailability(sync, generation, armSequence)
        }
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
        if (length > Int.MAX_VALUE.toLong() || offset > Int.MAX_VALUE.toLong()) {
            oversizedSamplesDropped++
            Log.w(LogTags.TIS, "MediaEvent range をblock modelのInt範囲へ安全にnarrowできません offset=$offset length=$length")
            return null
        }
        if (event.isSecureMemory) {
            if (isAudio) logAudioUnavailable(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "audio secure MediaEvent は clear playback 対象外です") else emitUnavailable(PlaybackUnavailableReason.VIDEO_CODEC_ERROR, "video secure MediaEvent は clear playback 対象外です")
            return null
        }
        val block = event.linearBlock ?: return null
        val capacity = runCatching { block.map().duplicate().capacity().toLong() }
            .onFailure { Log.w(LogTags.TIS, "MediaEvent LinearBlock の map に失敗しました", it) }.getOrNull() ?: return null
        if (end > capacity) {
            malformedSamplesDropped++
            Log.w(LogTags.TIS, "MediaEvent が LinearBlock 範囲外です offset=$offset length=$length capacity=$capacity")
            return null
        }
        val rawPts = event.pts
        if (!isAuthoritativePtsValid(rawPts)) {
            malformedSamplesDropped++
            val detail = "producer-authoritative MediaEvent PTS is invalid pts90k=$rawPts isAudio=$isAudio"
            if (isAudio) Log.w(LogTags.TIS, "$detail; playback profile contract violated") else emitUnavailable(PlaybackUnavailableReason.INVALID_MEDIA_TIMESTAMP, detail)
            return null
        }
        val track = if (isAudio) PtsTrack.AUDIO else PtsTrack.VIDEO
        val presentationTimeUs = ptsEpochCoordinator.normalize(track, rawPts)
        return MediaSample(event, block, offset.toInt(), length.toInt(), presentationTimeUs, isAudio)
    }

    private abstract inner class DecoderPipeline : AutoCloseable {
        protected var codec: MediaCodec? = null
        private val configBytes = ByteArrayOutputStream()
        private val pendingSamples = java.util.ArrayDeque<MediaSample>()
        private val availableInputIndexes = java.util.ArrayDeque<Int>()
        protected abstract val budget: PlaybackBudget
        protected abstract val generation: Long
        private var firstOutputSeen = false
        private var backpressureStartedAtMs: Long? = null

        fun queue(sample: MediaSample) {
            try {
                if (sample.size > budget.singleEventLimitBytes) {
                    oversizedSamplesDropped++
                    releaseMediaEvent(sample.event)
                    onSampleRejected("SAMPLE_TOO_LARGE size=${sample.size} limit=${budget.singleEventLimitBytes}")
                    return
                }
                val queueBudget = if (codec == null) budget.startup else budget.pending
                if (!withinQueueBudget(sample, queueBudget)) {
                    decoderBackpressureDrops++
                    releaseMediaEvent(sample.event)
                    val nowMs = SystemClock.elapsedRealtime()
                    val startedAtMs = backpressureStartedAtMs ?: nowMs.also { backpressureStartedAtMs = it }
                    val deadlineMs = if (firstOutputSeen) budget.steadyBackpressureDeadlineMs else budget.decoderStartupDeadlineMs
                    val reason = "PENDING_QUEUE_FULL bytes=${pendingSamples.sumOf { it.size }} samples=${pendingSamples.size} blockedMs=${nowMs - startedAtMs} deadlineMs=$deadlineMs"
                    onSampleRejected(reason)
                    if (backpressureDeadlineReached(startedAtMs, nowMs, deadlineMs)) {
                        onBackpressureDeadline(reason)
                        close()
                    }
                    return
                }
                pendingSamples.addLast(sample)
                queueInternal(sample)
            } catch (e: RuntimeException) {
                Log.w(LogTags.TIS, "decoder queue/drain に失敗しました", e)
                onDecoderFailure(e)
                close()
            }
        }

        private fun queueInternal(sample: MediaSample) {
            if (codec == null) {
                appendHeaderProbe(sample)
                val format = formatFromBufferedHeader(configBytes.toByteArray())
                if (format == null) {
                    if (configBytes.size() >= budget.headerProbeLimitBytes) {
                        onCodecConfigTimeout()
                        clearPending()
                    }
                    return
                }
                codec = createBlockModelDecoder(format)
            }
            drainPendingInput()
        }

        private fun withinQueueBudget(sample: MediaSample, queueBudget: QueueBudget): Boolean {
            if (pendingSamples.size + 1 > queueBudget.maxSamples) return false
            if (pendingSamples.sumOf { it.size.toLong() } + sample.size > queueBudget.maxBytes) return false
            val times = pendingSamples.map { it.presentationTimeUs } + sample.presentationTimeUs
            return timestampSpanUs(times) <= queueBudget.maxDurationUs
        }

        private fun appendHeaderProbe(sample: MediaSample) {
            val remaining = budget.headerProbeLimitBytes - configBytes.size()
            if (remaining <= 0) return
            val copyLength = minOf(sample.size, remaining)
            val mapped = sample.block.map().duplicate()
            mapped.position(sample.offset)
            mapped.limit(sample.offset + copyLength)
            val headerBytes = ByteArray(copyLength)
            mapped.get(headerBytes)
            configBytes.write(headerBytes)
        }

        private fun createBlockModelDecoder(format: MediaFormat): MediaCodec {
            val decoder = MediaCodec.createDecoderByType(codecMime())
            decoder.setCallback(object : MediaCodec.Callback() {
                override fun onInputBufferAvailable(codec: MediaCodec, index: Int) {
                    enqueuePlaybackAction {
                        if (generation != playbackGeneration || this@DecoderPipeline.codec !== codec) return@enqueuePlaybackAction
                        availableInputIndexes.addLast(index)
                        drainPendingInput()
                    }
                }
                override fun onOutputBufferAvailable(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
                    enqueuePlaybackAction {
                        if (generation != playbackGeneration || this@DecoderPipeline.codec !== codec) {
                            runCatching { codec.releaseOutputBuffer(index, false) }
                            return@enqueuePlaybackAction
                        }
                        if (info.size > 0) {
                            firstOutputSeen = true
                            backpressureStartedAtMs = null
                        }
                        onOutput(codec, index, info)
                    }
                }
                override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) {
                    enqueuePlaybackAction {
                        if (generation == playbackGeneration && this@DecoderPipeline.codec === codec) onOutputFormatChanged(format)
                    }
                }
                override fun onError(codec: MediaCodec, error: MediaCodec.CodecException) {
                    enqueuePlaybackAction {
                        if (generation == playbackGeneration && this@DecoderPipeline.codec === codec) onDecoderFailure(error)
                    }
                }
            }, codecCallbackHandler)
            try {
                decoder.configure(format, decoderOutputSurface(), null, MediaCodec.CONFIGURE_FLAG_USE_BLOCK_MODEL)
                onDecoderPrepared(decoder)
                decoder.start()
                onDecoderConfigured(format)
                return decoder
            } catch (error: RuntimeException) {
                runCatching { decoder.release() }
                throw error
            }
        }

        private fun drainPendingInput() {
            val decoder = codec ?: return
            while (pendingSamples.isNotEmpty() && availableInputIndexes.isNotEmpty()) {
                val inputIndex = availableInputIndexes.removeFirst()
                val sample = pendingSamples.removeFirst()
                try {
                    decoder.getQueueRequest(inputIndex)
                        .setLinearBlock(sample.block, sample.offset, sample.size)
                        .setPresentationTimeUs(sample.presentationTimeUs)
                        .queue()
                    backpressureStartedAtMs = null
                    releaseMediaEvent(sample.event)
                    onCompressedInputQueued(sample)
                } catch (error: RuntimeException) {
                    releaseMediaEvent(sample.event)
                    throw error
                }
            }
        }

        protected abstract fun codecMime(): String
        protected abstract fun formatFromBufferedHeader(bytes: ByteArray): MediaFormat?
        protected abstract fun decoderOutputSurface(): Surface?
        protected open fun onDecoderPrepared(codec: MediaCodec) = Unit
        protected open fun onDecoderConfigured(format: MediaFormat) = Unit
        protected open fun onOutputFormatChanged(format: MediaFormat) = Unit
        protected abstract fun onOutput(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo)
        protected abstract fun onDecoderFailure(error: RuntimeException)
        protected abstract fun onCodecConfigTimeout()
        protected abstract fun onBackpressureDeadline(detail: String)
        protected open fun onSampleRejected(reason: String) { Log.w(LogTags.TIS, "decoder sampleを拒否しました reason=$reason") }

        private fun clearPending() {
            while (pendingSamples.isNotEmpty()) releaseMediaEvent(pendingSamples.removeFirst().event)
        }

        override fun close() {
            clearPending()
            availableInputIndexes.clear()
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
        override val generation: Long,
        private val errorSink: (PlaybackUnavailableReason, String) -> Unit,
    ) : DecoderPipeline() {
        override val budget = PlaybackBudget.forVideo(kind)
        override fun codecMime(): String = kind.mime
        override fun decoderOutputSurface(): Surface = outputSurface
        override fun onDecoderPrepared(codec: MediaCodec) {
            codec.setOnFrameRenderedListener(MediaCodec.OnFrameRenderedListener { callbackCodec, _, nanoTime ->
                enqueuePlaybackAction {
                    if (generation != playbackGeneration || this@VideoDecoderPipeline.codec !== callbackCodec) return@enqueuePlaybackAction
                    commitCompatibilityVideoAvailability(generation, nanoTime)
                }
            }, codecCallbackHandler)
        }
        override fun formatFromBufferedHeader(bytes: ByteArray): MediaFormat? = when (kind) {
            VideoCodecKind.MPEG2 -> EsHeaderParser.mpeg2VideoFormat(bytes)
            VideoCodecKind.AVC -> EsHeaderParser.avcVideoFormat(bytes)
            VideoCodecKind.HEVC -> EsHeaderParser.hevcVideoFormat(bytes)
        }?.also { format ->
            onVideoFormatDiscovered(generation, VideoFormatInfo(kind.streamType, kind.mime, getIntegerOrDefault(format, MediaFormat.KEY_WIDTH, 0), getIntegerOrDefault(format, MediaFormat.KEY_HEIGHT, 0), EsHeaderParser.displayAspectRatio(kind, bytes)))
        }
        override fun onDecoderFailure(error: RuntimeException) { errorSink(PlaybackUnavailableReason.VIDEO_CODEC_ERROR, error.message.orEmpty()) }
        override fun onCodecConfigTimeout() { errorSink(PlaybackUnavailableReason.CODEC_CONFIG_TIMEOUT, "video decoder 構成に必要な ES header が見つかりません") }
        override fun onBackpressureDeadline(detail: String) { errorSink(PlaybackUnavailableReason.VIDEO_CODEC_ERROR, "DECODER_BACKPRESSURE_TIMEOUT $detail") }
        override fun onOutput(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            if (info.size <= 0) { codec.releaseOutputBuffer(index, false); return }
            if (!outputSurface.isValid) {
                codec.releaseOutputBuffer(index, false)
                errorSink(PlaybackUnavailableReason.VIDEO_OUTPUT_RENDER_FAILED, "video output Surface が無効です")
                return
            }
            codec.releaseOutputBuffer(index, info.presentationTimeUs * 1_000L)
        }
    }

    private inner class AudioDecoderPipeline(
        private val kind: AudioCodecKind,
        private val componentType: Int?,
        initialDualMonoPresentation: DualMonoPresentation,
        initialVolume: Float,
        override val generation: Long,
        private val errorSink: (PlaybackUnavailableReason, String) -> Unit,
    ) : DecoderPipeline() {
        private var volume: Float = initialVolume
        private var dualMonoPresentation: DualMonoPresentation = initialDualMonoPresentation
        private val isDualMonoStream: Boolean = isAribDualMonoComponentType(componentType)
        private var outputSampleRate: Int = DEFAULT_AUDIO_SAMPLE_RATE
        private var outputChannels: Int = DEFAULT_AUDIO_CHANNEL_COUNT
        private var outputPcmFormat: OutputPcmFormat? = null
        override val budget = PlaybackBudget.forAudio(kind)

        fun setVolume(value: Float) { volume = value; audioTrack?.let { track -> runCatching { track.setVolume(value) } } }
        fun setDualMonoPresentation(presentation: DualMonoPresentation): Boolean {
            if (!isDualMonoStream) return false
            dualMonoPresentation = presentation
            val track = audioTrack ?: return true
            return track.setDualMonoMode(audioTrackDualMonoMode(presentation))
        }
        override fun codecMime(): String = kind.mime
        override fun decoderOutputSurface(): Surface? = null
        override fun formatFromBufferedHeader(bytes: ByteArray): MediaFormat? = when (kind) {
            AudioCodecKind.AAC_ADTS -> EsHeaderParser.adtsAacFormat(bytes)
            AudioCodecKind.MPEG1, AudioCodecKind.MPEG2 -> EsHeaderParser.mpegAudioFormat(bytes)
        }
        override fun onDecoderConfigured(format: MediaFormat) {
            outputSampleRate = getIntegerOrDefault(format, MediaFormat.KEY_SAMPLE_RATE, DEFAULT_AUDIO_SAMPLE_RATE)
            outputChannels = getIntegerOrDefault(format, MediaFormat.KEY_CHANNEL_COUNT, DEFAULT_AUDIO_CHANNEL_COUNT)
        }
        override fun onDecoderFailure(error: RuntimeException) { errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, error.message.orEmpty()) }
        override fun onCodecConfigTimeout() { errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "audio decoder 構成に必要な ES header が見つかりません") }
        override fun onBackpressureDeadline(detail: String) { errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "DECODER_BACKPRESSURE_TIMEOUT $detail") }
        override fun onOutputFormatChanged(format: MediaFormat) {
            val sampleRate = getIntegerOrDefault(format, MediaFormat.KEY_SAMPLE_RATE, outputSampleRate)
            val channelCount = getIntegerOrDefault(format, MediaFormat.KEY_CHANNEL_COUNT, outputChannels)
            val decoderMask = if (format.containsKey(MediaFormat.KEY_CHANNEL_MASK)) format.getInteger(MediaFormat.KEY_CHANNEL_MASK) else null
            val channelMask = PcmChannelMaskPolicy.resolve(decoderMask, channelCount, componentType)
            if (channelMask == null) {
                errorSink(
                    PlaybackUnavailableReason.AUDIO_UNAVAILABLE,
                    "decoded PCM channel topology is inconsistent channelCount=$channelCount decoderMask=$decoderMask componentType=$componentType",
                )
                return
            }
            if (decoderMask == null && kind == AudioCodecKind.AAC_ADTS && channelCount > 2) {
                Log.w(LogTags.TIS, "CDD C-7-2 violation: default AAC multichannel decoder output lacks KEY_CHANNEL_MASK channelCount=$channelCount")
            }
            val next = OutputPcmFormat(sampleRate, channelCount, channelMask)
            val previous = outputPcmFormat
            if (previous != null && previous != next) {
                restartForOutputFormatChange(previous, next)
                return
            }
            outputSampleRate = sampleRate
            outputChannels = channelCount
            outputPcmFormat = next
            ensureAudioTrack(channelMask)
        }
        override fun onOutput(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            if (info.size <= 0) { codec.releaseOutputBuffer(index, false); return }
            val frame = codec.getOutputFrame(index)
            val block = frame.linearBlock ?: run {
                codec.releaseOutputBuffer(index, false)
                errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "block model audio outputにLinearBlockがありません")
                return
            }
            val mapped = block.map().duplicate()
            val end = info.offset.toLong() + info.size.toLong()
            if (info.offset < 0 || info.size <= 0 || end > mapped.capacity().toLong()) {
                codec.releaseOutputBuffer(index, false)
                errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "audio OutputFrame rangeが不正です")
                return
            }
            mapped.position(info.offset)
            mapped.limit(info.offset + info.size)
            val bytes = mapped.slice().asReadOnlyBuffer()
            val outputClaim = claimAudioOutput(info.size, info.presentationTimeUs, budget.pending, budget.steadyBackpressureDeadlineMs)
            if (!outputClaim.accepted) {
                codec.releaseOutputBuffer(index, false)
                if (outputClaim.deadlineReached) errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, outputClaim.detail)
                return
            }
            val bufferId = nextAudioBufferId++
            outstandingAudioOutputs[bufferId] = AudioOutput(codec, index, frame, block, bytes, info.size, info.presentationTimeUs)
            try {
                requireNotNull(mediaSync).queueAudio(bytes, bufferId, info.presentationTimeUs)
            } catch (error: RuntimeException) {
                outstandingAudioOutputs.remove(bufferId)
                audioOutputBackpressureStartedAtMs = null
                codec.releaseOutputBuffer(index, false)
                errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, error.message.orEmpty())
            }
        }
        private fun ensureAudioTrack(channelMask: Int) {
            if (audioTrack != null) return
            val minBuffer = AudioTrack.getMinBufferSize(outputSampleRate, channelMask, AudioFormat.ENCODING_PCM_16BIT).coerceAtLeast(32 * 1024)
            val builder = AudioTrack.Builder()
                .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build())
                .setAudioFormat(AudioFormat.Builder().setSampleRate(outputSampleRate).setChannelMask(channelMask).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
                .setBufferSizeInBytes(minBuffer)
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setContext(requireNotNull(sessionContext) { "sessionContext is required for AudioTrack" })
            val created = builder.build()
            created.setVolume(volume)
            if (isDualMonoStream && !created.setDualMonoMode(audioTrackDualMonoMode(dualMonoPresentation))) {
                created.release()
                throw IllegalStateException("ARIB dual-mono presentationをAudioTrackへ設定できません componentType=$componentType")
            }
            requireNotNull(mediaSync).setAudioTrack(created)
            audioTrack = created
            maybeStartMediaSync()
        }

        private fun restartForOutputFormatChange(previous: OutputPcmFormat, next: OutputPcmFormat) {
            if (generation != playbackGeneration) return
            val tuner = activeTuner
            val channel = activeChannel
            val selection = activeSelection
            if (tuner == null || channel == null || selection == null) {
                errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "audio output format changed without restart context old=$previous new=$next")
                return
            }
            Log.i(LogTags.TIS, "audio output format changed; rebuilding playback generation old=$previous new=$next")
            startOnPlaybackExecutor(tuner, channel, selection)
        }
    }

    enum class MediaEventBoundsDecision { ACCEPT, MALFORMED, OVERSIZED, OUT_OF_BOUNDS }
    private data class CaptionPesSample(val payload: ByteArray, val pts90k: Long?)

    private fun captionTimestampFrom(rawPts90k: Long?, superimpose: Boolean): CaptionTimestamp {
        val rawPts = rawPts90k?.takeIf { it in 0..PTS_MASK }
        val timestamp = rawPts?.let { raw -> CaptionTimestamp.Pts(com.maleicacid.tvinput.common.CaptionPtsMillis(ptsEpochCoordinator.normalize(if (superimpose) PtsTrack.SUPERIMPOSE else PtsTrack.CAPTION, raw) / 1_000L)) } ?: CaptionTimestamp.NoPts
        if (timestamp == CaptionTimestamp.NoPts) subtitleMissingPtsSamples++
        return timestamp
    }

    private fun captionSampleFromPes(pes: ByteArray, superimpose: Boolean): CaptionPesSample? {
        if (pes.size < 6 || pes[0] != 0.toByte() || pes[1] != 0.toByte() || pes[2] != 1.toByte()) return null
        val expectedStreamId = if (superimpose) PES_STREAM_ID_PRIVATE_STREAM_2 else PES_STREAM_ID_PRIVATE_STREAM_1
        if (pes[3].toInt() and 0xff != expectedStreamId) return null
        if (superimpose) {
            val payload = pes.copyOfRange(6, pes.size)
            if (!validCaptionDataHeader(payload, DATA_IDENTIFIER_SUPERIMPOSE)) return null
            return CaptionPesSample(payload, null)
        }
        if (pes.size < 9) return null
        val flags1 = pes[6].toInt() and 0xff
        val flags2 = pes[7].toInt() and 0xff
        if (flags1 and 0xc0 != 0x80) return null
        val ptsDtsFlags = (flags2 ushr 6) and 0x03
        if (ptsDtsFlags == 0x01) return null
        val headerLength = pes[8].toInt() and 0xff
        val payloadStart = 9 + headerLength
        if (payloadStart > pes.size) return null
        val pts90k = if (ptsDtsFlags == 0x02 || ptsDtsFlags == 0x03) {
            if (headerLength < 5 || pes.size < 14) return null
            val b0 = pes[9].toInt() and 0xff
            val b1 = pes[10].toInt() and 0xff
            val b2 = pes[11].toInt() and 0xff
            val b3 = pes[12].toInt() and 0xff
            val b4 = pes[13].toInt() and 0xff
            val expectedPrefix = if (ptsDtsFlags == 0x02) 0x20 else 0x30
            if (b0 and 0xf0 != expectedPrefix || b0 and 0x01 != 1 || b2 and 0x01 != 1 || b4 and 0x01 != 1) return null
            ((b0.toLong() and 0x0eL) shl 29) or (b1.toLong() shl 22) or ((b2.toLong() and 0xfeL) shl 14) or (b3.toLong() shl 7) or ((b4.toLong() and 0xfeL) ushr 1)
        } else null
        val payload = pes.copyOfRange(payloadStart, pes.size)
        if (!validCaptionDataHeader(payload, DATA_IDENTIFIER_CAPTION)) return null
        return CaptionPesSample(payload, pts90k)
    }

    private fun validCaptionDataHeader(payload: ByteArray, expectedDataIdentifier: Int): Boolean =
        payload.size >= 3 &&
            payload[0].toInt() and 0xff == expectedDataIdentifier &&
            payload[1].toInt() and 0xff == CAPTION_PRIVATE_STREAM_ID

    private enum class PtsTrack { VIDEO, AUDIO, CAPTION, SUPERIMPOSE }
    private fun isAuthoritativePtsValid(pts90k: Long): Boolean = pts90k in 0..PTS_MASK

    private class PtsEpochCoordinator {
        private data class TrackState(var rawPrevious: Long, var extendedPrevious: Long)
        private data class SharedReference(var raw: Long, var extended: Long)
        private val tracks = linkedMapOf<PtsTrack, TrackState>()
        private var sharedReference: SharedReference? = null
        fun reset() { tracks.clear(); sharedReference = null }
        fun normalize(track: PtsTrack, rawPts: Long): Long = Math.multiplyExact(normalizeTicks(track, rawPts), 1_000_000L) / 90_000L
        fun normalizeTicks(track: PtsTrack, rawPts: Long): Long {
            require(rawPts in 0..PTS_MASK)
            val state = tracks[track]
            val extended = if (state == null) {
                val reference = sharedReference
                if (reference == null) PTS_HALF else reference.extended + signedDelta(reference.raw, rawPts)
            } else state.extendedPrevious + signedDelta(state.rawPrevious, rawPts)
            tracks[track] = TrackState(rawPts, extended)
            val reference = sharedReference
            if (reference == null || extended > reference.extended) sharedReference = SharedReference(rawPts, extended)
            return extended
        }
        private fun signedDelta(previousRaw: Long, raw: Long): Long = Math.floorMod(raw - previousRaw + PTS_HALF, PTS_MODULUS) - PTS_HALF
    }

    private data class QueueBudget(val maxBytes: Long, val maxSamples: Int, val maxDurationUs: Long)
    private data class AudioOutputClaimDecision(val accepted: Boolean, val deadlineReached: Boolean = false, val detail: String = "")
    private fun claimAudioOutput(size: Int, presentationTimeUs: Long, queueBudget: QueueBudget, deadlineMs: Long): AudioOutputClaimDecision {
        val byteCount = outstandingAudioOutputs.values.fold(size.toLong()) { total, output ->
            val outputSize = output.size.toLong()
            if (total > Long.MAX_VALUE - outputSize) Long.MAX_VALUE else total + outputSize
        }
        val sampleCount = outstandingAudioOutputs.size + 1
        val timestamps = outstandingAudioOutputs.values.map { it.presentationTimeUs } + presentationTimeUs
        val durationUs = timestampSpanUs(timestamps)
        if (byteCount <= queueBudget.maxBytes && sampleCount <= queueBudget.maxSamples && durationUs <= queueBudget.maxDurationUs) {
            audioOutputBackpressureStartedAtMs = null
            return AudioOutputClaimDecision(accepted = true)
        }
        decoderBackpressureDrops++
        val nowMs = SystemClock.elapsedRealtime()
        val startedAtMs = audioOutputBackpressureStartedAtMs ?: nowMs.also { audioOutputBackpressureStartedAtMs = it }
        val deadlineReached = backpressureDeadlineReached(startedAtMs, nowMs, deadlineMs)
        return AudioOutputClaimDecision(false, deadlineReached, "AUDIO_OUTPUT_BACKPRESSURE_TIMEOUT bytes=$byteCount samples=$sampleCount durationUs=$durationUs deadlineMs=$deadlineMs")
    }
    private fun backpressureDeadlineReached(startedAtMs: Long, nowMs: Long, deadlineMs: Long): Boolean = backpressureDeadlineReachedForTest(startedAtMs, nowMs, deadlineMs)
    private fun timestampSpanUs(timestamps: List<Long>): Long {
        val minimum = timestamps.minOrNull() ?: return 0L
        val maximum = timestamps.maxOrNull() ?: return 0L
        return runCatching { Math.subtractExact(maximum, minimum) }.getOrDefault(Long.MAX_VALUE)
    }

    private data class PlaybackBudget(
        val singleEventLimitBytes: Int,
        val headerProbeLimitBytes: Int,
        val startup: QueueBudget,
        val pending: QueueBudget,
        val decoderStartupDeadlineMs: Long,
        val steadyBackpressureDeadlineMs: Long,
    ) {
        companion object {
            fun forVideo(kind: VideoCodecKind): PlaybackBudget = when (kind) {
                VideoCodecKind.MPEG2 -> PlaybackBudget(6 * MIB, 256 * KIB, QueueBudget(24L * MIB, 24, 2_000_000L), QueueBudget(36L * MIB, 48, 3_000_000L), 4_000L, 2_000L)
                VideoCodecKind.AVC -> PlaybackBudget(12 * MIB, 512 * KIB, QueueBudget(48L * MIB, 32, 2_500_000L), QueueBudget(72L * MIB, 64, 3_500_000L), 5_000L, 2_000L)
                VideoCodecKind.HEVC -> PlaybackBudget(16 * MIB, 768 * KIB, QueueBudget(64L * MIB, 32, 2_500_000L), QueueBudget(96L * MIB, 64, 3_500_000L), 5_000L, 2_000L)
            }
            fun forAudio(kind: AudioCodecKind): PlaybackBudget = when (kind) {
                AudioCodecKind.AAC_ADTS -> PlaybackBudget(1 * MIB, 64 * KIB, QueueBudget(8L * MIB, 96, 2_000_000L), QueueBudget(12L * MIB, 192, 3_000_000L), 3_000L, 1_500L)
                AudioCodecKind.MPEG1, AudioCodecKind.MPEG2 -> PlaybackBudget(1 * MIB, 64 * KIB, QueueBudget(8L * MIB, 96, 2_000_000L), QueueBudget(12L * MIB, 192, 3_000_000L), 3_000L, 1_500L)
            }
        }
    }

    private object EsHeaderParser {
        private val sampleRates = intArrayOf(96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350)
        private val AVC_SAR_TABLE = mapOf(1 to (1 to 1), 2 to (12 to 11), 3 to (10 to 11), 4 to (16 to 11), 5 to (40 to 33), 6 to (24 to 11), 7 to (20 to 11), 8 to (32 to 11), 9 to (80 to 33), 10 to (18 to 11), 11 to (15 to 11), 12 to (64 to 33), 13 to (160 to 99), 14 to (4 to 3), 15 to (3 to 2), 16 to (2 to 1))
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
                setByteBuffer("csd-0", ByteBuffer.wrap(sps)); setByteBuffer("csd-1", ByteBuffer.wrap(pps))
            }
        }
        fun hevcVideoFormat(bytes: ByteArray): MediaFormat? {
            val vps = findHevcNal(bytes, 32) ?: return null
            val sps = findHevcNal(bytes, 33) ?: return null
            val pps = findHevcNal(bytes, 34) ?: return null
            val dimensions = parseHevcSpsDimensions(sps) ?: return null
            return MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_HEVC, dimensions.width, dimensions.height).apply {
                setByteBuffer("csd-0", ByteBuffer.wrap(vps + sps + pps))
            }
        }
        private fun parseHevcSpsDimensions(spsWithStartCode: ByteArray): VideoDimensions? = runCatching {
            val rbsp = hevcNalRbspPayload(spsWithStartCode)
            val bits = BitReader(rbsp)
            bits.readBits(4)
            val maxSubLayersMinus1 = bits.readBits(3)
            bits.readBit()
            skipHevcProfileTierLevel(bits, maxSubLayersMinus1)
            bits.readUE()
            val chromaFormatIdc = bits.readUE()
            val separateColourPlaneFlag = if (chromaFormatIdc == 3) bits.readBit() else 0
            val width = bits.readUE()
            val height = bits.readUE()
            var left = 0
            var right = 0
            var top = 0
            var bottom = 0
            if (bits.readBit() == 1) {
                left = bits.readUE()
                right = bits.readUE()
                top = bits.readUE()
                bottom = bits.readUE()
            }
            val subWidthC = if (separateColourPlaneFlag == 1) 1 else if (chromaFormatIdc == 1 || chromaFormatIdc == 2) 2 else 1
            val subHeightC = if (separateColourPlaneFlag == 1) 1 else if (chromaFormatIdc == 1) 2 else 1
            VideoDimensions(
                (width - subWidthC * (left + right)).coerceAtLeast(1),
                (height - subHeightC * (top + bottom)).coerceAtLeast(1),
            )
        }.getOrNull()
        private fun skipHevcProfileTierLevel(bits: BitReader, maxSubLayersMinus1: Int) {
            bits.skipBits(2 + 1 + 5 + 32 + 4 + 44 + 8)
            val profilePresent = BooleanArray(maxSubLayersMinus1)
            val levelPresent = BooleanArray(maxSubLayersMinus1)
            repeat(maxSubLayersMinus1) { index ->
                profilePresent[index] = bits.readBit() == 1
                levelPresent[index] = bits.readBit() == 1
            }
            if (maxSubLayersMinus1 > 0) repeat(8 - maxSubLayersMinus1) { bits.skipBits(2) }
            repeat(maxSubLayersMinus1) { index ->
                if (profilePresent[index]) bits.skipBits(88)
                if (levelPresent[index]) bits.skipBits(8)
            }
        }
        private fun hevcNalRbspPayload(nalWithStartCode: ByteArray): ByteArray {
            val prefix = when {
                nalWithStartCode.size >= 6 && nalWithStartCode[0] == 0.toByte() && nalWithStartCode[1] == 0.toByte() && nalWithStartCode[2] == 0.toByte() && nalWithStartCode[3] == 1.toByte() -> 4
                nalWithStartCode.size >= 5 && nalWithStartCode[0] == 0.toByte() && nalWithStartCode[1] == 0.toByte() && nalWithStartCode[2] == 1.toByte() -> 3
                else -> 0
            }
            val start = prefix + 2
            require(start <= nalWithStartCode.size)
            val out = ArrayList<Byte>(nalWithStartCode.size)
            var zeros = 0
            for (index in start until nalWithStartCode.size) {
                val byte = nalWithStartCode[index]
                if (zeros >= 2 && byte == 0x03.toByte()) {
                    zeros = 0
                    continue
                }
                out += byte
                zeros = if (byte == 0.toByte()) zeros + 1 else 0
            }
            return out.toByteArray()
        }
        fun displayAspectRatio(kind: VideoCodecKind, bytes: ByteArray): Double? {
            return when (kind) {
            VideoCodecKind.MPEG2 -> {
                val pos = findStartCode(bytes, 0xb3) ?: return null
                if (pos + 7 >= bytes.size) return null
                val width = ((bytes[pos + 4].toInt() and 0xff) shl 4) or ((bytes[pos + 5].toInt() and 0xf0) ushr 4)
                val height = ((bytes[pos + 5].toInt() and 0x0f) shl 8) or (bytes[pos + 6].toInt() and 0xff)
                when ((bytes[pos + 7].toInt() ushr 4) and 0x0f) { 1 -> width.toDouble() / height.coerceAtLeast(1).toDouble(); 2 -> 4.0 / 3.0; 3 -> 16.0 / 9.0; 4 -> 2.21; else -> null }
            }
            VideoCodecKind.AVC -> findNal(bytes, 7)?.let(::parseAvcSpsDimensions)?.let { dimensions -> dimensions.width.toDouble() * dimensions.sarWidth.toDouble() / (dimensions.height.toDouble() * dimensions.sarHeight.toDouble()) }
            VideoCodecKind.HEVC -> null
        }
        }
        data class VideoDimensions(val width: Int, val height: Int, val sarWidth: Int = 1, val sarHeight: Int = 1)
        fun parseAvcSpsDimensionsForTest(spsWithStartCode: ByteArray): VideoDimensions? = parseAvcSpsDimensions(spsWithStartCode)
        private fun parseAvcSpsDimensions(spsWithStartCode: ByteArray): VideoDimensions? = runCatching {
            val rbsp = nalRbspPayload(spsWithStartCode)
            val bits = BitReader(rbsp)
            val profileIdc = bits.readBits(8); bits.readBits(8); bits.readBits(8); bits.readUE()
            var chromaFormatIdc = 1
            if (profileIdc in setOf(100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135)) {
                chromaFormatIdc = bits.readUE(); if (chromaFormatIdc == 3) bits.readBit(); bits.readUE(); bits.readUE(); bits.readBit()
                if (bits.readBit() == 1) { val count = if (chromaFormatIdc == 3) 12 else 8; repeat(count) { index -> if (bits.readBit() == 1) skipScalingList(bits, if (index < 6) 16 else 64) } }
            }
            bits.readUE(); val picOrderCntType = bits.readUE(); if (picOrderCntType == 0) bits.readUE() else if (picOrderCntType == 1) { bits.readBit(); bits.readSE(); bits.readSE(); repeat(bits.readUE()) { bits.readSE() } }
            bits.readUE(); bits.readBit(); val picWidthInMbsMinus1 = bits.readUE(); val picHeightInMapUnitsMinus1 = bits.readUE(); val frameMbsOnlyFlag = bits.readBit(); if (frameMbsOnlyFlag == 0) bits.readBit(); bits.readBit()
            var cropLeft = 0; var cropRight = 0; var cropTop = 0; var cropBottom = 0
            if (bits.readBit() == 1) { cropLeft = bits.readUE(); cropRight = bits.readUE(); cropTop = bits.readUE(); cropBottom = bits.readUE() }
            val frameMbsFactor = 2 - frameMbsOnlyFlag
            val cropUnitX = if (chromaFormatIdc == 0 || chromaFormatIdc == 3) 1 else 2
            val cropUnitY = when (chromaFormatIdc) { 0 -> frameMbsFactor; 1 -> 2 * frameMbsFactor; 2 -> frameMbsFactor; else -> frameMbsFactor }
            val width = ((picWidthInMbsMinus1 + 1) * 16) - (cropLeft + cropRight) * cropUnitX
            val height = ((picHeightInMapUnitsMinus1 + 1) * 16 * frameMbsFactor) - (cropTop + cropBottom) * cropUnitY
            var sarWidth = 1; var sarHeight = 1
            if (bits.readBit() == 1 && bits.readBit() == 1) { val aspectRatioIdc = bits.readBits(8); val sar = if (aspectRatioIdc == 255) bits.readBits(16) to bits.readBits(16) else AVC_SAR_TABLE[aspectRatioIdc]; if (sar != null && sar.first > 0 && sar.second > 0) { sarWidth = sar.first; sarHeight = sar.second } }
            VideoDimensions(width.coerceAtLeast(1), height.coerceAtLeast(1), sarWidth, sarHeight)
        }.getOrNull()
        private fun nalRbspPayload(nalWithStartCode: ByteArray): ByteArray {
            val start = when { nalWithStartCode.size >= 5 && nalWithStartCode[0] == 0.toByte() && nalWithStartCode[1] == 0.toByte() && nalWithStartCode[2] == 0.toByte() && nalWithStartCode[3] == 1.toByte() -> 5; nalWithStartCode.size >= 4 && nalWithStartCode[0] == 0.toByte() && nalWithStartCode[1] == 0.toByte() && nalWithStartCode[2] == 1.toByte() -> 4; else -> 1 }
            val out = ArrayList<Byte>(nalWithStartCode.size); var zeros = 0
            for (i in start until nalWithStartCode.size) { val b = nalWithStartCode[i]; if (zeros >= 2 && b == 0x03.toByte()) { zeros = 0; continue }; out += b; zeros = if (b == 0.toByte()) zeros + 1 else 0 }
            return out.toByteArray()
        }
        private fun skipScalingList(bits: BitReader, size: Int) { var lastScale = 8; var nextScale = 8; repeat(size) { if (nextScale != 0) nextScale = (lastScale + bits.readSE() + 256) % 256; lastScale = if (nextScale == 0) lastScale else nextScale } }
        private class BitReader(private val bytes: ByteArray) { private var bitOffset = 0; fun readBit(): Int = readBits(1); fun skipBits(count: Int) { repeat(count) { readBit() } }; fun readBits(count: Int): Int { var value = 0; repeat(count) { val byteIndex = bitOffset / 8; require(byteIndex < bytes.size) { "SPS bitstream ended" }; val bitIndex = 7 - (bitOffset % 8); value = (value shl 1) or ((bytes[byteIndex].toInt() ushr bitIndex) and 1); bitOffset++ }; return value }; fun readUE(): Int { var zeros = 0; while (readBit() == 0) zeros++; return if (zeros == 0) 0 else ((1 shl zeros) - 1) + readBits(zeros) }; fun readSE(): Int { val codeNum = readUE(); val value = (codeNum + 1) / 2; return if (codeNum % 2 == 0) -value else value } }
        fun adtsAacFormat(bytes: ByteArray): MediaFormat? { val header = findAdtsHeader(bytes) ?: return null; val sampleRate = sampleRates.getOrNull(header.frequencyIndex) ?: return null; val asc0 = ((2 shl 3) or (header.frequencyIndex ushr 1)).toByte(); val asc1 = (((header.frequencyIndex and 1) shl 7) or (header.channelConfig shl 3)).toByte(); return MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_AAC, sampleRate, header.channelConfig.coerceAtLeast(1)).apply { setInteger(MediaFormat.KEY_IS_ADTS, 1); setByteBuffer("csd-0", ByteBuffer.wrap(byteArrayOf(asc0, asc1))) } }
        fun mpegAudioFormat(bytes: ByteArray): MediaFormat? { val offset = (0 until bytes.size - 3).firstOrNull { i -> (bytes[i].toInt() and 0xff) == 0xff && (bytes[i + 1].toInt() and 0xe0) == 0xe0 } ?: return null; val b2 = bytes[offset + 2].toInt() and 0xff; val b3 = bytes[offset + 3].toInt() and 0xff; val version = (bytes[offset + 1].toInt() ushr 3) and 0x03; val sampleRateIndex = (b2 ushr 2) and 0x03; val channelMode = (b3 ushr 6) and 0x03; val base = when (sampleRateIndex) { 0 -> 44100; 1 -> 48000; 2 -> 32000; else -> return null }; val sampleRate = when (version) { 3 -> base; 2 -> base / 2; 0 -> base / 4; else -> base }; val channels = if (channelMode == 3) 1 else 2; return MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_MPEG, sampleRate, channels) }
        private data class AdtsHeader(val frequencyIndex: Int, val channelConfig: Int)
        private fun findAdtsHeader(bytes: ByteArray): AdtsHeader? { for (i in 0 until bytes.size - 7) if ((bytes[i].toInt() and 0xff) == 0xff && (bytes[i + 1].toInt() and 0xf0) == 0xf0) { val freqIndex = (bytes[i + 2].toInt() ushr 2) and 0x0f; val channelConfig = ((bytes[i + 2].toInt() and 0x01) shl 2) or ((bytes[i + 3].toInt() ushr 6) and 0x03); return AdtsHeader(freqIndex, channelConfig) }; return null }
        private fun findStartCode(bytes: ByteArray, code: Int): Int? { for (i in 0 until bytes.size - 4) if (bytes[i] == 0.toByte() && bytes[i + 1] == 0.toByte() && bytes[i + 2] == 1.toByte() && (bytes[i + 3].toInt() and 0xff) == code) return i; return null }
        private fun findHevcNal(bytes: ByteArray, nalType: Int): ByteArray? {
            var index = 0
            while (index < bytes.size - 4) {
                val prefixLength = when {
                    index + 4 < bytes.size && bytes[index] == 0.toByte() && bytes[index + 1] == 0.toByte() && bytes[index + 2] == 0.toByte() && bytes[index + 3] == 1.toByte() -> 4
                    bytes[index] == 0.toByte() && bytes[index + 1] == 0.toByte() && bytes[index + 2] == 1.toByte() -> 3
                    else -> { index++; continue }
                }
                if (index + prefixLength + 1 >= bytes.size) return null
                val start = index
                val type = (bytes[index + prefixLength].toInt() ushr 1) and 0x3f
                index += prefixLength + 2
                while (index < bytes.size - 3 && !isStartCode(bytes, index)) index++
                val end = if (index < bytes.size - 3) index else bytes.size
                if (type == nalType) return bytes.copyOfRange(start, end)
            }
            return null
        }
        private fun findNal(bytes: ByteArray, nalType: Int): ByteArray? { var i = 0; while (i + 3 < bytes.size) { val prefixLength = when { i + 3 < bytes.size && bytes[i] == 0.toByte() && bytes[i + 1] == 0.toByte() && bytes[i + 2] == 0.toByte() && bytes[i + 3] == 1.toByte() -> 4; i + 2 < bytes.size && bytes[i] == 0.toByte() && bytes[i + 1] == 0.toByte() && bytes[i + 2] == 1.toByte() -> 3; else -> { i++; continue } }; if (i + prefixLength >= bytes.size) return null; val start = i; val nalHeader = bytes[i + prefixLength].toInt() and 0x1f; i += prefixLength + 1; while (i < bytes.size && !isStartCode(bytes, i)) i++; if (nalHeader == nalType) return bytes.copyOfRange(start, i) }; return null }
        private fun isStartCode(bytes: ByteArray, i: Int): Boolean = i + 2 < bytes.size && bytes[i] == 0.toByte() && bytes[i + 1] == 0.toByte() && (bytes[i + 2] == 1.toByte() || (i + 3 < bytes.size && bytes[i + 2] == 0.toByte() && bytes[i + 3] == 1.toByte()))
    }

    private object PcmChannelMaskPolicy {
        fun resolve(decoderMask: Int?, channelCount: Int, componentType: Int?): Int? {
            if (channelCount <= 0) return null
            if (decoderMask != null) {
                return decoderMask.takeIf { it != 0 && Integer.bitCount(it) == channelCount }
            }
            val arib = fromAribComponentType(componentType)
            if (arib != null && Integer.bitCount(arib) == channelCount) return arib
            return canonicalForCount(channelCount)
        }

        fun fromAribComponentType(componentType: Int?): Int? = when (componentType?.and(0x1f)) {
            0x01 -> AudioFormat.CHANNEL_OUT_MONO
            0x02, 0x03 -> AudioFormat.CHANNEL_OUT_STEREO
            0x04 -> AudioFormat.CHANNEL_OUT_STEREO or AudioFormat.CHANNEL_OUT_BACK_CENTER
            0x05 -> AudioFormat.CHANNEL_OUT_STEREO or AudioFormat.CHANNEL_OUT_FRONT_CENTER
            0x06 -> AudioFormat.CHANNEL_OUT_QUAD
            0x07 -> AudioFormat.CHANNEL_OUT_SURROUND
            0x08 -> AudioFormat.CHANNEL_OUT_QUAD or AudioFormat.CHANNEL_OUT_FRONT_CENTER
            0x09 -> AudioFormat.CHANNEL_OUT_5POINT1
            else -> null
        }

        fun canonicalForCount(channelCount: Int): Int? = when (channelCount) {
            1 -> AudioFormat.CHANNEL_OUT_MONO
            2 -> AudioFormat.CHANNEL_OUT_STEREO
            3 -> AudioFormat.CHANNEL_OUT_STEREO or AudioFormat.CHANNEL_OUT_FRONT_CENTER
            4 -> AudioFormat.CHANNEL_OUT_QUAD
            5 -> AudioFormat.CHANNEL_OUT_QUAD or AudioFormat.CHANNEL_OUT_FRONT_CENTER
            6 -> AudioFormat.CHANNEL_OUT_5POINT1
            8 -> AudioFormat.CHANNEL_OUT_7POINT1_SURROUND
            else -> null
        }
    }

    private fun getIntegerOrDefault(format: MediaFormat, key: String, defaultValue: Int): Int = if (format.containsKey(key)) format.getInteger(key) else defaultValue
    private fun mapVideoStreamType(streamType: Int): Int = when (streamType) { 0x02 -> AvSettings.VIDEO_STREAM_TYPE_MPEG2; 0x1b -> AvSettings.VIDEO_STREAM_TYPE_AVC; 0x24 -> AvSettings.VIDEO_STREAM_TYPE_HEVC; else -> AvSettings.VIDEO_STREAM_TYPE_UNDEFINED }
    private fun mapAudioStreamType(streamType: Int): Int = when (streamType) { 0x03 -> AvSettings.AUDIO_STREAM_TYPE_MPEG1; 0x04 -> AvSettings.AUDIO_STREAM_TYPE_MPEG2; 0x0f -> AvSettings.AUDIO_STREAM_TYPE_AAC_ADTS; else -> AvSettings.AUDIO_STREAM_TYPE_UNDEFINED }

    fun stop() { runOnPlaybackExecutorBlocking { stopOnPlaybackExecutor() } }
    private fun stopOnPlaybackExecutor() {
        playbackGeneration++
        videoAvailableNotified.set(false)
        val previousVideoFilter = videoFilter; val previousAudioFilter = audioFilter; val previousSubtitleFilter = subtitleFilter; val previousSuperimposeFilter = superimposeFilter
        videoFilter = null; audioFilter = null; subtitleFilter = null; superimposeFilter = null
        closeFilter(previousVideoFilter); closeFilter(previousAudioFilter); closeFilter(previousSubtitleFilter); closeFilter(previousSuperimposeFilter)
        releaseOutstandingAudioOutputs(); videoDecoder?.close(); audioDecoder?.close(); videoDecoder = null; audioDecoder = null; waitingAvailabilityArm = null
        val sync = mediaSync; mediaSync = null
        if (sync != null) { runCatching { sync.setPlaybackParams(PlaybackParams().setSpeed(0.0f)) }; runCatching { sync.setCallback(null, null) }; runCatching { sync.setOnErrorListener(null, null) } }
        runCatching { mediaSyncInputSurface?.release() }; mediaSyncInputSurface = null; runCatching { sync?.release() }; runCatching { audioTrack?.release() }; audioTrack = null
        mediaSyncStarted = false; mediaSyncSurfaceFailed = false; videoAvailabilityMode = null; videoInputQueued = false; audioInputQueued = false; videoPathExpected = false; audioPathExpected = false
        activeChannel = null; activeTuner = null; activeSelection = null; ptsEpochCoordinator.reset()
    }
    private fun releaseOutstandingAudioOutputs() { outstandingAudioOutputs.values.forEach { output -> runCatching { output.codec.releaseOutputBuffer(output.index, false) }.onFailure { Log.w(LogTags.TIS, "audio outputの回収に失敗しました index=${output.index}", it) } }; outstandingAudioOutputs.clear(); audioOutputBackpressureStartedAtMs = null }
    private fun closeFilter(filter: Filter?) { if (filter == null) return; runCatching { filter.stop() }.onFailure { Log.w(LogTags.TIS, "AV filter stop に失敗しました", it) }; runCatching { filter.close() }.onFailure { Log.w(LogTags.TIS, "AV filter close に失敗しました", it) } }
    private fun emitUnavailableForGeneration(generation: Long, reason: PlaybackUnavailableReason, detail: String = "") { if (generation != playbackGeneration) return; Log.w(LogTags.TIS, "映像を利用できません inputId=$inputId sessionId=$sessionId reason=$reason detail=$detail generation=$generation"); onVideoUnavailable(PlaybackUnavailable(reason, detail, generation)) }
    private fun emitUnavailable(reason: PlaybackUnavailableReason, detail: String = "") = emitUnavailableForGeneration(playbackGeneration, reason, detail)
    fun release() { if (!released.compareAndSet(false, true)) return; runOnPlaybackExecutorBlocking { stopOnPlaybackExecutor() }; executor.shutdownNow(); codecCallbackThread.quitSafely() }
    override fun close() = release()

    companion object {
        private fun isAribDualMonoComponentType(componentType: Int?): Boolean = componentType != null && (componentType and 0x1f) == 0x02
        private fun audioTrackDualMonoMode(presentation: DualMonoPresentation): Int = when (presentation) { DualMonoPresentation.MAIN -> AudioTrack.DUAL_MONO_MODE_LL; DualMonoPresentation.SUB -> AudioTrack.DUAL_MONO_MODE_RR; DualMonoPresentation.MAIN_SUB -> AudioTrack.DUAL_MONO_MODE_LR }
        fun isAribDualMonoComponentTypeForTest(componentType: Int?): Boolean = isAribDualMonoComponentType(componentType)
        fun dualMonoModeForTest(presentation: DualMonoPresentation): Int = audioTrackDualMonoMode(presentation)
        fun h264DimensionsForTest(spsWithStartCode: ByteArray): Pair<Int, Int>? = EsHeaderParser.parseAvcSpsDimensionsForTest(spsWithStartCode)?.let { it.width to it.height }
        fun acceptsFirstFrameForTest(callbackGeneration: Long, currentGeneration: Long, surfaceValid: Boolean, alreadyNotified: Boolean): Boolean = callbackGeneration == currentGeneration && surfaceValid && !alreadyNotified
        fun shouldTriggerFirstFrameTimeoutForTest(timeoutGeneration: Long, currentGeneration: Long, alreadyNotified: Boolean): Boolean = timeoutGeneration == currentGeneration && !alreadyNotified
        fun normalizedPtsTicksForTest(samples: List<Pair<String, Long>>): List<Long> { val coordinator = PtsEpochCoordinator(); return samples.map { (track, rawPts) -> coordinator.normalizeTicks(when (track) { "video" -> PtsTrack.VIDEO; "audio" -> PtsTrack.AUDIO; "subtitle", "caption" -> PtsTrack.CAPTION; "superimpose" -> PtsTrack.SUPERIMPOSE; else -> throw IllegalArgumentException("unknown PTS track: $track") }, rawPts) } }
        fun backpressureDeadlineReachedForTest(startedAtMs: Long, nowMs: Long, deadlineMs: Long): Boolean = deadlineMs > 0L && nowMs >= startedAtMs && nowMs - startedAtMs >= deadlineMs
        fun unavailableReasonForMediaEventCallbackFailureForTest(isAudio: Boolean): PlaybackUnavailableReason = if (isAudio) PlaybackUnavailableReason.AUDIO_UNAVAILABLE else PlaybackUnavailableReason.VIDEO_CODEC_ERROR
        @Suppress("UNUSED_PARAMETER") fun shouldQueueMediaEventForPtsForTest(isPtsPresent: Boolean, pts90k: Long?): Boolean = pts90k != null && pts90k in 0..PTS_MASK
        fun captionTimestampForTest(isPtsPresent: Boolean, pts90k: Long?): CaptionTimestamp = if (isPtsPresent) PesPts90k.fromOrNull(pts90k)?.toCaptionPtsMillis()?.let { CaptionTimestamp.Pts(it) } ?: CaptionTimestamp.NoPts else CaptionTimestamp.NoPts
        fun mediaEventBoundsDecisionForTest(offset: Long, dataLength: Long, mappedCapacity: Long): MediaEventBoundsDecision { if (offset < 0L || dataLength <= 0L) return MediaEventBoundsDecision.MALFORMED; val end = offset + dataLength; if (end < offset) return MediaEventBoundsDecision.MALFORMED; if (offset > Int.MAX_VALUE.toLong() || dataLength > Int.MAX_VALUE.toLong()) return MediaEventBoundsDecision.OVERSIZED; if (end > mappedCapacity) return MediaEventBoundsDecision.OUT_OF_BOUNDS; return MediaEventBoundsDecision.ACCEPT }
        fun normalizedAudioStreamTypeForTest(streamType: Int): Int = when (streamType) { 0x03 -> AvSettings.AUDIO_STREAM_TYPE_MPEG1; 0x04 -> AvSettings.AUDIO_STREAM_TYPE_MPEG2; 0x0f -> AvSettings.AUDIO_STREAM_TYPE_AAC_ADTS; else -> AvSettings.AUDIO_STREAM_TYPE_UNDEFINED }
        fun isSupportedAudioStreamTypeForTest(streamType: Int): Boolean = AudioCodecKind.fromStreamType(streamType) != null
        fun channelMaskForPcmOutputForTest(channelCount: Int): Int? = PcmChannelMaskPolicy.canonicalForCount(channelCount)
        fun aribChannelMaskForComponentTypeForTest(componentType: Int?): Int? = PcmChannelMaskPolicy.fromAribComponentType(componentType)
        fun resolvePcmChannelMaskForTest(decoderMask: Int?, channelCount: Int, componentType: Int?): Int? =
            PcmChannelMaskPolicy.resolve(decoderMask, channelCount, componentType)
        fun videoFormatInfoForTest(streamType: Int, spsWithStartCode: ByteArray): VideoFormatInfo? { val dimensions = h264DimensionsForTest(spsWithStartCode) ?: return null; return VideoFormatInfo(streamType, MediaFormat.MIMETYPE_VIDEO_AVC, dimensions.first, dimensions.second) }
        private const val AV_FILTER_BUFFER_BYTES = 16 * 1024 * 1024L
        private const val SUBTITLE_FILTER_BUFFER_BYTES = 256 * 1024L
        private const val MAX_SUBTITLE_PES_BYTES = 256 * 1024
        private const val PES_STREAM_ID_PRIVATE_STREAM_1 = 0xbd
        private const val PES_STREAM_ID_PRIVATE_STREAM_2 = 0xbf
        private const val DATA_IDENTIFIER_CAPTION = 0x80
        private const val DATA_IDENTIFIER_SUPERIMPOSE = 0x81
        private const val CAPTION_PRIVATE_STREAM_ID = 0xff
        private const val DEFAULT_VIDEO_WIDTH = 1920
        private const val DEFAULT_VIDEO_HEIGHT = 1080
        private const val DEFAULT_AUDIO_SAMPLE_RATE = 48_000
        private const val DEFAULT_AUDIO_CHANNEL_COUNT = 2
        private const val FIRST_FRAME_TIMEOUT_MS = 10_000L
        private const val SERVICE_TYPE_DIGITAL_AUDIO = 0x02
        private const val KIB = 1024
        private const val MIB = 1024 * KIB
        private const val PTS_MODULUS = 1L shl 33
        private const val PTS_HALF = 1L shl 32
        private const val PTS_MASK = PTS_MODULUS - 1L
    }
}
