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
 * カード I/O、CW 生成、鍵発行は MediaCas/Maleicacid CAS plugin 側の責務とする。
 * Tuner HAL には 不透明 トークン と ES PID 登録だけを渡す。
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

        fun addPid(elementaryPid: TsPid): Result<Unit>

        fun removePid(elementaryPid: TsPid): Result<Unit>

        override fun close()
    }

    private data class SessionKey(
        val serviceKey: com.maleicacid.tvinput.common.ServiceKey,
        val caSystemId: Int,
        val ecmPid: TsPid,
        val privateData: List<Byte>,
    )

    private class CasSessionState(
        val key: SessionKey,
        val session: MediaCasSessionBridge,
    ) {
        val elementaryPids: MutableSet<TsPid> = linkedSetOf()
        var descrambler: TunerDescramblerBridge? = null
        val descramblerPids: MutableSet<TsPid> = linkedSetOf()
        var keyLinked: Boolean = false
        var retiring: Boolean = false
        var sessionClosed: Boolean = false
    }

    private class CasSystemState(
        val caSystemId: Int,
        val cas: MediaCasBridge,
    ) {
        val sessions: MutableMap<SessionKey, CasSessionState> = linkedMapOf()
        var retiring: Boolean = false
        var casClosed: Boolean = false
    }

    private class SessionProvisioningException(
        val errorCode: ErrorCode,
        cause: Throwable,
    ) : IllegalStateException(cause.message, cause)

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

    private val pluginsBySystemId = LinkedHashMap<Int, CasSystemState>()
    private val ecmPidToSessions = LinkedHashMap<TsPid, MutableSet<SessionKey>>()
    private val emmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private var closed = false

    @Volatile private var lastDiagnostic = Diagnostic(State.IDLE)

    /** 所有bridgeを退役させ、解放失敗中はECM/EMMの配送対象から外す。 */
    fun clearForResourceLoss(): Unit = onExecutor { clearForResourceLossLocked() }

    // AOSPはDescramblerを閉じてから資源回収を通知する。閉鎖済みhandleへVOIDを再投入しない。
    @Suppress("SpreadOperator")
    internal fun onTunerResourcesReclaimed(): Unit =
        onExecutor {
            invalidateMetadataLocked()
            pluginsBySystemId.values.forEach { it.retiring = true }
            SectionFilterPolicy.completeCleanup(
                *pluginsBySystemId.values
                    .map { system ->
                        {
                            SectionFilterPolicy.completeCleanup(
                                *system.sessions.values
                                    .map { state ->
                                        { closeDescramblerLocked(state) }
                                    }.toTypedArray(),
                            )
                            closeSystemLocked(system.caSystemId)
                        }
                    }.toTypedArray(),
            )
            lastDiagnostic = Diagnostic(State.IDLE)
        }

    private fun clearForResourceLossLocked() {
        try {
            // 各sessionのMediaCas tokenをVOID unlinkしてから、そのsession専用descramblerまで退役させる。
            clearForClearServiceLocked()
        } finally {
            ecmPidToSessions.clear()
            emmPidToSystems.clear()
        }
    }

    fun clearForClearService(): Unit = onExecutor { clearForClearServiceLocked() }

    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    @Suppress("SpreadOperator")
    private fun clearForClearServiceLocked() {
        invalidateMetadataLocked()
        pluginsBySystemId.values.forEach { it.retiring = true }
        // system同士は独立に全件cleanupを試し、各sessionのVOID・close・descrambler cleanup後にpluginを閉じる。
        SectionFilterPolicy.completeCleanup(
            *pluginsBySystemId.keys
                .map { systemId ->
                    { closeSystemLocked(systemId) }
                }.toTypedArray(),
        )
        ecmPidToSessions.clear()
        emmPidToSystems.clear()
        lastDiagnostic = Diagnostic(State.IDLE)
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
                )
            }
            // 配送indexはmetadata全体の成功時だけ公開する。物理解放の途中でsurvivorを再公開しない。
            invalidateMetadataLocked()
            if (metadata.isEmpty()) {
                clearForClearServiceLocked()
                return@onExecutor UpdateResult(emptyList(), emptySet(), emptySet())
            }
            SectionFilterPolicy.completeCleanup(
                *pluginsBySystemId.values
                    .filter { it.retiring }
                    .map { state ->
                        { closeSystemLocked(state.caSystemId) }
                    }.toTypedArray(),
            )
            val diagnostics = mutableListOf<Diagnostic>()
            val supported =
                metadata.filter { ca ->
                    if (ca.caSystemId in supportedSystemIds) {
                        true
                    } else {
                        diagnostics +=
                            Diagnostic(
                                State.ERROR,
                                ErrorCode.UNSUPPORTED_SYSTEM_ID,
                                ca.caSystemId,
                                ca.ecmPid ?: ca.emmPid ?: ca.elementaryPid,
                                "B25/B1 対象外の CA_system_id です",
                            )
                        false
                    }
                }
            val es = supported.filter { it.source == CaMetadataSource.ELEMENTARY_STREAM }
            // ESに具体化済みのPROGRAM情報から、同じECM用の余分なsessionを作らない。
            val sessionMetadata =
                supported.filter { ca ->
                    ca.serviceKey != null && ca.ecmPid != null && ca.source != CaMetadataSource.CAT &&
                        (
                            ca.source != CaMetadataSource.PROGRAM ||
                                es.none {
                                    it.serviceKey == ca.serviceKey && it.caSystemId == ca.caSystemId && it.ecmPid == ca.ecmPid
                                }
                        )
                }
            val bindings =
                sessionMetadata.groupBy { ca ->
                    SessionKey(requireNotNull(ca.serviceKey), ca.caSystemId, requireNotNull(ca.ecmPid), ca.privateData.toList())
                }
            val pidOwners = linkedMapOf<TsPid, MutableSet<SessionKey>>()
            bindings.forEach { (key, entries) ->
                entries.mapNotNull { it.elementaryPid }.forEach { pid ->
                    pidOwners.getOrPut(pid) { linkedSetOf() } += key
                }
            }
            pidOwners.filterValues { it.size > 1 }.forEach { (pid, _) ->
                diagnostics +=
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.DESCRAMBLER_FAILED,
                        pid = pid,
                        message = "同一ES PIDに異なるCAS sessionを同時割当できません",
                    )
            }
            if (diagnostics.isNotEmpty()) {
                lastDiagnostic = diagnostics.last()
                return@onExecutor UpdateResult(diagnostics, emptySet(), emptySet())
            }
            val emmBindings =
                supported.filter {
                    it.source == CaMetadataSource.CAT && it.emmPid != null && SupportedCasSystemIds.supportsEmm(it.caSystemId)
                }
            val targetSystems = (bindings.keys.map { it.caSystemId } + emmBindings.map { it.caSystemId }).toSet()
            val obsolete = pluginsBySystemId.values.filter { it.caSystemId !in targetSystems }
            obsolete.forEach { it.retiring = true }
            pluginsBySystemId.values.forEach { system ->
                system.sessions.values
                    .filter { it.key !in bindings }
                    .forEach { it.retiring = true }
            }
            // 旧sessionの解放を完了してから、新sessionやPID対応を公開する。
            SectionFilterPolicy.completeCleanup(
                *(
                    obsolete.map { system -> { closeSystemLocked(system.caSystemId) } } +
                        pluginsBySystemId.values.filterNot { it.retiring }.flatMap { system ->
                            system.sessions.values.filter { it.retiring }.map { state ->
                                { closeSessionLocked(system, state) }
                            }
                        }
                ).toTypedArray(),
            )
            bindings.forEach { (key, entries) ->
                ensureSessionLocked(key)
                    .onSuccess { state ->
                        state.elementaryPids.clear()
                        state.elementaryPids.addAll(entries.mapNotNull { it.elementaryPid })
                        state.session.setPrivateData(key.privateData.toByteArray()).onFailure { failure ->
                            diagnostics +=
                                Diagnostic(
                                    State.ERROR,
                                    ErrorCode.PRIVATE_DATA_FAILED,
                                    key.caSystemId,
                                    key.ecmPid,
                                    failure.message.orEmpty(),
                                )
                        }
                        if (state.elementaryPids.isNotEmpty() && state.descrambler == null && createDescrambler != null) {
                            runCatching {
                                val candidate = createDescrambler()
                                check(
                                    pluginsBySystemId.values.none { system ->
                                        system.sessions.values.any { it !== state && it.descrambler === candidate }
                                    },
                                ) { "異なるCAS sessionで同じDescrambler instanceを共有できません" }
                                state.descrambler = candidate
                            }.onFailure { failure ->
                                diagnostics +=
                                    Diagnostic(
                                        State.ERROR,
                                        ErrorCode.DESCRAMBLER_FAILED,
                                        key.caSystemId,
                                        message = failure.message.orEmpty(),
                                    )
                            }
                        }
                        runCatching { syncDescramblerPidsLocked(state, state.elementaryPids) }.onFailure {
                            diagnostics += lastDiagnostic
                        }
                    }.onFailure { failure ->
                        diagnostics += sessionFailureDiagnostic(key.caSystemId, failure, key.ecmPid)
                    }
            }
            emmBindings.forEach { binding ->
                ensureCasOnlyLocked(binding.caSystemId)
                    .onSuccess { cas ->
                        cas.setPrivateData(binding.privateData).onFailure { failure ->
                            diagnostics +=
                                Diagnostic(
                                    State.ERROR,
                                    ErrorCode.PRIVATE_DATA_FAILED,
                                    binding.caSystemId,
                                    binding.emmPid,
                                    failure.message.orEmpty(),
                                )
                        }
                    }.onFailure { failure ->
                        diagnostics += sessionFailureDiagnostic(binding.caSystemId, failure, binding.emmPid)
                    }
            }
            if (diagnostics.isEmpty()) {
                rebuildPidIndexesLocked()
                emmBindings.filter { pluginsBySystemId[it.caSystemId]?.retiring == false }.forEach { binding ->
                    emmPidToSystems.getOrPut(requireNotNull(binding.emmPid)) { linkedSetOf() } += binding.caSystemId
                }
            }
            lastDiagnostic =
                if (diagnostics.isEmpty()) Diagnostic(if (targetSystems.isEmpty()) State.IDLE else State.ACTIVE) else diagnostics.last()
            UpdateResult(diagnostics, ecmPidToSessions.keys.toSet(), emmPidToSystems.keys.toSet())
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
            if (closed) return@onExecutor listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"))
            val sessionKeys = ecmPidToSessions[pid].orEmpty()
            if (sessionKeys.isEmpty()) return@onExecutor emptyList()
            val diagnostics = mutableListOf<Diagnostic>()
            sessionKeys.forEach { key ->
                val systemId = key.caSystemId
                val system = pluginsBySystemId[systemId]?.takeUnless { it.retiring }
                val state = system?.sessions?.get(key)?.takeUnless { it.retiring || it.sessionClosed }
                if (state == null) {
                    diagnostics += Diagnostic(State.ERROR, ErrorCode.SESSION_OPEN_FAILED, systemId, pid, "CAS session がありません")
                    return@forEach
                }
                val tokenResult = state.session.processEcm(section)
                if (tokenResult.isFailure) {
                    diagnostics +=
                        Diagnostic(State.ERROR, ErrorCode.ECM_FAILED, systemId, pid, tokenResult.exceptionOrNull()?.message.orEmpty())
                    return@forEach
                }
                when (val ecmResult = tokenResult.getOrNull()) {
                    is EcmProcessResult.RealKeyToken -> {
                        val token = ecmResult.token
                        val setTokenResult =
                            state.descrambler?.setKeyToken(token)
                                ?: Result.failure(IllegalStateException("CAS session専用Tuner descrambler を利用できません"))
                        if (setTokenResult.isFailure) {
                            diagnostics +=
                                Diagnostic(
                                    State.ERROR,
                                    ErrorCode.DESCRAMBLER_FAILED,
                                    systemId,
                                    pid,
                                    setTokenResult.exceptionOrNull()?.message.orEmpty(),
                                )
                            return@forEach
                        }
                        state.keyLinked = true
                        state.elementaryPids.filter { it !in state.descramblerPids }.forEach { elementaryPid ->
                            val addResult =
                                state.descrambler?.addPid(elementaryPid)
                                    ?: Result.failure(IllegalStateException("CAS session専用Tuner descrambler を利用できません"))
                            if (addResult.isFailure) {
                                diagnostics +=
                                    Diagnostic(
                                        State.ERROR,
                                        ErrorCode.DESCRAMBLER_FAILED,
                                        systemId,
                                        elementaryPid,
                                        addResult.exceptionOrNull()?.message.orEmpty(),
                                    )
                            } else {
                                state.descramblerPids += elementaryPid
                            }
                        }
                    }

                    is EcmProcessResult.DiagnosticOnly -> {
                        diagnostics +=
                            Diagnostic(State.ERROR, ErrorCode.KEY_TOKEN_MISSING, systemId, pid, ecmResult.message)
                    }

                    null -> {
                        diagnostics +=
                            Diagnostic(State.ERROR, ErrorCode.KEY_TOKEN_MISSING, systemId, pid, "MediaCas session から実 key token を取得できません")
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
            if (closed) return@onExecutor listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"))
            val systems = emmPidToSystems[pid].orEmpty()
            if (systems.isEmpty()) return@onExecutor emptyList()
            val diagnostics = mutableListOf<Diagnostic>()
            systems.forEach { systemId ->
                val cas =
                    pluginsBySystemId[systemId]?.cas
                        ?: ensureCasOnlyLocked(systemId).getOrElse { failure ->
                            diagnostics += sessionFailureDiagnostic(systemId, failure, pid)
                            return@forEach
                        }
                cas.processEmm(section).onFailure { e ->
                    diagnostics +=
                        Diagnostic(State.ERROR, ErrorCode.EMM_FAILED, systemId, pid, e.message.orEmpty())
                }
            }
            if (diagnostics.isNotEmpty()) lastDiagnostic = diagnostics.last()
            diagnostics
        }

    fun lastDiagnostic(): Diagnostic = lastDiagnostic

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    // 境界呼出しの失敗を漏らさず扱い、既存の診断・解放・失敗伝播へ渡す。
    @Suppress("ReturnCount", "TooGenericExceptionCaught")
    private fun ensureSessionLocked(key: SessionKey): Result<CasSessionState> {
        val system = ensurePluginLocked(key.caSystemId).getOrElse { return Result.failure(it) }
        system.sessions[key]?.let { return Result.success(it) }
        val session =
            system.cas.openSession().getOrElse { failure ->
                try {
                    closeSystemLocked(key.caSystemId)
                } catch (cleanup: Exception) {
                    if (cleanup !== failure) failure.addSuppressed(cleanup)
                }
                return Result.failure(SessionProvisioningException(ErrorCode.SESSION_OPEN_FAILED, failure))
            }
        val state = CasSessionState(key, session)
        system.sessions[key] = state
        return Result.success(state)
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun ensurePluginLocked(caSystemId: Int): Result<CasSystemState> {
        pluginsBySystemId[caSystemId]?.let {
            return if (it.retiring) Result.failure(IllegalStateException("CAS資源は解放再試行待ちです")) else Result.success(it)
        }
        val cas =
            mediaCasFactory.create(caSystemId).getOrElse { failure ->
                return Result.failure(SessionProvisioningException(ErrorCode.PLUGIN_UNAVAILABLE, failure))
            }
        val state = CasSystemState(caSystemId, cas)
        pluginsBySystemId[caSystemId] = state
        return Result.success(state)
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    private fun ensureCasOnlyLocked(caSystemId: Int): Result<MediaCasBridge> = ensurePluginLocked(caSystemId).map { it.cas }

    private fun sessionFailureDiagnostic(
        caSystemId: Int,
        failure: Throwable,
        pid: TsPid? = null,
    ): Diagnostic {
        val errorCode =
            (failure as? SessionProvisioningException)?.errorCode
                ?: ErrorCode.SESSION_OPEN_FAILED
        return Diagnostic(State.ERROR, errorCode, caSystemId, pid, failure.message.orEmpty())
    }

    private fun unlinkDescramblerKeyIfOwnedByLocked(state: CasSessionState) {
        if (!state.keyLinked) return
        val bridge = requireNotNull(state.descrambler) { "MediaCas key token に対応するCAS session専用descramblerがありません" }
        bridge
            .setKeyToken(TunerKeyToken(Tuner.VOID_KEYTOKEN))
            .onFailure { error ->
                lastDiagnostic =
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.DESCRAMBLER_FAILED,
                        state.key.caSystemId,
                        message = "MediaCas session close前のVOID key-token unlinkに失敗しました: ${error.message.orEmpty()}",
                    )
            }.getOrThrow()
        state.keyLinked = false
    }

    private fun closeDescramblerLocked(state: CasSessionState) {
        state.descrambler?.close()
        state.descrambler = null
        state.descramblerPids.clear()
        state.keyLinked = false
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    @Suppress("MaxLineLength", "SpreadOperator")
    private fun syncDescramblerPidsLocked(
        state: CasSessionState,
        activePids: Set<TsPid>,
    ) {
        val bridge = state.descrambler ?: return
        SectionFilterPolicy.completeCleanup(
            *(state.descramblerPids - activePids)
                .map { pid ->
                    {
                        bridge
                            .removePid(pid)
                            .onFailure { error ->
                                lastDiagnostic =
                                    Diagnostic(
                                        State.ERROR,
                                        ErrorCode.DESCRAMBLER_FAILED,
                                        state.key.caSystemId,
                                        pid,
                                        error.message.orEmpty(),
                                    )
                            }.getOrThrow()
                        state.descramblerPids.remove(pid)
                        Unit
                    }
                }.toTypedArray(),
        )
    }

    private fun invalidateMetadataLocked() {
        ecmPidToSessions.clear()
        emmPidToSystems.clear()
    }

    private fun rebuildPidIndexesLocked() {
        invalidateMetadataLocked()
        pluginsBySystemId.values.filterNot { it.retiring }.forEach { system ->
            system.sessions.values.filterNot { it.retiring }.forEach { state ->
                ecmPidToSessions.getOrPut(state.key.ecmPid) { linkedSetOf() } += state.key
            }
        }
    }

    private fun closeSessionLocked(
        system: CasSystemState,
        state: CasSessionState,
    ) {
        state.retiring = true
        unlinkDescramblerKeyIfOwnedByLocked(state)
        if (!state.sessionClosed) {
            state.session.close()
            state.sessionClosed = true
        }
        closeDescramblerLocked(state)
        system.sessions.remove(state.key)
    }

    // 同一pluginのsessionを全件解放してからpluginを閉じる。
    @Suppress("SpreadOperator")
    private fun closeSystemLocked(caSystemId: Int) {
        val system = pluginsBySystemId[caSystemId] ?: return
        system.retiring = true
        SectionFilterPolicy.completeCleanup(
            *system.sessions.values
                .map { state ->
                    { closeSessionLocked(system, state) }
                }.toTypedArray(),
        )
        if (!system.casClosed) {
            system.cas.close()
            system.casClosed = true
        }
        pluginsBySystemId.remove(caSystemId)
    }

    override fun close() {
        if (executor.isShutdown) return
        onExecutor {
            closed = true
            clearForResourceLossLocked()
            lastDiagnostic = Diagnostic(State.CLOSED)
        }
        // cleanup失敗時は所有とexecutorを残し、close()の再試行を許す。
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
    override fun setPrivateData(privateData: ByteArray): Result<Unit> =
        runCatching {
            session.setPrivateData(privateData)
        }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("MaxLineLength")
    override fun processEcm(section: ByteArray): Result<EcmProcessResult> =
        runCatching {
            session.processEcm(section, 0, section.size)
            EcmProcessResult.DiagnosticOnly("MediaCas 標準 API は ECM 投入完了を返すが、r51 の placeholder CAS では Tuner 用の実 key token を返しません")
        }

    @Synchronized
    override fun close() {
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
