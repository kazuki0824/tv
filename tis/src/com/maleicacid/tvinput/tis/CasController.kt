package com.maleicacid.tvinput.tis

import android.media.tv.tuner.Descrambler
import android.media.tv.tuner.Tuner
import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

// 同じ状態・境界を扱う操作群を一つの所有者に保つ。

/**
 * B25/B1 向け CAS 制御。
 * PMT/CAT の CA情報 は arib_si_engine_rs の snapshot から受ける。
 * ECM/EMM は完全な section として扱い、生 TS packet は扱わない。
 * カード I/O、CW 生成、鍵発行は MediaCas/CAS HAL 側の責務とする。
 * Tuner HAL には opaque token と ES PID 登録だけを渡す。
 */
@Suppress("TooManyFunctions")
class CasController(
    private val supportedSystemIds: Set<Int> = SupportedCasSystemIds.B25_B1,
    private val mediaCasFactory: MediaCasBridgeFactory = FrameworkMediaCasBridgeFactory(),
) : AutoCloseable {
    enum class ErrorCode {
        NONE,
        UNSUPPORTED_SYSTEM_ID,
        PLUGIN_UNAVAILABLE,
        SESSION_OPEN_FAILED,
        PRIVATE_DATA_FAILED,
        ECM_FAILED,
        EMM_FAILED,
        KEY_TOKEN_MISSING,
        INVALID_KEY_TOKEN,
        DESCRAMBLER_FAILED,
        CLOSED,
    }

    enum class State { IDLE, ACTIVE, ERROR, CLOSED }

    enum class Readiness { CLEAR, WAITING_FOR_KEY, READY, ERROR, CLOSED }

    data class Diagnostic(
        val state: State,
        val errorCode: ErrorCode = ErrorCode.NONE,
        val caSystemId: Int? = null,
        val pid: TsPid? = null,
        val message: String = "",
    )

    data class UpdateResult(
        val diagnostics: List<Diagnostic>,
        val ecmPids: Set<TsPid>,
        val emmPids: Set<TsPid>,
        val readiness: Readiness = Readiness.CLEAR,
    )

    interface MediaCasBridgeFactory {
        fun create(caSystemId: Int): Result<MediaCasBridge>
    }

    interface MediaCasBridge : AutoCloseable {
        fun setPrivateData(privateData: ByteArray): Result<Unit>

        fun openSession(): Result<MediaCasSessionBridge>

        fun processEmm(section: ByteArray): Result<Unit>

        override fun close()
    }

    interface MediaCasSessionBridge : AutoCloseable {
        fun setPrivateData(privateData: ByteArray): Result<Unit>

        fun processEcm(section: ByteArray): Result<EcmProcessResult>

        override fun close()
    }

    interface TunerDescramblerBridge : AutoCloseable {
        fun setKeyToken(keyToken: TunerKeyToken): Result<Unit>

        fun clearKeyToken(): Result<Unit> = Result.success(Unit)

        fun addPid(elementaryPid: TsPid): Result<Unit>

        fun removePid(elementaryPid: TsPid): Result<Unit>

        fun newSibling(): Result<TunerDescramblerBridge> =
            Result.failure(
                UnsupportedOperationException("independent descrambler creation is unsupported"),
            )

        override fun close()
    }

    private data class EsCaBinding(
        val serviceKeyText: String,
        val caSystemId: Int,
        val ecmPid: TsPid,
        val elementaryPid: TsPid,
        val privateData: ByteArray,
    )

    private data class ProgramCaBinding(
        val serviceKeyText: String,
        val caSystemId: Int,
        val ecmPid: TsPid,
        val privateData: ByteArray,
    )

    private data class EmmBinding(
        val caSystemId: Int,
        val emmPid: TsPid,
        val privateData: ByteArray,
    )

    /** 同じ鍵を使用する複数PIDを、一つのSessionとDescramblerへ結び付ける。 */
    private data class DescrambleContextKey(
        val serviceKeyText: String,
        val caSystemId: Int,
        val ecmPid: TsPid,
        val privateData: List<Byte>,
    )

    private data class ContextPlan(
        val key: DescrambleContextKey,
        val elementaryPids: Set<TsPid>,
    )

    private data class CasSessionState(
        val key: DescrambleContextKey,
        val session: MediaCasSessionBridge,
        var descrambler: TunerDescramblerBridge? = null,
        val desiredElementaryPids: MutableSet<TsPid> = linkedSetOf(),
        val linkedElementaryPids: MutableSet<TsPid> = linkedSetOf(),
        var keyLinked: Boolean = false,
        var ecmReady: Boolean = false,
        var retiring: Boolean = false,
        var sessionClosed: Boolean = false,
        var descramblerClosed: Boolean = false,
    )

    @Volatile private var executorThread: Thread? = null
    private val executor: ExecutorService =
        Executors.newSingleThreadExecutor { runnable ->
            Thread(runnable, "maleicacid-cas-controller").also { thread ->
                thread.isDaemon = true
                executorThread = thread
            }
        }

    private fun <T> onExecutor(block: () -> T): T {
        if (Thread.currentThread() === executorThread) return block()
        check(!executor.isShutdown) { "CasController executor は停止済みです" }
        return executor.submit<T> { block() }.get()
    }

    private val pluginsBySystemId = LinkedHashMap<Int, MediaCasBridge>()
    private val sessionsByContext = LinkedHashMap<DescrambleContextKey, CasSessionState>()
    private val ecmPidToContexts = LinkedHashMap<TsPid, MutableSet<DescrambleContextKey>>()
    private val emmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private val retiringPlugins = linkedSetOf<Int>()
    private var resourceLossCleanupPending = false
    private var closed = false

    @Volatile private var lastDiagnostic = Diagnostic(State.IDLE)

    /** 解放失敗中の所有を保持し、旧ECM/EMMの配送を遮断する。 */
    fun clearForResourceLoss(): Unit = onExecutor { clearForResourceLossLocked() }

    private fun clearForResourceLossLocked() {
        resourceLossCleanupPending = true
        try {
            clearForClearServiceLocked()
            resourceLossCleanupPending = false
        } finally {
            invalidateMetadataLocked()
        }
    }

    fun clearForClearService(): Unit = onExecutor { clearForClearServiceLocked() }

    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    @Suppress("SpreadOperator")
    private fun clearForClearServiceLocked() {
        invalidateMetadataLocked()
        sessionsByContext.values.forEach { it.retiring = true }
        retiringPlugins += pluginsBySystemId.keys
        retryRetiredResourcesLocked()
        lastDiagnostic = Diagnostic(State.IDLE)
    }

    private fun retryRetiredResourcesLocked() {
        SectionFilterPolicy.completeCleanup(
            {
                SectionFilterPolicy.completeCleanup(
                    *sessionsByContext.values
                        .filter { it.retiring }
                        .map { state -> { closeContextLocked(state.key) } }
                        .toTypedArray(),
                )
            },
            {
                SectionFilterPolicy.completeCleanup(
                    *retiringPlugins
                        .toList()
                        .map { systemId -> { closePluginLocked(systemId) } }
                        .toTypedArray(),
                )
            },
        )
    }

    private fun closePluginLocked(systemId: Int) {
        retiringPlugins += systemId
        pluginsBySystemId[systemId]?.close()
        pluginsBySystemId.remove(systemId)
        retiringPlugins.remove(systemId)
    }

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    // 境界呼出しの失敗を漏らさず扱い、既存の診断・解放・失敗伝播へ渡す。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MaxLineLength", "SpreadOperator", "TooGenericExceptionCaught")
    internal fun updateFromCaMetadata(
        metadata: List<CaMetadata>,
        createDescrambler: (() -> TunerDescramblerBridge)? = null,
    ): UpdateResult =
        onExecutor {
            if (closed) {
                return@onExecutor UpdateResult(
                    listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, message = "CAS 制御は終了済みです")),
                    emptySet(),
                    emptySet(),
                    Readiness.CLOSED,
                )
            }
            // metadata全体と解放処理が成功するまで配送表を再公開しない。
            invalidateMetadataLocked()
            if (metadata.isEmpty()) {
                clearForClearServiceLocked()
                return@onExecutor UpdateResult(emptyList(), emptySet(), emptySet(), Readiness.CLEAR)
            }
            if (resourceLossCleanupPending) clearForResourceLossLocked()
            retryRetiredResourcesLocked()

            val diagnostics = mutableListOf<Diagnostic>()
            val programBindings = mutableListOf<ProgramCaBinding>()
            val esBindings = mutableListOf<EsCaBinding>()
            val emmBindings = mutableListOf<EmmBinding>()
            metadata.forEach { ca ->
                if (ca.caSystemId !in supportedSystemIds) {
                    diagnostics +=
                        Diagnostic(
                            State.ERROR,
                            ErrorCode.UNSUPPORTED_SYSTEM_ID,
                            ca.caSystemId,
                            ca.ecmPid ?: ca.emmPid ?: ca.elementaryPid,
                            "B25/B1 対象外の CA_system_id です",
                        )
                    return@forEach
                }
                when (ca.source) {
                    CaMetadataSource.PROGRAM -> {
                        val serviceKey = ca.serviceKey ?: return@forEach
                        val ecmPid = ca.ecmPid ?: return@forEach
                        programBindings +=
                            ProgramCaBinding(
                                serviceKey.toString(),
                                ca.caSystemId,
                                ecmPid,
                                ca.privateData.copyOf(),
                            )
                    }

                    CaMetadataSource.ELEMENTARY_STREAM -> {
                        val serviceKey = ca.serviceKey ?: return@forEach
                        val ecmPid = ca.ecmPid ?: return@forEach
                        val elementaryPid = ca.elementaryPid ?: return@forEach
                        esBindings +=
                            EsCaBinding(
                                serviceKey.toString(),
                                ca.caSystemId,
                                ecmPid,
                                elementaryPid,
                                ca.privateData.copyOf(),
                            )
                    }

                    CaMetadataSource.CAT -> {
                        if (!SupportedCasSystemIds.supportsEmm(ca.caSystemId)) return@forEach
                        val emmPid = ca.emmPid ?: return@forEach
                        emmBindings += EmmBinding(ca.caSystemId, emmPid, ca.privateData.copyOf())
                    }
                }
            }

            val contextPlans =
                esBindings.groupBy { it.contextKey() }.mapValues { (key, bindings) ->
                    ContextPlan(key, bindings.map { it.elementaryPid }.toSet())
                }
            val requiresDescrambling = programBindings.isNotEmpty() || esBindings.isNotEmpty()

            val obsolete = sessionsByContext.values.filter { it.key !in contextPlans }
            obsolete.forEach { it.retiring = true }
            val targetSystems = contextPlans.keys.map { it.caSystemId }.toSet() + emmBindings.map { it.caSystemId }
            retiringPlugins += pluginsBySystemId.keys.filter { it !in targetSystems }
            retryRetiredResourcesLocked()
            contextPlans.values.forEach { plan -> ensureContextLocked(plan, createDescrambler, diagnostics) }

            emmBindings.forEach { binding ->
                val plugin = ensurePluginLocked(binding.caSystemId, diagnostics)
                if (plugin != null) {
                    plugin.setPrivateData(binding.privateData).onFailure { error ->
                        diagnostics +=
                            Diagnostic(
                                State.ERROR,
                                ErrorCode.PRIVATE_DATA_FAILED,
                                binding.caSystemId,
                                binding.emmPid,
                                error.message.orEmpty(),
                            )
                    }
                }
            }
            if (diagnostics.isEmpty()) {
                rebuildPidIndexesLocked()
                emmBindings.filter { it.caSystemId in pluginsBySystemId && it.caSystemId !in retiringPlugins }.forEach { binding ->
                    emmPidToSystems.getOrPut(binding.emmPid) { linkedSetOf() } += binding.caSystemId
                }
            }
            val ecmPids =
                if (diagnostics.isEmpty()) {
                    (programBindings.map { it.ecmPid } + esBindings.map { it.ecmPid }).toSet()
                } else {
                    emptySet()
                }
            val readiness = readinessLocked(requiresDescrambling, diagnostics)
            lastDiagnostic =
                when (readiness) {
                    Readiness.ERROR -> diagnostics.lastOrNull() ?: Diagnostic(State.ERROR, message = "CAS 構成に失敗しました")

                    Readiness.CLOSED -> Diagnostic(State.CLOSED)

                    Readiness.CLEAR -> Diagnostic(State.IDLE)

                    Readiness.WAITING_FOR_KEY,
                    Readiness.READY,
                    -> Diagnostic(State.ACTIVE)
                }
            UpdateResult(diagnostics, ecmPids, emmPidToSystems.keys.toSet(), readiness)
        }

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MaxLineLength")
    fun onEcmSection(
        pid: TsPid,
        section: ByteArray,
    ): List<Diagnostic> =
        onExecutor {
            if (closed) {
                return@onExecutor listOf(
                    Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"),
                )
            }
            val contextKeys = ecmPidToContexts[pid].orEmpty().toList()
            if (contextKeys.isEmpty()) return@onExecutor emptyList()
            val diagnostics = mutableListOf<Diagnostic>()
            contextKeys.forEach { key ->
                val state = sessionsByContext[key]?.takeUnless { it.retiring } ?: return@forEach
                // 鍵の所有と直近ECMの成否を分け、metadata再通知だけではREADYへ戻さない。
                state.ecmReady = false
                val tokenResult = state.session.processEcm(section)
                if (tokenResult.isFailure) {
                    diagnostics +=
                        Diagnostic(
                            State.ERROR,
                            ErrorCode.ECM_FAILED,
                            key.caSystemId,
                            pid,
                            tokenResult.exceptionOrNull()?.message.orEmpty(),
                        )
                    return@forEach
                }
                when (val result = tokenResult.getOrNull()) {
                    is EcmProcessResult.RealKeyToken -> {
                        if (!state.keyLinked) linkContextKeyLocked(state, result.token, pid, diagnostics)
                        state.ecmReady = state.keyLinked
                    }

                    is EcmProcessResult.InvalidKeyToken -> {
                        diagnostics +=
                            Diagnostic(
                                State.ERROR,
                                ErrorCode.INVALID_KEY_TOKEN,
                                key.caSystemId,
                                pid,
                                result.message,
                            )
                    }

                    is EcmProcessResult.DiagnosticOnly -> {
                        diagnostics +=
                            Diagnostic(
                                State.ERROR,
                                ErrorCode.KEY_TOKEN_MISSING,
                                key.caSystemId,
                                pid,
                                result.message,
                            )
                    }

                    null -> {
                        diagnostics +=
                            Diagnostic(
                                State.ERROR,
                                ErrorCode.KEY_TOKEN_MISSING,
                                key.caSystemId,
                                pid,
                                "MediaCas session から実 key token を取得できません",
                            )
                    }
                }
            }
            if (diagnostics.isNotEmpty()) lastDiagnostic = diagnostics.last()
            diagnostics
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    fun onEmmSection(
        pid: TsPid,
        section: ByteArray,
    ): List<Diagnostic> =
        onExecutor {
            if (closed) {
                return@onExecutor listOf(
                    Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"),
                )
            }
            val systems = emmPidToSystems[pid].orEmpty().filter { it == SupportedCasSystemIds.ARIB_STD_B25 }
            if (systems.isEmpty()) return@onExecutor emptyList()
            val diagnostics = mutableListOf<Diagnostic>()
            systems.forEach { systemId ->
                val plugin = pluginsBySystemId[systemId]?.takeUnless { systemId in retiringPlugins }
                if (plugin == null) {
                    diagnostics +=
                        Diagnostic(
                            State.ERROR,
                            ErrorCode.PLUGIN_UNAVAILABLE,
                            systemId,
                            pid,
                            "MediaCas plugin を利用できません",
                        )
                    return@forEach
                }
                plugin.processEmm(section).onFailure { error ->
                    diagnostics +=
                        Diagnostic(
                            State.ERROR,
                            ErrorCode.EMM_FAILED,
                            systemId,
                            pid,
                            error.message.orEmpty(),
                        )
                }
            }

            if (diagnostics.isNotEmpty()) lastDiagnostic = diagnostics.last()
            diagnostics
        }

    fun lastDiagnostic(): Diagnostic = lastDiagnostic

    fun currentReadiness(): Readiness {
        if (executor.isShutdown) return Readiness.CLOSED
        return onExecutor {
            when {
                closed -> Readiness.CLOSED
                sessionsByContext.isEmpty() -> Readiness.CLEAR
                sessionsByContext.values.all { it.isFullyLinked() } -> Readiness.READY
                else -> Readiness.WAITING_FOR_KEY
            }
        }
    }

    private fun EsCaBinding.contextKey(): DescrambleContextKey =
        DescrambleContextKey(
            serviceKeyText,
            caSystemId,
            ecmPid,
            privateData.toList(),
        )

    private fun ensurePluginLocked(
        caSystemId: Int,
        diagnostics: MutableList<Diagnostic>,
    ): MediaCasBridge? {
        if (caSystemId in retiringPlugins) {
            diagnostics +=
                Diagnostic(
                    State.ERROR,
                    ErrorCode.PLUGIN_UNAVAILABLE,
                    caSystemId,
                    message = "CAS資源は解放再試行待ちです",
                )
            return null
        }
        pluginsBySystemId[caSystemId]?.let { return it }
        val plugin =
            mediaCasFactory.create(caSystemId).getOrElse { error ->
                diagnostics +=
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.PLUGIN_UNAVAILABLE,
                        caSystemId,
                        message = error.message.orEmpty(),
                    )
                return null
            }
        pluginsBySystemId[caSystemId] = plugin
        return plugin
    }

    private fun ensureContextLocked(
        plan: ContextPlan,
        createDescrambler: (() -> TunerDescramblerBridge)?,
        diagnostics: MutableList<Diagnostic>,
    ) {
        val existing = sessionsByContext[plan.key]
        if (existing != null) {
            check(!existing.retiring) { "CAS contextは解放再試行待ちです" }
            syncContextPidsLocked(existing, plan.elementaryPids, diagnostics)
            return
        }
        val plugin = ensurePluginLocked(plan.key.caSystemId, diagnostics) ?: return
        val session =
            plugin.openSession().getOrElse { error ->
                if (sessionsByContext.keys.none { it.caSystemId == plan.key.caSystemId }) {
                    try {
                        closePluginLocked(plan.key.caSystemId)
                    } catch (
                        cleanup: Exception,
                    ) {
                        if (cleanup !== error) error.addSuppressed(cleanup)
                    }
                }
                diagnostics +=
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.SESSION_OPEN_FAILED,
                        plan.key.caSystemId,
                        plan.key.ecmPid,
                        error.message.orEmpty(),
                    )
                return
            }
        // 設定やDescrambler生成の例外より先に、取得したSessionの所有を確定する。
        val state =
            CasSessionState(
                plan.key,
                session,
                desiredElementaryPids = plan.elementaryPids.toMutableSet(),
            )
        sessionsByContext[plan.key] = state
        var failureCode = ErrorCode.PRIVATE_DATA_FAILED
        try {
            session.setPrivateData(plan.key.privateData.toByteArray()).getOrThrow()
            failureCode = ErrorCode.DESCRAMBLER_FAILED
            state.descrambler = requireNotNull(createDescrambler) { "Tuner descrambler を利用できません" }.invoke()
        } catch (failure: Exception) {
            state.retiring = true
            try {
                closeContextLocked(plan.key)
            } catch (cleanup: Exception) {
                if (cleanup !== failure) failure.addSuppressed(cleanup)
            }
            diagnostics +=
                Diagnostic(
                    State.ERROR,
                    failureCode,
                    plan.key.caSystemId,
                    plan.key.ecmPid,
                    failure.message.orEmpty(),
                )
        }
    }

    private fun syncContextPidsLocked(
        state: CasSessionState,
        targetPids: Set<TsPid>,
        diagnostics: MutableList<Diagnostic>,
    ) {
        state.desiredElementaryPids.clear()
        state.desiredElementaryPids += targetPids
        (state.linkedElementaryPids - state.desiredElementaryPids).forEach { pid ->
            val removeResult = requireNotNull(state.descrambler).removePid(pid)
            if (removeResult.isSuccess) {
                state.linkedElementaryPids.remove(pid)
            } else {
                val error = removeResult.exceptionOrNull()
                diagnostics +=
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.DESCRAMBLER_FAILED,
                        state.key.caSystemId,
                        pid,
                        error?.message.orEmpty(),
                    )
            }
        }
        if (state.keyLinked) {
            (state.desiredElementaryPids - state.linkedElementaryPids).forEach { pid ->
                val addResult = requireNotNull(state.descrambler).addPid(pid)
                if (addResult.isSuccess) {
                    state.linkedElementaryPids += pid
                } else {
                    val error = addResult.exceptionOrNull()
                    diagnostics +=
                        Diagnostic(
                            State.ERROR,
                            ErrorCode.DESCRAMBLER_FAILED,
                            state.key.caSystemId,
                            pid,
                            error?.message.orEmpty(),
                        )
                }
            }
        }
    }

    private fun linkContextKeyLocked(
        state: CasSessionState,
        token: TunerKeyToken,
        ecmPid: TsPid,
        diagnostics: MutableList<Diagnostic>,
    ) {
        val tokenResult = requireNotNull(state.descrambler).setKeyToken(token)
        if (tokenResult.isFailure) {
            diagnostics +=
                Diagnostic(
                    State.ERROR,
                    ErrorCode.DESCRAMBLER_FAILED,
                    state.key.caSystemId,
                    ecmPid,
                    tokenResult.exceptionOrNull()?.message.orEmpty(),
                )
            return
        }
        state.keyLinked = true
        var failed = false
        (state.desiredElementaryPids - state.linkedElementaryPids).forEach { elementaryPid ->
            val addResult = requireNotNull(state.descrambler).addPid(elementaryPid)
            if (addResult.isFailure) {
                failed = true
                diagnostics +=
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.DESCRAMBLER_FAILED,
                        state.key.caSystemId,
                        elementaryPid,
                        addResult.exceptionOrNull()?.message.orEmpty(),
                    )
            } else {
                state.linkedElementaryPids += elementaryPid
            }
        }
        if (failed) {
            state.linkedElementaryPids.toList().forEach { pid ->
                val removeResult = requireNotNull(state.descrambler).removePid(pid)
                if (removeResult.isSuccess) {
                    state.linkedElementaryPids.remove(pid)
                } else {
                    diagnostics +=
                        Diagnostic(
                            State.ERROR,
                            ErrorCode.DESCRAMBLER_FAILED,
                            state.key.caSystemId,
                            pid,
                            removeResult.exceptionOrNull()?.message.orEmpty(),
                        )
                }
            }
            val clearResult = requireNotNull(state.descrambler).clearKeyToken()
            if (clearResult.isSuccess) {
                state.keyLinked = false
            } else {
                diagnostics +=
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.DESCRAMBLER_FAILED,
                        state.key.caSystemId,
                        ecmPid,
                        clearResult.exceptionOrNull()?.message.orEmpty(),
                    )
            }
            return
        }
    }

    private fun CasSessionState.isFullyLinked(): Boolean =
        !retiring && ecmReady && keyLinked && desiredElementaryPids == linkedElementaryPids

    private fun invalidateMetadataLocked() {
        ecmPidToContexts.clear()
        emmPidToSystems.clear()
    }

    private fun rebuildPidIndexesLocked() {
        invalidateMetadataLocked()
        sessionsByContext.values.filterNot { it.retiring }.forEach { state ->
            ecmPidToContexts.getOrPut(state.key.ecmPid) { linkedSetOf() } += state.key
        }
    }

    private fun closeContextLocked(key: DescrambleContextKey) {
        val state = sessionsByContext[key] ?: return
        state.retiring = true
        ecmPidToContexts.values.forEach { it.remove(key) }
        ecmPidToContexts.entries.removeAll { it.value.isEmpty() }
        // PID、鍵、Descrambler、Sessionの順に全解放を試行し、失敗した所有だけを残す。
        SectionFilterPolicy.completeCleanup(
            {
                SectionFilterPolicy.completeCleanup(
                    *state.linkedElementaryPids
                        .toList()
                        .map { pid ->
                            {
                                requireNotNull(state.descrambler)
                                    .removePid(pid)
                                    .onFailure { failure ->
                                        lastDiagnostic =
                                            Diagnostic(
                                                State.ERROR,
                                                ErrorCode.DESCRAMBLER_FAILED,
                                                key.caSystemId,
                                                pid,
                                                failure.message.orEmpty(),
                                            )
                                    }.getOrThrow()
                                state.linkedElementaryPids.remove(pid)
                                Unit
                            }
                        }.toTypedArray(),
                )
            },
            {
                if (state.keyLinked) {
                    requireNotNull(state.descrambler).clearKeyToken().getOrThrow()
                    state.keyLinked = false
                }
            },
            {
                if (!state.descramblerClosed) {
                    state.descrambler?.close()
                    state.descramblerClosed = true
                    state.linkedElementaryPids.clear()
                    state.keyLinked = false
                }
            },
            {
                if (!state.sessionClosed) {
                    state.session.close()
                    state.sessionClosed = true
                }
            },
        )
        sessionsByContext.remove(key)
    }

    private fun readinessLocked(
        requiresDescrambling: Boolean,
        diagnostics: List<Diagnostic>,
    ): Readiness {
        if (closed) return Readiness.CLOSED
        if (diagnostics.any { it.isBlockingForPlayback() }) return Readiness.ERROR
        if (!requiresDescrambling) return Readiness.CLEAR
        if (sessionsByContext.isEmpty()) return Readiness.WAITING_FOR_KEY
        return if (sessionsByContext.values.all { it.isFullyLinked() }) Readiness.READY else Readiness.WAITING_FOR_KEY
    }

    private fun Diagnostic.isBlockingForPlayback(): Boolean =
        when (errorCode) {
            ErrorCode.NONE,
            ErrorCode.EMM_FAILED,
            -> false

            else -> state == State.ERROR
        }

    override fun close() {
        if (executor.isShutdown) return
        onExecutor {
            closed = true
            clearForResourceLossLocked()
            lastDiagnostic = Diagnostic(State.CLOSED)
        }
        // 解放失敗時は所有とexecutorを残し、close()を再試行できるようにする。
        executor.shutdown()
    }

    fun release() = close()

    object SupportedCasSystemIds {
        const val ARIB_STD_B25 = 0x0005
        const val ARIB_STD_B1 = 0x0001
        val B25_B1: Set<Int> = setOf(ARIB_STD_B25, ARIB_STD_B1)

        fun supportsEmm(caSystemId: Int): Boolean = caSystemId == ARIB_STD_B25
    }
}

