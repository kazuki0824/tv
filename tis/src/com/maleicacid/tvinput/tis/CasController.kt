package com.maleicacid.tvinput.tis

import android.media.tv.tuner.Descrambler
import android.media.tv.tuner.Tuner
import com.maleicacid.tvinput.aribsi.CaMetadata
import com.maleicacid.tvinput.aribsi.CaMetadataSource
import com.maleicacid.tvinput.common.TsPid
import com.maleicacid.tvinput.common.TunerKeyToken

// 同じ状態・境界を扱う操作群を一つの所有者に保つ。

/**
 * B25/B1 向け CAS 制御。
 * PMT/CAT の CA情報 は arib_si_engine_rs の snapshot から受ける。
 * ECM/EMM は完全な section として扱い、生 TS packet は扱わない。
 * カード I/O、CW 生成、鍵発行は MediaCas/Maleicacid CAS plugin 側の責務とする。
 * Tuner HAL には 不透明 トークン と ES PID 登録だけを渡す。
 * 初期化・通知失効・通常終了・強制回収を同じplugin/session台帳で処理し、所有を分散させない。
 */
@Suppress("TooManyFunctions", "LargeClass")
class CasController(
    private val supportedSystemIds: Set<Int> = SupportedCasSystemIds.B25_B1,
    private val mediaCasFactory: MediaCasBridgeFactory = FrameworkMediaCasBridgeFactory(),
) : AutoCloseable {
    enum class ErrorCode {
        NONE,
        UNSUPPORTED_SYSTEM_ID,
        PLUGIN_UNAVAILABLE,
        PLUGIN_INITIALIZING,
        MEDIA_CAS_RESOURCE_LOST,
        SESSION_OPEN_FAILED,
        PRIVATE_DATA_FAILED,
        ECM_FAILED,
        EMM_FAILED,
        KEY_TOKEN_MISSING,
        INVALID_KEY_TOKEN,
        DESCRAMBLER_FAILED,
        MEDIA_CAS_INVALIDATED,
        CLOSED,
    }

    enum class State { IDLE, ACTIVE, ERROR, CLOSED }

    data class Diagnostic(
        val state: State,
        val errorCode: ErrorCode = ErrorCode.NONE,
        val caSystemId: Int? = null,
        val pid: TsPid? = null,
        val message: String = "",
        val cause: Throwable? = null,
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

    /** Frameworkの容量反映を待つ接続。通知はcontrollerへ非同期に戻す。 */
    interface InitializingMediaCasBridge : MediaCasBridge {
        val initializationBudgetMillis: Long

        fun elapsedRealtime(): Long

        fun initialize(listener: ConnectionListener)

        fun scheduleTimeout(
            delayMillis: Long,
            action: () -> Unit,
        ): () -> Unit
    }

    interface ConnectionListener {
        fun onConstructed()

        fun onCapacity(capacity: Int)

        fun onFailure(failure: Throwable)

        fun onResourceLost()
    }

    enum class ConnectionChange { READY, KEY_STATE_CHANGED, INITIALIZATION_FAILED, RESOURCES_LOST }

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

        // 初回のECMと鍵結合の成立を、参照の物理所有とは別に記録する。
        var ecmSucceeded: Boolean = false
        var retiring: Boolean = false
        var sessionClosed: Boolean = false
    }

    private class CasSystemState(
        val caSystemId: Int,
        val cas: MediaCasBridge,
        val generation: Long,
    ) {
        val sessions: MutableMap<SessionKey, CasSessionState> = linkedMapOf()
        var retiring: Boolean = false
        var casClosed: Boolean = false
        var invalidated: Boolean = false
        var reclaimed: Boolean = false
        var initializing: Boolean = cas is InitializingMediaCasBridge
        var initializationDeadline: Long = 0L
        var cancelTimeout: (() -> Unit)? = null
    }

    private class SessionProvisioningException(
        val errorCode: ErrorCode,
        cause: Throwable,
    ) : IllegalStateException(cause.message, cause)

    private val pluginsBySystemId = LinkedHashMap<Int, CasSystemState>()
    private val ecmPidToSessions = LinkedHashMap<TsPid, MutableSet<SessionKey>>()
    private val emmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private var closed = false
    private var receiveGeneration = 0L
    private var terminalReceiveGeneration: Long? = null
    private var onConnectionChanged: ((Long, ConnectionChange) -> Unit)? = null
    private var postToOwner: ((() -> Unit) -> Unit) = { action -> action() }

    internal fun setOwnerDispatcher(dispatcher: ((() -> Unit) -> Unit)) {
        postToOwner = dispatcher
    }

    internal fun setOnConnectionChanged(callback: ((Long, ConnectionChange) -> Unit)?) {
        onConnectionChanged = callback
    }

    internal fun isServiceDescramblingReady(
        serviceKey: com.maleicacid.tvinput.common.ServiceKey,
        generation: Long,
    ): Boolean = generation == receiveGeneration && serviceKey in descramblingReadyServicesLocked()

    private fun descramblingReadyServicesLocked(): Set<com.maleicacid.tvinput.common.ServiceKey> {
        if (closed || terminalReceiveGeneration == receiveGeneration) return emptySet()
        return pluginsBySystemId.values
            .filterNot { it.retiring || it.initializing }
            .flatMap { it.sessions.values }
            .groupBy { it.key.serviceKey }
            .filterValues { sessions -> sessions.all(::sessionDescramblingReadyLocked) }
            .keys
    }

    private fun sessionDescramblingReadyLocked(state: CasSessionState): Boolean {
        val live = !state.retiring && !state.sessionClosed
        val keyReady = state.keyLinked && state.ecmSucceeded && state.descrambler != null
        if (!live || !keyReady) return false
        val currentMetadata = state.key in ecmPidToSessions[state.key.ecmPid].orEmpty()
        return currentMetadata && state.elementaryPids.isNotEmpty() &&
            state.descramblerPids.containsAll(state.elementaryPids)
    }

    private fun postConnectionEvent(block: () -> Unit) {
        postToOwner(block)
    }

    private fun stopInitializationLocked(system: CasSystemState) {
        system.initializing = false
        system.cancelTimeout?.invoke()
        system.cancelTimeout = null
    }

    private fun failInitializationLocked(
        system: CasSystemState,
        failure: Throwable,
    ) {
        stopInitializationLocked(system)
        system.retiring = true
        terminalReceiveGeneration = system.generation
        invalidateMetadataLocked()
        lastDiagnostic = Diagnostic(State.ERROR, ErrorCode.PLUGIN_UNAVAILABLE, system.caSystemId, cause = failure)
        onConnectionChanged?.invoke(system.generation, ConnectionChange.INITIALIZATION_FAILED)
        runCatching { closeSystemLocked(system.caSystemId) }.onFailure {
            lastDiagnostic = lastDiagnostic.copy(cause = it)
        }
    }

    private fun isOwnedLocked(system: CasSystemState): Boolean = pluginsBySystemId[system.caSystemId] === system

    private fun beginInitializationLocked(
        system: CasSystemState,
        bridge: InitializingMediaCasBridge,
    ) {
        require(bridge.initializationBudgetMillis > 0L) { "CAS初期化予算は正の有限値が必要です" }
        system.initializationDeadline = Math.addExact(bridge.elapsedRealtime(), bridge.initializationBudgetMillis)
        system.cancelTimeout =
            bridge.scheduleTimeout(bridge.initializationBudgetMillis) {
                postConnectionEvent {
                    if (isOwnedLocked(system) && system.initializing &&
                        bridge.elapsedRealtime() >= system.initializationDeadline
                    ) {
                        failInitializationLocked(system, IllegalStateException("CAS初期容量通知が期限内に届きませんでした"))
                    }
                }
            }
        bridge.initialize(
            object : ConnectionListener {
                override fun onConstructed() =
                    postConnectionEvent {
                        if (isOwnedLocked(system) && system.retiring) {
                            runCatching { closeSystemLocked(system.caSystemId) }.onFailure {
                                lastDiagnostic = lastDiagnostic.copy(cause = it)
                            }
                        }
                    }

                override fun onCapacity(capacity: Int) =
                    postConnectionEvent {
                        if (!isOwnedLocked(system) || closed) return@postConnectionEvent
                        if (system.retiring || !system.initializing) return@postConnectionEvent
                        if (system.generation != receiveGeneration || capacity <= 0 ||
                            bridge.elapsedRealtime() >= system.initializationDeadline
                        ) {
                            failInitializationLocked(system, IllegalStateException("CAS初期容量通知が無効または期限切れです"))
                        } else {
                            stopInitializationLocked(system)
                            onConnectionChanged?.invoke(system.generation, ConnectionChange.READY)
                        }
                    }

                override fun onFailure(failure: Throwable) =
                    postConnectionEvent {
                        if (isOwnedLocked(system) && !system.retiring && system.initializing) {
                            failInitializationLocked(system, failure)
                        }
                    }

                override fun onResourceLost() =
                    postConnectionEvent {
                        if (!isOwnedLocked(system) || system.reclaimed || closed) return@postConnectionEvent
                        stopInitializationLocked(system)
                        system.reclaimed = true
                        system.retiring = true
                        terminalReceiveGeneration = system.generation
                        system.sessions.values.forEach { it.retiring = true }
                        invalidateMetadataLocked()
                        lastDiagnostic = Diagnostic(State.ERROR, ErrorCode.MEDIA_CAS_RESOURCE_LOST, system.caSystemId)
                        onConnectionChanged?.invoke(system.generation, ConnectionChange.RESOURCES_LOST)
                    }
            },
        )
    }

    @Volatile private var lastDiagnostic = Diagnostic(State.IDLE)

    /** 所有bridgeを退役させ、解放失敗中はECM/EMMの配送対象から外す。 */
    fun clearForResourceLoss(): Unit = clearForResourceLossLocked()

    // AOSPはDescramblerを閉じてから資源回収を通知する。閉鎖済みhandleへVOIDを再投入しない。
    @Suppress("SpreadOperator")
    internal fun onTunerResourcesReclaimed() {
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
        if (lastDiagnostic.errorCode != ErrorCode.MEDIA_CAS_INVALIDATED &&
            terminalReceiveGeneration != receiveGeneration
        ) {
            lastDiagnostic = Diagnostic(State.IDLE)
        }
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

    fun clearForClearService(): Unit = clearForClearServiceLocked()

    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    @Suppress("SpreadOperator")
    private fun clearForClearServiceLocked(propagateInvalidation: Boolean = false) {
        invalidateMetadataLocked()
        pluginsBySystemId.values.forEach { it.retiring = true }
        // system同士は独立に全件cleanupを試し、各sessionのVOID・close・descrambler cleanup後にpluginを閉じる。
        SectionFilterPolicy.completeCleanup(
            *pluginsBySystemId.keys
                .map { systemId ->
                    { closeSystemLocked(systemId, propagateInvalidation) }
                }.toTypedArray(),
        )
        ecmPidToSessions.clear()
        emmPidToSystems.clear()
        if (lastDiagnostic.errorCode != ErrorCode.MEDIA_CAS_INVALIDATED &&
            terminalReceiveGeneration != receiveGeneration
        ) {
            lastDiagnostic = Diagnostic(State.IDLE)
        }
    }

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    // 境界呼出しの失敗を漏らさず扱い、既存の診断・解放・失敗伝播へ渡す。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MaxLineLength", "ReturnCount", "SpreadOperator", "TooGenericExceptionCaught")
    internal fun updateFromCaMetadata(
        metadata: List<CaMetadata>,
        generation: Long = 0L,
        createDescrambler: (() -> TunerDescramblerBridge)? = null,
    ): UpdateResult {
        if (closed) {
            return UpdateResult(
                listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, message = "CAS 制御は終了済みです")),
                emptySet(),
                emptySet(),
            )
        }
        if (terminalReceiveGeneration == generation) {
            return UpdateResult(listOf(lastDiagnostic), emptySet(), emptySet())
        }
        receiveGeneration = generation
        // 配送indexはmetadata全体の成功時だけ公開する。物理解放の途中でsurvivorを再公開しない。
        invalidateMetadataLocked()
        if (metadata.isEmpty()) {
            clearForClearServiceLocked(propagateInvalidation = true)
            return UpdateResult(emptyList(), emptySet(), emptySet())
        }
        SectionFilterPolicy.completeCleanup(
            *pluginsBySystemId.values
                .filter { it.retiring }
                .map { state ->
                    { closeSystemLocked(state.caSystemId, propagateInvalidation = true) }
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
            return UpdateResult(diagnostics, emptySet(), emptySet())
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
                obsolete.map { system -> { closeSystemLocked(system.caSystemId, propagateInvalidation = true) } } +
                    pluginsBySystemId.values.filterNot { it.retiring }.flatMap { system ->
                        system.sessions.values.filter { it.retiring }.map { state ->
                            { closeSessionLocked(system, state) }
                        }
                    }
            ).toTypedArray(),
        )
        bindings.forEach { (key, entries) ->
            if (diagnostics.any { it.errorCode == ErrorCode.MEDIA_CAS_INVALIDATED }) return@forEach
            ensureSessionLocked(key)
                .onSuccess { state ->
                    state.elementaryPids.clear()
                    state.elementaryPids.addAll(entries.mapNotNull { it.elementaryPid })
                    state.session.setPrivateData(key.privateData.toByteArray()).onFailure { failure ->
                        diagnostics +=
                            casFailureDiagnosticLocked(
                                ErrorCode.PRIVATE_DATA_FAILED,
                                key.caSystemId,
                                key.ecmPid,
                                failure,
                            )
                    }
                    if (state.retiring) return@onSuccess
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
            if (diagnostics.any { it.errorCode == ErrorCode.MEDIA_CAS_INVALIDATED }) return@forEach
            ensureCasOnlyLocked(binding.caSystemId)
                .onSuccess { cas ->
                    cas.setPrivateData(binding.privateData).onFailure { failure ->
                        diagnostics +=
                            casFailureDiagnosticLocked(
                                ErrorCode.PRIVATE_DATA_FAILED,
                                binding.caSystemId,
                                binding.emmPid,
                                failure,
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
        return UpdateResult(diagnostics, ecmPidToSessions.keys.toSet(), emmPidToSystems.keys.toSet())
    }

    // 同じ入力に対する分岐・項目写像を保持し、処理分割による状態の受け渡しを増やさない。
    // 同じ入力と資源寿命を扱う手順を一続きに確認できる形に保つ。
    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    @Suppress("CyclomaticComplexMethod", "LongMethod", "MaxLineLength", "NestedBlockDepth", "ReturnCount")
    fun onEcmSection(
        pid: TsPid,
        section: ByteArray,
    ): List<Diagnostic> {
        if (closed) return listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"))
        val sessionKeys = ecmPidToSessions[pid].orEmpty()
        if (sessionKeys.isEmpty()) return emptyList()
        val readyBefore = descramblingReadyServicesLocked()
        val diagnostics = mutableListOf<Diagnostic>()
        sessionKeys.forEach { key ->
            if (diagnostics.any { it.errorCode == ErrorCode.MEDIA_CAS_INVALIDATED }) return@forEach
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
                    casFailureDiagnosticLocked(ErrorCode.ECM_FAILED, systemId, pid, requireNotNull(tokenResult.exceptionOrNull()))
                return@forEach
            }
            when (val ecmResult = tokenResult.getOrNull()) {
                is EcmProcessResult.RealKeyToken -> {
                    if (!state.keyLinked) {
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
                    }
                    state.ecmSucceeded = true
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
        if (readyBefore != descramblingReadyServicesLocked()) {
            onConnectionChanged?.invoke(receiveGeneration, ConnectionChange.KEY_STATE_CHANGED)
        }
        return diagnostics
    }

    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 無効化済みCASの後続処理を止めるガード節を、処理箇所から離さない。
    @Suppress("MaxLineLength", "ReturnCount")
    fun onEmmSection(
        pid: TsPid,
        section: ByteArray,
    ): List<Diagnostic> {
        if (closed) return listOf(Diagnostic(State.CLOSED, ErrorCode.CLOSED, pid = pid, message = "CAS 制御は終了済みです"))
        val systems = emmPidToSystems[pid].orEmpty()
        if (systems.isEmpty()) return emptyList()
        val diagnostics = mutableListOf<Diagnostic>()
        systems.forEach { systemId ->
            if (diagnostics.any { it.errorCode == ErrorCode.MEDIA_CAS_INVALIDATED }) return@forEach
            val cas =
                pluginsBySystemId[systemId]?.cas
                    ?: ensureCasOnlyLocked(systemId).getOrElse { failure ->
                        diagnostics += sessionFailureDiagnostic(systemId, failure, pid)
                        return@forEach
                    }
            cas.processEmm(section).onFailure { e ->
                diagnostics +=
                    casFailureDiagnosticLocked(ErrorCode.EMM_FAILED, systemId, pid, e)
            }
        }
        if (diagnostics.isNotEmpty()) lastDiagnostic = diagnostics.last()
        return diagnostics
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
                recordInvalidationLocked(key.caSystemId, failure)
                try {
                    closeSystemLocked(key.caSystemId, propagateInvalidation = true)
                } catch (cleanup: Exception) {
                    if (cleanup !== failure) failure.addSuppressed(cleanup)
                }
                val code = if (system.invalidated) ErrorCode.MEDIA_CAS_INVALIDATED else ErrorCode.SESSION_OPEN_FAILED
                return Result.failure(SessionProvisioningException(code, failure))
            }
        val state = CasSessionState(key, session)
        system.sessions[key] = state
        return Result.success(state)
    }

    // 入力拒否・未準備・失敗を発生点で返し、成功経路を深い入れ子にしない。
    @Suppress("ReturnCount")
    private fun ensurePluginLocked(caSystemId: Int): Result<CasSystemState> {
        pluginsBySystemId[caSystemId]?.let {
            return when {
                it.retiring -> {
                    Result.failure(IllegalStateException("CAS資源は解放再試行待ちです"))
                }

                it.initializing -> {
                    Result.failure(
                        SessionProvisioningException(
                            ErrorCode.PLUGIN_INITIALIZING,
                            IllegalStateException("CAS初期容量通知待ちです"),
                        ),
                    )
                }

                else -> {
                    Result.success(it)
                }
            }
        }
        val cas =
            mediaCasFactory.create(caSystemId).getOrElse { failure ->
                return Result.failure(SessionProvisioningException(ErrorCode.PLUGIN_UNAVAILABLE, failure))
            }
        val state = CasSystemState(caSystemId, cas, receiveGeneration)
        pluginsBySystemId[caSystemId] = state
        if (cas is InitializingMediaCasBridge) {
            runCatching { beginInitializationLocked(state, cas) }.onFailure { failInitializationLocked(state, it) }
            return Result.failure(
                SessionProvisioningException(
                    if (state.retiring) ErrorCode.PLUGIN_UNAVAILABLE else ErrorCode.PLUGIN_INITIALIZING,
                    IllegalStateException("CAS初期容量通知待ちです"),
                ),
            )
        }
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
        if (errorCode == ErrorCode.PLUGIN_INITIALIZING) {
            return Diagnostic(State.IDLE, errorCode, caSystemId, pid, "CAS初期容量通知待ちです")
        }
        return casFailureDiagnosticLocked(errorCode, caSystemId, pid, failure)
    }

    private fun recordInvalidationLocked(
        caSystemId: Int,
        failure: Throwable,
    ): Boolean {
        val cause = if (failure is SessionProvisioningException) failure.cause else failure
        if (cause !is MediaCasInvalidatedException) return false
        pluginsBySystemId[caSystemId]?.let { system ->
            system.invalidated = true
            system.retiring = true
            system.sessions.values.forEach { it.retiring = true }
        }
        invalidateMetadataLocked()
        return true
    }

    private fun casFailureDiagnosticLocked(
        errorCode: ErrorCode,
        caSystemId: Int,
        pid: TsPid?,
        failure: Throwable,
    ): Diagnostic {
        val code = if (recordInvalidationLocked(caSystemId, failure)) ErrorCode.MEDIA_CAS_INVALIDATED else errorCode
        return Diagnostic(State.ERROR, code, caSystemId, pid, failure.message.orEmpty(), failure)
            .also { lastDiagnostic = it }
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
        if (system.invalidated || system.reclaimed) return
        unlinkDescramblerKeyIfOwnedByLocked(state)
        if (!state.sessionClosed) {
            try {
                state.session.close()
            } catch (failure: MediaCasInvalidatedException) {
                casFailureDiagnosticLocked(ErrorCode.SESSION_OPEN_FAILED, system.caSystemId, state.key.ecmPid, failure)
                throw failure
            }
            state.sessionClosed = true
        }
        closeDescramblerLocked(state)
        system.sessions.remove(state.key)
    }

    // 無効化されたSessionの再closeを待たず、残るTuner参照とFramework側の所有を終了する。
    // 通常close失敗・終了処理失敗・metadata適用中の無効化を、それぞれの境界で返す。
    @Suppress("SpreadOperator", "TooGenericExceptionCaught", "ThrowsCount")
    private fun closeSystemLocked(
        caSystemId: Int,
        propagateInvalidation: Boolean = false,
    ) {
        val system = pluginsBySystemId[caSystemId] ?: return
        system.retiring = true
        stopInitializationLocked(system)
        var invalidationFailure: Exception? = null
        try {
            SectionFilterPolicy.completeCleanup(
                *system.sessions.values
                    .map { state -> { closeSessionLocked(system, state) } }
                    .toTypedArray(),
            )
        } catch (failure: Exception) {
            if (!system.invalidated) throw failure
            invalidationFailure = failure
            lastDiagnostic = lastDiagnostic.copy(cause = failure)
        }
        try {
            if (system.invalidated || system.reclaimed) {
                SectionFilterPolicy.completeCleanup(
                    *system.sessions.values
                        .map { state -> { closeDescramblerLocked(state) } }
                        .toTypedArray(),
                )
            }
            if (!system.casClosed) {
                system.cas.close()
                system.casClosed = true
            }
        } catch (cleanup: Exception) {
            invalidationFailure?.takeUnless { it === cleanup }?.let { cleanup.addSuppressed(it) }
            throw cleanup
        }
        system.sessions.clear()
        pluginsBySystemId.remove(caSystemId)
        // 終了が完了しても、その途中で無効化を検出したmetadata適用は成功へ変換しない。
        if (propagateInvalidation) invalidationFailure?.let { throw it }
    }

    override fun close() {
        if (closed && pluginsBySystemId.isEmpty()) return
        closed = true
        clearForResourceLossLocked()
        lastDiagnostic =
            if (lastDiagnostic.errorCode == ErrorCode.MEDIA_CAS_INVALIDATED) {
                lastDiagnostic.copy(state = State.CLOSED)
            } else {
                Diagnostic(State.CLOSED)
            }
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

/** Framework内で接続が無効化され、同じMediaCasのSessionを再利用できない失敗。 */
internal class MediaCasInvalidatedException(
    cause: IllegalStateException,
) : IllegalStateException("MediaCas instanceが無効化されました", cause)

class FrameworkMediaCasBridgeFactory(
    private val context: android.content.Context? = null,
    private val sessionId: String? = null,
    private val priorityHint: Int = android.media.tv.TvInputService.PRIORITY_HINT_USE_CASE_TYPE_LIVE,
) : CasController.MediaCasBridgeFactory {
    override fun create(caSystemId: Int): Result<CasController.MediaCasBridge> =
        runCatching {
            if (context == null) {
                val mediaCas = android.media.MediaCas(caSystemId)
                FrameworkMediaCasBridge({ mediaCas })
            } else {
                ManagedFrameworkMediaCasBridge(context, caSystemId, sessionId, priorityHint)
            }
        }
}

internal class FrameworkMediaCasBridge(
    private val mediaCas: () -> android.media.MediaCas,
    private val typedSession: Boolean = false,
) : CasController.MediaCasBridge {
    private var invalidation: MediaCasInvalidatedException? = null

    // 既存の無効状態・CAS固有状態・新規無効化を区別し、元の例外を保持する。
    @Suppress("ThrowsCount")
    private fun <T> callMediaCas(block: () -> T): T {
        invalidation?.let { throw it }
        return try {
            block()
        } catch (failure: android.media.MediaCasStateException) {
            throw failure
        } catch (failure: IllegalStateException) {
            throw MediaCasInvalidatedException(failure).also { invalidation = it }
        }
    }

    override fun setPrivateData(privateData: ByteArray): Result<Unit> =
        runCatching {
            callMediaCas { mediaCas().setPrivateData(privateData) }
        }

    override fun openSession(): Result<CasController.MediaCasSessionBridge> =
        runCatching {
            FrameworkMediaCasSessionBridge(
                requireNotNull(
                    callMediaCas {
                        if (typedSession) {
                            mediaCas().openSession(
                                android.media.MediaCas.SESSION_USAGE_LIVE,
                                android.media.MediaCas.SCRAMBLING_MODE_MULTI2,
                            )
                        } else {
                            mediaCas().openSession()
                        }
                    },
                ) { "MediaCasがsessionを返しませんでした" },
            )
        }

    override fun processEmm(section: ByteArray): Result<Unit> =
        runCatching {
            callMediaCas { mediaCas().processEmm(section, 0, section.size) }
        }

    @Synchronized
    override fun close() {
        mediaCas().close()
    }

    private inner class FrameworkMediaCasSessionBridge(
        private val session: android.media.MediaCas.Session,
    ) : CasController.MediaCasSessionBridge {
        override fun setPrivateData(privateData: ByteArray): Result<Unit> =
            runCatching {
                callMediaCas { session.setPrivateData(privateData) }
            }

        // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
        @Suppress("MaxLineLength")
        override fun processEcm(section: ByteArray): Result<EcmProcessResult> =
            runCatching {
                callMediaCas { session.processEcm(section, 0, section.size) }
                if (typedSession) {
                    val id = callMediaCas { session.sessionId }
                    require(!id.contentEquals(Tuner.VOID_KEYTOKEN)) { "MediaCas session IDがVOID予約値です" }
                    EcmProcessResult.RealKeyToken(TunerKeyToken(id))
                } else {
                    EcmProcessResult.DiagnosticOnly("診断用MediaCas接続では実key tokenを公開しません")
                }
            }

        @Synchronized
        override fun close() {
            callMediaCas { session.close() }
        }
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
