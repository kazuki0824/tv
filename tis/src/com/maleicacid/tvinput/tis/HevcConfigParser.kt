package com.maleicacid.tvinput.tis

/** 起動時の寸法とCSDだけを抽出する。画像の復号・全構文の適合判定は行わない。 */
internal object HevcConfigParser {
    data class Config(
        val width: Int,
        val height: Int,
        val csd: ByteArray,
    )

    private data class Sps(
        val nal: ByteArray,
        val width: Int,
        val height: Int,
    )

    private const val VPS = 32
    private const val SPS = 33
    private const val PPS = 34
    private const val NAL_HEADER_BYTES = 2
    private const val NAL_TYPE_MASK = 0x3f
    private const val FORBIDDEN_BIT = 0x80
    private const val TEMPORAL_ID_MASK = 0x07
    private const val ESCAPE_BYTE = 3
    private const val BYTE_MASK = 0xff
    private const val VPS_ID_BITS = 4
    private const val SUB_LAYER_COUNT_BITS = 3
    private const val MAX_SUB_LAYERS_MINUS_ONE = 6
    private const val MAX_SPS_ID = 15
    private const val CHROMA_444 = 3
    private const val GENERAL_PROFILE_TIER_LEVEL_BITS = 96
    private const val SUB_LAYER_FLAG_SLOTS = 8
    private const val SUB_LAYER_PROFILE_BITS = 88
    private const val LEVEL_BITS = 8
    private val startCode = byteArrayOf(0, 0, 0, 1)

    /** 不足時はnull、区切りまで受信した構成の不正はIllegalArgumentExceptionで返す。 */
    fun parse(bytes: ByteArray): Config? {
        var vps: ByteArray? = null
        var sps: Sps? = null
        var pps: ByteArray? = null
        var start = findStartCode(bytes, 0)
        while (start >= 0) {
            val payload = start + prefixLength(bytes, start)
            val end = findStartCode(bytes, payload)
            if (end < 0) break
            require(end - payload >= NAL_HEADER_BYTES) { "HEVC NAL headerが切断されています" }
            val type = (bytes[payload].toInt() ushr 1) and NAL_TYPE_MASK
            if (type in VPS..PPS) {
                val nal = parameterSet(bytes, payload, end)
                when (type) {
                    VPS -> {
                        vps = nal
                    }

                    SPS -> {
                        sps = parseSps(nal)
                    }

                    PPS -> {
                        pps = nal
                    }
                }
            }
            start = end
            if (vps != null && sps != null && pps != null) {
                return Config(sps.width, sps.height, vps + sps.nal + pps)
            }
        }
        return null
    }

    private fun parameterSet(
        bytes: ByteArray,
        start: Int,
        end: Int,
    ): ByteArray {
        require(bytes[start].toInt() and FORBIDDEN_BIT == 0) { "HEVC forbidden_zero_bitが不正です" }
        require(bytes[start + 1].toInt() and TEMPORAL_ID_MASK != 0) { "HEVC temporal_id_plus1が0です" }
        var payloadEnd = end
        while (payloadEnd > start + NAL_HEADER_BYTES && bytes[payloadEnd - 1] == 0.toByte()) payloadEnd--
        require(payloadEnd > start + NAL_HEADER_BYTES) { "HEVC parameter setが空です" }
        return startCode + bytes.copyOfRange(start, payloadEnd)
    }

    private fun findStartCode(
        bytes: ByteArray,
        from: Int,
    ): Int {
        var index = from
        while (index < bytes.size - 2) {
            if (prefixLength(bytes, index) != 0) return index
            index++
        }
        return -1
    }

    private fun prefixLength(
        bytes: ByteArray,
        offset: Int,
    ): Int =
        when {
            offset + 2 >= bytes.size || bytes[offset] != 0.toByte() || bytes[offset + 1] != 0.toByte() -> 0

            bytes[offset + 2] == 1.toByte() -> startCode.size - 1

            offset + startCode.size <= bytes.size && bytes[offset + 2] == 0.toByte() &&
                bytes[offset + startCode.size - 1] == 1.toByte() -> startCode.size

            else -> 0
        }

