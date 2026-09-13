from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, got {count}")
    return text.replace(old, new, 1)


p = Path('tis/src/com/maleicacid/tvinput/tis/CasController.kt')
s = p.read_text(encoding='utf-8')

s = replace_once(
    s,
    '''    private data class CasSessionState(
        val caSystemId: Int,
        val cas: MediaCasBridge,
        var session: MediaCasSessionBridge? = null,
        val ecmPids: MutableSet<TsPid> = linkedSetOf(),
        val elementaryPids: MutableSet<TsPid> = linkedSetOf(),
        var retiring: Boolean = false,
        var sessionClosed: Boolean = false,
        var casClosed: Boolean = false,
    )
''',
    '''    private data class CasSessionState(
        val caSystemId: Int,
        val cas: MediaCasBridge,
        var session: MediaCasSessionBridge? = null,
        val ecmPids: MutableSet<TsPid> = linkedSetOf(),
        val elementaryPids: MutableSet<TsPid> = linkedSetOf(),
        var descrambler: TunerDescramblerBridge? = null,
        val descramblerPids: MutableSet<TsPid> = linkedSetOf(),
        var keyLinked: Boolean = false,
        var retiring: Boolean = false,
        var sessionClosed: Boolean = false,
        var casClosed: Boolean = false,
    )
''',
    'session descrambler ownership',
)

s = replace_once(
    s,
    '''    private val sessionsBySystemId = LinkedHashMap<Int, CasSessionState>()
    private val ecmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private val emmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private val elementaryPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private var descrambler: TunerDescramblerBridge? = null
    private var descramblerClosing = false

    // AOSP Descrambler は1個のcurrent key slotだけを持つため、現在リンク中のMediaCas systemだけを保持する。
    private var descramblerKeyOwnerSystemId: Int? = null

    // addPid成功済みの物理所有。logical ownerが消えてもremove成功まで保持する。
    private val descramblerPids = linkedSetOf<TsPid>()
    private var closed = false
''',
    '''    private val sessionsBySystemId = LinkedHashMap<Int, CasSessionState>()
    private val ecmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private val emmPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private val elementaryPidToSystems = LinkedHashMap<TsPid, MutableSet<Int>>()
    private var closed = false
''',
    'remove shared descrambler state',
)

s = replace_once(
    s,
    '''    private fun clearForResourceLossLocked() {
        descramblerClosing = true
        try {
            // MediaCas由来keyはVOID unlink後にだけsession/pluginを閉じ、その後でdescramblerを閉じる。
            clearForClearServiceLocked()
            closeDescramblerLocked()
        } finally {
            ecmPidToSystems.clear()
            emmPidToSystems.clear()
            elementaryPidToSystems.clear()
        }
    }

    private fun closeDescramblerLocked() {
        descramblerClosing = true
        check(descramblerKeyOwnerSystemId == null) { "MediaCas key token がリンク中のdescramblerはcloseできません" }
        descrambler?.close()
        descrambler = null
        descramblerPids.clear()
        descramblerClosing = false
    }
''',
    '''    private fun clearForResourceLossLocked() {
        try {
            // 各CA systemのMediaCas tokenをVOID unlinkしてから、そのsystem専用descramblerまで退役させる。
            clearForClearServiceLocked()
        } finally {
            ecmPidToSystems.clear()
            emmPidToSystems.clear()
            elementaryPidToSystems.clear()
        }
    }
''',
    'resource loss shared descrambler removal',
)

s = replace_once(
    s,
    '''        if (!descramblerClosing) syncDescramblerPidsLocked(emptySet())
        ecmPidToSystems.clear()
''',
    '''        ecmPidToSystems.clear()
''',
    'clear service shared pid cleanup',
)

s = replace_once(
    s,
    '''            if (descramblerClosing) clearForResourceLossLocked()
            SectionFilterPolicy.completeCleanup(
''',
    '''            SectionFilterPolicy.completeCleanup(
''',
    'remove shared descrambler closing gate',
)

