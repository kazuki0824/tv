package com.maleicacid.tvinput.tis

import android.media.tv.tuner.Descrambler
import android.media.tv.tuner.Tuner
import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * B25/B1 向け CAS 制御。
 * PMT/CAT の CA情報 は arib_si_engine_rs の snapshot から受ける。
 * ECM/EMM は完全な section として扱い、生 TS packet は扱わない。
 * カード I/O、CW 生成、鍵発行は MediaCas/CAS HAL 側の責務とする。
 * Tuner HAL には opaque token と ES PID 登録だけを渡す。
 */
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

    interface MediaCasBridgeFactory { fun create(caSystemId: Int): Result<MediaCasBridge> }
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
        fun newSibling(): Result<TunerDescramblerBridge> = Result.failure(
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
    private data class EmmBinding(val caSystemId: Int, val emmPid: TsPid, val privateData: ByteArray)

    /** One MediaCas.Session / key-slot / Descrambler context. Multiple PIDs may share it. */
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
        val descrambler: TunerDescramblerBridge,
        val elementaryPids: MutableSet<TsPid> = linkedSetOf(),
        var keyLinked: Boolean = false,
    )

    @Volatile private var executorThread: Thread? = null
    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
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
    private var descramblerPrototype: TunerDescramblerBridge? = null
    private var closed = false
    @Volatile private var lastDiagnostic = Diagnostic(State.IDLE)

    fun attachDescrambler(bridge: TunerDescramblerBridge?): Unit = onExecutor {
        if (closed) {
            bridge?.close()
            lastDiagnostic = Diagnostic(State.CLOSED, ErrorCode.CLOSED, message = "CAS 制御は終了済みです")
            return@onExecutor
        }
        if (bridge === descramblerPrototype) return@onExecutor
        descramblerPrototype?.close()
        descramblerPrototype = bridge
    }

    fun clearForClearService(): Unit = onExecutor { clearForClearServiceLocked() }

    private fun clearForClearServiceLocked() {
        sessionsByContext.keys.toList().forEach { closeContextLocked(it) }
        pluginsBySystemId.values.toList().forEach { plugin -> runCatching { plugin.close() } }
        pluginsBySystemId.clear()
        ecmPidToContexts.clear()
        emmPidToSystems.clear()
        lastDiagnostic = Diagnostic(State.IDLE)
    }

    fun updateFromCaMetadata(
        metadata: List<CaMetadata>,
        descramblerBridge: TunerDescramblerBridge? = null,
    ): UpdateResult = onExecutor {
        if (closed) {
            return@onExecutor UpdateResult(
                listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, message = "CAS 制御は終了済みです")),
                emptySet(),
                emptySet(),
                Readiness.CLOSED,
            )
        }
        if (metadata.isEmpty()) {
            clearForClearServiceLocked()
            return@onExecutor UpdateResult(emptyList(), emptySet(), emptySet(), Readiness.CLEAR)
        }
        if (descramblerBridge != null && descramblerBridge !== descramblerPrototype) {
            descramblerPrototype?.close()
            descramblerPrototype = descramblerBridge
        }

        val diagnostics = mutableListOf<Diagnostic>()
        val programBindings = mutableListOf<ProgramCaBinding>()
        val esBindings = mutableListOf<EsCaBinding>()
        val emmBindings = mutableListOf<EmmBinding>()
        metadata.forEach { ca ->
            if (ca.caSystemId !in supportedSystemIds) {
                diagnostics += Diagnostic(
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
                    programBindings += ProgramCaBinding(
                        serviceKey.toString(), ca.caSystemId, ecmPid, ca.privateData.copyOf(),
                    )
                }
                CaMetadataSource.ELEMENTARY_STREAM -> {
                    val serviceKey = ca.serviceKey ?: return@forEach
                    val ecmPid = ca.ecmPid ?: return@forEach
                    val elementaryPid = ca.elementaryPid ?: return@forEach
                    esBindings += EsCaBinding(
                        serviceKey.toString(), ca.caSystemId, ecmPid, elementaryPid, ca.privateData.copyOf(),
                    )
                }
                CaMetadataSource.CAT -> {
                    // B1 is ECM-only. Its CAT metadata remains an SI fact but is not an EMM plan.
                    if (ca.caSystemId != SupportedCasSystemIds.ARIB_STD_B25) return@forEach
                    val emmPid = ca.emmPid ?: return@forEach
                    emmBindings += EmmBinding(ca.caSystemId, emmPid, ca.privateData.copyOf())
                }
            }
        }

        val contextPlans = esBindings.groupBy { it.contextKey() }.mapValues { (key, bindings) ->
            ContextPlan(key, bindings.map { it.elementaryPid }.toSet())
        }
        val requiresDescrambling = programBindings.isNotEmpty() || esBindings.isNotEmpty()

        (sessionsByContext.keys - contextPlans.keys).toList().forEach { closeContextLocked(it) }
        contextPlans.values.forEach { plan -> ensureContextLocked(plan, diagnostics) }

        val targetPluginSystems = contextPlans.keys.map { it.caSystemId }.toMutableSet()
        emmBindings.mapTo(targetPluginSystems) { it.caSystemId }
        emmBindings.forEach { binding ->
            val plugin = ensurePluginLocked(binding.caSystemId, diagnostics)
            if (plugin != null) {
                plugin.setPrivateData(binding.privateData).onFailure { error ->
                    diagnostics += Diagnostic(
                        State.ERROR, ErrorCode.PRIVATE_DATA_FAILED, binding.caSystemId, binding.emmPid,
                        error.message.orEmpty(),
                    )
                }
            }
        }
        pluginsBySystemId.keys.filter { it !in targetPluginSystems }.toList().forEach { systemId ->
            pluginsBySystemId.remove(systemId)?.let { plugin -> runCatching { plugin.close() } }
        }

        rebuildPidIndexesLocked()
        emmPidToSystems.clear()
        emmBindings.forEach { binding ->
            emmPidToSystems.getOrPut(binding.emmPid) { linkedSetOf() } += binding.caSystemId
        }

        val ecmPids = (programBindings.map { it.ecmPid } + esBindings.map { it.ecmPid }).toSet()
        val readiness = readinessLocked(requiresDescrambling, diagnostics)
        lastDiagnostic = when (readiness) {
            Readiness.ERROR -> diagnostics.lastOrNull() ?: Diagnostic(State.ERROR, message = "CAS 構成に失敗しました")
            Readiness.CLOSED -> Diagnostic(State.CLOSED)
            Readiness.CLEAR -> Diagnostic(State.IDLE)
            Readiness.WAITING_FOR_KEY,
            Readiness.READY -> Diagnostic(State.ACTIVE)
        }
        UpdateResult(diagnostics, ecmPids, emmPidToSystems.keys.toSet(), readiness)
    }

    fun onEcmSection(pid: TsPid, section: ByteArray): List<Diagnostic> = onExecutor {
        if (closed) return@onExecutor listOf(
            Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"),
        )
        val contextKeys = ecmPidToContexts[pid].orEmpty().toList()
        if (contextKeys.isEmpty()) return@onExecutor emptyList()
        val diagnostics = mutableListOf<Diagnostic>()
        contextKeys.forEach { key ->
            val state = sessionsByContext[key] ?: return@forEach
            val tokenResult = state.session.processEcm(section)
            if (tokenResult.isFailure) {
                diagnostics += Diagnostic(
                    State.ERROR, ErrorCode.ECM_FAILED, key.caSystemId, pid,
                    tokenResult.exceptionOrNull()?.message.orEmpty(),
                )
                return@forEach
            }
            when (val result = tokenResult.getOrNull()) {
                is EcmProcessResult.RealKeyToken -> {
                    if (!state.keyLinked) linkContextKeyLocked(state, result.token, pid, diagnostics)
                }
                is EcmProcessResult.InvalidKeyToken -> diagnostics += Diagnostic(
                    State.ERROR, ErrorCode.INVALID_KEY_TOKEN, key.caSystemId, pid, result.message,
                )
                is EcmProcessResult.DiagnosticOnly -> diagnostics += Diagnostic(
                    State.ERROR, ErrorCode.KEY_TOKEN_MISSING, key.caSystemId, pid, result.message,
                )
                null -> diagnostics += Diagnostic(
                    State.ERROR, ErrorCode.KEY_TOKEN_MISSING, key.caSystemId, pid,
                    "MediaCas session から実 key token を取得できません",
                )
            }
        }
        if (diagnostics.isNotEmpty()) lastDiagnostic = diagnostics.last()
        diagnostics
    }

    fun onEmmSection(pid: TsPid, section: ByteArray): List<Diagnostic> = onExecutor {
        if (closed) return@onExecutor listOf(
            Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"),
        )
        val systems = emmPidToSystems[pid].orEmpty().filter { it == SupportedCasSystemIds.ARIB_STD_B25 }
        if (systems.isEmpty()) return@onExecutor emptyList()
        val diagnostics = mutableListOf<Diagnostic>()
        systems.forEach { systemId ->
            val plugin = pluginsBySystemId[systemId] ?: ensurePluginLocked(systemId, diagnostics)
            if (plugin == null) {
                diagnostics += Diagnostic(
                    State.ERROR, ErrorCode.PLUGIN_UNAVAILABLE, systemId, pid,
                    "MediaCas plugin を利用できません",
                )
                return@forEach
            }
            plugin.processEmm(section).onFailure { error ->
                diagnostics += Diagnostic(
                    State.ERROR, ErrorCode.EMM_FAILED, systemId, pid, error.message.orEmpty(),
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
                sessionsByContext.values.all { it.keyLinked } -> Readiness.READY
                else -> Readiness.WAITING_FOR_KEY
            }
        }
    }

    private fun EsCaBinding.contextKey(): DescrambleContextKey = DescrambleContextKey(
        serviceKeyText, caSystemId, ecmPid, privateData.toList(),
    )

    private fun ensurePluginLocked(
        caSystemId: Int,
        diagnostics: MutableList<Diagnostic>,
    ): MediaCasBridge? {
        pluginsBySystemId[caSystemId]?.let { return it }
        val plugin = mediaCasFactory.create(caSystemId).getOrElse { error ->
            diagnostics += Diagnostic(
                State.ERROR, ErrorCode.PLUGIN_UNAVAILABLE, caSystemId, message = error.message.orEmpty(),
            )
            return null
        }
        pluginsBySystemId[caSystemId] = plugin
        return plugin
    }

    private fun ensureContextLocked(plan: ContextPlan, diagnostics: MutableList<Diagnostic>) {
        val existing = sessionsByContext[plan.key]
        if (existing != null) {
            syncContextPidsLocked(existing, plan.elementaryPids, diagnostics)
            return
        }
        val plugin = ensurePluginLocked(plan.key.caSystemId, diagnostics) ?: return
        val session = plugin.openSession().getOrElse { error ->
            diagnostics += Diagnostic(
                State.ERROR, ErrorCode.SESSION_OPEN_FAILED, plan.key.caSystemId, plan.key.ecmPid,
                error.message.orEmpty(),
            )
            return
        }
        val privateResult = session.setPrivateData(plan.key.privateData.toByteArray())
        if (privateResult.isFailure) {
            runCatching { session.close() }
            diagnostics += Diagnostic(
                State.ERROR, ErrorCode.PRIVATE_DATA_FAILED, plan.key.caSystemId, plan.key.ecmPid,
                privateResult.exceptionOrNull()?.message.orEmpty(),
            )
            return
        }
        val descrambler = descramblerPrototype?.newSibling()?.getOrElse { error ->
            runCatching { session.close() }
            diagnostics += Diagnostic(
                State.ERROR, ErrorCode.DESCRAMBLER_FAILED, plan.key.caSystemId, plan.key.ecmPid,
                error.message.orEmpty(),
            )
            return
        }
        if (descrambler == null) {
            runCatching { session.close() }
            diagnostics += Diagnostic(
                State.ERROR, ErrorCode.DESCRAMBLER_FAILED, plan.key.caSystemId, plan.key.ecmPid,
                "Tuner descrambler prototype がありません",
            )
            return
        }
        sessionsByContext[plan.key] = CasSessionState(
            key = plan.key,
            session = session,
            descrambler = descrambler,
            elementaryPids = plan.elementaryPids.toMutableSet(),
        )
    }

    private fun syncContextPidsLocked(
        state: CasSessionState,
        targetPids: Set<TsPid>,
        diagnostics: MutableList<Diagnostic>,
    ) {
        val previous = state.elementaryPids.toSet()
        if (state.keyLinked) {
            (previous - targetPids).forEach { pid ->
                state.descrambler.removePid(pid).onFailure { error ->
                    diagnostics += Diagnostic(
                        State.ERROR, ErrorCode.DESCRAMBLER_FAILED, state.key.caSystemId, pid,
                        error.message.orEmpty(),
                    )
                }
            }
            (targetPids - previous).forEach { pid ->
                state.descrambler.addPid(pid).onFailure { error ->
                    diagnostics += Diagnostic(
                        State.ERROR, ErrorCode.DESCRAMBLER_FAILED, state.key.caSystemId, pid,
                        error.message.orEmpty(),
                    )
                }
            }
        }
        state.elementaryPids.clear()
        state.elementaryPids += targetPids
    }

    private fun linkContextKeyLocked(
        state: CasSessionState,
        token: TunerKeyToken,
        ecmPid: TsPid,
        diagnostics: MutableList<Diagnostic>,
    ) {
        val tokenResult = state.descrambler.setKeyToken(token)
        if (tokenResult.isFailure) {
            diagnostics += Diagnostic(
                State.ERROR, ErrorCode.DESCRAMBLER_FAILED, state.key.caSystemId, ecmPid,
                tokenResult.exceptionOrNull()?.message.orEmpty(),
            )
            return
        }
        val added = mutableListOf<TsPid>()
        var failed = false
        state.elementaryPids.forEach { elementaryPid ->
            val addResult = state.descrambler.addPid(elementaryPid)
            if (addResult.isFailure) {
                failed = true
                diagnostics += Diagnostic(
                    State.ERROR, ErrorCode.DESCRAMBLER_FAILED, state.key.caSystemId, elementaryPid,
                    addResult.exceptionOrNull()?.message.orEmpty(),
                )
            } else {
                added += elementaryPid
            }
        }
        if (failed) {
            added.forEach { pid -> runCatching { state.descrambler.removePid(pid) } }
            runCatching { state.descrambler.clearKeyToken() }
            state.keyLinked = false
            return
        }
        state.keyLinked = true
    }

    private fun rebuildPidIndexesLocked() {
        ecmPidToContexts.clear()
        sessionsByContext.keys.forEach { key ->
            ecmPidToContexts.getOrPut(key.ecmPid) { linkedSetOf() } += key
        }
    }

    private fun closeContextLocked(key: DescrambleContextKey) {
        // Remove first: callbacks that race teardown can no longer resolve this context.
        val state = sessionsByContext.remove(key) ?: return
        rebuildPidIndexesLocked()
        // AOSP lifecycle: remove PIDs, unlink MediaCas-derived key, close descrambler,
        // then close MediaCas.Session. Every cleanup step is best-effort.
        state.elementaryPids.toList().forEach { pid -> runCatching { state.descrambler.removePid(pid) } }
        if (state.keyLinked) runCatching { state.descrambler.clearKeyToken() }
        runCatching { state.descrambler.close() }
        runCatching { state.session.close() }
        state.keyLinked = false
    }

    private fun readinessLocked(
        requiresDescrambling: Boolean,
        diagnostics: List<Diagnostic>,
    ): Readiness {
        if (closed) return Readiness.CLOSED
        if (diagnostics.any { it.isBlockingForPlayback() }) return Readiness.ERROR
        if (!requiresDescrambling) return Readiness.CLEAR
        if (sessionsByContext.isEmpty()) return Readiness.WAITING_FOR_KEY
        return if (sessionsByContext.values.all { it.keyLinked }) Readiness.READY else Readiness.WAITING_FOR_KEY
    }

    private fun Diagnostic.isBlockingForPlayback(): Boolean = when (errorCode) {
        ErrorCode.NONE,
        ErrorCode.EMM_FAILED -> false
        else -> state == State.ERROR
    }

    override fun close() {
        if (executor.isShutdown) return
        try {
            onExecutor {
                if (closed) return@onExecutor
                closed = true
                sessionsByContext.keys.toList().forEach { closeContextLocked(it) }
                pluginsBySystemId.values.toList().forEach { plugin -> runCatching { plugin.close() } }
                pluginsBySystemId.clear()
                ecmPidToContexts.clear()
                emmPidToSystems.clear()
                descramblerPrototype?.close()
                descramblerPrototype = null
                lastDiagnostic = Diagnostic(State.CLOSED)
            }
        } finally {
            executor.shutdown()
        }
    }

    fun release() = close()

    object SupportedCasSystemIds {
        const val ARIB_STD_B25 = 0x0005
        const val ARIB_STD_B1 = 0x0001
        val B25_B1: Set<Int> = setOf(ARIB_STD_B25, ARIB_STD_B1)
    }
}

