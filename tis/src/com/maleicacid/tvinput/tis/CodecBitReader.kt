package com.maleicacid.tvinput.tis

internal class CodecBitReader(
    private val bytes: ByteArray,
) {
    companion object {
        private const val MAX_EXP_GOLOMB_LEADING_ZEROS = 30
    }

    private var bitOffset = 0

    fun readBit(): Int = readBits(1)

    fun skipBits(count: Int) {
        repeat(count) { readBit() }
    }

    // この処理の規格値・ビット幅・単位換算・固定上限をリテラルのまま照合できる形に保つ。
    @Suppress("MagicNumber")
    fun readBits(count: Int): Int {
        var value = 0
        repeat(count) {
            val byteIndex =
                bitOffset / 8
            require(byteIndex < bytes.size) { "SPS bitstream ended" }
            val bitIndex =
                7 - (bitOffset % 8)
            value = (value shl 1) or ((bytes[byteIndex].toInt() ushr bitIndex) and 1)
            bitOffset++
        }
        return value
    }

    fun readUE(): Int {
        var zeros = 0
        while (readBit() ==
            0
        ) {
            zeros++
            require(zeros <= MAX_EXP_GOLOMB_LEADING_ZEROS) { "Exp-Golomb value exceeds signed Int" }
        }
        return if (zeros ==
            0
        ) {
            0
        } else {
            ((1 shl zeros) - 1) + readBits(zeros)
        }
    }

    fun readSE(): Int {
        val codeNum = readUE()
        val value =
            (codeNum + 1) / 2
        return if (codeNum % 2 == 0) -value else value
    }
}
