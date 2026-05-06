package com.maleicacid.tvinput.aribsi

class AribTextDecoder(private val engine: AribSiEngine) {
    fun decode(bytes: ByteArray): String = engine.decodeAribString(bytes)
}