sealed class EcmProcessResult {
    data class RealKeyToken(val token: TunerKeyToken) : EcmProcessResult()
    data class InvalidKeyToken(val message: String) : EcmProcessResult()
    data class DiagnosticOnly(val message: String) : EcmProcessResult()
}

class FrameworkMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
    override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> = runCatching {
        FrameworkMediaCasBridge(caSystemId)
    }
}

private class FrameworkMediaCasBridge(caSystemId: Int) : CasController.MediaCasBridge {
    private val mediaCas = android.media.MediaCas(caSystemId)
    override fun setPrivateData(privateData: ByteArray): Result<Unit> = runCatching { mediaCas.setPrivateData(privateData) }
    override fun openSession(): Result<CasController.MediaCasSessionBridge> = runCatching {
        FrameworkMediaCasSessionBridge(mediaCas.openSession())
    }
    override fun processEmm(section: ByteArray): Result<Unit> = runCatching {
        mediaCas.processEmm(section, 0, section.size)
    }
    @Synchronized override fun close() { runCatching { mediaCas.close() } }
}

private class FrameworkMediaCasSessionBridge(
    private val session: android.media.MediaCas.Session,
) : CasController.MediaCasSessionBridge {
    override fun setPrivateData(privateData: ByteArray): Result<Unit> = runCatching { session.setPrivateData(privateData) }
    override fun processEcm(section: ByteArray): Result<EcmProcessResult> = runCatching {
        session.processEcm(section, 0, section.size)
        val token = TunerKeyToken.fromOrNull(session.sessionId)
        if (token == null) {
            EcmProcessResult.InvalidKeyToken("MediaCas session ID は 1..16 byte かつ VOID [0x00] 以外でなければなりません")
        } else {
            EcmProcessResult.RealKeyToken(token)
        }
    }
    @Synchronized override fun close() { runCatching { session.close() } }
}

