package android.media

import android.os.Handler
import android.view.Surface
import java.nio.ByteBuffer

@Suppress("UNUSED_PARAMETER")
class MediaSync {
    abstract class Callback {
        open fun onAudioBufferConsumed(sync: MediaSync, audioBuffer: ByteBuffer, bufferId: Int) = Unit
    }

    fun interface OnErrorListener {
        fun onError(sync: MediaSync, what: Int, extra: Int)
    }

    fun interface OnFirstVideoFrameQueuedToOutputListener {
        fun onFirstVideoFrameQueuedToOutput(sync: MediaSync, armSequence: Long)
    }

    fun setCallback(callback: Callback?, handler: Handler?) = Unit

    fun setOnErrorListener(listener: OnErrorListener?, handler: Handler?) = Unit

    fun setOnFirstVideoFrameQueuedToOutputListener(
        armSequence: Long,
        listener: OnFirstVideoFrameQueuedToOutputListener?,
        handler: Handler?,
    ) = Unit

    fun setSurface(surface: Surface?) = Unit

    fun createInputSurface(): Surface = throw UnsupportedOperationException("host compile stub")

    fun setPlaybackParams(params: PlaybackParams): PlaybackParams = params

    val timestamp: MediaTimestamp
        get() = throw UnsupportedOperationException("host compile stub")

    fun queueAudio(audioBuffer: ByteBuffer, bufferId: Int, presentationTimeUs: Long) = Unit

    fun setAudioTrack(audioTrack: AudioTrack?) = Unit

    fun release() = Unit

    companion object {
        const val MEDIASYNC_ERROR_AUDIOTRACK_FAIL = 1
        const val MEDIASYNC_ERROR_SURFACE_FAIL = 2
    }
}
