package com.maleicacid.tvinput.tis

import android.media.AudioTrack
import java.nio.ByteBuffer

interface AudioSink {
    fun play()
    /**
     * [buffer] から書き込み、sink が受け付けた byte 数だけ [buffer.position] を進める。
     * 正の戻り値を返す場合、実装は position を未変更のままにしてはならない。
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
