package com.maleicacid.tvinput.tis

/** Android Tuner資源所有から独立したsection-filterの純粋判定。 */
object SectionFilterPolicy {
    const val MAX_SECTION_EVENT_BYTES = 4096L

    enum class ReadDecision { INGEST, SHORT_READ, READ_ERROR, STALE_SOURCE }
    enum class DataLengthDecision { ACCEPT, MALFORMED, OVERSIZED }

    fun readDecision(expected: Int, actual: Int, sourceIsCurrent: Boolean): ReadDecision = when {
        !sourceIsCurrent -> ReadDecision.STALE_SOURCE
        expected <= 0 -> ReadDecision.READ_ERROR
        actual == expected -> ReadDecision.INGEST
        actual > 0 -> ReadDecision.SHORT_READ
        else -> ReadDecision.READ_ERROR
    }

    fun dataLengthDecision(dataLength: Long): DataLengthDecision = when {
        dataLength <= 0L -> DataLengthDecision.MALFORMED
        dataLength > MAX_SECTION_EVENT_BYTES -> DataLengthDecision.OVERSIZED
        else -> DataLengthDecision.ACCEPT
    }
}
