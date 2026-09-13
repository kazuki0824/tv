package com.maleicacid.tvinput.tis

import android.media.MediaFormat
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class HevcPlaybackTest {
    // ffmpeg color=1920x1080:rate=25 / libx265 / 1 frameの実VPS/SPS/PPS。
    private fun headers(): List<ByteArray> {
        val path = "hevc_1080p_headers.hex"
        val hostRoot = System.getProperty("maleicacid.tis.testAssetsRoot")
        val text =
            if (hostRoot != null) {
                java.io.File(hostRoot, path).readText()
            } else {
                InstrumentationRegistry
                    .getInstrumentation()
                    .context.assets
                    .open(path)
                    .bufferedReader()
                    .use { it.readText() }
            }
        return text
            .lineSequence()
            .filter { it.isNotBlank() }
            .map { line ->
                line.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
            }.toList()
    }

    private fun format(parts: List<ByteArray>) =
        PlaybackPipeline.EsHeaderParser.hevcVideoFormat(
            parts.reduce { bytes, nal -> bytes + nal } + byteArrayOf(0, 0, 0, 1),
        )

    @Test fun realHevcHeadersProduceDimensionsMimeAndCompleteCsd() {
        val parts = headers()
        val format = requireNotNull(format(parts))
        assertEquals(MediaFormat.MIMETYPE_VIDEO_HEVC, format.getString(MediaFormat.KEY_MIME))
        assertEquals(1920, format.getInteger(MediaFormat.KEY_WIDTH))
        assertEquals(1080, format.getInteger(MediaFormat.KEY_HEIGHT))
        val csd = requireNotNull(format.getByteBuffer("csd-0"))
        val bytes = ByteArray(csd.remaining()).also { csd.get(it) }
        assertArrayEquals(parts.reduce { left, right -> left + right }, bytes)
        assertEquals(0x24, PlaybackPipeline.VideoCodecKind.fromStreamType(0x24)?.streamType)
    }

    @Test fun eachParameterSetIsRequired() {
        val parts = headers()
        parts.indices.forEach { omitted -> assertNull(format(parts.filterIndexed { index, _ -> index != omitted })) }
    }

    @Test fun truncatedSpsCannotConfigureDecoder() {
        val parts = headers()
        assertNull(format(listOf(parts[0], parts[2])))
        for (size in 4..20) {
            assertThrows(IllegalArgumentException::class.java) {
                format(listOf(parts[0], parts[1].copyOf(size), parts[2]))
            }
        }
    }

    @Test fun threeByteStartCodesAreAccepted() {
        val parts = headers().map { it.copyOfRange(1, it.size) }
        val result = requireNotNull(format(parts))
        assertEquals(1920, result.getInteger(MediaFormat.KEY_WIDTH))
        assertEquals(1080, result.getInteger(MediaFormat.KEY_HEIGHT))
        val csd = requireNotNull(result.getByteBuffer("csd-0"))
        assertArrayEquals(
            headers().reduce { left, right -> left + right },
            ByteArray(csd.remaining()).also { csd.get(it) },
        )
    }

    @Test fun invalidNalHeadersAreRejected() {
        val parts = headers()
        val forbidden = parts[1].copyOf().apply { this[4] = (this[4].toInt() or 0x80).toByte() }
        val temporalZero = parts[1].copyOf().apply { this[5] = 0 }
        for (sps in listOf(forbidden, temporalZero)) {
            assertThrows(IllegalArgumentException::class.java) { format(listOf(parts[0], sps, parts[2])) }
        }
    }

    private fun ue(value: Int): String {
        val binary = (value + 1).toString(2)
        return "0".repeat(binary.length - 1) + binary
    }

    private fun sps(
        width: Int,
        height: Int,
        chroma: Int = 1,
        cropRight: Int = 0,
    ): ByteArray {
        val fields =
            "00000001" + "0".repeat(96) + ue(0) + ue(chroma) + ue(width) + ue(height) +
                "1" + ue(0) + ue(cropRight) + ue(0) + ue(0) + "1"
        val rbsp = fields.padEnd((fields.length + 7) / 8 * 8, '0').chunked(8).map { it.toInt(2).toByte() }
        // Annex-Bでstart codeと誤認されないようemulation preventionを挿入する。
        val escaped = mutableListOf<Byte>()
        var zeros = 0
        for (byte in rbsp) {
            if (zeros == 2 && byte.toInt() and 0xff <= 3) {
                escaped += 3.toByte()
                zeros = 0
            }
            escaped += byte
            zeros = if (byte == 0.toByte()) zeros + 1 else 0
        }
        return byteArrayOf(0, 0, 0, 1, 0x42, 1) + escaped.toByteArray()
    }

    @Test fun invalidDimensionsAndConformanceWindowAreRejected() {
        val parts = headers()
        for (invalid in listOf(
            sps(0, 1080),
            sps(1920, 0),
            sps(1920, 1080, cropRight = 960),
            sps(1920, 1080, cropRight = Int.MAX_VALUE - 1),
        )) {
            assertThrows(IllegalArgumentException::class.java) { format(listOf(parts[0], invalid, parts[2])) }
        }
    }

    @Test fun reservedChromaAndSubLayersAreRejected() {
        val parts = headers()
        assertThrows(IllegalArgumentException::class.java) {
            format(listOf(parts[0], sps(1920, 1080, chroma = 4), parts[2]))
        }
        val reservedLayers = parts[1].copyOf().apply { this[6] = 0x0f }
        assertThrows(IllegalArgumentException::class.java) { format(listOf(parts[0], reservedLayers, parts[2])) }
    }

    @Test fun conformanceCropUsesChromaSubsamplingUnits() {
        val parts = headers()
        val cropped = format(listOf(parts[0], sps(1920, 1080, cropRight = 8), parts[2]))
        assertNotNull(cropped)
        assertEquals(1904, cropped?.getInteger(MediaFormat.KEY_WIDTH))
        assertEquals(1080, cropped?.getInteger(MediaFormat.KEY_HEIGHT))
    }

    @Test fun everyPartialParameterSetWaitsForItsDelimiter() {
        val stream = headers().reduce { left, right -> left + right } + byteArrayOf(0, 0, 0, 1)
        for (size in 0 until stream.size) {
            assertNull("受信長=$size", HevcConfigParser.parse(stream.copyOf(size)))
        }
        assertNotNull(HevcConfigParser.parse(stream))
    }

    @Test fun malformedDelimitedSpsFailsBeforePpsArrives() {
        val parts = headers()
        val stream = parts[0] + parts[1].copyOf(12) + byteArrayOf(0, 0, 0, 1)
        assertThrows(IllegalArgumentException::class.java) { HevcConfigParser.parse(stream) }
    }

    @Test fun emptyParameterSetsAreRejected() {
        val parts = headers()
        for (index in parts.indices) {
            val broken = parts.toMutableList().apply { this[index] = this[index].copyOf(6) }
            assertThrows(IllegalArgumentException::class.java) { format(broken) }
        }
    }

    @Test fun invalidEmulationPreventionIsRejected() {
        val parts = headers()
        for (suffix in listOf(byteArrayOf(0, 0, 3), byteArrayOf(0, 0, 3, 4))) {
            assertThrows(IllegalArgumentException::class.java) { format(listOf(parts[0], parts[1] + suffix, parts[2])) }
        }
    }

    @Test fun mixedPrefixesAndTrailingZerosProduceCanonicalCsd() {
        val parts = headers()
        val stream =
            parts[0] + byteArrayOf(0, 0) + parts[1].copyOfRange(1, parts[1].size) + parts[2] +
                byteArrayOf(0, 0, 0, 0, 0, 1)
        val config = requireNotNull(HevcConfigParser.parse(stream))
        assertArrayEquals(parts.reduce { left, right -> left + right }, config.csd)
    }
}
