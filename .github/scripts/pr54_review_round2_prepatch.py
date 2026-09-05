from pathlib import Path

p = Path('.github/scripts/pr54_review_round2_once.py')
text = p.read_text()
old = '''# manual known-field helperはnested validationから不要。
for fn_name in ("field_object_has_only", "object_has_only"):
    marker = f"fn {fn_name}("
    if marker in text:
        s = text.index(marker)
        # 次の関数境界まで削除。ただしobject_has_onlyが他用途なら後段compileで検出する。
        m = re.search(r"\\nfn [A-Za-z0-9_]+\\(", text[s + 1:])
        if m:
            e = s + 1 + m.start() + 1
            candidate = text[:s] + text[e:]
            if fn_name not in candidate:
                text = candidate
'''
new = '''# manual known-field helperはnested validationから不要。
manual_helpers = """fn field_object_has_only(parent: &serde_json::Value, field: &str, known_keys: &[&str]) -> bool {
    parent
        .get(field)
        .map(|value| object_has_only(value, known_keys))
        .unwrap_or(true)
}

fn object_has_only(value: &serde_json::Value, known_keys: &[&str]) -> bool {
    value
        .as_object()
        .map(|object| {
            object
                .keys()
                .all(|key| known_keys.iter().any(|known| *known == key))
        })
        .unwrap_or(false)
}

"""
if manual_helpers not in text:
    raise SystemExit("F02 manual helper blockが見つかりません")
text = text.replace(manual_helpers, "", 1)
'''
if old in text:
    text = text.replace(old, new, 1)

append = r'''

# F15回帰: versioned fallback前提のtestをdynamic discovery/fail-closed契約へ更新する。
p = Path("tis/tests/src/com/maleicacid/tvinput/tis/ScanPlanPolicyTest.kt")
t = p.read_text()
old = '''    @Test
    fun bsTsidTableRemainsExplicitCompatibilityFallback() {
        val bs = JapanIsdbScanPlan.isdbsBsTsidStreams()
        assertTrue(bs.isNotEmpty())
        assertTrue(bs.all { it.streamSelector.type == StreamSelectorType.TSID })
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        assertTrue(JapanIsdbScanPlan.fallbackBsCandidates(seed).all { it.streamSelector.type == StreamSelectorType.TSID })
    }

    @Test
    fun bs23CompatibilityFallbackTracksCurrentTransports() {
        val bs23 = JapanIsdbScanPlan.isdbsBsTsidStreams()
            .filter { it.physicalChannel == 23 }
            .mapNotNull { it.streamSelector.value }
            .toSet()
        assertEquals(setOf(18288, 18801, 18803), bs23)
        assertFalse(18802 in bs23)
    }
'''
new = '''    @Test
    fun bsDynamicDiscoveryUsesOnlyReportedStreamIds() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(
            seed,
            listOf(18288, 18801, 18803, 18803, -1, 0xffff),
        )
        assertEquals(setOf(18288, 18801, 18803), discovered.mapNotNull { it.streamSelector.value }.toSet())
        assertTrue(discovered.all { it.streamSelector.type == StreamSelectorType.TSID })
    }

    @Test
    fun bsDynamicDiscoveryWithNoReportedStreamIdsIsEmpty() {
        val seed = JapanIsdbScanPlan.isdbsBsBands().first()
        assertTrue(JapanIsdbScanPlan.explicitBsCandidatesFromScan(seed, emptyList()).isEmpty())
    }
'''
if t.count(old) != 1:
    raise SystemExit("F15 ScanPlanPolicyTest置換対象が一意ではありません")
t = t.replace(old, new, 1)
p.write_text(t)

p = Path("tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt")
t = p.read_text()
old = '''        check(discoveredBs.single().streamSelector.value == 18803)
        check(JapanIsdbScanPlan.fallbackBsCandidates(bsSeed).all { it.streamSelector.type == com.maleicacid.tvinput.common.StreamSelectorType.TSID })
'''
new = '''        check(discoveredBs.single().streamSelector.value == 18803)
        check(JapanIsdbScanPlan.explicitBsCandidatesFromScan(bsSeed, emptyList()).isEmpty())
'''
if t.count(old) != 1:
    raise SystemExit("F15 acceptance test置換対象が一意ではありません")
t = t.replace(old, new, 1)
p.write_text(t)

# generated patch内部の空白行を正規化する。
p = Path("tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch")
if p.exists():
    lines = p.read_text().splitlines()
    p.write_text("\n".join(line.rstrip() for line in lines).rstrip() + "\n")
'''
if "# F15回帰: versioned fallback前提のtest" not in text:
    text += append
p.write_text(text)
