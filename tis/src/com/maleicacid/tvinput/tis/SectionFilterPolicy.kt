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

    /** registryの所有はTunerControllerに残し、解放成功まで置換しない。 */
    internal fun openOwnedFilter(
        pid: com.maleicacid.tvinput.common.TsPid,
        handles: MutableMap<com.maleicacid.tvinput.common.TsPid, TunerController.SectionFilterHandle>,
        create: () -> TunerController.SectionFilterHandle,
    ): TunerController.SectionFilterHandle {
        handles[pid]?.let { existing ->
            if (existing.isOpen) return existing
            existing.close()
            handles.remove(pid)
        }
        val handle = create()
        if (handle.isOpen) handles[pid] = handle
        return handle
    }

    fun replaceDynamicPids(
        current: MutableSet<com.maleicacid.tvinput.common.TsPid>,
        next: Set<com.maleicacid.tvinput.common.TsPid>,
        close: (com.maleicacid.tvinput.common.TsPid) -> Unit,
        open: (com.maleicacid.tvinput.common.TsPid) -> Boolean,
        isOpen: (com.maleicacid.tvinput.common.TsPid) -> Boolean,
    ) {
        completeCleanup(*(current - next).map { pid -> {
            close(pid)
            current.remove(pid)
            Unit
        } }.toTypedArray())
        next.filter { it !in current || !isOpen(it) }.forEach { pid -> if (open(pid)) current += pid }
    }

    /** 他の解放を省略せず、最初の失敗に後続失敗を添えて返す。 */
    fun completeCleanup(vararg actions: () -> Unit) {
        var failure: Exception? = null
        for (action in actions) {
            try { action() } catch (error: Exception) {
                val primary = failure
                if (primary == null) failure = error else if (primary !== error) primary.addSuppressed(error)
            }
        }
        failure?.let { throw it }
    }

    fun updateFiltersAndStopOnFailure(
        casDecisionReady: Boolean,
        updateFilters: () -> Unit,
        clearCas: () -> Unit,
        stopPlayback: () -> Unit,
    ) {
        var updateFailed = false
        completeCleanup(
            { try { updateFilters() } catch (error: Exception) { updateFailed = true; throw error } },
            { if (!casDecisionReady || updateFailed) completeCleanup(clearCas, stopPlayback) },
        )
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
