package com.maleicacid.tvinput.tis

/** Android Tuner資源の所有から分離したsection-filterの判定・集合更新・配送。 */
object SectionFilterPolicy {
    data class CasPids(
        val ecm: Set<com.maleicacid.tvinput.common.TsPid>,
        val emm: Set<com.maleicacid.tvinput.common.TsPid>,
    )

    fun casPidsFor(
        decision: com.maleicacid.tvinput.aribsi.ServicePolicyDecision,
        metadata: List<com.maleicacid.tvinput.aribsi.CaMetadata>,
    ): CasPids = if (!decision.casDecisionReady) CasPids(emptySet(), emptySet()) else CasPids(
        metadata.mapNotNull { it.ecmPid }.toSet(),
        metadata.filter { CasController.SupportedCasSystemIds.supportsEmm(it.caSystemId) }.mapNotNull { it.emmPid }.toSet(),
    )

    fun replaceDynamicPids(
        current: MutableSet<com.maleicacid.tvinput.common.TsPid>,
        next: Set<com.maleicacid.tvinput.common.TsPid>,
        close: (com.maleicacid.tvinput.common.TsPid) -> Unit,
        open: (com.maleicacid.tvinput.common.TsPid) -> Boolean,
    ) {
        (current - next).toList().forEach { pid ->
            current.remove(pid)
            close(pid)
        }
        (next - current).forEach { pid -> if (open(pid)) current += pid }
    }

    fun dispatchSection(
        pid: com.maleicacid.tvinput.common.TsPid,
        siPids: Set<com.maleicacid.tvinput.common.TsPid>,
        ecmPids: Set<com.maleicacid.tvinput.common.TsPid>,
        emmPids: Set<com.maleicacid.tvinput.common.TsPid>,
        onSi: () -> Unit,
        onEcm: () -> Unit,
        onEmm: () -> Unit,
    ) {
        if (pid in siPids) onSi()
        if (pid in ecmPids) onEcm()
        if (pid in emmPids) onEmm()
    }

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
