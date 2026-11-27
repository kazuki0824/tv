// HotSpotの割当計測はhost試験だけに閉じ、本番・Android試験へ持ち込まない。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.sun.management.ThreadMXBean
import org.junit.Test
import java.lang.management.ManagementFactory
import java.nio.ByteBuffer

class CodecHeaderProbeAllocationTest {
    @Test
    fun fixedHeaderAllocationDependsOnCandidatesRatherThanInputLength() {
        val bean = ManagementFactory.getThreadMXBean() as ThreadMXBean
        check(bean.isThreadAllocatedMemorySupported)
        bean.isThreadAllocatedMemoryEnabled = true
        val threadId = Thread.currentThread().id
        val headers =
            listOf(
                CodecHeaderProbe.Kind.MPEG2_VIDEO to byteArrayOf(0, 0, 1, 0xb3.toByte(), 0x5a, 4, 0x38, 0x30),
                CodecHeaderProbe.Kind.MPEG_AUDIO to byteArrayOf(0xff.toByte(), 0xfb.toByte(), 0x90.toByte(), 0x64),
            )
        for ((kind, header) in headers) {
            val probe = CodecHeaderProbe(kind)
            val warmup = ByteBuffer.wrap(ByteArray(4096) { 0x55 })
            repeat(100) {
                warmup.rewind()
                probe.find(warmup) { false }
            }
            for (length in listOf(4096, 1_700_000)) {
                for (candidates in listOf(0, 1, 64)) {
                    val bytes = ByteArray(length) { 0x55 }
                    repeat(candidates) { header.copyInto(bytes, it * 32) }
                    val input = ByteBuffer.wrap(bytes)
                    var found = 0
                    val accept: (ByteArray) -> Boolean = {
                        check(it.contentEquals(header))
                        found++
                        false
                    }
                    probe.reset()
                    val before = bean.getThreadAllocatedBytes(threadId)
                    probe.find(input, accept)
                    val allocated = bean.getThreadAllocatedBytes(threadId) - before
                    check(found == candidates)
                    // VMの固定費に余裕を持たせるが、入力byteごとの配列確保は許さない。
                    check(allocated <= 65_536L + candidates * 64L) { "割当量=$allocated 入力量=$length 候補数=$candidates" }
                }
            }
        }
    }
}
