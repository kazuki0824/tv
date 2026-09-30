package com.maleicacid.tvinput.tis

import java.nio.ByteBuffer

interface AudioSink {
    fun play()

    /**
     * [buffer] から書き込み、sink が受け付けた byte 数だけ [ByteBuffer.position] を進める。
     * 正の戻り値を返す場合、実装は position を未変更のままにしてはならない。
     */
    fun write(
        buffer: ByteBuffer,
        size: Int,
    ): Int

    fun setVolume(volume: Float)

    fun release()
}
