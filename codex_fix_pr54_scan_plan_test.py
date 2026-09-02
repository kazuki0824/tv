from pathlib import Path

path = Path("tis/tests/src/com/maleicacid/tvinput/tis/ScanPlanPolicyTest.kt")
text = path.read_text(encoding="utf-8")
old = '''    @Test
    fun defaultScanIncludesCatvAndUsesOnlyTsidCandidatesForBs() {
        val scan = JapanIsdbScanPlan.defaultInitialScan()
        assertTrue(scan.any { it.kind == ScanCandidateKind.ISDB_T_CATV && it.displayChannel == "C13" })
        assertTrue(scan.filter { it.kind == ScanCandidateKind.ISDB_S_BS }.all { it.streamSelector.type == StreamSelectorType.TSID })
    }
    @Test
    fun defaultScanKeepsBsTsidAsFirstClassSelector() {
        val bs = JapanIsdbScanPlan.isdbsBsTsidStreams()
        assertTrue(bs.isNotEmpty())
        assertTrue(bs.all { it.streamSelector.type == StreamSelectorType.TSID })
    }
'''
new = '''    @Test
    fun defaultScanIncludesCatvAndUsesRfDiscoverySeedsForBs() {
        val scan = JapanIsdbScanPlan.defaultInitialScan()
        assertTrue(scan.any { it.kind == ScanCandidateKind.ISDB_T_CATV && it.displayChannel == "C13" })
        val bs = scan.filter { it.kind == ScanCandidateKind.ISDB_S_BS }
        assertTrue(bs.isNotEmpty())
        assertTrue(bs.all { it.streamSelector.type == StreamSelectorType.NONE })
        assertTrue(bs.all { it.backendHint == JapanIsdbScanPlan.BS_DISCOVERY_BACKEND_HINT })
    }

    @Test
    fun bsTsidTableRemainsExplicitCompatibilityFallback() {
        val bs = JapanIsdbScanPlan.isdbsBsTsidStreams()
        assertTrue(bs.isNotEmpty())
        assertTrue(bs.all { it.streamSelector.type == StreamSelectorType.TSID })
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        assertTrue(JapanIsdbScanPlan.fallbackBsCandidates(seed).all { it.streamSelector.type == StreamSelectorType.TSID })
    }
'''
if text.count(old) != 1:
    raise SystemExit(f"scan policy test block count={text.count(old)}")
path.write_text(text.replace(old, new, 1), encoding="utf-8")
print("updated BS scan policy host test")
