package com.maleicacid.tvinput.tis

import android.media.tv.tuner.Tuner
import android.media.tv.tuner.Descrambler
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
 * Tuner HAL には 不透明 トークン と ES PID 登録だけを渡す。
 */
class CasController(
    private val supportedSystemIds: Set<Int> = SupportedCasSystemIds.B25_B1,
    private val mediaCasFactory: MediaCasBridgeFactory = FrameworkMediaCasBridgeFactory(),
) : AutoCloseable {
    enum class ErrorCode { NONE, UNSUPPORTED_SYSTEM_ID, PLUGIN_UNAVAILABLE, SESSION_OPEN_FAILED, PRIVATE_DATA_FAILED, ECM_FAILED, EMM_FAILED, KEY_TOKEN_MISSING, INVALID_KEY_TOKEN, DESCRAMBLER_FAILED, CLOSED }
    enum class State { IDLE, ACTIVE, ERROR, CLOSED }
    data class Diagnostic(val state: State, val errorCode: ErrorCode = ErrorCode.NONE, val caSystemId: Int? = null, val pid: TsPid? = null, val message: String = "")
    data class UpdateResult(val diagnostics: List<Diagnostic>, val ecmPids: Set<TsPid>, val emmPids: Set<TsPid>)

    interface MediaCasBridgeFactory { fun create(caSystemId: Int): Result<MediaCasBridge> }
    interface MediaCasBridge : AutoCloseable { fun setPrivateData(privateData: ByteArray): Result<Unit>; fun openSession(): Result<MediaCasSessionBridge>; fun processEmm(section: ByteArray): Result<Unit>; override fun close() }
    interface MediaCasSessionBridge : AutoCloseable { fun setPrivateData(privateData: ByteArray): Result<Unit>; fun processEcm(section: ByteArray): Result<EcmProcessResult>; override fun close() }
    interface TunerDescramblerBridge : AutoCloseable { fun setKeyToken(keyToken: TunerKeyToken): Result<Unit>; fun addPid(elementaryPid: TsPid): Result<Unit>; fun removePid(elementaryPid: TsPid): Result<Unit>; override fun close() }

    private data class EsCaBinding(val serviceKeyText: String, val caSystemId: Int, val ecmPid: TsPid, val elementaryPid: TsPid, val privateData: ByteArray)
    private data class ProgramCaBinding(val serviceKeyText: String, val caSystemId: Int, val ecmPid: TsPid, val privateData: ByteArray)
    private data class EmmBinding(val caSystemId: Int, val emmPid: TsPid, val privateData: ByteArray)
    private data class CasSessionState(val caSystemId: Int, val cas: MediaCasBridge, val session: MediaCasSessionBridge, val ecmPids: MutableSet<TsPid> = linkedSetOf(), val elementaryPids: MutableSet<TsPid> = linkedSetOf())

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

    private val sessionsBySystemId = LinkedHashMap<Int, CasSessionState>()
    private val ecmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private val emmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private val elementaryPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private var descrambler: TunerDescramblerBridge? = null
    private var closed = false
    @Volatile private var lastDiagnostic = Diagnostic(State.IDLE)

    fun attachDescrambler(bridge: TunerDescramblerBridge?): Unit = onExecutor {
        if (closed) {
            bridge?.close()
            lastDiagnostic = Diagnostic(State.CLOSED, ErrorCode.CLOSED, message = "CAS 制御は終了済みです")
            return@onExecutor
        }
        if (bridge === descrambler) return@onExecutor
        descrambler?.close()
        descrambler = bridge
    }

    fun clearForClearService(): Unit = onExecutor { clearForClearServiceLocked() }

    private fun clearForClearServiceLocked() {
        sessionsBySystemId.keys.toList().forEach { closeSystemLocked(it) }
        ecmPidToSystems.clear()
        emmPidToSystems.clear()
        elementaryPidToSystems.clear()
        lastDiagnostic = Diagnostic(State.IDLE)
    }

    fun updateFromCaMetadata(metadata: List<CaMetadata>, descramblerBridge: TunerDescramblerBridge? = null): UpdateResult = onExecutor {
        if (closed) return@onExecutor UpdateResult(listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, message = "CAS 制御は終了済みです")), emptySet(), emptySet())
        if (metadata.isEmpty()) {
            clearForClearServiceLocked()
            return@onExecutor UpdateResult(emptyList(), emptySet(), emptySet())
        }
        if (descramblerBridge != null && descramblerBridge !== descrambler) {
            descrambler?.close()
            descrambler = descramblerBridge
        }
        val diagnostics = mutableListOf<Diagnostic>()
        val previousElementaryPids = elementaryPidToSystems.keys.toSet()
        val programBindings = mutableListOf<ProgramCaBinding>()
        val esBindings = mutableListOf<EsCaBinding>()
        val emmBindings = mutableListOf<EmmBinding>()
        metadata.forEach { ca ->
            if (ca.caSystemId !in supportedSystemIds) {
                diagnostics += Diagnostic(State.ERROR, ErrorCode.UNSUPPORTED_SYSTEM_ID, ca.caSystemId, (ca.ecmPid ?: ca.emmPid ?: ca.elementaryPid), "B25/B1 対象外の CA_system_id です")
                return@forEach
            }
            when (ca.source) {
                CaMetadataSource.PROGRAM -> {
                    val serviceKey = ca.serviceKey ?: return@forEach
                    val ecmPid = ca.ecmPid ?: return@forEach
                    programBindings += ProgramCaBinding(serviceKey.toString(), ca.caSystemId, ecmPid, ca.privateData.copyOf())
                }
                CaMetadataSource.ELEMENTARY_STREAM -> {
                    val serviceKey = ca.serviceKey ?: return@forEach
                    val ecmPid = ca.ecmPid ?: return@forEach
                    val elementaryPid = ca.elementaryPid ?: return@forEach
                    esBindings += EsCaBinding(serviceKey.toString(), ca.caSystemId, ecmPid, elementaryPid, ca.privateData.copyOf())
                }
                CaMetadataSource.CAT -> {
                    val emmPid = ca.emmPid ?: return@forEach
                    emmBindings += EmmBinding(ca.caSystemId, emmPid, ca.privateData.copyOf())
                }
            }
        }
        val targetSystems = (programBindings.map { it.caSystemId } + esBindings.map { it.caSystemId } + emmBindings.map { it.caSystemId }).toSet()
        sessionsBySystemId.keys.filter { it !in targetSystems }.toList().forEach { closeSystemLocked(it) }
        sessionsBySystemId.values.forEach { state -> state.ecmPids.clear(); state.elementaryPids.clear() }
        (programBindings.map { it.caSystemId } + esBindings.map { it.caSystemId }).toSet().forEach { systemId ->
            val result = ensureSessionLocked(systemId)
            if (result.isFailure) diagnostics += Diagnostic(State.ERROR, ErrorCode.SESSION_OPEN_FAILED, systemId, message = result.exceptionOrNull()?.message.orEmpty())
        }
        programBindings.forEach { binding ->
            sessionsBySystemId[binding.caSystemId]?.let { state ->
                state.ecmPids += binding.ecmPid
                state.session.setPrivateData(binding.privateData).onFailure { e -> diagnostics += Diagnostic(State.ERROR, ErrorCode.PRIVATE_DATA_FAILED, binding.caSystemId, binding.ecmPid, e.message.orEmpty()) }
            }
        }
        esBindings.forEach { binding ->
            sessionsBySystemId[binding.caSystemId]?.let { state ->
                state.ecmPids += binding.ecmPid
                state.elementaryPids += binding.elementaryPid
                state.session.setPrivateData(binding.privateData).onFailure { e -> diagnostics += Diagnostic(State.ERROR, ErrorCode.PRIVATE_DATA_FAILED, binding.caSystemId, binding.ecmPid, e.message.orEmpty()) }
            }
        }
        emmBindings.forEach { binding ->
            ensureCasOnlyLocked(binding.caSystemId)?.let { cas ->
                cas.setPrivateData(binding.privateData).onFailure { e -> diagnostics += Diagnostic(State.ERROR, ErrorCode.PRIVATE_DATA_FAILED, binding.caSystemId, binding.emmPid, e.message.orEmpty()) }
            } ?: run { diagnostics += Diagnostic(State.ERROR, ErrorCode.PLUGIN_UNAVAILABLE, binding.caSystemId, binding.emmPid, "MediaCas plugin を利用できません") }
        }
        rebuildPidIndexesLocked()
        emmBindings.forEach { binding -> emmPidToSystems.getOrPut(binding.emmPid) { linkedSetOf() } += binding.caSystemId }
        val activePids = esBindings.map { it.elementaryPid }.toSet()
        syncDescramblerPidsLocked(previousElementaryPids, activePids, diagnostics)
        lastDiagnostic = if (diagnostics.isEmpty()) Diagnostic(if (targetSystems.isEmpty()) State.IDLE else State.ACTIVE) else diagnostics.last()
        UpdateResult(diagnostics, ecmPidToSystems.keys.toSet(), emmPidToSystems.keys.toSet())
    }

    fun onEcmSection(pid: TsPid, section: ByteArray): List<Diagnostic> = onExecutor {
        if (closed) return@onExecutor listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"))
        val systems = ecmPidToSystems[pid].orEmpty()
        if (systems.isEmpty()) return@onExecutor emptyList()
        val diagnostics = mutableListOf<Diagnostic>()
        systems.forEach { systemId ->
            val state = sessionsBySystemId[systemId]
            if (state == null) {
                diagnostics += Diagnostic(State.ERROR, ErrorCode.SESSION_OPEN_FAILED, systemId, pid, "CAS session がありません")
                return@forEach
            }
            val tokenResult = state.session.processEcm(section)
            if (tokenResult.isFailure) {
                diagnostics += Diagnostic(State.ERROR, ErrorCode.ECM_FAILED, systemId, pid, tokenResult.exceptionOrNull()?.message.orEmpty())
                return@forEach
            }
            when (val ecmResult = tokenResult.getOrNull()) {
                is EcmProcessResult.RealKeyToken -> {
                    val token = ecmResult.token
                    val setTokenResult = descrambler?.setKeyToken(token) ?: Result.failure(IllegalStateException("Tuner descrambler を利用できません"))
                    if (setTokenResult.isFailure) {
                        diagnostics += Diagnostic(State.ERROR, ErrorCode.DESCRAMBLER_FAILED, systemId, pid, setTokenResult.exceptionOrNull()?.message.orEmpty())
                        return@forEach
                    }
                    state.elementaryPids.forEach { elementaryPid ->
                        val addResult = descrambler?.addPid(elementaryPid) ?: Result.failure(IllegalStateException("Tuner descrambler を利用できません"))
                        if (addResult.isFailure) diagnostics += Diagnostic(State.ERROR, ErrorCode.DESCRAMBLER_FAILED, systemId, elementaryPid, addResult.exceptionOrNull()?.message.orEmpty())
                    }
                }
                is EcmProcessResult.DiagnosticOnly -> diagnostics += Diagnostic(State.ERROR, ErrorCode.KEY_TOKEN_MISSING, systemId, pid, ecmResult.message)
                null -> diagnostics += Diagnostic(State.ERROR, ErrorCode.KEY_TOKEN_MISSING, systemId, pid, "MediaCas session から実 key token を取得できません")
            }
        }
        if (diagnostics.isNotEmpty()) lastDiagnostic = diagnostics.last()
        diagnostics
    }

    fun onEmmSection(pid: TsPid, section: ByteArray): List<Diagnostic> = onExecutor {
        if (closed) return@onExecutor listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"))
        val systems = emmPidToSystems[pid].orEmpty()
        if (systems.isEmpty()) return@onExecutor emptyList()
        val diagnostics = mutableListOf<Diagnostic>()
        systems.forEach { systemId ->
            val cas = sessionsBySystemId[systemId]?.cas ?: ensureCasOnlyLocked(systemId)
            if (cas == null) {
                diagnostics += Diagnostic(State.ERROR, ErrorCode.PLUGIN_UNAVAILABLE, systemId, pid, "MediaCas plugin を利用できません")
                return@forEach
            }
            cas.processEmm(section).onFailure { e -> diagnostics += Diagnostic(State.ERROR, ErrorCode.EMM_FAILED, systemId, pid, e.message.orEmpty()) }
        }
        if (diagnostics.isNotEmpty()) lastDiagnostic = diagnostics.last()
        diagnostics
    }

    fun lastDiagnostic(): Diagnostic = lastDiagnostic

    private fun ensureSessionLocked(caSystemId: Int): Result<CasSessionState> {
        sessionsBySystemId[caSystemId]?.let { return Result.success(it) }
        val cas = mediaCasFactory.create(caSystemId).getOrElse { return Result.failure(it) }
        val session = cas.openSession().getOrElse { cas.close(); return Result.failure(it) }
        val state = CasSessionState(caSystemId, cas, session)
        sessionsBySystemId[caSystemId] = state
        return Result.success(state)
    }

    private fun ensureCasOnlyLocked(caSystemId: Int): MediaCasBridge? {
        sessionsBySystemId[caSystemId]?.let { return it.cas }
        return ensureSessionLocked(caSystemId).getOrNull()?.cas
    }

    private fun syncDescramblerPidsLocked(previousPids: Set<TsPid>, activePids: Set<TsPid>, diagnostics: MutableList<Diagnostic>) {
        val bridge = descrambler ?: return
        (previousPids - activePids).forEach { pid ->
            bridge.removePid(pid).onFailure { e -> diagnostics += Diagnostic(State.ERROR, ErrorCode.DESCRAMBLER_FAILED, pid = pid, message = e.message.orEmpty()) }
        }
    }

    private fun rebuildPidIndexesLocked() {
        ecmPidToSystems.clear()
        emmPidToSystems.clear()
        elementaryPidToSystems.clear()
        sessionsBySystemId.values.forEach { state ->
            state.ecmPids.forEach { ecmPid -> ecmPidToSystems.getOrPut(ecmPid) { linkedSetOf() } += state.caSystemId }
            state.elementaryPids.forEach { elementaryPid -> elementaryPidToSystems.getOrPut(elementaryPid) { linkedSetOf() } += state.caSystemId }
        }
    }

    private fun closeSystemLocked(caSystemId: Int) {
        sessionsBySystemId.remove(caSystemId)?.let { state ->
            state.elementaryPids.forEach { pid -> descrambler?.removePid(pid) }
            state.session.close()
            state.cas.close()
        }
        rebuildPidIndexesLocked()
    }

    override fun close() {
        if (executor.isShutdown) return
        try {
            onExecutor {
                if (closed) return@onExecutor
                closed = true
                sessionsBySystemId.keys.toList().forEach { closeSystemLocked(it) }
                ecmPidToSystems.clear(); emmPidToSystems.clear(); elementaryPidToSystems.clear()
                descrambler?.close(); descrambler = null
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
    data class DiagnosticOnly(val message: String) : EcmProcessResult()
}

class FrameworkMediaCasBridgeFactory : CasController.MediaCasBridgeFactory {
    override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> = runCatching {
        FrameworkMediaCasBridge(caSystemId)
    }
}

private class FrameworkMediaCasBridge(caSystemId: Int) : CasController.MediaCasBridge {
    private val mediaCas = android.media.MediaCas(caSystemId)

    override fun setPrivateData(privateData: ByteArray): Result<Unit> = runCatching {
        mediaCas.setPrivateData(privateData)
    }

    override fun openSession(): Result<CasController.MediaCasSessionBridge> = runCatching {
        FrameworkMediaCasSessionBridge(mediaCas.openSession())
    }

    override fun processEmm(section: ByteArray): Result<Unit> = runCatching {
        mediaCas.processEmm(section, 0, section.size)
    }

    @Synchronized
    override fun close() {
        runCatching { mediaCas.close() }
    }
}

private class FrameworkMediaCasSessionBridge(
    private val session: android.media.MediaCas.Session,
) : CasController.MediaCasSessionBridge {
    override fun setPrivateData(privateData: ByteArray): Result<Unit> = runCatching {
        session.setPrivateData(privateData)
    }

    override fun processEcm(section: ByteArray): Result<EcmProcessResult> = runCatching {
        session.processEcm(section, 0, section.size)
        EcmProcessResult.DiagnosticOnly("MediaCas 標準 API は ECM 投入完了を返すが、r51 の placeholder CAS では Tuner 用の実 key token を返しません")
    }

    @Synchronized
    override fun close() {
        runCatching { session.close() }
    }
}

class DirectTunerDescramblerBridge(private val tuner: Tuner?) : CasController.TunerDescramblerBridge {
    private val descrambler: Descrambler? by lazy { tuner?.openDescrambler() }

    override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> = runCatching {
        val d = requireNotNull(descrambler) { "Tuner descrambler を利用できません" }
        val result = d.setKeyToken(keyToken.toByteArray())
        require(result == Tuner.RESULT_SUCCESS) { "Descrambler.setKeyToken が失敗しました result=$result" }
    }

    override fun addPid(elementaryPid: TsPid): Result<Unit> = runCatching {
        val d = requireNotNull(descrambler) { "Tuner descrambler を利用できません" }
        val result = d.addPid(DESCRAMBLER_PID_TYPE_T, elementaryPid.value, null)
        require(result == Tuner.RESULT_SUCCESS) { "Descrambler.addPid が失敗しました pid=${elementaryPid.value} result=$result" }
    }

    override fun removePid(elementaryPid: TsPid): Result<Unit> = runCatching {
        val d = requireNotNull(descrambler) { "Tuner descrambler を利用できません" }
        val result = d.removePid(DESCRAMBLER_PID_TYPE_T, elementaryPid.value, null)
        require(result == Tuner.RESULT_SUCCESS) { "Descrambler.removePid が失敗しました pid=${elementaryPid.value} result=$result" }
    }

    @Synchronized
    override fun close() { runCatching { descrambler?.close() } }

    companion object {
        // AOSP Descrambler.PID_TYPE_T
        private const val DESCRAMBLER_PID_TYPE_T = 1
    }
}
