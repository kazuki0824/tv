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
import android.media.tv.tuner.filter.TsFilterConfiguration
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Surface
import com.maleicacid.tvinput.aribsi.AribElementaryStream
import com.maleicacid.tvinput.common.LogTags
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class PlaybackPipeline(
    private val inputId: String,
    private val sessionId: String,
    @Suppress("UNUSED_PARAMETER") private val attributionSource: AttributionSource? = null,
) : AutoCloseable {
    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "maleicacid-playback-$inputId").apply { isDaemon = true }
    }
    private val mainHandler = Handler(Looper.getMainLooper())
    private var surface: Surface? = null
    private var streamVolume: Float = 1.0f
    private var playbackGeneration: Long = 0L
    private var onVideoAvailable: () -> Unit = {}
    private var onVideoUnavailable: (PlaybackUnavailable) -> Unit = {}
    private var videoFilter: Filter? = null
    private var audioFilter: Filter? = null
    private var videoDecoder: VideoDecoderPipeline? = null
    private var audioDecoder: AudioDecoderPipeline? = null
    private val videoAvailableNotified = AtomicBoolean(false)
    private val syncClock = PlaybackSyncClock()

    enum class PlaybackUnavailableReason {
        SURFACE_DETACHED, SURFACE_NOT_SET, VIDEO_FILTER_NOT_STARTED, AUDIO_FILTER_NOT_STARTED,
        VIDEO_OUTPUT_RENDER_FAILED, VIDEO_CODEC_ERROR, CODEC_CONFIG_TIMEOUT, UNSUPPORTED_VIDEO_STREAM,
        UNSUPPORTED_AUDIO_STREAM, AUDIO_UNAVAILABLE, CAS_NO_KEY, UNKNOWN,
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
        AAC_ADTS(0x0f, MediaFormat.MIMETYPE_AUDIO_AAC),
        AAC_LATM(0x11, MediaFormat.MIMETYPE_AUDIO_AAC);

        companion object {
            fun fromStreamType(streamType: Int): AudioCodecKind? = values().firstOrNull { it.streamType == streamType }
        }
    }

    private data class MediaSample(val bytes: ByteArray, val presentationTimeUs: Long)

    fun setCallbacks(onAvailable: () -> Unit, onUnavailable: (PlaybackUnavailable) -> Unit) {
        onVideoAvailable = onAvailable
        onVideoUnavailable = onUnavailable
    }

    fun reportUnavailable(reason: PlaybackUnavailableReason, detail: String = "") {
        emitUnavailable(reason, detail)
    }

    fun setVolume(volume: Float) {
        streamVolume = volume.coerceIn(0.0f, 1.0f)
        audioDecoder?.setVolume(streamVolume)
    }

    fun setSurface(newSurface: Surface?) {
        surface = newSurface
        if (newSurface == null) emitUnavailable(PlaybackUnavailableReason.SURFACE_DETACHED)
    }

    fun start(
        tuner: Tuner,
        channel: TunerController.ResolvedChannel,
        selection: TunerController.AvStreamSelection,
    ): StartResult {
        stop()
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
        val videoDecoderLocal = VideoDecoderPipeline(videoKind, currentSurface, syncClock, ::markFirstFrameRendered) { reason, detail -> emitUnavailable(reason, detail) }
        videoDecoder = videoDecoderLocal
        if (audio != null && audioKind != null) {
            audioDecoder = AudioDecoderPipeline(audioKind, streamVolume, syncClock) { reason, detail -> emitUnavailable(reason, detail) }
        }

        val openedVideo = createAndStartAvFilter(tuner, video, isAudio = false).getOrElse { error ->
            emitUnavailable(PlaybackUnavailableReason.VIDEO_FILTER_NOT_STARTED, error.message.orEmpty())
            diagnostics += "video filter start failed: ${error.message}"
            stop()
            return StartResult(false, false, diagnostics)
        }
        videoFilter = openedVideo
        diagnostics += "videoPid=${video.elementaryPid}"
        diagnostics += "videoCodec=$videoKind"

        var audioStarted = false
        if (audio != null && audioKind != null) {
            val openedAudio = createAndStartAvFilter(tuner, audio, isAudio = true).getOrElse { error ->
                emitUnavailable(PlaybackUnavailableReason.AUDIO_FILTER_NOT_STARTED, error.message.orEmpty())
                diagnostics += "audio filter start failed: ${error.message}"
                closeFilter(openedVideo)
                videoFilter = null
                stop()
                return StartResult(false, false, diagnostics)
            }
            audioFilter = openedAudio
            audioStarted = true
            diagnostics += "audioPid=${audio.elementaryPid}"
            diagnostics += "audioCodec=$audioKind"
        } else {
            diagnostics += "audio=absent-or-unsupported-video-only"
        }
        diagnostics += "service=${selection.serviceKey}"
        diagnostics += "channel=${channel.displayNumber}"
        diagnostics += "volume=$streamVolume"
        diagnostics += "surfaceAttached=${currentSurface.isValid}"
        Log.i(LogTags.TIS, "AV filter と遅延 decoder を開始しました inputId=$inputId sessionId=$sessionId ${diagnostics.joinToString(" ")}")
        return StartResult(startedVideo = true, startedAudio = audioStarted, diagnostics = diagnostics)
    }

    private fun createAndStartAvFilter(tuner: Tuner, stream: AribElementaryStream, isAudio: Boolean): Result<Filter> = runCatching {
        val pid = stream.elementaryPid
        require(pid in 0..0x1fff) { "PID が範囲外です: $pid" }
        val subtype = if (isAudio) Filter.SUBTYPE_AUDIO else Filter.SUBTYPE_VIDEO
        val filter = tuner.openFilter(Filter.TYPE_TS, subtype, AV_FILTER_BUFFER_BYTES, executor, object : FilterCallback {
            override fun onFilterEvent(filter: Filter, events: Array<FilterEvent>) {
                for (event in events.filterIsInstance<MediaEvent>()) {
                    val sample = try {
                        sampleFromEvent(event)
                    } finally {
                        releaseMediaEvent(event)
                    } ?: continue
                    if (isAudio) audioDecoder?.queue(sample) else videoDecoder?.queue(sample)
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
            closeFilter(filter)
            error("AV filter configure failed result=$configureResult pid=$pid isAudio=$isAudio")
        }
        val startResult = filter.start()
        if (startResult != Tuner.RESULT_SUCCESS) {
            closeFilter(filter)
            error("AV filter start failed result=$startResult pid=$pid isAudio=$isAudio")
        }
        filter
    }

    private fun markFirstFrameRendered() {
        if (surface?.isValid == true && videoAvailableNotified.compareAndSet(false, true)) onVideoAvailable()
    }


    private fun releaseMediaEvent(event: MediaEvent) {
        runCatching { event.release() }.onFailure { Log.w(LogTags.TIS, "MediaEvent の release に失敗しました", it) }
    }

    private fun sampleFromEvent(event: MediaEvent): MediaSample? {
        if (event.dataLength <= 0L) return null
        if (event.isSecureMemory) {
            emitUnavailable(PlaybackUnavailableReason.VIDEO_CODEC_ERROR, "secure MediaEvent は clear playback 対象外です")
            return null
        }
        val block = event.linearBlock ?: return null
        val bytes = runCatching {
            val mapped = block.map().duplicate()
            val start = event.offset.toInt()
            val length = event.dataLength.toInt()
            mapped.position(start)
            mapped.limit(start + length)
            ByteArray(length).also { mapped.get(it) }
        }.onFailure { Log.w(LogTags.TIS, "MediaEvent LinearBlock の map に失敗しました", it) }.getOrNull() ?: return null
        val ptsUs = PesTimestampNormalizer.toPresentationUs(if (event.isPtsPresent) event.pts else null)
        return MediaSample(bytes, ptsUs)
    }

    private abstract inner class DecoderPipeline : AutoCloseable {
        protected var codec: MediaCodec? = null
        private val configBytes = ByteArrayOutputStream()
        private val bufferInfo = MediaCodec.BufferInfo()

        fun queue(sample: MediaSample) {
            var decoder = codec
            if (decoder == null) {
                configBytes.write(sample.bytes)
                decoder = configureFromBufferedHeader(configBytes.toByteArray())
                if (decoder == null) {
                    if (configBytes.size() > CODEC_CONFIG_MAX_BYTES) {
                        emitUnavailable(PlaybackUnavailableReason.CODEC_CONFIG_TIMEOUT, "decoder 構成に必要な ES header が見つかりません")
                        configBytes.reset()
                    }
                    return
                }
                codec = decoder
            }
            val inputIndex = decoder.dequeueInputBuffer(CODEC_DEQUEUE_TIMEOUT_US)
            if (inputIndex >= 0) {
                val input = decoder.getInputBuffer(inputIndex) ?: return
                input.clear()
                val size = sample.bytes.size.coerceAtMost(input.remaining())
                input.put(sample.bytes, 0, size)
                decoder.queueInputBuffer(inputIndex, 0, size, sample.presentationTimeUs, 0)
            }
            drain(decoder)
        }

        protected abstract fun configureFromBufferedHeader(bytes: ByteArray): MediaCodec?
        protected open fun onOutputFormatChanged(format: MediaFormat) = Unit
        protected abstract fun releaseOutput(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo)

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
        private val firstFrameRendered: () -> Unit,
        private val errorSink: (PlaybackUnavailableReason, String) -> Unit,
    ) : DecoderPipeline() {
        override fun configureFromBufferedHeader(bytes: ByteArray): MediaCodec? {
            val format = when (kind) {
                VideoCodecKind.MPEG2 -> EsHeaderParser.mpeg2VideoFormat(bytes)
                VideoCodecKind.AVC -> EsHeaderParser.avcVideoFormat(bytes)
            } ?: return null
            val decoder = MediaCodec.createDecoderByType(kind.mime)
            decoder.configure(format, outputSurface, null, 0)
            decoder.setOnFrameRenderedListener({ _, _, _ -> firstFrameRendered() }, mainHandler)
            decoder.start()
            return decoder
        }

        override fun releaseOutput(codec: MediaCodec, index: Int, info: MediaCodec.BufferInfo) {
            val render = info.size > 0 && outputSurface.isValid
            if (render) {
                val renderAtNs = syncClock.renderTimeNs(info.presentationTimeUs)
                val lateNs = System.nanoTime() - renderAtNs
                if (lateNs > VIDEO_DROP_LATE_NS) {
                    codec.releaseOutputBuffer(index, false)
                } else {
                    codec.releaseOutputBuffer(index, renderAtNs)
                }
            } else {
                codec.releaseOutputBuffer(index, false)
                errorSink(PlaybackUnavailableReason.VIDEO_OUTPUT_RENDER_FAILED, "video output buffer を render できません")
            }
        }
    }

    private inner class AudioDecoderPipeline(
        private val kind: AudioCodecKind,
        initialVolume: Float,
        private val syncClock: PlaybackSyncClock,
        private val errorSink: (PlaybackUnavailableReason, String) -> Unit,
    ) : DecoderPipeline() {
        private var track: AudioTrack? = null
        private var volume: Float = initialVolume
        private var outputSampleRate: Int = DEFAULT_AUDIO_SAMPLE_RATE
        private var outputChannels: Int = DEFAULT_AUDIO_CHANNEL_COUNT

        fun setVolume(value: Float) {
            volume = value
            track?.setVolume(volume)
        }

        override fun configureFromBufferedHeader(bytes: ByteArray): MediaCodec? {
            val format = when (kind) {
                AudioCodecKind.AAC_ADTS, AudioCodecKind.AAC_LATM -> EsHeaderParser.aacFormat(bytes)
                AudioCodecKind.MPEG1, AudioCodecKind.MPEG2 -> EsHeaderParser.mpegAudioFormat(bytes)
            } ?: return null
            outputSampleRate = getIntegerOrDefault(format, MediaFormat.KEY_SAMPLE_RATE, DEFAULT_AUDIO_SAMPLE_RATE)
            outputChannels = getIntegerOrDefault(format, MediaFormat.KEY_CHANNEL_COUNT, DEFAULT_AUDIO_CHANNEL_COUNT)
            val decoder = MediaCodec.createDecoderByType(kind.mime)
            decoder.configure(format, null, null, 0)
            decoder.start()
            recreateTrack()
            return decoder
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
                    syncClock.anchorIfNeeded(info.presentationTimeUs)
                    ensureTrack().write(output, info.size, AudioTrack.WRITE_NON_BLOCKING)
                } else {
                    errorSink(PlaybackUnavailableReason.AUDIO_UNAVAILABLE, "audio output buffer が null です")
                }
            }
            codec.releaseOutputBuffer(index, false)
        }

        private fun ensureTrack(): AudioTrack = track ?: recreateTrack()

        private fun recreateTrack(): AudioTrack {
            track?.let { runCatching { it.release() } }
            val channelMask = if (outputChannels <= 1) AudioFormat.CHANNEL_OUT_MONO else AudioFormat.CHANNEL_OUT_STEREO
            val minBuffer = AudioTrack.getMinBufferSize(outputSampleRate, channelMask, AudioFormat.ENCODING_PCM_16BIT).coerceAtLeast(32 * 1024)
            val created = AudioTrack.Builder()
                .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build())
                .setAudioFormat(AudioFormat.Builder().setSampleRate(outputSampleRate).setChannelMask(channelMask).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
                .setBufferSizeInBytes(minBuffer)
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build()
            created.setVolume(volume)
            created.play()
            track = created
            return created
        }

        override fun close() {
            super.close()
            track?.let { runCatching { it.stop() }; runCatching { it.release() } }
            track = null
        }
    }

    private class PlaybackSyncClock {
        private var basePresentationUs: Long? = null
        private var baseSystemNs: Long? = null

        fun anchorIfNeeded(presentationUs: Long) {
            if (basePresentationUs == null || baseSystemNs == null) {
                basePresentationUs = presentationUs
                baseSystemNs = System.nanoTime()
            }
        }

        fun renderTimeNs(presentationUs: Long): Long {
            anchorIfNeeded(presentationUs)
            val baseUs = basePresentationUs ?: presentationUs
            val baseNs = baseSystemNs ?: System.nanoTime()
            return baseNs + (presentationUs - baseUs) * 1_000L
        }

        fun reset() {
            basePresentationUs = null
            baseSystemNs = null
        }
    }

    private object PesTimestampNormalizer {
        fun toPresentationUs(pts90k: Long?): Long {
            if (pts90k == null || pts90k < 0) return System.nanoTime() / 1000L
            return (pts90k * 1_000_000L) / 90_000L
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
            return MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, DEFAULT_VIDEO_WIDTH, DEFAULT_VIDEO_HEIGHT).apply {
                setByteBuffer("csd-0", ByteBuffer.wrap(sps))
                setByteBuffer("csd-1", ByteBuffer.wrap(pps))
            }
        }

        fun aacFormat(bytes: ByteArray): MediaFormat? {
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
        0x11 -> AvSettings.AUDIO_STREAM_TYPE_AAC_LATM
        else -> AvSettings.AUDIO_STREAM_TYPE_UNDEFINED
    }

    fun stop() {
        playbackGeneration++
        videoAvailableNotified.set(false)
        closeFilter(videoFilter)
        closeFilter(audioFilter)
        videoFilter = null
        audioFilter = null
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
        stop()
        executor.shutdownNow()
    }

    override fun close() = release()

    companion object {
        private const val AV_FILTER_BUFFER_BYTES = 16 * 1024 * 1024L
        private const val CODEC_DEQUEUE_TIMEOUT_US = 0L
        private const val CODEC_CONFIG_MAX_BYTES = 512 * 1024
        private const val DEFAULT_VIDEO_WIDTH = 1920
        private const val DEFAULT_VIDEO_HEIGHT = 1080
        private const val DEFAULT_AUDIO_SAMPLE_RATE = 48_000
        private const val DEFAULT_AUDIO_CHANNEL_COUNT = 2
        private const val VIDEO_DROP_LATE_NS = 500_000_000L
    }
}
