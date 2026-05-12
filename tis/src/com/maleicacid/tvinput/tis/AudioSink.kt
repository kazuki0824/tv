package com.maleicacid.tvinput.tis

import android.media.AudioTrack
import java.nio.ByteBuffer

interface AudioSink {
    fun play()
    fun write(buffer: ByteBuffer, size: Int): Int
    fun setVolume(volume: Float)
    fun release()
}

class AndroidAudioSink(private val track: AudioTrack) : AudioSink {
    override fun play() {
        track.play()
    }

    override fun write(buffer: ByteBuffer, size: Int): Int {
        return track.write(buffer.duplicate(), size, AudioTrack.WRITE_BLOCKING)
    }

    override fun setVolume(volume: Float) {
        track.setVolume(volume)
    }

    override fun release() {
        runCatching { track.stop() }
        runCatching { track.release() }
    }
}
