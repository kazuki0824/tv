package com.maleicacid.tvinput.aribsi

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** STD-B24のbinary parseはRust caption JNIが所有し、Kotlinはこのprivate typed packetのdecodeだけを担当する。 */
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
            if ((buffer.get().toInt() and 0xff) != PACKET_VERSION) return null
            val disposition = when (buffer.get().toInt() and 0xff) {
                0 -> Disposition.NONE
                1 -> Disposition.FRAGMENT_PENDING
                2 -> Disposition.MANAGEMENT
                3 -> Disposition.STATEMENT_TIMED
                4 -> Disposition.STATEMENT_INVALID
                5 -> Disposition.INVALID
                else -> return null
            }
            val flags = buffer.get().toInt() and 0xff
            val management = if ((flags and FLAG_MANAGEMENT) != 0) decodeManagement(buffer) else null
            val statement = if ((flags and FLAG_STATEMENT_TIME) != 0) decodeStatementTime(buffer) else null
            if (buffer.hasRemaining()) return null
            return FactBatch(disposition, management, statement)
        }

        private fun decodeManagement(buffer: ByteBuffer): Management? {
            if (buffer.remaining() < 2) return null
            val tmd = buffer.get().toInt() and 0xff
            val count = buffer.get().toInt() and 0xff
            if (buffer.remaining() < count * LANGUAGE_BYTES) return null
            val languages = buildList {
                repeat(count) {
                    val tag = buffer.get().toInt() and 0xff
                    val dmf = buffer.get().toInt() and 0xff
                    val automatic = (buffer.get().toInt() and 0xff) != 0
                    val displayRaw = buffer.get().toInt() and 0xff
                    val languageBytes = ByteArray(3)
                    buffer.get(languageBytes)
                    val iso639 = languageBytes.toString(Charsets.US_ASCII)
                    val format = buffer.get().toInt() and 0xff
                    val tcs = buffer.get().toInt() and 0xff
                    val rollup = buffer.get().toInt() and 0xff
                    add(
                        Language(
                            languageTag = tag,
                            iso639LanguageCode = iso639,
                            dmf = dmf,
                            automaticPresentationOnReception = automatic,
                            displayCondition = displayRaw.takeUnless { it == 0xff },
                            format = format,
                            tcs = tcs,
                            rollupMode = rollup,
                        ),
                    )
                }
            }
            return Management(tmd, languages)
        }

        private fun decodeStatementTime(buffer: ByteBuffer): StatementTime? {
            if (buffer.remaining() < 9) return null
            val tmd = buffer.get().toInt() and 0xff
            val millis = buffer.long
            return StatementTime(tmd, millis)
        }
    }
}
