// 固定TSのpacket/PES識別子と合成data groupを具体値で照合する。
@file:Suppress("MagicNumber")

package com.maleicacid.tvinput.tis

import com.maleicacid.tvinput.aribsi.NativeAribCaptionFactParser
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.ByteArrayOutputStream
import java.io.File

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class RealTsCaptionFactTest {
    @Test
    fun broadcastCaptionManagementLanguageSurvivesJni() {
        val fixture = File(checkNotNull(System.getProperty("realTs.fixtureDirectory")), "test.ts")
        NativeAribCaptionFactParser(superimpose = false).use { parser ->
            val management = captionPayloads(fixture.readBytes()).mapNotNull { parser.ingest(it)?.management }
            check(management.size == 11)
            check(management.all { it.languages.single().iso639LanguageCode == "jpn" })
            check(management.all { it.languages.single().languageTag == 0 })
        }
    }

    @Test
    fun superimposeTimeSurvivesJniWithAndWithoutManagement() {
        val management = "00000000070001006a706e1051dd"
        val statement = "04000000064012000512306219"
        NativeAribCaptionFactParser(superimpose = true).use { parser ->
            val timeOnly = checkNotNull(parser.ingest(hex("81ff00" + statement)))
            check(timeOnly.management == null)
            check(timeOnly.statementTime == NativeAribCaptionFactParser.StatementTime(1, 43_205_123))
            parser.reset()
            val both = checkNotNull(parser.ingest(hex("81ff00" + management + statement)))
            check(both.statementTime == timeOnly.statementTime)
            val language = checkNotNull(both.management).languages.single()
            check(language.iso639LanguageCode == "jpn")
        }
    }

    // 固定fixtureのTBSフルセグ字幕PIDから長さ指定のprivate_stream_1 PESを取り出す。
    private fun captionPayloads(ts: ByteArray): List<ByteArray> {
        val result = mutableListOf<ByteArray>()
        val pes = ByteArrayOutputStream()

        fun collect() {
            val bytes = pes.toByteArray()
            if (bytes.isEmpty()) return
            check(bytes.size >= 9 && unsigned(bytes[3]) == 0xbd)
            val end = 6 + (unsigned(bytes[4]) shl 8) + unsigned(bytes[5])
            result += bytes.copyOfRange(9 + unsigned(bytes[8]), end)
            pes.reset()
        }
        for (offset in 0..(ts.size - 188) step 188) {
            val flags = unsigned(ts[offset + 1])
            val pid = ((flags and 0x1f) shl 8) + unsigned(ts[offset + 2])
            val control = unsigned(ts[offset + 3])
            if (pid != 0x0114 || control and 0x10 == 0) continue
            if (flags and 0x40 != 0) collect()
            val payload = offset + 4 + if (control and 0x20 != 0) 1 + unsigned(ts[offset + 4]) else 0
            pes.write(ts, payload, offset + 188 - payload)
        }
        collect()
        return result
    }

    private fun unsigned(value: Byte): Int = value.toInt() and 0xff

    private fun hex(value: String): ByteArray =
        value
            .chunked(2)
            .map { it.toInt(16).toByte() }
            .toByteArray()
}
