package com.maleicacid.tvinput.tis

// 境界呼出しの失敗を漏らさず扱い、既存の診断・解放・失敗伝播へ渡す。

/** Android Tuner資源の所有から分離したsection-filterの判定・集合更新・配送。 */
object SectionFilterPolicy {
    fun metadataForCasDecision(
        ready: Boolean,
        metadata: List<com.maleicacid.tvinput.aribsi.CaMetadata>,
    ): List<com.maleicacid.tvinput.aribsi.CaMetadata> = if (ready) metadata else emptyList()

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

    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    @Suppress("SpreadOperator")
    fun replaceDynamicPids(
        current: MutableSet<com.maleicacid.tvinput.common.TsPid>,
        next: Set<com.maleicacid.tvinput.common.TsPid>,
        close: (com.maleicacid.tvinput.common.TsPid) -> Unit,
        open: (com.maleicacid.tvinput.common.TsPid) -> Boolean,
        isOpen: (com.maleicacid.tvinput.common.TsPid) -> Boolean,
    ) {
        completeCleanup(
            *(current - next)
                .map { pid ->
                    {
                        close(pid)
                        current.remove(pid)
                        Unit
                    }
                }.toTypedArray(),
        )
        next.filter { it !in current || !isOpen(it) }.forEach { pid -> if (open(pid)) current += pid }
    }

    /** 他の解放を省略せず、最初の失敗に後続失敗を添えて返す。 */
    @Suppress("TooGenericExceptionCaught")
    fun completeCleanup(vararg actions: () -> Unit) {
        var failure: Exception? = null
        for (action in actions) {
            try {
                action()
            } catch (error: Exception) {
                val primary = failure
                if (primary == null) {
                    failure = error
                } else if (primary !== error) {
                    primary.addSuppressed(error)
                }
            }
        }
        failure?.let { throw it }
    }

    // 境界呼出しの失敗を漏らさず扱い、既存の診断・解放・失敗伝播へ渡す。

    /** 同一controller executor内でmetadata成功後だけfilterを公開し、両方の失敗を清掃する。 */
    @Suppress("TooGenericExceptionCaught")
    internal fun commitCasAndFilters(
        updateCas: () -> CasController.UpdateResult,
        commitFilters: (CasController.UpdateResult) -> Unit,
        reject: () -> Unit,
    ): CasController.UpdateResult {
        val result: CasController.UpdateResult
        try {
            result = updateCas()
            if (result.diagnostics.none { it.state == CasController.State.ERROR }) {
                commitFilters(result)
                return result
            }
        } catch (failure: Exception) {
            try {
                reject()
            } catch (cleanup: Exception) {
                if (cleanup !== failure) failure.addSuppressed(cleanup)
            }
            throw failure
        }
        reject()
        return result
    }

    // 独立した既存入力を明示し、引数数だけを理由に別の状態保持型を導入しない。
    @Suppress("LongParameterList")
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

    fun readDecision(
        expected: Int,
        actual: Int,
        sourceIsCurrent: Boolean,
    ): ReadDecision =
        when {
            !sourceIsCurrent -> ReadDecision.STALE_SOURCE
            expected <= 0 -> ReadDecision.READ_ERROR
            actual == expected -> ReadDecision.INGEST
            actual > 0 -> ReadDecision.SHORT_READ
            else -> ReadDecision.READ_ERROR
        }

    fun dataLengthDecision(dataLength: Long): DataLengthDecision =
        when {
            dataLength <= 0L -> DataLengthDecision.MALFORMED
            dataLength > MAX_SECTION_EVENT_BYTES -> DataLengthDecision.OVERSIZED
            else -> DataLengthDecision.ACCEPT
        }
}
