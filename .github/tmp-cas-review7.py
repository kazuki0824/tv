from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if text.count(old) != 1:
        raise RuntimeError(f"{label}: expected exactly one match, got {text.count(old)}")
    return text.replace(old, new, 1)


# TIS implementation: enforce AOSP VOID unlink before MediaCas session close.
p = Path('tis/src/com/maleicacid/tvinput/tis/CasController.kt')
s = p.read_text(encoding='utf-8')

s = replace_once(
    s,
    '''    private var descrambler: TunerDescramblerBridge? = null
    private var descramblerClosing = false

    // addPid成功済みの物理所有。logical ownerが消えてもremove成功まで保持する。
''',
    '''    private var descrambler: TunerDescramblerBridge? = null
    private var descramblerClosing = false
    // AOSP Descrambler は1個のcurrent key slotだけを持つため、現在リンク中のMediaCas systemだけを保持する。
    private var descramblerKeyOwnerSystemId: Int? = null

    // addPid成功済みの物理所有。logical ownerが消えてもremove成功まで保持する。
''',
    'descrambler owner field',
)

s = replace_once(
    s,
    '''    private fun clearForResourceLossLocked() {
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
''',
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
    'resource loss order',
)

s = replace_once(
    s,
    '''        SectionFilterPolicy.completeCleanup(
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
''',
    '''        // system同士は独立に全件cleanupを試すが、PID cleanupは全session/plugin teardown成功後だけ行う。
        SectionFilterPolicy.completeCleanup(
            *sessionsBySystemId.keys
                .map { systemId ->
                    { closeSystemLocked(systemId) }
                }.toTypedArray(),
        )
        if (!descramblerClosing) syncDescramblerPidsLocked(emptySet())
''',
    'clear service dependency order',
)

s = replace_once(
    s,
    '''                                try {
                                    state.session?.close()
                                    state.session = null
                                    state.sessionClosed = true
                                } catch (failure: Exception) {
''',
    '''                                try {
                                    unlinkDescramblerKeyIfOwnedByLocked(state.caSystemId)
                                    state.session?.close()
                                    state.session = null
                                    state.sessionClosed = true
                                } catch (failure: Exception) {
''',
    'session-only retirement unlink',
)

s = replace_once(
    s,
    '''                        if (setTokenResult.isFailure) {
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
''',
    '''                        if (setTokenResult.isFailure) {
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
                        descramblerKeyOwnerSystemId = systemId
                        state.elementaryPids.filter { it !in descramblerPids }.forEach { elementaryPid ->
''',
    'record key owner',
)

insert_marker = '''    // 標準整形後に残る型・式・診断の長さだけを、この宣言で許容する。
    // 動的な引数列を既存の可変長APIへ渡すため、一時配列のコピーを許容する。
    @Suppress("MaxLineLength", "SpreadOperator")
    private fun syncDescramblerPidsLocked(activePids: Set<TsPid>) {
'''
helper = '''    private fun unlinkDescramblerKeyIfOwnedByLocked(caSystemId: Int) {
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

'''
if s.count(insert_marker) != 1:
    raise RuntimeError('unlink helper insertion marker mismatch')
s = s.replace(insert_marker, helper + insert_marker, 1)

s = replace_once(
    s,
    '''    private fun closeSystemLocked(caSystemId: Int) {
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
''',
    '''    private fun closeSystemLocked(caSystemId: Int) {
        val state = sessionsBySystemId[caSystemId] ?: return
        state.retiring = true
        // AOSP契約上、MediaCas session由来tokenはsession closeより先にVOIDでunlinkする。
        unlinkDescramblerKeyIfOwnedByLocked(caSystemId)
        if (!state.sessionClosed) {
            state.session?.close()
            state.sessionClosed = true
        }
        if (!state.casClosed) {
            state.cas.close()
            state.casClosed = true
        }
        sessionsBySystemId.remove(caSystemId)
    }
''',
    'close system dependency order',
)

p.write_text(s, encoding='utf-8')