sealed class EcmProcessResult {
    data class RealKeyToken(
        val token: TunerKeyToken,
    ) : EcmProcessResult()

    data class InvalidKeyToken(
        val message: String,
    ) : EcmProcessResult()

    data class DiagnosticOnly(
        val message: String,
    ) : EcmProcessResult()
}

class FrameworkMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
    override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> =
        runCatching {
            FrameworkMediaCasBridge(caSystemId)
        }
}

private class FrameworkMediaCasBridge(
    caSystemId: Int,
) : CasController.MediaCasBridge {
    private val mediaCas = android.media.MediaCas(caSystemId)

    override fun setPrivateData(privateData: ByteArray): Result<Unit> =
        runCatching {
            mediaCas.setPrivateData(privateData)
        }

    override fun openSession(): Result<CasController.MediaCasSessionBridge> =
        runCatching {
            FrameworkMediaCasSessionBridge(mediaCas.openSession())
        }

    override fun processEmm(section: ByteArray): Result<Unit> =
        runCatching {
            mediaCas.processEmm(section, 0, section.size)
        }

    @Synchronized
    override fun close() {
        mediaCas.close()
    }
}

private class FrameworkMediaCasSessionBridge(
    private val session: android.media.MediaCas.Session,
) : CasController.MediaCasSessionBridge {
    override fun setPrivateData(privateData: ByteArray): Result<Unit> = runCatching { session.setPrivateData(privateData) }

    override fun processEcm(section: ByteArray): Result<EcmProcessResult> =
        runCatching {
            session.processEcm(section, 0, section.size)
            val token = TunerKeyToken.fromOrNull(session.sessionId)
            if (token == null) {
                EcmProcessResult.InvalidKeyToken("MediaCas session ID は 1..16 byte かつ VOID [0x00] 以外でなければなりません")
            } else {
                EcmProcessResult.RealKeyToken(token)
            }
        }

    @Synchronized override fun close() {
        session.close()
    }
}

