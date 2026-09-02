from pathlib import Path

script_path = Path(__file__).with_name("codex_apply_pr54_21.py")
namespace = {"__name__": "__main__", "__file__": str(script_path)}
try:
    exec(compile(script_path.read_text(encoding="utf-8"), str(script_path), "exec"), namespace)
except SystemExit as error:
    if str(error) != "initial scan loop anchor mismatch":
        raise
else:
    raise SystemExit("original #21 staging script unexpectedly completed; fixed wrapper is obsolete")

# ScanPlan and TunerController changes have already been applied before the guarded failure.
path = Path("tis/src/com/maleicacid/tvinput/tis/ChannelScanController.kt")
text = path.read_text(encoding="utf-8")
start = text.index("    fun startInitialScan(")
end = text.index("\n    fun startBootEpgSync", start)
segment = text[start:end]
old = '''        candidates.forEach { candidate ->
            if (cancelled.get()) return@forEach
            engine.reset(discoveryProfile(candidate.kind))
            currentCandidate = candidate
            val tune = tunerController.tuneForScan(candidate)
'''
if segment.count(old) != 1:
    raise SystemExit(f"startInitialScan loop anchor count={segment.count(old)}")
new = '''        val executionCandidates = candidates.flatMap { candidate ->
            if (candidate.kind == ScanCandidateKind.ISDB_S_BS && candidate.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE) {
                val discovery = tunerController.discoverIsdbsStreamIds(candidate)
                val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(candidate, discovery.streamIds)
                if (discovered.isNotEmpty()) {
                    discovered
                } else {
                    val fallback = JapanIsdbScanPlan.fallbackBsCandidates(candidate)
                    diagnostics += ScanDiagnostic(candidate, "AOSP BS scanでstream IDを取得できないためversioned TSID fallbackを使用します result=${discovery.resultCode} message=${discovery.message} fallbackCount=${fallback.size}")
                    fallback
                }
            } else {
                listOf(candidate)
            }
        }
        executionCandidates.forEach { candidate ->
            if (cancelled.get()) return@forEach
            engine.reset(discoveryProfile(candidate.kind))
            currentCandidate = candidate
            val tune = tunerController.tuneForScan(candidate)
'''
segment = segment.replace(old, new, 1)
old_return = "return ScanResult(candidates.size, published, diagnostics, successfulCandidates = successfulCandidates, terminalCancelObserved = terminalCancelObserved)"
if segment.count(old_return) != 1:
    raise SystemExit("startInitialScan return anchor mismatch")
segment = segment.replace(old_return, "return ScanResult(executionCandidates.size, published, diagnostics, successfulCandidates = successfulCandidates, terminalCancelObserved = terminalCancelObserved)", 1)
path.write_text(text[:start] + segment + text[end:], encoding="utf-8")

# Synchronize normative design docs with scan-first behavior.
path = Path("開発規則.md")
text = path.read_text(encoding="utf-8")
old = '''BSの通常実行時候補は、TISが保持するBS TSID表からIF周波数と`STREAM_ID 0..65534`として生成する。TISはHALのeffective capabilityやdriver名で候補を分岐しない。Tuner HALはselector kindを保持して各backend ABIへ写像する。px4の相対slot表またはlegacy数値域をTISへ複製してはならない。'''
new = '''BSのsetup/rescanは、TISがBS物理RF候補を生成し、AOSP `Tuner.scan()` の`ScanCallback.onInputStreamIdsReported()`で当該RFに現在存在するstream IDを取得してから、IF周波数+typed `STREAM_ID`のexplicit tune候補へ展開する経路を第一選択とする。scan callbackでstream IDを取得できないfrontendでは、現行ARIB運用資料に同期したTIS内versioned BS TSID表をcompatibility fallbackとしてだけ使用してよい。fallback表の値を受信後のservice identityのauthorityとせず、channel登録するONID/TSID/SIDは実際に受信したPAT/NIT/SDT等のPSI/SIを正とする。TISはHALのeffective capabilityやdriver名で候補を分岐せず、Tuner HALはselector kindを保持して各backend ABIへ写像する。px4の相対slot表またはlegacy数値域をTISへ複製してはならない。'''
if old not in text: raise SystemExit("開発規則 BS normal path anchor missing")
text = text.replace(old, new, 1)
old = '''製品用 channel scan の実行主体と候補表の実装データ保持者は TIS とする。TIS は本書で固定した設計契約に従い、地上UHF、CATV C13〜C63、BS、CS110 の候補表を実装データとして持つ。候補表の具体値、BS TSID 表、CATV中心周波数表、表示番号、サービス検出結果から作る channel key は TIS 側の実装データを正とする。'''
new = '''製品用 channel scan の実行主体と物理候補表の実装データ保持者は TIS とする。TIS は本書で固定した設計契約に従い、地上UHF、CATV C13〜C63、BS物理RF、CS110 の候補表を実装データとして持つ。BSの通常setup/rescanではAOSP scan callbackが返すcurrent stream IDを第一の実行時入力とし、TISのBS TSID表はscan非対応時のversioned compatibility fallbackに限定する。CATV中心周波数、表示番号、サービス検出結果から作るchannel keyはTIS側の実装データと受信PSI/SIを正とし、fallback TSID表を受信後service identityのSSOTにしない。'''
if old not in text: raise SystemExit("開発規則 scan SSOT anchor missing")
path.write_text(text.replace(old, new, 1), encoding="utf-8")

