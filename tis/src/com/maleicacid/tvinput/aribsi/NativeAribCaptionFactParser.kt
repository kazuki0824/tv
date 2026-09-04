package com.maleicacid.tvinput.aribsi

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** STD-B24 binary parsing is owned by the Rust caption JNI; Kotlin only decodes this private typed packet. */
class NativeAribCaptionFactParser(
    superimpose: Boolean,
) : AutoCloseable {
    data class Language(
        val languageTag: Int,
        val iso639LanguageCode: String,
        val dmf: Int,
        val automaticPresentationOnReception: Boolean,
        val displayCondition: Int?,
        val format: Int,
        val tcs: Int,
        val rollupMode: Int,
    )

    data class Management(
        val tmd: Int,
        val languages: List<Language>,
    )

    data class StatementTime(
        val tmd: Int,
        val millisOfDay: Long,
    )

    enum class Disposition {
        NONE,
        FRAGMENT_PENDING,
        MANAGEMENT,
        STATEMENT_TIMED,
        STATEMENT_INVALID,
        INVALID,
    }

    data class FactBatch(
        val disposition: Disposition,
        val management: Management?,
        val statementTime: StatementTime?,
    )

    init {
        System.loadLibrary("maleicacid_arib_caption_jni")
    }

    private var handle: Long = nativeCreateFactParser(superimpose)

    fun ingest(pesPayload: ByteArray): FactBatch? {
        val current = handle.takeIf { it != 0L } ?: return null
        if (pesPayload.isEmpty()) return null
        return nativeIngestFactParser(current, pesPayload)?.let(::decodePacket)
    }

    fun reset() {
        handle.takeIf { it != 0L }?.let(::nativeResetFactParser)
    }

    override fun close() {
        val current = handle
        handle = 0L
        if (current != 0L) nativeReleaseFactParser(current)
    }

    private external fun nativeCreateFactParser(superimpose: Boolean): Long
    private external fun nativeIngestFactParser(handle: Long, pesPayload: ByteArray): ByteArray?
    private external fun nativeResetFactParser(handle: Long)
    private external fun nativeReleaseFactParser(handle: Long)

    companion object {
        private const val PACKET_VERSION = 2
        private const val FLAG_MANAGEMENT = 1
        private const val FLAG_STATEMENT_TIME = 2
        private const val LANGUAGE_BYTES = 10

        internal fun decodePacket(packet: ByteArray): FactBatch? {
            if (packet.size < 3) return null
            val buffer = ByteBuffer.wrap(packet).order(ByteOrder.LITTLE_ENDIAN)
            if (buffer.get().toInt() and 0xff != PACKET_VERSION) return null
            val flags = buffer.get().toInt() and 0xff
            val disposition = when (buffer.get().toInt() and 0xff) {
                0 -> Disposition.NONE
                1 -> Disposition.FRAGMENT_PENDING
                2 -> Disposition.MANAGEMENT
                3 -> Disposition.STATEMENT_TIMED
                4 -> Disposition.STATEMENT_INVALID
                5 -> Disposition.INVALID
                else -> return null
            }
            var statementTime: StatementTime? = null
            if (flags and FLAG_STATEMENT_TIME != 0) {
                if (buffer.remaining() < 5) return null
                val tmd = buffer.get().toInt() and 0xff
                val millisOfDay = buffer.int.toLong() and 0xffff_ffffL
                if (millisOfDay >= 24L * 60L * 60L * 1_000L) return null
                statementTime = StatementTime(tmd, millisOfDay)
            }
            var management: Management? = null
            if (flags and FLAG_MANAGEMENT != 0) {
                if (buffer.remaining() < 2) return null
                val tmd = buffer.get().toInt() and 0xff
                val count = buffer.get().toInt() and 0xff
                if (count > 8 || buffer.remaining() < count * LANGUAGE_BYTES) return null
                val languages = ArrayList<Language>(count)
                repeat(count) {
                    val languageTag = buffer.get().toInt() and 0xff
                    val dmf = buffer.get().toInt() and 0xff
                    val automatic = buffer.get().toInt() and 0xff
                    val dcRaw = buffer.get().toInt() and 0xff
                    val format = buffer.get().toInt() and 0xff
                    val tcs = buffer.get().toInt() and 0xff
                    val rollup = buffer.get().toInt() and 0xff
                    val iso = ByteArray(3)
                    buffer.get(iso)
                    if (languageTag !in 0..7 || automatic !in 0..1 || !iso.all { byte ->
                            val value = byte.toInt() and 0xff
                            value in 'A'.code..'Z'.code || value in 'a'.code..'z'.code
                        }) return null
                    languages += Language(
                        languageTag = languageTag,
                        iso639LanguageCode = iso.toString(Charsets.ISO_8859_1).lowercase(),
                        dmf = dmf,
                        automaticPresentationOnReception = automatic != 0,
                        displayCondition = dcRaw.takeUnless { it == 0xff },
                        format = format,
                        tcs = tcs,
                        rollupMode = rollup,
                    )
                }
                management = Management(tmd, languages.sortedBy { it.languageTag })
            }
            if (buffer.hasRemaining()) return null
            if (disposition == Disposition.STATEMENT_TIMED && statementTime == null) return null
            return FactBatch(disposition = disposition, management = management, statementTime = statementTime)
        }
    }
}