class DirectTunerDescramblerBridge(
    private val tuner: Tuner?,
) : CasController.TunerDescramblerBridge {
    private val descramblerHandle = lazy { tuner?.openDescrambler() }
    private var closing = false
    private var closed = false

    private fun activeDescrambler(): Descrambler {
        check(!closing && !closed) { "退役済みのdescrambler bridgeは再利用できません" }
        return requireNotNull(descramblerHandle.value) { "Tuner descrambler を利用できません" }
    }

    override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> =
        runCatching {
            val d = activeDescrambler()
            val result = d.setKeyToken(keyToken.toByteArray())
            require(result == Tuner.RESULT_SUCCESS) { "Descrambler.setKeyToken が失敗しました result=$result" }
        }

    override fun clearKeyToken(): Result<Unit> =
        runCatching {
            if (!descramblerHandle.isInitialized()) return@runCatching
            val current = descramblerHandle.value ?: return@runCatching
            val result = current.setKeyToken(Tuner.VOID_KEYTOKEN)
            require(result == Tuner.RESULT_SUCCESS) { "Descrambler.setKeyToken(VOID) が失敗しました result=$result" }
        }

    override fun newSibling(): Result<CasController.TunerDescramblerBridge> =
        runCatching {
            check(!closing && !closed) { "退役済みのdescrambler bridgeは再利用できません" }
            DirectTunerDescramblerBridge(tuner)
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    override fun addPid(elementaryPid: TsPid): Result<Unit> =
        runCatching {
            val d = activeDescrambler()
            val result = d.addPid(DESCRAMBLER_PID_TYPE_T, elementaryPid.value, null)
            require(result == Tuner.RESULT_SUCCESS) { "Descrambler.addPid が失敗しました pid=${elementaryPid.value} result=$result" }
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    override fun removePid(elementaryPid: TsPid): Result<Unit> =
        runCatching {
            val d = activeDescrambler()
            val result = d.removePid(DESCRAMBLER_PID_TYPE_T, elementaryPid.value, null)
            require(result == Tuner.RESULT_SUCCESS) { "Descrambler.removePid が失敗しました pid=${elementaryPid.value} result=$result" }
        }

    @Synchronized
    override fun close() {
        if (closed) return
        closing = true
        // closeのために未生成のAOSP handleを生成してはならない。
        if (descramblerHandle.isInitialized()) descramblerHandle.value?.close()
        closed = true
    }

    companion object {
        // AOSP Descrambler.PID_TYPE_T
        private const val DESCRAMBLER_PID_TYPE_T = 1
    }
}