path = Path("tis/DESIGN_JA.md")
text = path.read_text(encoding="utf-8")
old = '''TISの候補表は製品scan実装データのSSOTであり、`開発規則.md`の規範値に従うscan候補の実装データを唯一保持する。実行時にexplicit tune candidateを生成し、Tuner HALへ渡すscan値はTISが生成したexplicit tune candidateに限定する。TIS以外の文書や実装に同等の候補表を重複保持せず、Tuner HALは日本向けscan候補表を自前生成しない。候補生成をHALのeffective capabilityやdriver名で分岐せず、driver固有slotまたはlegacy数値域への写像はTuner HALへ委ねる。'''
new = '''TISの物理候補表は製品scan実装データのSSOTであり、`開発規則.md`の規範値に従うRF候補を唯一保持する。BS setup/rescanは物理RFごとにstream selector未指定の`IsdbsFrontendSettings`でAOSP `Tuner.scan()`を実行し、`ScanCallback.onInputStreamIdsReported()`で得たcurrent stream IDをtyped `STREAM_ID` explicit tune candidateへ変換する。scan callbackがstream IDを返せないfrontendに限りTISのversioned BS TSID表へfallbackするが、受信後のservice identityはPAT/NIT/SDT由来ONID/TSID/SIDを正とする。TIS以外に日本向け候補表を重複保持せず、候補生成をHALのeffective capabilityやdriver名で分岐しない。driver固有slotまたはlegacy数値域への写像はTuner HALへ委ねる。'''
if old not in text: raise SystemExit("TIS design scan SSOT anchor missing")
path.write_text(text.replace(old, new, 1), encoding="utf-8")

# Extend an existing host test without changing test count.
test_path = Path("tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt")
text = test_path.read_text(encoding="utf-8")
marker = '''        check(TunerSelectionPolicy.selectVideo(service.streams) == null)
'''
if text.count(marker) < 1: raise SystemExit("BS scan test insertion marker missing")
addition = marker + '''        val bsSeed = JapanIsdbScanPlan.isdbsBsBands().first()
        check(bsSeed.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE)
        val discoveredBs = JapanIsdbScanPlan.explicitBsCandidatesFromScan(bsSeed, listOf(18803, 18803, 0xffff, -1))
        check(discoveredBs.size == 1)
        check(discoveredBs.single().streamSelector.value == 18803)
        check(JapanIsdbScanPlan.fallbackBsCandidates(bsSeed).all { it.streamSelector.type == com.maleicacid.tvinput.common.StreamSelectorType.TSID })
'''
test_path.write_text(text.replace(marker, addition, 1), encoding="utf-8")

projection = Path("ARIB_SI_EPG_TvProvider投影方針.md").read_text(encoding="utf-8")
if "Tuner.scan()" in projection or "onInputStreamIdsReported" in projection:
    raise SystemExit("scan policy leaked into projection MD")

print("applied PR54 #21 AOSP scan-first BS discovery")
