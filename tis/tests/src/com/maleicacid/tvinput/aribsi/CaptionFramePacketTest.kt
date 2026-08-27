package com.maleicacid.tvinput.aribsi

import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Test

class CaptionFramePacketTest {
    @Test fun oneShotPacketPreservesRgbaMetadataAndTiming() {
        val packet = packet(
            ptsMillis = 100L,
            durationMillis = 25L,
            metadata = intArrayOf(2, 3, 1, 1, 4, 4),
            rgba = byteArrayOf(0x11, 0x22, 0x33, 0x44),
        )

        val frame = checkNotNull(NativeAribCaptionRenderer.decodeFramePacket(packet))
        check(frame.ptsMillis == 100L)
        check(frame.durationMillis == 25L)
        check(frame.images.single().rgba8888.contentEquals(byteArrayOf(0x11, 0x22, 0x33, 0x44)))
    }

    @Test fun truncatedOrLengthMismatchedPacketIsRejected() {
        val valid = packet(
            ptsMillis = 100L,
            durationMillis = -1L,
            metadata = intArrayOf(0, 0, 1, 1, 4, 4),
            rgba = byteArrayOf(1, 2, 3, 4),
        )
        check(NativeAribCaptionRenderer.decodeFramePacket(valid.copyOf(valid.size - 1)) == null)

        val mismatched = valid.clone()
        ByteBuffer.wrap(mismatched).order(ByteOrder.LITTLE_ENDIAN).putInt(40, 8)
        check(NativeAribCaptionRenderer.decodeFramePacket(mismatched) == null)
    }

    private fun packet(
        ptsMillis: Long,
        durationMillis: Long,
        metadata: IntArray,
        rgba: ByteArray,
    ): ByteArray = ByteBuffer.allocate(20 + 24 + rgba.size)
        .order(ByteOrder.LITTLE_ENDIAN)
        .putLong(ptsMillis)
        .putLong(durationMillis)
        .putInt(1)
        .also { buffer -> metadata.forEach(buffer::putInt) }
        .put(rgba)
        .array()
}
