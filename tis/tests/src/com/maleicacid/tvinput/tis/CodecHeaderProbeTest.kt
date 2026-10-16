// 入力のバイト境界と有限保持を実データ形状で確認する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import java.nio.ByteBuffer
import org.junit.Test

class CodecHeaderProbeTest {
    private fun capture(
        probe: CodecHeaderProbe,
        vararg chunks: ByteArray,
    ): List<ByteArray> {
        val found = mutableListOf<ByteArray>()
        for (chunk in chunks) {
            probe.find(ByteBuffer.wrap(chunk)) {
                found += it
                false
            }
        }
        return found
    }

    @Test
    fun mpeg2HeaderBeyondOneMegabyteAndAcrossEventBoundary() {
        val probe = CodecHeaderProbe(CodecHeaderProbe.Kind.MPEG2_VIDEO)
        val prefix = byteArrayOf(0, 0, 1, 0xb3.toByte(), 0x5a, 0x04, 0x38, 0x30)
        val junk = ByteArray(1_000_000) { 0x55.toByte() }
        val found = capture(probe, junk, prefix.copyOfRange(0, 3), prefix.copyOfRange(3, 8))
        check(found.size == 1)
        check(found.single().contentEquals(prefix))
    }

    @Test
    fun avcParameterSetsSurviveInterveningFramesWithoutSavingThem() {
        val probe = CodecHeaderProbe(CodecHeaderProbe.Kind.AVC)
        val sps = byteArrayOf(0, 0, 0, 1, 0x67, 0x42, 0, 0x1e)
        val pps = byteArrayOf(0, 0, 1, 0x68, 0xce.toByte(), 0x06)
        val unrelated = byteArrayOf(0, 0, 1, 0x65) + ByteArray(300_000) { 0x55 }
        val end = byteArrayOf(0, 0, 1, 0x09)
        val found = capture(probe, sps + byteArrayOf(0, 0), byteArrayOf(1, 0x65) + unrelated, pps, end)
        check(found.size == 1)
        check(found.single().contentEquals(sps + byteArrayOf(0, 0, 0, 1) + pps.copyOfRange(3, pps.size) + end))
    }

    @Test
    fun hevcParameterSetsUseCanonicalDelimitersAcrossEvents() {
        val probe = CodecHeaderProbe(CodecHeaderProbe.Kind.HEVC)
        val vps = byteArrayOf(0, 0, 0, 1, 0x40, 1, 0x11)
        val sps = byteArrayOf(0, 0, 1, 0x42, 1, 0x22)
        val pps = byteArrayOf(0, 0, 0, 1, 0x44, 1, 0x33)
        val trailer = byteArrayOf(0, 0, 1, 0x26)
        val found = capture(probe, vps, sps + byteArrayOf(0), byteArrayOf(0, 1) + pps, trailer)
        check(found.size == 1)
        val expected =
            byteArrayOf(0, 0, 0, 1, 0x40, 1, 0x11) +
                byteArrayOf(0, 0, 0, 1, 0x42, 1, 0x22) +
                pps + byteArrayOf(0, 0, 0, 1)
        check(found.single().contentEquals(expected))
    }

    @Test
    fun mpegAudioHeaderCanCrossInputBoundary() {
        val probe = CodecHeaderProbe(CodecHeaderProbe.Kind.MPEG_AUDIO)
        val expected = byteArrayOf(0xff.toByte(), 0xfb.toByte(), 0x90.toByte(), 0x64)
        val found = capture(probe, byteArrayOf(0x33, 0xff.toByte()), expected.copyOfRange(1, 4))
        check(found.size == 1)
        check(found.single().contentEquals(expected))
    }

    @Test
    fun adtsFrameIsNotPublishedBeforeAllPayloadBytesArrive() {
        val probe = CodecHeaderProbe(CodecHeaderProbe.Kind.AAC_ADTS)
        val frame = byteArrayOf(0xff.toByte(), 0xf1.toByte(), 0x50, 0x80.toByte(), 0, 0xff.toByte(), 0xfc.toByte())
        val found = capture(probe, frame.copyOfRange(0, 3), frame.copyOfRange(3, 6))
        check(found.isEmpty())
        val complete = capture(probe, frame.copyOfRange(6, frame.size))
        check(complete.size == 1)
        check(complete.single().contentEquals(frame))
    }

    @Test
    fun oversizedConfigurationNalIsRejectedAndResetDropsOldParameters() {
        val probe = CodecHeaderProbe(CodecHeaderProbe.Kind.AVC)
        val oversized = byteArrayOf(0, 0, 1, 0x67) + ByteArray(20_000) { 0x55 }
        check(runCatching { capture(probe, oversized) }.exceptionOrNull() is IllegalArgumentException)
        probe.reset()
        val onlyPps = byteArrayOf(0, 0, 1, 0x68, 0x55) + byteArrayOf(0, 0, 1, 0x09)
        check(capture(probe, onlyPps).isEmpty())
    }
}
