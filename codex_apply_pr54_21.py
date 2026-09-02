from pathlib import Path

ROOT = Path('.')

def read(path): return (ROOT/path).read_text(encoding='utf-8')
def write(path, text): (ROOT/path).write_text(text, encoding='utf-8')
def replace_once(path, old, new, label):
    text = read(path); n = text.count(old)
    if n != 1: raise SystemExit(f'{label}: expected 1 occurrence, found {n}')
    write(path, text.replace(old, new, 1))

# ScanCandidate permits a BS RF discovery seed with NONE only when explicitly marked.
replace_once('tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt',
'''        if (kind == ScanCandidateKind.ISDB_S_BS) {\n            val validBsSelector = streamSelector.type == StreamSelectorType.TSID || (backendHint == "px4" && streamSelector.type == StreamSelectorType.RELATIVE)\n            require(validBsSelector) { "BS はTSIDを基本とし、相対TS番号はpx4向け候補だけで許可します" }\n        }\n''',
'''        if (kind == ScanCandidateKind.ISDB_S_BS) {\n            val discoverySeed = backendHint == BS_DISCOVERY_BACKEND_HINT && streamSelector.type == StreamSelectorType.NONE\n            val explicitTune = streamSelector.type == StreamSelectorType.TSID || (backendHint == "px4" && streamSelector.type == StreamSelectorType.RELATIVE)\n            require(discoverySeed || explicitTune) { "BS はscan discovery seed(NONE)または明示TSIDを使用します" }\n        }\n''', 'BS candidate validation')
replace_once('tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt',
'''object JapanIsdbScanPlan {\n    private data class BsTsidEntry(val frequencyHz: FrequencyHz, val tsid: TransportStreamId16, val label: String, val physical: Int)\n''',
'''object JapanIsdbScanPlan {\n    const val BS_DISCOVERY_BACKEND_HINT = "jp-bs-discovery"\n    private data class BsTsidEntry(val frequencyHz: FrequencyHz, val tsid: TransportStreamId16, val label: String, val physical: Int)\n''', 'BS discovery constant')
replace_once('tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt',
'''    fun isdbsBsTsidStreams(backendHint: String = "earth_pt1"): List<ScanCandidate> = bsTsidEntries.map { entry ->\n        ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, entry.frequencyHz, streamSelector = StreamSelector.Tsid(entry.tsid), displayChannel = entry.label, physicalChannel = entry.physical, backendHint = backendHint, satelliteBand = "BS", kind = ScanCandidateKind.ISDB_S_BS)\n    }\n''',
'''    /** AOSP frontend scan用。TSIDを事前決め打ちせず、BS物理RFだけを列挙する。 */\n    fun isdbsBsBands(): List<ScanCandidate> = bsTsidEntries\n        .distinctBy { entry -> entry.frequencyHz.value to entry.physical }\n        .map { entry ->\n            ScanCandidate(\n                ChannelRecord.DELIVERY_SYSTEM_ISDB_S,\n                entry.frequencyHz,\n                streamSelector = StreamSelector.NONE,\n                displayChannel = "BS${entry.physical.toString().padStart(2, '0')}",\n                physicalChannel = entry.physical,\n                backendHint = BS_DISCOVERY_BACKEND_HINT,\n                satelliteBand = "BS",\n                kind = ScanCandidateKind.ISDB_S_BS,\n            )\n        }\n\n    /** scan callbackがstream IDを返せないfrontend向けcompatibility fallback。 */\n    fun isdbsBsTsidStreams(backendHint: String = "earth_pt1"): List<ScanCandidate> = bsTsidEntries.map { entry ->\n        ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, entry.frequencyHz, streamSelector = StreamSelector.Tsid(entry.tsid), displayChannel = entry.label, physicalChannel = entry.physical, backendHint = backendHint, satelliteBand = "BS", kind = ScanCandidateKind.ISDB_S_BS)\n    }\n\n    fun explicitBsCandidatesFromScan(seed: ScanCandidate, inputStreamIds: Collection<Int>): List<ScanCandidate> {\n        require(seed.kind == ScanCandidateKind.ISDB_S_BS && seed.streamSelector.type == StreamSelectorType.NONE)\n        return inputStreamIds\n            .asSequence()\n            .filter { it in 0..0xfffe }\n            .distinct()\n            .sorted()\n            .map { tsid ->\n                ScanCandidate(\n                    deliverySystem = ChannelRecord.DELIVERY_SYSTEM_ISDB_S,\n                    frequencyHz = seed.frequencyHz,\n                    streamSelector = StreamSelector.tsid(tsid),\n                    displayChannel = "${seed.displayChannel}-$tsid",\n                    physicalChannel = seed.physicalChannel,\n                    backendHint = "aosp-scan",\n                    satelliteBand = "BS",\n                    kind = ScanCandidateKind.ISDB_S_BS,\n                )\n            }\n            .toList()\n    }\n\n    fun fallbackBsCandidates(seed: ScanCandidate): List<ScanCandidate> = bsTsidEntries\n        .filter { it.frequencyHz == seed.frequencyHz && it.physical == seed.physicalChannel }\n        .map { entry ->\n            ScanCandidate(ChannelRecord.DELIVERY_SYSTEM_ISDB_S, entry.frequencyHz, StreamSelector.Tsid(entry.tsid), entry.label, entry.physical, "bs-tsid-fallback", "BS", ScanCandidateKind.ISDB_S_BS)\n        }\n''', 'BS scan functions')
replace_once('tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt',
'''    fun defaultInitialScan(): List<ScanCandidate> = isdbtUhf13To62() + isdbtCatvC13ToC63() + isdbsBsTsidStreams() + isdbs110CsBands()\n''',
'''    fun defaultInitialScan(): List<ScanCandidate> = isdbtUhf13To62() + isdbtCatvC13ToC63() + isdbsBsBands() + isdbs110CsBands()\n''', 'default BS scan-first')