class DirectTunerDescramblerBridge(private val tuner: Tuner?) : CasController.TunerDescramblerBridge {
    private var descrambler: Descrambler? = null
    private fun requireDescrambler(): Descrambler {
        descrambler?.let { return it }
        val opened = tuner?.openDescrambler() ?: throw IllegalStateException("Tuner descrambler を利用できません")
        descrambler = opened
        return opened
    }
    override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> = runCatching {
        val result = requireDescrambler().setKeyToken(keyToken.toByteArray())
        require(result == Tuner.RESULT_SUCCESS) { "Descrambler.setKeyToken が失敗しました result=$result" }
    }
    override fun clearKeyToken(): Result<Unit> = runCatching {
        val current = descrambler ?: return@runCatching
        val result = current.setKeyToken(Tuner.VOID_KEYTOKEN)
        require(result == Tuner.RESULT_SUCCESS) { "Descrambler.setKeyToken(VOID) が失敗しました result=$result" }
    }
    override fun addPid(elementaryPid: TsPid): Result<Unit> = runCatching {
        val result = requireDescrambler().addPid(DESCRAMBLER_PID_TYPE_T, elementaryPid.value, null)
        require(result == Tuner.RESULT_SUCCESS) { "Descrambler.addPid が失敗しました pid=${elementaryPid.value} result=$result" }
    }
    override fun removePid(elementaryPid: TsPid): Result<Unit> = runCatching {
        val current = descrambler ?: return@runCatching
        val result = current.removePid(DESCRAMBLER_PID_TYPE_T, elementaryPid.value, null)
        require(result == Tuner.RESULT_SUCCESS) { "Descrambler.removePid が失敗しました pid=${elementaryPid.value} result=$result" }
    }
    override fun newSibling(): Result<CasController.TunerDescramblerBridge> =
        Result.success(DirectTunerDescramblerBridge(tuner))
    @Synchronized override fun close() {
        val current = descrambler
        descrambler = null
        runCatching { current?.close() }
    }
    companion object {
        private const val DESCRAMBLER_PID_TYPE_T = 1
    }
}