s = replace_once(
    s,
    '''            if (descrambler == null && createDescrambler != null) descrambler = createDescrambler()
            val diagnostics = mutableListOf<Diagnostic>()
''',
    '''            val diagnostics = mutableListOf<Diagnostic>()
''',
    'remove shared descrambler creation',
)

s = replace_once(
    s,
    '''            val targetSystems =
                (
                    programBindings.map { it.caSystemId } + esBindings.map { it.caSystemId } +
                        emmBindings.map { it.caSystemId }
                ).toSet()
''',
    '''            val ambiguousElementaryPids =
                esBindings
                    .groupBy { it.elementaryPid }
                    .mapValues { (_, bindings) -> bindings.map { it.caSystemId }.toSet() }
                    .filterValues { it.size > 1 }
            ambiguousElementaryPids.forEach { (pid, systems) ->
                diagnostics +=
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.DESCRAMBLER_FAILED,
                        pid = pid,
                        message = "同一ES PIDに複数CA systemを同時割当できません systems=${systems.sorted()}",
                    )
            }
            val effectiveEsBindings = esBindings.filter { it.elementaryPid !in ambiguousElementaryPids }
            val targetSystems =
                (
                    programBindings.map { it.caSystemId } + effectiveEsBindings.map { it.caSystemId } +
                        emmBindings.map { it.caSystemId }
                ).toSet()
''',
    'ambiguous ES CA ownership',
)

s = s.replace('programBindings.map { it.caSystemId } + esBindings.map { it.caSystemId }', 'programBindings.map { it.caSystemId } + effectiveEsBindings.map { it.caSystemId }')
s = replace_once(s, '            esBindings.forEach { binding ->\n', '            effectiveEsBindings.forEach { binding ->\n', 'effective ES binding loop')

insert_after = '''            effectiveEsBindings.forEach { binding ->
                sessionsBySystemId[binding.caSystemId]?.takeUnless { it.retiring }?.let { state ->
                    state.ecmPids += binding.ecmPid
                    state.elementaryPids += binding.elementaryPid
                    requireNotNull(state.session).setPrivateData(binding.privateData).onFailure { e ->
                        diagnostics +=
                            Diagnostic(State.ERROR, ErrorCode.PRIVATE_DATA_FAILED, binding.caSystemId, binding.ecmPid, e.message.orEmpty())
                    }
                }
            }
'''
insert_new = insert_after + '''            sessionsBySystemId.values
                .filter { !it.retiring && it.elementaryPids.isNotEmpty() }
                .forEach { state ->
                    if (state.descrambler == null && createDescrambler != null) {
                        runCatching { createDescrambler() }
                            .onSuccess { candidate ->
                                val reused =
                                    sessionsBySystemId.values.any { other ->
                                        other.caSystemId != state.caSystemId && other.descrambler === candidate
                                    }
                                if (reused) {
                                    diagnostics +=
                                        Diagnostic(
                                            State.ERROR,
                                            ErrorCode.DESCRAMBLER_FAILED,
                                            state.caSystemId,
                                            message = "異なるCA systemで同じDescrambler instanceを共有できません",
                                        )
                                } else {
                                    state.descrambler = candidate
                                }
                            }.onFailure { failure ->
                                diagnostics +=
                                    Diagnostic(
                                        State.ERROR,
                                        ErrorCode.DESCRAMBLER_FAILED,
                                        state.caSystemId,
                                        message = failure.message.orEmpty(),
                                    )
                            }
                    }
                }
'''
s = replace_once(s, insert_after, insert_new, 'per-system descrambler creation')

s = replace_once(
    s,
    '''            val activePids =
                sessionsBySystemId.values
                    .filterNot { it.retiring }
                    .flatMap { it.elementaryPids }
                    .toSet()
            runCatching { syncDescramblerPidsLocked(activePids) }.onFailure {
                diagnostics += lastDiagnostic
            }
''',
    '''            sessionsBySystemId.values.filterNot { it.retiring }.forEach { state ->
                runCatching { syncDescramblerPidsLocked(state, state.elementaryPids) }.onFailure {
                    diagnostics += lastDiagnostic
                }
            }
''',
    'per-system pid sync',
)

