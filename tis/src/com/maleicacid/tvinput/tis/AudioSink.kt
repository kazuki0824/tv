package com.maleicacid.tvinput.tis

import android.media.AudioTrack
import java.nio.ByteBuffer

interface AudioSink {
    fun play()
    /**
     * Writes from [buffer] and advances [buffer.position] by the number of bytes
     * accepted by the sink. Implementations must not leave position unchanged on
     * a positive return value.
     */
    fun write(buffer: ByteBuffer, size: Int): Int
    fun setVolume(volume: Float)
    fun release()
}

class AndroidAudioSink(private val track: AudioTrack) : AudioSink {
    override fun play() {
        track.play()
    }

    override fun write(buffer: ByteBuffer, size: Int): Int {
        return track.write(buffer, size, AudioTrack.WRITE_BLOCKING)
    }

    override fun setVolume(volume: Float) {
        track.setVolume(volume)
    }

    override fun release() {
        runCatching { track.stop() }
        runCatching { track.release() }
    }
}