# TunerController: typed AOSP scan API, bounded wait, always cancel scan after terminal collection.
path='tis/src/com/maleicacid/tvinput/tis/TunerController.kt'
replace_once(path, 'import android.media.tv.tuner.frontend.OnTuneEventListener\n', 'import android.media.tv.tuner.frontend.OnTuneEventListener\nimport android.media.tv.tuner.frontend.ScanCallback\nimport android.media.tv.tuner.frontend.Atsc3PlpInfo\n', 'scan imports')
replace_once(path, 'import java.util.concurrent.ExecutorService\n', 'import java.util.concurrent.CountDownLatch\nimport java.util.concurrent.ExecutorService\nimport java.util.concurrent.TimeUnit\n', 'scan concurrency imports')
anchor='''    fun tuneForScan(candidate: ScanCandidate): TuneOutcome = callOnController { tuneForScanOnController(candidate) }\n'''
addition='''    data class StreamIdDiscoveryResult(\n        val success: Boolean,\n        val streamIds: Set<Int>,\n        val resultCode: Int,\n        val message: String = "",\n    )\n\n    fun discoverIsdbsStreamIds(seed: ScanCandidate, timeoutMs: Long = BS_STREAM_ID_SCAN_TIMEOUT_MS): StreamIdDiscoveryResult =\n        callOnController { discoverIsdbsStreamIdsOnController(seed, timeoutMs) }\n\n    private fun discoverIsdbsStreamIdsOnController(seed: ScanCandidate, timeoutMs: Long): StreamIdDiscoveryResult {\n        require(seed.kind == ScanCandidateKind.ISDB_S_BS && seed.streamSelector == StreamSelector.NONE)\n        val tunerInstance = tuner ?: return StreamIdDiscoveryResult(false, emptySet(), Tuner.RESULT_UNAVAILABLE, "Tunerを利用できません")\n        resetBeforeTune()\n        val settings = IsdbsFrontendSettings.builder()\n            .setFrequencyLong(seed.frequencyHz.value)\n            .build()\n        val terminal = CountDownLatch(1)\n        val ids = linkedSetOf<Int>()\n        val callback = object : ScanCallback {\n            override fun onLocked() = Unit\n            override fun onUnlocked() = Unit\n            override fun onScanStopped() { terminal.countDown() }\n            override fun onProgress(percent: Int) { if (percent >= 100) terminal.countDown() }\n            @Suppress("DEPRECATION")\n            override fun onFrequenciesReported(frequencies: IntArray) = Unit\n            override fun onFrequenciesLongReported(frequencies: LongArray) = Unit\n            override fun onSymbolRatesReported(rate: IntArray) = Unit\n            override fun onPlpIdsReported(plpIds: IntArray) = Unit\n            override fun onGroupIdsReported(groupIds: IntArray) = Unit\n            override fun onInputStreamIdsReported(inputStreamIds: IntArray) {\n                synchronized(ids) { inputStreamIds.filterTo(ids) { it in 0..0xfffe } }\n            }\n            override fun onDvbsStandardReported(dvbsStandard: Int) = Unit\n            override fun onDvbtStandardReported(dvbtStandard: Int) = Unit\n            override fun onAnalogSifStandardReported(sif: Int) = Unit\n            override fun onAtsc3PlpInfosReported(atsc3PlpInfos: Array<Atsc3PlpInfo>) = Unit\n            override fun onHierarchyReported(hierarchy: Int) = Unit\n            override fun onSignalTypeReported(signalType: Int) = Unit\n            override fun onModulationReported(modulation: Int) = Unit\n            override fun onPriorityReported(isHighPriority: Boolean) = Unit\n            override fun onDvbcAnnexReported(dvbcAnnex: Int) = Unit\n            override fun onDvbtCellIdsReported(dvbtCellIds: IntArray) = Unit\n        }\n        val directExecutor = java.util.concurrent.Executor { command -> command.run() }\n        val result = runCatching { tunerInstance.scan(settings, Tuner.SCAN_TYPE_AUTO, directExecutor, callback) }\n            .getOrElse { error ->\n                return StreamIdDiscoveryResult(false, emptySet(), Tuner.RESULT_UNAVAILABLE, error.message.orEmpty())\n            }\n        if (result != Tuner.RESULT_SUCCESS) {\n            runCatching { tunerInstance.cancelScanning() }\n            return StreamIdDiscoveryResult(false, emptySet(), result, "Tuner.scanに失敗しました result=$result")\n        }\n        val completed = runCatching { terminal.await(timeoutMs.coerceAtLeast(1L), TimeUnit.MILLISECONDS) }.getOrDefault(false)\n        runCatching { tunerInstance.cancelScanning() }\n        val snapshot = synchronized(ids) { ids.toSet() }\n        return if (snapshot.isNotEmpty()) {\n            StreamIdDiscoveryResult(true, snapshot, result, if (completed) "" else "scan callback timeout後に報告済みstream IDを採用")\n        } else {\n            StreamIdDiscoveryResult(false, emptySet(), result, if (completed) "stream ID報告なし" else "scan callback timeout")\n        }\n    }\n\n'''+anchor
if read(path).count(anchor)!=1: raise SystemExit('tuneForScan anchor mismatch')
write(path, read(path).replace(anchor, addition, 1))
replace_once(path,
'''        private const val SECTION_FILTER_BUFFER_BYTES = 64 * 1024L\n''',
'''        private const val SECTION_FILTER_BUFFER_BYTES = 64 * 1024L\n        private const val BS_STREAM_ID_SCAN_TIMEOUT_MS = 2_500L\n''', 'scan timeout constant')