# Strengthen an existing state test without increasing the test-count contract.
p = Path('tis/tests/src/com/maleicacid/tvinput/tis/CasControllerStateTest.kt')
s = p.read_text(encoding='utf-8')
start = s.index('    @Test fun resourceLossRetriesOnlyUnreleasedCasArtifacts() {')
end = s.index('    // 一つの契約の試験集合・時系列を保持し、検証シナリオを分断しない。', start)
replacement = '''    @Test fun resourceLossRetriesVoidThenSessionThenPluginThenDescrambler() {
        var rejectSessionClose = true
        var sessionCloses = 0
        var pluginCloses = 0
        val factory =
            object : CasController.MediaCasBridgeFactory {
                override fun create(caSystemId: Int) =
                    Result.success(
                        object : CasController.MediaCasBridge {
                            override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                            override fun processEmm(section: ByteArray) = Result.success(Unit)

                            override fun close() {
                                pluginCloses++
                            }

                            override fun openSession() =
                                Result.success(
                                    object : CasController.MediaCasSessionBridge {
                                        override fun setPrivateData(privateData: ByteArray) = Result.success(Unit)

                                        override fun processEcm(section: ByteArray) =
                                            Result.success<EcmProcessResult>(
                                                EcmProcessResult.RealKeyToken(TunerKeyToken(byteArrayOf(1))),
                                            )

                                        override fun close() {
                                            sessionCloses++
                                            if (rejectSessionClose) error("session close failed")
                                        }
                                    },
                                )
                        },
                    )
            }
        val bridge = RecordingDescrambler().apply {
            failUnlink = true
            failClose = true
        }
        val controller = CasController(mediaCasFactory = factory)
        controller.updateFromCaMetadata(b25Metadata(TsPid(0x101), TsPid(0x123), TsPid(0x010))) { bridge }
        check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())

        // VOID失敗時はMediaCas session/plugin/descramblerを先に閉じない。
        check(runCatching { controller.clearForResourceLoss() }.isFailure)
        check(bridge.unlinks == 1 && sessionCloses == 0 && pluginCloses == 0 && bridge.closes == 0)
        check(controller.onEcmSection(TsPid(0x123), byteArrayOf(1)).isEmpty())

        // VOID成功後にsession closeへ進み、session失敗時はplugin/descramblerを保持する。
        bridge.failUnlink = false
        check(runCatching { controller.clearForResourceLoss() }.isFailure)
        check(bridge.unlinks == 2 && sessionCloses == 1 && pluginCloses == 0 && bridge.closes == 0)

        // session/plugin成功後だけdescrambler closeへ進む。
        rejectSessionClose = false
        check(runCatching { controller.clearForResourceLoss() }.isFailure)
        check(bridge.unlinks == 2 && sessionCloses == 2 && pluginCloses == 1 && bridge.closes == 1)

        bridge.failClose = false
        controller.clearForResourceLoss()
        check(bridge.unlinks == 2 && sessionCloses == 2 && pluginCloses == 1 && bridge.closes == 2)
        controller.close()
    }

'''
s = s[:start] + replacement + s[end:]

s = replace_once(
    s,
    '''    private class RecordingDescrambler : CasController.TunerDescramblerBridge {
        var closes = 0
        var tokens = 0
        var failClose = false

        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> {
            tokens++
            return Result.success(Unit)
        }
''',
    '''    private class RecordingDescrambler : CasController.TunerDescramblerBridge {
        var closes = 0
        var tokens = 0
        var unlinks = 0
        var failClose = false
        var failUnlink = false

        override fun setKeyToken(keyToken: TunerKeyToken): Result<Unit> {
            if (keyToken.toByteArray().contentEquals(byteArrayOf(0))) {
                unlinks++
                if (failUnlink) return Result.failure(IllegalStateException("VOID unlink failed"))
            } else {
                tokens++
            }
            return Result.success(Unit)
        }
''',
    'recording descrambler VOID behavior',
)

p.write_text(s, encoding='utf-8')