    private fun rbsp(nal: ByteArray): ByteArray {
        val payload = startCode.size + NAL_HEADER_BYTES
        val out = ByteArray(nal.size - payload)
        var length = 0
        var zeros = 0
        for (index in payload until nal.size) {
            val byte = nal[index]
            if (zeros >= 2 && byte == ESCAPE_BYTE.toByte()) {
                require(index + 1 < nal.size && (nal[index + 1].toInt() and BYTE_MASK) <= ESCAPE_BYTE) {
                    "HEVC emulation_prevention_three_byteが不正です"
                }
                zeros = 0
            } else {
                out[length++] = byte
                zeros = if (byte == 0.toByte()) zeros + 1 else 0
            }
        }
        return out.copyOf(length)
    }

    private fun parseSps(nal: ByteArray): Sps {
        val bits = CodecBitReader(rbsp(nal))
        bits.readBits(VPS_ID_BITS)
        val maxSubLayersMinus1 = bits.readBits(SUB_LAYER_COUNT_BITS)
        require(maxSubLayersMinus1 <= MAX_SUB_LAYERS_MINUS_ONE) { "HEVC sub-layer数が予約値です" }
        bits.readBit()
        skipHevcProfileTierLevel(bits, maxSubLayersMinus1)
        require(bits.readUE() <= MAX_SPS_ID) { "HEVC SPS IDが範囲外です" }
        val chromaFormatIdc = bits.readUE()
        require(chromaFormatIdc in 0..CHROMA_444) { "HEVC chroma_format_idcが予約値です" }
        val separateColourPlaneFlag = if (chromaFormatIdc == CHROMA_444) bits.readBit() else 0
        val width = bits.readUE()
        val height = bits.readUE()
        var left = 0
        var right = 0
        var top = 0
        var bottom = 0
        if (bits.readBit() == 1) {
            left = bits.readUE()
            right = bits.readUE()
            top = bits.readUE()
            bottom = bits.readUE()
        }
        val subWidthC =
            if (separateColourPlaneFlag == 1) {
                1
            } else if (chromaFormatIdc == 1 || chromaFormatIdc == 2) {
                2
            } else {
                1
            }
        val subHeightC =
            if (separateColourPlaneFlag == 1) {
                1
            } else if (chromaFormatIdc == 1) {
                2
            } else {
                1
            }
        val croppedWidth = width.toLong() - subWidthC * (left.toLong() + right)
        val croppedHeight = height.toLong() - subHeightC * (top.toLong() + bottom)
        require(croppedWidth in 1..Int.MAX_VALUE.toLong() && croppedHeight in 1..Int.MAX_VALUE.toLong()) {
            "HEVC寸法またはconformance windowが不正です"
        }
        return Sps(nal, croppedWidth.toInt(), croppedHeight.toInt())
    }

    private fun skipHevcProfileTierLevel(
        bits: CodecBitReader,
        maxSubLayersMinus1: Int,
    ) {
        bits.skipBits(GENERAL_PROFILE_TIER_LEVEL_BITS)
        val profilePresent = BooleanArray(maxSubLayersMinus1)
        val levelPresent = BooleanArray(maxSubLayersMinus1)
        repeat(maxSubLayersMinus1) { index ->
            profilePresent[index] = bits.readBit() == 1
            levelPresent[index] = bits.readBit() == 1
        }
        if (maxSubLayersMinus1 > 0) repeat(SUB_LAYER_FLAG_SLOTS - maxSubLayersMinus1) { bits.skipBits(2) }
        repeat(maxSubLayersMinus1) { index ->
            if (profilePresent[index]) bits.skipBits(SUB_LAYER_PROFILE_BITS)
            if (levelPresent[index]) bits.skipBits(LEVEL_BITS)
        }
    }
}