# ChannelScanController resolves BS RF seeds to explicit TSID candidates; fallback is diagnostics-visible.
path='tis/src/com/maleicacid/tvinput/tis/ChannelScanController.kt'
old='''        candidates.forEach { candidate ->\n            if (cancelled.get()) return@forEach\n            engine.reset(discoveryProfile(candidate.kind))\n            currentCandidate = candidate\n            val tune = tunerController.tuneForScan(candidate)\n'''
new='''        val executionCandidates = candidates.flatMap { candidate ->\n            if (candidate.kind == ScanCandidateKind.ISDB_S_BS && candidate.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE) {\n                val discovery = tunerController.discoverIsdbsStreamIds(candidate)\n                val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(candidate, discovery.streamIds)\n                if (discovered.isNotEmpty()) {\n                    discovered\n                } else {\n                    val fallback = JapanIsdbScanPlan.fallbackBsCandidates(candidate)\n                    diagnostics += ScanDiagnostic(candidate, "AOSP BS scanでstream IDを取得できないためversioned TSID fallbackを使用します result=${discovery.resultCode} message=${discovery.message} fallbackCount=${fallback.size}")\n                    fallback\n                }\n            } else {\n                listOf(candidate)\n            }\n        }\n        executionCandidates.forEach { candidate ->\n            if (cancelled.get()) return@forEach\n            engine.reset(discoveryProfile(candidate.kind))\n            currentCandidate = candidate\n            val tune = tunerController.tuneForScan(candidate)\n'''
if read(path).count(old)!=1: raise SystemExit('initial scan loop anchor mismatch')
text=read(path).replace(old,new,1)
text=text.replace('return ScanResult(candidates.size, published, diagnostics, successfulCandidates = successfulCandidates, terminalCancelObserved = terminalCancelObserved)', 'return ScanResult(executionCandidates.size, published, diagnostics, successfulCandidates = successfulCandidates, terminalCancelObserved = terminalCancelObserved)',1)
write(path,text)