# CAS SSOT: make failure ownership explicit.
p = Path('cas_plugin/DESIGN_JA.md')
s = p.read_text(encoding='utf-8')
s = replace_once(
    s,
    '''MediaCas由来tokenをTuner descramblerで使用した場合は、MediaCas sessionをcloseする前に、そのtokenを保持する全descramblerで `setKeyToken(VOID)` を成功させる。VOID成功を、そのdescramblerが以後の新規packet処理でtokenを使用しない確定点とする。

backend物理cleanupのretry/reset/taint方式はbackend resource ownerの実装詳細とし、service-global `CleanupPending` worker/tableを必須化しない。
''',
    '''MediaCas由来tokenをTuner descramblerで使用した場合は、MediaCas sessionをcloseする前に、そのtokenを保持する全descramblerで `setKeyToken(VOID)` を成功させる。VOID成功を、そのdescramblerが以後の新規packet処理でtokenを使用しない確定点とする。VOID unlinkが失敗した場合は当該MediaCas session/pluginを先にclose/releaseせず、既存のtoken linkとresource ownershipを保持してcleanup再試行を可能にする。session close成功後にplugin close/revokeへ進み、その後にPID/descrambler cleanupを行う。

backend物理cleanupのretry/reset/taint方式はbackend resource ownerの実装詳細とし、service-global `CleanupPending` worker/tableを必須化しない。
''',
    'CAS VOID failure ownership',
)
p.write_text(s, encoding='utf-8')

# TIS SSOT: align runtime teardown with the CAS/AOSP contract.
p = Path('tis/DESIGN_JA.md')
s = p.read_text(encoding='utf-8')
marker = '''resource-lostと再選局時はCasController.clearForResourceLoss()がCAS state清掃とbridge closeを全件試行する。close成功後だけ所有参照を落とし、失敗中はclosingとして保持してECM/EMMを配送しない。資源喪失時はbridge全体を閉じるため個別removePidで遅延生成を起こさない。CAS session/pluginも退役時に配送対象から外し、各closeの成功を記録して未解放資源だけを再試行する。次のbridge生成前またはclose()で退役資源の解放を再試行し、成功するまで新bridgeを生成・attachしない。失敗時に独立したretry queueや新しい世代は作らない。DirectTunerDescramblerBridge.close()は未生成handleを生成せず、実close失敗を伝播し、閉鎖開始後のsetKeyToken/addPid/removePidと再利用を拒否する。CAS全体のclose失敗でもexecutorと所有を残してclose再試行を可能にする。
'''
replacement = '''resource-lostと再選局時はCasController.clearForResourceLoss()がCAS state清掃とbridge closeを行う。MediaCas session由来のcurrent key tokenがdescramblerへリンクされている場合、TISはそのtokenを現在所有するCA systemだけを記録し、当該systemのsession close前に `Tuner.VOID_KEYTOKEN` を `Descrambler.setKeyToken()` へ渡してunlink成功を確認する。VOID失敗時はsession/pluginを先にcloseせず、token link、session/plugin、descrambler/PID所有を保持して次のcleanupで再試行する。VOID成功後にsession close、session close成功後にplugin closeを行い、全対象systemのこの依存teardownが成功した後だけPID removeまたはdescrambler closeへ進む。current tokenを所有していない別systemのsession closeに不要なVOIDを送らない。

close成功後だけ所有参照を落とし、失敗中はclosingとして保持してECM/EMMを配送しない。資源喪失時はbridge全体を閉じるため個別removePidで遅延生成を起こさない。CAS session/pluginも退役時に配送対象から外し、各closeの成功を記録して未解放資源だけを再試行する。次のbridge生成前またはclose()で退役資源の解放を再試行し、成功するまで新bridgeを生成・attachしない。失敗時に独立したretry queueや新しい世代は作らない。DirectTunerDescramblerBridge.close()は未生成handleを生成せず、実close失敗を伝播し、閉鎖開始後のsetKeyToken/addPid/removePidと再利用を拒否する。CAS全体のclose失敗でもexecutorと所有を残してclose再試行を可能にする。
'''
s = replace_once(s, marker, replacement, 'TIS VOID teardown contract')
p.write_text(s, encoding='utf-8')