s = replace_once(
    s,
    '''                        val setTokenResult =
                            descrambler?.setKeyToken(token) ?: Result.failure(IllegalStateException("Tuner descrambler を利用できません"))
''',
    '''                        val setTokenResult =
                            state.descrambler?.setKeyToken(token)
                                ?: Result.failure(IllegalStateException("CA system専用Tuner descrambler を利用できません"))
''',
    'per-system token target',
)
s = replace_once(s, '                        descramblerKeyOwnerSystemId = systemId\n', '                        state.keyLinked = true\n', 'per-system linked state')
s = replace_once(s, '                        state.elementaryPids.filter { it !in descramblerPids }.forEach { elementaryPid ->\n', '                        state.elementaryPids.filter { it !in state.descramblerPids }.forEach { elementaryPid ->\n', 'per-system pid membership')
s = replace_once(
    s,
    '''                            val addResult =
                                descrambler?.addPid(elementaryPid) ?: Result.failure(IllegalStateException("Tuner descrambler を利用できません"))
''',
    '''                            val addResult =
                                state.descrambler?.addPid(elementaryPid)
                                    ?: Result.failure(IllegalStateException("CA system専用Tuner descrambler を利用できません"))
''',
    'per-system pid target',
)
s = replace_once(s, '                                descramblerPids += elementaryPid\n', '                                state.descramblerPids += elementaryPid\n', 'per-system pid record')

s = replace_once(
    s,
    '''    private fun unlinkDescramblerKeyIfOwnedByLocked(caSystemId: Int) {
        if (descramblerKeyOwnerSystemId != caSystemId) return
        val bridge = requireNotNull(descrambler) { "MediaCas key token のownerに対応するdescramblerがありません" }
        bridge
            .setKeyToken(TunerKeyToken(Tuner.VOID_KEYTOKEN))
            .onFailure { error ->
                lastDiagnostic =
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.DESCRAMBLER_FAILED,
                        caSystemId,
                        message = "MediaCas session close前のVOID key-token unlinkに失敗しました: ${error.message.orEmpty()}",
                    )
            }.getOrThrow()
        descramblerKeyOwnerSystemId = null
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
''',
    '''    private fun unlinkDescramblerKeyIfOwnedByLocked(state: CasSessionState) {
        if (!state.keyLinked) return
        val bridge = requireNotNull(state.descrambler) { "MediaCas key token に対応するCA system専用descramblerがありません" }
        bridge
            .setKeyToken(TunerKeyToken(Tuner.VOID_KEYTOKEN))
            .onFailure { error ->
                lastDiagnostic =
                    Diagnostic(
                        State.ERROR,
                        ErrorCode.DESCRAMBLER_FAILED,
                        state.caSystemId,
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
                                        state.caSystemId,
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
''',
    'per-system unlink and pid cleanup helpers',
)

s = s.replace('                                    unlinkDescramblerKeyIfOwnedByLocked(state.caSystemId)\n', '                                    unlinkDescramblerKeyIfOwnedByLocked(state)\n', 1)
s = replace_once(
    s,
    '''                                    state.session = null
                                    state.sessionClosed = true
''',
    '''                                    state.session = null
                                    state.sessionClosed = true
                                    closeDescramblerLocked(state)
''',
    'session-only descrambler retirement',
)

s = replace_once(
    s,
    '''        unlinkDescramblerKeyIfOwnedByLocked(caSystemId)
        if (!state.sessionClosed) {
''',
    '''        unlinkDescramblerKeyIfOwnedByLocked(state)
        if (!state.sessionClosed) {
''',
    'close-system unlink state',
)
s = replace_once(
    s,
    '''        if (!state.casClosed) {
            state.cas.close()
            state.casClosed = true
        }
        sessionsBySystemId.remove(caSystemId)
''',
    '''        if (!state.casClosed) {
            state.cas.close()
            state.casClosed = true
        }
        closeDescramblerLocked(state)
        sessionsBySystemId.remove(caSystemId)
''',
    'close-system descrambler cleanup',
)

p.write_text(s, encoding='utf-8')