# Design SSOT: scan-first, explicit tune remains TIS owned, fixed table is compatibility fallback not service identity authority.
path='開発規則.md'; text=read(path)
text=text.replace(
'''BSの通常実行時候補は、TISが保持するBS TSID表からIF周波数と`STREAM_ID 0..65534`として生成する。TISはHALのeffective capabilityやdriver名で候補を分岐しない。Tuner HALはselector kindを保持して各backend ABIへ写像する。px4の相対slot表またはlegacy数値域をTISへ複製してはならない。''',
'''BSのsetup/rescanは、TISがBS物理RF候補を生成し、AOSP `Tuner.scan()` の`ScanCallback.onInputStreamIdsReported()`で当該RFに現在存在するstream IDを取得してから、IF周波数+typed `STREAM_ID`のexplicit tune候補へ展開する経路を第一選択とする。scan callbackでstream IDを取得できないfrontendでは、現行ARIB運用資料に同期したTIS内versioned BS TSID表をcompatibility fallbackとしてだけ使用してよい。fallback表の値を受信後のservice identityのauthorityとせず、channel登録するONID/TSID/SIDは実際に受信したPAT/NIT/SDT等のPSI/SIを正とする。TISはHALのeffective capabilityやdriver名で候補を分岐せず、Tuner HALはselector kindを保持して各backend ABIへ写像する。px4の相対slot表またはlegacy数値域をTISへ複製してはならない。''')
text=text.replace(
'''製品用 channel scan の実行主体と候補表の実装データ保持者は TIS とする。TIS は本書で固定した設計契約に従い、地上UHF、CATV C13〜C63、BS、CS110 の候補表を実装データとして持つ。候補表の具体値、BS TSID 表、CATV中心周波数表、表示番号、サービス検出結果から作る channel key は TIS 側の実装データを正とする。''',
'''製品用 channel scan の実行主体と物理候補表の実装データ保持者は TIS とする。TIS は本書で固定した設計契約に従い、地上UHF、CATV C13〜C63、BS物理RF、CS110 の候補表を実装データとして持つ。BSの通常setup/rescanではAOSP scan callbackが返すcurrent stream IDを第一の実行時入力とし、TISのBS TSID表はscan非対応時のversioned compatibility fallbackに限定する。CATV中心周波数、表示番号、サービス検出結果から作るchannel keyはTIS側の実装データと受信PSI/SIを正とし、fallback TSID表を受信後service identityのSSOTにしない。''')
write(path,text)

path='tis/DESIGN_JA.md'; text=read(path)
text=text.replace(
'''TISの候補表は製品scan実装データのSSOTであり、`開発規則.md`の規範値に従うscan候補の実装データを唯一保持する。実行時にexplicit tune candidateを生成し、Tuner HALへ渡すscan値はTISが生成したexplicit tune candidateに限定する。TIS以外の文書や実装に同等の候補表を重複保持せず、Tuner HALは日本向けscan候補表を自前生成しない。候補生成をHALのeffective capabilityやdriver名で分岐せず、driver固有slotまたはlegacy数値域への写像はTuner HALへ委ねる。''',
'''TISの物理候補表は製品scan実装データのSSOTであり、`開発規則.md`の規範値に従うRF候補を唯一保持する。BS setup/rescanは物理RFごとにstream selector未指定の`IsdbsFrontendSettings`でAOSP `Tuner.scan()`を実行し、`ScanCallback.onInputStreamIdsReported()`で得たcurrent stream IDをtyped `STREAM_ID` explicit tune candidateへ変換する。scan callbackがstream IDを返せないfrontendに限りTISのversioned BS TSID表へfallbackするが、受信後のservice identityはPAT/NIT/SDT由来ONID/TSID/SIDを正とする。TIS以外に日本向け候補表を重複保持せず、候補生成をHALのeffective capabilityやdriver名で分岐しない。driver固有slotまたはlegacy数値域への写像はTuner HALへ委ねる。''')
write(path,text)

# Extend existing scan-plan test if present.
for testpath in [
    'tis/tests/src/com/maleicacid/tvinput/tis/ScanPlanTest.kt',
    'tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt',
]:
    p=ROOT/testpath
    if not p.exists(): continue
    text=p.read_text(encoding='utf-8')
    marker='''        check(TunerSelectionPolicy.selectVideo(service.streams) == null)\n'''
    if marker in text:
        text=text.replace(marker, marker+'''        val bsSeed = JapanIsdbScanPlan.isdbsBsBands().first()\n        check(bsSeed.streamSelector == com.maleicacid.tvinput.common.StreamSelector.NONE)\n        val discoveredBs = JapanIsdbScanPlan.explicitBsCandidatesFromScan(bsSeed, listOf(18803, 18803, 0xffff, -1))\n        check(discoveredBs.size == 1)\n        check(discoveredBs.single().streamSelector.value == 18803)\n        check(JapanIsdbScanPlan.fallbackBsCandidates(bsSeed).all { it.streamSelector.type == com.maleicacid.tvinput.common.StreamSelectorType.TSID })\n''',1)
        p.write_text(text,encoding='utf-8')
        break

# Projection MD must remain untouched by #21.
projection=read('ARIB_SI_EPG_TvProvider投影方針.md')
if 'Tuner.scan()' in projection or 'onInputStreamIdsReported' in projection:
    raise SystemExit('scan policy leaked into projection MD')

print('applied PR54 #21 AOSP scan-first BS discovery')
