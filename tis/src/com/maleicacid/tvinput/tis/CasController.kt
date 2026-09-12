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

    private data class CasSessionState(
        val caSystemId: Int,
        val cas: MediaCasBridge,
        var session: MediaCasSessionBridge? = null,
        val ecmPids: MutableSet<TsPid> = linkedSetOf(),
        val elementaryPids: MutableSet<TsPid> = linkedSetOf(),
        var retiring: Boolean = false,
        var sessionClosed: Boolean = false,
        var casClosed: Boolean = false,
    )

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

    private val sessionsBySystemId = LinkedHashMap<Int, CasSessionState>()
    private val ecmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private val emmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private val elementaryPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private var descrambler: TunerDescramblerBridge? = null
    private var descramblerClosing = false

    // addPid成功済みの物理所有。logical ownerが消えてもremove成功まで保持する。
    private val descramblerPids = linkedSetOf<TsPid>()
    private var closed = false

    @Volatile private var lastDiagnostic = Diagnostic(State.IDLE)

    /** 所有bridgeを退役させ、解放失敗中はECM/EMMの配送対象から外す。 */
    fun clearForResourceLoss(): Unit = onExecutor { clearForResourceLossLocked() }

    private fun clearForResourceLossLocked() {
        descramblerClosing = true
        try {
            SectionFilterPolicy.completeCleanup(
                { clearForClearServiceLocked() },
                { closeDescramblerLocked() },
            )
        } finally {
            ecmPidToSystems.clear()
            emmPidToSystems.clear()
            elementaryPidToSystems.clear()
        }
    }

    private fun closeDescramblerLocked() {
        descramblerClosing = true
        descrambler?.close()
        descrambler = null
        descramblerPids.clear()
        descramblerClosing = false
    }

    fun clearForClearService(): Unit = onExecutor { clearForClearServiceLocked() }

    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    @Suppress("SpreadOperator")
    private fun clearForClearServiceLocked() {
        invalidateMetadataLocked()
        sessionsBySystemId.values.forEach { it.retiring = true }
        SectionFilterPolicy.completeCleanup(
            {
                SectionFilterPolicy.completeCleanup(
                    *sessionsBySystemId.keys
                        .map { systemId ->
                            { closeSystemLocked(systemId) }
                        }.toTypedArray(),
                )
            },
            { if (!descramblerClosing) syncDescramblerPidsLocked(emptySet()) },
        )
        ecmPidToSystems.clear()
        emmPidToSystems.clear()
        elementaryPidToSystems.clear()
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
            if (descramblerClosing) clearForResourceLossLocked()
            SectionFilterPolicy.completeCleanup(
                *sessionsBySystemId.values
                    .filter { it.retiring }
                    .map { state ->
                        { closeSystemLocked(state.caSystemId) }
                    }.toTypedArray(),
            )
            if (descrambler == null && createDescrambler != null) descrambler = createDescrambler()
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
                            (ca.ecmPid ?: ca.emmPid ?: ca.elementaryPid),
                            "B25/B1 対象外の CA_system_id です",
                        )
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
                        if (!SupportedCasSystemIds.supportsEmm(ca.caSystemId)) return@forEach
                        val emmPid = ca.emmPid ?: return@forEach
                        emmBindings += EmmBinding(ca.caSystemId, emmPid, ca.privateData.copyOf())
                    }
                }
            }
            val targetSystems =
                (
                    programBindings.map { it.caSystemId } + esBindings.map { it.caSystemId } +
                        emmBindings.map { it.caSystemId }
                ).toSet()
            val obsolete = sessionsBySystemId.values.filter { it.caSystemId !in targetSystems }
            obsolete.forEach { it.retiring = true }
            val sessionSystems = (programBindings.map { it.caSystemId } + esBindings.map { it.caSystemId }).toSet()
            val sessionOnlyRetirements =
                sessionsBySystemId.values.filter {
                    !it.retiring && it.caSystemId !in sessionSystems &&
                        it.session != null
                }
            SectionFilterPolicy.completeCleanup(
                *(
                    obsolete.map { state -> { closeSystemLocked(state.caSystemId) } } +
                        sessionOnlyRetirements.map { state ->
                            {
                                try {
                                    state.session?.close()
                                    state.session = null
                                    state.sessionClosed = true
                                } catch (failure: Exception) {
                                    state.retiring = true
                                    throw failure
                                }
                            }
                        }
                ).toTypedArray(),
            )
            sessionsBySystemId.values.forEach { state ->
                state.ecmPids.clear()
                state.elementaryPids.clear()
            }
            (programBindings.map { it.caSystemId } + esBindings.map { it.caSystemId }).toSet().forEach { systemId ->
                val result = ensureSessionLocked(systemId)
                result.exceptionOrNull()?.let { failure ->
                    diagnostics += sessionFailureDiagnostic(systemId, failure)
                }
            }
            programBindings.forEach { binding ->
                sessionsBySystemId[binding.caSystemId]?.takeUnless { it.retiring }?.let { state ->
                    state.ecmPids += binding.ecmPid
                    requireNotNull(state.session).setPrivateData(binding.privateData).onFailure { e ->
                        diagnostics +=
                            Diagnostic(State.ERROR, ErrorCode.PRIVATE_DATA_FAILED, binding.caSystemId, binding.ecmPid, e.message.orEmpty())
                    }
                }
            }
            esBindings.forEach { binding ->
                sessionsBySystemId[binding.caSystemId]?.takeUnless { it.retiring }?.let { state ->
                    state.ecmPids += binding.ecmPid
                    state.elementaryPids += binding.elementaryPid
                    requireNotNull(state.session).setPrivateData(binding.privateData).onFailure { e ->
                        diagnostics +=
                            Diagnostic(State.ERROR, ErrorCode.PRIVATE_DATA_FAILED, binding.caSystemId, binding.ecmPid, e.message.orEmpty())
                    }
                }
            }
            emmBindings.forEach { binding ->
                ensureCasOnlyLocked(binding.caSystemId)
                    .onSuccess { cas ->
                        cas.setPrivateData(binding.privateData).onFailure { e ->
                            diagnostics +=
                                Diagnostic(
                                    State.ERROR,
                                    ErrorCode.PRIVATE_DATA_FAILED,
                                    binding.caSystemId,
                                    binding.emmPid,
                                    e.message.orEmpty(),
                                )
                        }
                    }.onFailure { failure ->
                        diagnostics += sessionFailureDiagnostic(binding.caSystemId, failure, binding.emmPid)
                    }
            }
            val activePids =
                sessionsBySystemId.values
                    .filterNot { it.retiring }
                    .flatMap { it.elementaryPids }
                    .toSet()
            runCatching { syncDescramblerPidsLocked(activePids) }.onFailure {
                diagnostics += lastDiagnostic
            }
            if (diagnostics.isEmpty()) {
                rebuildPidIndexesLocked()
                emmBindings.filter { sessionsBySystemId[it.caSystemId]?.retiring == false }.forEach { binding ->
                    emmPidToSystems.getOrPut(binding.emmPid) { linkedSetOf() } += binding.caSystemId
                }
            }
            lastDiagnostic =
                if (diagnostics.isEmpty()) Diagnostic(if (targetSystems.isEmpty()) State.IDLE else State.ACTIVE) else diagnostics.last()
            UpdateResult(diagnostics, ecmPidToSystems.keys.toSet(), emmPidToSystems.keys.toSet())
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
            val systems = ecmPidToSystems[pid].orEmpty()
            if (systems.isEmpty()) return@onExecutor emptyList()
            val diagnostics = mutableListOf<Diagnostic>()
            systems.forEach { systemId ->
                val state = sessionsBySystemId[systemId]
                if (state == null || state.retiring || state.session == null) {
                    diagnostics += Diagnostic(State.ERROR, ErrorCode.SESSION_OPEN_FAILED, systemId, pid, "CAS session がありません")
                    return@forEach
                }
                val tokenResult = requireNotNull(state.session).processEcm(section)
                if (tokenResult.isFailure) {
                    diagnostics +=
                        Diagnostic(State.ERROR, ErrorCode.ECM_FAILED, systemId, pid, tokenResult.exceptionOrNull()?.message.orEmpty())
                    return@forEach
                }
                when (val ecmResult = tokenResult.getOrNull()) {
                    is EcmProcessResult.RealKeyToken -> {
                        val token = ecmResult.token
                        val setTokenResult =
                            descrambler?.setKeyToken(token) ?: Result.failure(IllegalStateException("Tuner descrambler を利用できません"))
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
                        state.elementaryPids.filter { it !in descramblerPids }.forEach { elementaryPid ->
                            val addResult =
                                descrambler?.addPid(elementaryPid) ?: Result.failure(IllegalStateException("Tuner descrambler を利用できません"))
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
                                descramblerPids += elementaryPid
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
                    sessionsBySystemId[systemId]?.cas
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
    private fun ensureSessionLocked(caSystemId: Int): Result<CasSessionState> {
        val state = ensurePluginLocked(caSystemId).getOrElse { return Result.failure(it) }
        if (state.session != null) return Result.success(state)
        val session =
            state.cas.openSession().getOrElse { failure ->
                try {
                    closeSystemLocked(caSystemId)
                } catch (cleanup: Exception) {
                    if (cleanup !== failure) failure.addSuppressed(cleanup)
                }
                return Result.failure(SessionProvisioningException(ErrorCode.SESSION_OPEN_FAILED, failure))
            }
        state.session = session
        state.sessionClosed = false
        return Result.success(state)
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun ensurePluginLocked(caSystemId: Int): Result<CasSessionState> {
        sessionsBySystemId[caSystemId]?.let {
            return if (it.retiring) Result.failure(IllegalStateException("CAS資源は解放再試行待ちです")) else Result.success(it)
        }
        val cas =
            mediaCasFactory.create(caSystemId).getOrElse { failure ->
                return Result.failure(SessionProvisioningException(ErrorCode.PLUGIN_UNAVAILABLE, failure))
            }
        val state = CasSessionState(caSystemId, cas)
        sessionsBySystemId[caSystemId] = state
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

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    @Suppress("MaxLineLength", "SpreadOperator")
    private fun syncDescramblerPidsLocked(activePids: Set<TsPid>) {
        val bridge = descrambler ?: return
        SectionFilterPolicy.completeCleanup(
            *(descramblerPids - activePids)
                .map { pid ->
                    {
                        bridge
                            .removePid(pid)
                            .onFailure { error ->
                                lastDiagnostic =
                                    Diagnostic(State.ERROR, ErrorCode.DESCRAMBLER_FAILED, pid = pid, message = error.message.orEmpty())
                            }.getOrThrow()
                        descramblerPids.remove(pid)
                        Unit
                    }
                }.toTypedArray(),
        )
    }

    private fun invalidateMetadataLocked() {
        ecmPidToSystems.clear()
        emmPidToSystems.clear()
        elementaryPidToSystems.clear()
    }

    private fun rebuildPidIndexesLocked() {
        invalidateMetadataLocked()
        sessionsBySystemId.values.filterNot { it.retiring }.forEach { state ->
            state.ecmPids.forEach { ecmPid -> ecmPidToSystems.getOrPut(ecmPid) { linkedSetOf() } += state.caSystemId }
            state.elementaryPids.forEach { elementaryPid ->
                elementaryPidToSystems.getOrPut(elementaryPid) { linkedSetOf() } +=
                    state.caSystemId
            }
        }
    }

    private fun closeSystemLocked(caSystemId: Int) {
        val state = sessionsBySystemId[caSystemId] ?: return
        state.retiring = true
        SectionFilterPolicy.completeCleanup(
            {
                if (!state.sessionClosed) {
                    state.session?.close()
                    state.sessionClosed = true
                }
            },
            {
                if (!state.casClosed) {
                    state.cas.close()
                    state.casClosed = true
                }
            },
        )
        sessionsBySystemId.remove(caSystemId)
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