p = Path('tis/tests/src/com/maleicacid/tvinput/tis/CasControllerStateTest.kt')
s = p.read_text(encoding='utf-8')
start = s.index('    @Test\n    fun sharedElementaryPidSurvivesOneSystemRetirementAndRemovalFailure() {')
end = s.index('    @Test fun clearServiceRetriesOnlyPidsWhoseRemovalFailed()', start)
replacement = '''    @Test
    fun ambiguousElementaryPidAcrossCaSystemsFailsClosed() {
        val pid = TsPid(0x101)
        var bridgeCreates = 0
        CasController(mediaCasFactory = FakeMediaCasBridgeFactory()).use { controller ->
            val b25 = b25Metadata(pid, TsPid(0x123), TsPid(0x010))
            val b1 =
                b25
                    .filter { it.source != CaMetadataSource.CAT }
                    .map { it.copy(caSystemId = 1, ecmPid = TsPid(0x124)) }
            val update =
                controller.updateFromCaMetadata(b25 + b1) {
                    bridgeCreates++
                    RecordingDescrambler()
                }
            check(update.diagnostics.any { it.errorCode == CasController.ErrorCode.DESCRAMBLER_FAILED && it.pid == pid })
            check(update.ecmPids.isEmpty())
            check(bridgeCreates == 0)
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())
            check(controller.onEcmSection(TsPid(0x124), byteArrayOf(1)).isEmpty())
        }
    }

'''
s = s[:start] + replacement + s[end:]

old = '''    @Test fun sharedEmmPidOnlyDispatchesToB25WhileB1EcmRemainsUsable() {
        val factory = FakeMediaCasBridgeFactory()
        val descrambler = FakeTunerDescramblerBridge()
        CasController(mediaCasFactory = factory).use { controller ->
            val b1 =
                b25Metadata(TsPid(0x102), TsPid(0x124), TsPid(0x010))
                    .map { it.copy(caSystemId = CasController.SupportedCasSystemIds.ARIB_STD_B1) }
            val update =
                controller.updateFromCaMetadata(
                    b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010)) + b1,
                    { descrambler },
                )
            check(update.diagnostics.isEmpty())
            check(controller.onEmmSection(TsPid(0x010), byteArrayOf(0x82.toByte())).isEmpty())
            check(factory.created.getValue(CasController.SupportedCasSystemIds.ARIB_STD_B25).processedEmmCount == 1)
            check(factory.created.getValue(CasController.SupportedCasSystemIds.ARIB_STD_B1).processedEmmCount == 0)
            check(controller.onEcmSection(TsPid(0x124), byteArrayOf(0x80.toByte())).isEmpty())
            check(0x102 in descrambler.addedPids)
        }
    }
'''
new = '''    @Test fun sharedEmmPidOnlyDispatchesToB25WhileB1EcmRemainsUsable() {
        val factory = FakeMediaCasBridgeFactory()
        val descramblers = mutableListOf<FakeTunerDescramblerBridge>()
        CasController(mediaCasFactory = factory).use { controller ->
            val b1 =
                b25Metadata(TsPid(0x102), TsPid(0x124), TsPid(0x010))
                    .map { it.copy(caSystemId = CasController.SupportedCasSystemIds.ARIB_STD_B1) }
            val update =
                controller.updateFromCaMetadata(
                    b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010)) + b1,
                    {
                        FakeTunerDescramblerBridge().also { descramblers += it }
                    },
                )
            check(update.diagnostics.isEmpty())
            check(descramblers.size == 2)
            check(controller.onEmmSection(TsPid(0x010), byteArrayOf(0x82.toByte())).isEmpty())
            check(factory.created.getValue(CasController.SupportedCasSystemIds.ARIB_STD_B25).processedEmmCount == 1)
            check(factory.created.getValue(CasController.SupportedCasSystemIds.ARIB_STD_B1).processedEmmCount == 0)
            check(controller.onEcmSection(TsPid(0x123), byteArrayOf(0x80.toByte())).isEmpty())
            check(controller.onEcmSection(TsPid(0x124), byteArrayOf(0x80.toByte())).isEmpty())
            check(descramblers.count { 0x101 in it.addedPids } == 1)
            check(descramblers.count { 0x102 in it.addedPids } == 1)
            check(descramblers.none { 0x101 in it.addedPids && 0x102 in it.addedPids })
        }
    }
'''
s = replace_once(s, old, new, 'multi-system descrambler isolation test')
p.write_text(s, encoding='utf-8')

