package com.maleicacid.tvinput.tis

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer

/**
 * MediaFormatの構成に必要なヘッダだけを保存する。ESやaccess unitの組立て・queue境界は扱わない。
 * AVC/HEVCはconfig NAL、MPEG-2 videoとMPEG audioは固定長header、AACはADTS frameだけを対象とする。
 */
internal class CodecHeaderProbe(private val kind: Kind) {
    enum class Kind { MPEG2_VIDEO, AVC, HEVC, AAC_ADTS, MPEG_AUDIO }

    private val recent = ByteArray(if (kind == Kind.MPEG2_VIDEO) 8 else 4)
    private var recentCount = 0
    private var zeroRun = 0
    private var nextNalHeader = false
    private var nal: ByteArrayOutputStream? = null
    private var vps: ByteArray? = null
    private var sps: ByteArray? = null
    private var pps: ByteArray? = null
    private val adts = ByteArrayOutputStream()

    fun reset() {
        recentCount = 0
        zeroRun = 0
        nextNalHeader = false
        nal = null
        vps = null
        sps = null
        pps = null
        adts.reset()
    }

    /** acceptがtrueを返した時点で探索を終了する。入力本体のコピーは行わない。 */
    fun find(
        input: ByteBuffer,
        accept: (ByteArray) -> Boolean,
    ): Boolean {
        while (input.hasRemaining()) {
            val byte = input.get().toInt() and 0xff
            val candidate =
                when (kind) {
                    Kind.MPEG2_VIDEO, Kind.MPEG_AUDIO -> scanFixedHeader(byte)
                    Kind.AVC, Kind.HEVC -> scanNal(byte)
                    Kind.AAC_ADTS -> scanAdts(byte)
                }
            if (candidate != null && accept(candidate)) return true
        }
        return false
    }

    private fun scanFixedHeader(byte: Int): ByteArray? {
        if (recentCount == recent.size) {
            recent.copyInto(recent, 0, 1)
        } else {
            recentCount++
        }
        recent[recentCount - 1] = byte.toByte()
        if (recentCount != recent.size) return null
        return when (kind) {
            Kind.MPEG2_VIDEO ->
                recent.copyOf().takeIf {
                    it[0] == 0.toByte() && it[1] == 0.toByte() &&
                        it[2] == 1.toByte() && it[3] == 0xb3.toByte()
                }

            Kind.MPEG_AUDIO ->
                recent.copyOf().takeIf {
                    it[0] == 0xff.toByte() && it[1].toInt() and 0xe0 == 0xe0
                }

            else -> null
        }
    }

    private fun scanNal(byte: Int): ByteArray? {
        if (byte == 0) {
            // start codeを跨いだ時だけ必要なzero-run。NAL末尾のtrailing zeroは構成に不要。
            zeroRun = (zeroRun + 1).coerceAtMost(3)
            return null
        }
        if (byte == 1 && zeroRun >= 2) {
            val completed = completeNal()
            zeroRun = 0
            nextNalHeader = true
            return completed
        }
        repeat(zeroRun) { appendNal(0) }
        zeroRun = 0
        if (nextNalHeader) {
            nextNalHeader = false
            val type = if (kind == Kind.AVC) byte and 0x1f else (byte ushr 1) and 0x3f
            val required = if (kind == Kind.AVC) type == 7 || type == 8 else type in 32..34
            if (required) {
                nal = ByteArrayOutputStream().apply {
                    write(byteArrayOf(0, 0, 0, 1))
                    write(byte)
                }
            } else {
                nal = null
            }
        } else {
            appendNal(byte)
        }
        return null
    }

    private fun appendNal(byte: Int) {
        val active = nal ?: return
        require(active.size() < MAX_CONFIGURATION_NAL_BYTES) { "codec構成NALが上限を超えました" }
        active.write(byte)
    }

    private fun completeNal(): ByteArray? {
        val completed = nal?.toByteArray()
        nal = null
        if (completed == null || completed.size < 5) return null
        val type = if (kind == Kind.AVC) completed[4].toInt() and 0x1f else (completed[4].toInt() ushr 1) and 0x3f
        when (type) {
            7 -> if (kind == Kind.AVC) sps = completed
            8 -> if (kind == Kind.AVC) pps = completed
            32 -> if (kind == Kind.HEVC) vps = completed
            33 -> if (kind == Kind.HEVC) sps = completed
            34 -> if (kind == Kind.HEVC) pps = completed
        }
        val spsBytes = sps ?: return null
        val ppsBytes = pps ?: return null
        val vpsBytes = if (kind == Kind.HEVC) vps ?: return null else byteArrayOf()
        // HEVC既存parserは末尾NALの終端start codeを要求する。
        return vpsBytes + spsBytes + ppsBytes + byteArrayOf(0, 0, 0, 1)
    }

    private fun scanAdts(byte: Int): ByteArray? {
        val size = adts.size()
        if (size == 0) {
            if (byte == 0xff) adts.write(byte)
            return null
        }
        if (size == 1 && byte and 0xf0 != 0xf0) {
            adts.reset()
            if (byte == 0xff) adts.write(byte)
            return null
        }
        adts.write(byte)
        if (adts.size() < 7) return null
        val header = adts.toByteArray()
        val length =
            ((header[3].toInt() and 0x03) shl 11) or
                ((header[4].toInt() and 0xff) shl 3) or
                ((header[5].toInt() and 0xe0) ushr 5)
        val headerSize = if (header[1].toInt() and 1 != 0) 7 else 9
        if (length !in headerSize..MAX_ADTS_FRAME_BYTES) {
            adts.reset()
            return null
        }
        if (adts.size() < length) return null
        adts.reset()
        return header
    }

    private companion object {
        // Access-unit再構成用予算ではない。構成NAL一つだけの安全上限。
        const val MAX_CONFIGURATION_NAL_BYTES = 16 * 1024
        // ADTSフレーム長は13bit。
        const val MAX_ADTS_FRAME_BYTES = (1 shl 13) - 1
    }
}