p = Path('tis/DESIGN_JA.md')
s = p.read_text(encoding='utf-8')
s = replace_once(
    s,
    '''descrambler bridgeの単一所有者はCasControllerとし、TunerControllerに同じbridgeをcacheしない。scan/liveは取得済みbridgeを持ち回らず、受信snapshotに対応するtune generationを必須入力とするTunerController.updateCasMetadataAndFilters()を使う。同controller executor内でtuneAcceptedとgenerationを照合し、CasControllerのfactoryによるbridge生成・attach・metadata更新と、成功結果のECM/EMM PIDを使うfilter更新完了まで待つ。metadata成功前に新filter集合を公開しない。resource-lostは同executorで直列化するため、取得とattachの間に割り込ませない。旧世代・失効済み要求はfactoryを呼ばず拒否する。
''',
    '''descrambler bridgeの単一所有者はCasControllerとし、TunerControllerにbridgeをcacheしない。AOSP `Descrambler` は1 instanceにつき1 key slotだけをlinkできるため、CasControllerは実key tokenを使うactive CA system/sessionごとに独立したDescrambler bridgeを所有し、異なるCA systemのtokenを同じDescramblerへ上書きしない。各bridgeへ登録するES PIDはそのCA systemのbindingだけとする。同一ES PIDが同じcurrent snapshotで複数のsupported CA systemへ同時にbindingされる場合、TISはCA systemの優先順位を推測せずambiguous inputとしてCAS attachをfail-closedにし、そのPIDを複数Descramblerへ二重登録しない。scan/liveは取得済みbridgeを持ち回らず、受信snapshotに対応するtune generationを必須入力とするTunerController.updateCasMetadataAndFilters()を使う。同controller executor内でtuneAcceptedとgenerationを照合し、CasControllerのfactoryによるCA system単位のbridge生成・attach・metadata更新と、成功結果のECM/EMM PIDを使うfilter更新完了まで待つ。metadata成功前に新filter集合を公開しない。resource-lostは同executorで直列化するため、取得とattachの間に割り込ませない。旧世代・失効済み要求はfactoryを呼ばず拒否する。
''',
    'TIS per-system descrambler contract',
)
s = replace_once(
    s,
    '''resource-lostと再選局時はCasController.clearForResourceLoss()がCAS state清掃とbridge closeを行う。MediaCas session由来のcurrent key tokenがdescramblerへリンクされている場合、TISはそのtokenを現在所有するCA systemだけを記録し、当該systemのsession close前に `Tuner.VOID_KEYTOKEN` を `Descrambler.setKeyToken()` へ渡してunlink成功を確認する。VOID失敗時はsession/pluginを先にcloseせず、token link、session/plugin、descrambler/PID所有を保持して次のcleanupで再試行する。VOID成功後にsession close、session close成功後にplugin closeを行い、全対象systemのこの依存teardownが成功した後だけPID removeまたはdescrambler closeへ進む。current tokenを所有していない別systemのsession closeに不要なVOIDを送らない。
''',
    '''resource-lostと再選局時はCasController.clearForResourceLoss()がCAS state清掃とbridge closeを行う。MediaCas session由来のcurrent key tokenがCA system専用descramblerへリンクされている場合、そのsystemのsession close前に同じbridgeへ `Tuner.VOID_KEYTOKEN` を渡してunlink成功を確認する。VOID失敗時は当該systemのsession/pluginを先にcloseせず、token link、session/plugin、descrambler/PID所有を保持して次のcleanupで再試行する。VOID成功後にsession close、session close成功後にplugin closeを行い、そのsystemの依存teardownが成功した後だけ対応Descrambler/PIDを解放する。別CA systemのbridge/token状態へは影響させない。
''',
    'TIS per-system teardown contract',
)
p.write_text(s, encoding='utf-8')
