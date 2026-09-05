from pathlib import Path
import difflib
import os
import re


def read(path: str) -> str:
    return Path(path).read_text()


def write(path: str, text: str) -> None:
    Path(path).write_text(text)


def one(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"置換対象が一意ではありません: {path}: {count}: {old[:100]!r}")
    write(path, text.replace(old, new, 1))


# F02: nested DTOはclosed Serdeをvalidatorの正本とし、Value再走査/手書きunknown collectorを削除する。
p = Path("arib_si_engine_rs/src/core/provider_data.rs")
text = p.read_text()
old = '''    let raw_value = match serde_json::from_str::<serde_json::Value>(text.trim()) {
        Ok(value) => value,
        Err(err) => {
            return failure_result(
                "PROGRAM_PROVIDER_DATA_PARSE_FAILED",
                format!("Program provider-data JSON parse failed: {err}"),
                PROVIDER_SCHEMA_VERSION,
            )
        }
    };
    let data = match serde_json::from_value::<ProgramProviderDataV1>(raw_value.clone()) {
        Ok(data) => data,
        Err(err) => {
            return failure_result(
                "PROGRAM_PROVIDER_DATA_SCHEMA_FAILED",
                format!("Program provider-data JSON v1 schema parse failed: {err}"),
                PROVIDER_SCHEMA_VERSION,
            )
        }
    };
    let data = normalize_program_extensions(data, Some(&raw_value));'''
new = '''    let data = match serde_json::from_str::<ProgramProviderDataV1>(text.trim()) {
        Ok(data) => data,
        Err(err) => {
            let (code, message) = match err.classify() {
                serde_json::error::Category::Syntax | serde_json::error::Category::Eof => (
                    "PROGRAM_PROVIDER_DATA_PARSE_FAILED",
                    format!("Program provider-data JSONの構文解析に失敗しました: {err}"),
                ),
                serde_json::error::Category::Data | serde_json::error::Category::Io => (
                    "PROGRAM_PROVIDER_DATA_SCHEMA_FAILED",
                    format!("Program provider-data JSON v1の型契約に適合しません: {err}"),
                ),
            };
            return failure_result(code, message, PROVIDER_SCHEMA_VERSION);
        }
    };
    let data = normalize_program_extensions(data);'''
if text.count(old) != 1:
    raise SystemExit("F02 program typed deserialize対象が一意ではありません")
text = text.replace(old, new, 1)
old = '''    let Ok(raw_value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return String::new();
    };
    let Ok(data) = serde_json::from_value::<ChannelProviderDataV1>(raw_value.clone()) else {
        return String::new();
    };
    let data = normalize_channel_extensions(data, Some(&raw_value));'''
new = '''    let Ok(data) = serde_json::from_str::<ChannelProviderDataV1>(text.trim()) else {
        return String::new();
    };
    let data = normalize_channel_extensions(data);'''
if text.count(old) != 1:
    raise SystemExit("F02 channel typed deserialize対象が一意ではありません")
text = text.replace(old, new, 1)
text = text.replace(
    '''fn normalize_program_extensions(
    mut data: ProgramProviderDataV1,
    raw_value: Option<&serde_json::Value>,
) -> ProgramProviderDataV1 {''',
    '''fn normalize_program_extensions(mut data: ProgramProviderDataV1) -> ProgramProviderDataV1 {''',
    1,
)
text = text.replace(
    '''fn normalize_channel_extensions(
    mut data: ChannelProviderDataV1,
    raw_value: Option<&serde_json::Value>,
) -> ChannelProviderDataV1 {''',
    '''fn normalize_channel_extensions(mut data: ChannelProviderDataV1) -> ChannelProviderDataV1 {''',
    1,
)
for block in [
'''    if let Some(raw) = raw_value {
        let mut nested = Vec::new();
        collect_program_unknown_extensions(raw, &mut nested);
        for extension in nested {
            if !forbidden_program_extension(&extension.key)
                && !data
                    .diagnostics
                    .raw_provider_data_extensions
                    .iter()
                    .any(|existing| existing.key == extension.key)
            {
                data.diagnostics
                    .raw_provider_data_extensions
                    .push(extension);
            }
        }
    }
''',
'''    if let Some(raw) = raw_value {
        let mut nested = Vec::new();
        collect_channel_unknown_extensions(raw, &mut nested);
        for extension in nested {
            if !forbidden_channel_extension(&extension.key)
                && !data
                    .diagnostics
                    .raw_provider_data_extensions
                    .iter()
                    .any(|existing| existing.key == extension.key)
            {
                data.diagnostics
                    .raw_provider_data_extensions
                    .push(extension);
            }
        }
    }
''']:
    if block not in text:
        raise SystemExit("F02 nested collector call blockが見つかりません")
    text = text.replace(block, "", 1)
collector_start = text.index("fn collect_channel_unknown_extensions(")
collector_end = text.index("fn note_drop(", collector_start)
text = text[:collector_start] + text[collector_end:]
# descriptor diagnosticもtyped Serdeだけでclosed contractを検証する。
start = text.index("fn parse_descriptor_diagnostics(")
end = text.index("\nfn valid_descriptor_diagnostic(", start)
text = text[:start] + '''fn parse_descriptor_diagnostics(text: &str) -> Option<Vec<DescriptorDiagnosticV1>> {
    let items = serde_json::from_str::<Vec<DescriptorDiagnosticV1>>(text).ok()?;
    if items.iter().all(valid_descriptor_diagnostic) {
        Some(items)
    } else {
        None
    }
}
''' + text[end:]
# manual known-field helperはnested validationから不要。
for fn_name in ("field_object_has_only", "object_has_only"):
    marker = f"fn {fn_name}("
    if marker in text:
        s = text.index(marker)
        # 次の関数境界まで削除。ただしobject_has_onlyが他用途なら後段compileで検出する。
        m = re.search(r"\nfn [A-Za-z0-9_]+\(", text[s + 1:])
        if m:
            e = s + 1 + m.start() + 1
            candidate = text[:s] + text[e:]
            if fn_name not in candidate:
                text = candidate
# 既知の英語error messageを日本語化。machine-readable error codeは維持する。
repls = {
    "Program provider-data request JSON parse failed": "Program provider-data request JSONの解析に失敗しました",
    "Program provider-data request did not satisfy schema v1 invariants": "Program provider-data requestがschema v1の不変条件を満たしません",
    "Channel provider-data request JSON parse failed": "Channel provider-data request JSONの解析に失敗しました",
    "Channel provider-data request did not satisfy schema v1 invariants": "Channel provider-data requestがschema v1の不変条件を満たしません",
    "Program provider-data is not UTF-8": "Program provider-dataがUTF-8ではありません",
    "Program provider-data JSON v1 invariants failed": "Program provider-data JSON v1の不変条件を満たしません",
    "Program provider-data serialization failed": "Program provider-dataのserializationに失敗しました",
    "Truncated program provider-data serialization failed": "切詰め後Program provider-dataのserializationに失敗しました",
    "Channel provider-data JSON v1 invariants failed": "Channel provider-data JSON v1の不変条件を満たしません",
    "Channel provider-data serialization failed": "Channel provider-dataのserializationに失敗しました",
    "Truncated channel provider-data serialization failed": "切詰め後Channel provider-dataのserializationに失敗しました",
}
for a, b in repls.items():
    text = text.replace(a, b)
p.write_text(text)

# F08: RELATIVEは一時的tune selectorであり、永続identityへTSIDとして黙って変換しない。
p = Path("tis/src/com/maleicacid/tvinput/aribsi/ProviderDataBridge.kt")
text = p.read_text()
old = '''        val selector = when (channel.streamSelector.type) {
            com.maleicacid.tvinput.common.StreamSelectorType.RELATIVE -> StreamSelector.tsid(channel.serviceKey.transportStreamId)
            else -> channel.streamSelector
        }'''
new = '''        require(channel.streamSelector.type != com.maleicacid.tvinput.common.StreamSelectorType.RELATIVE) {
            "RELATIVE stream selectorはTvProvider永続identityとして保存できません"
        }
        if (channel.streamSelector.type == com.maleicacid.tvinput.common.StreamSelectorType.TSID) {
            require(channel.streamSelector.value == channel.serviceKey.transportStreamId) {
                "保存TSID selectorはbroadcast service identityのTSIDと一致する必要があります"
            }
        }
        val selector = channel.streamSelector'''
if text.count(old) != 1:
    raise SystemExit("F08 selector変換対象が一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

# F13: include!対象の生成表をSoong sandboxへ明示する。
p = Path("arib_si_engine_rs/Android.bp")
text = p.read_text()
needle = '        "src/core/arib_jis_x0208_table.rs",\n        "src/core/arib_string.rs",'
if text.count(needle) != 2:
    raise SystemExit(f"F13 Android.bp挿入箇所が2件ではありません: {text.count(needle)}")
text = text.replace(needle, '        "src/core/arib_jis_x0208_table.rs",\n        "src/core/arib_extended_graphic_table.rs",\n        "src/core/arib_string.rs",')
p.write_text(text)

# F15: scan失敗/timeout/0件をstatic TSID候補へ変換せずfail-closedにする。
p = Path("tis/src/com/maleicacid/tvinput/tis/ChannelScanController.kt")
text = p.read_text()
old = '''                val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(candidate, discovery.streamIds)
                if (discovered.isNotEmpty()) {
                    discovered
                } else {
                    val fallback = JapanIsdbScanPlan.fallbackBsCandidates(candidate)
                    diagnostics += ScanDiagnostic(candidate, "AOSP BS scanでstream IDを取得できないためversioned TSID fallbackを使用します result=${discovery.resultCode} message=${discovery.message} fallbackCount=${fallback.size}")
                    fallback
                }'''
new = '''                val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(candidate, discovery.streamIds)
                if (discovered.isNotEmpty()) {
                    discovered
                } else {
                    diagnostics += ScanDiagnostic(
                        candidate,
                        "BS dynamic stream-ID discoveryをfail-closedにします result=${discovery.resultCode} message=${discovery.message}",
                    )
                    emptyList()
                }'''
if text.count(old) != 1:
    raise SystemExit("F15 BS fallback対象が一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

p = Path("tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt")
text = p.read_text()
# fallback関数とそのversioned table ownerをproduction sourceから削除する。
if "fun fallbackBsCandidates(" in text:
    s = text.index("    fun fallbackBsCandidates(")
    e = text.index("\n\n    fun isdbs110CsBands()", s)
    text = text[:s] + text[e + 2:]
if "fun isdbsBsTsidStreams(" in text:
    # 直前commentを含めexplicitBsCandidatesFromScan直前まで削除。
    s = text.rfind("    /**", 0, text.index("    fun isdbsBsTsidStreams("))
    e = text.index("    fun explicitBsCandidatesFromScan", s)
    text = text[:s] + text[e:]
p.write_text(text)

# F16: 新versionのlast_section_number縮小で明示的に範囲外となった旧version sectionを回収する。
p = Path("arib_si_engine_rs/src/core/eit.rs")
text = p.read_text()
needle = '''        let previous_keys: BTreeSet<EitEventKey> = self
            .section_events
            .get(&section_key)
            .map(|old| old.event_keys.clone())
            .unwrap_or_default();
        let new_keys: BTreeSet<_> = parsed.iter().filter_map(stable_event_key).collect();'''
insert = '''        let previous_keys: BTreeSet<EitEventKey> = self
            .section_events
            .get(&section_key)
            .map(|old| old.event_keys.clone())
            .unwrap_or_default();
        let obsolete_section_keys: BTreeSet<EitSectionKey> = if deletion_authoritative {
            self.section_events
                .iter()
                .filter(|(key, old)| {
                    key.table_id == header.table_id
                        && key.service_id == service_id
                        && key.transport_stream_id == transport_stream_id
                        && key.original_network_id == original_network_id
                        && old.version != version
                        && key.section_number > header.last_section_number.unwrap_or(section_number)
                })
                .map(|(key, _)| *key)
                .collect()
        } else {
            BTreeSet::new()
        };
        let obsolete_event_keys: BTreeSet<EitEventKey> = obsolete_section_keys
            .iter()
            .filter_map(|key| self.section_events.get(key))
            .flat_map(|old| old.event_keys.iter().copied())
            .collect();
        let surviving_section_references: BTreeSet<EitEventKey> = self.section_events
            .iter()
            .filter(|(key, _)| **key != section_key && !obsolete_section_keys.contains(*key))
            .flat_map(|(_, old)| old.event_keys.iter().copied())
            .collect();
        let new_keys: BTreeSet<_> = parsed.iter().filter_map(stable_event_key).collect();'''
if text.count(needle) != 1:
    raise SystemExit("F16 EIT insertion anchorが一意ではありません")
text = text.replace(needle, insert, 1)
old = '''        let removable_previous_keys: BTreeSet<_> = if deletion_authoritative {
            previous_keys
                .difference(&new_keys)
                .filter(|old_key| !malformed_event_keys.contains(old_key))
                .copied()
                .collect()
        } else {
            BTreeSet::new()
        };'''
new = '''        let removal_candidates: BTreeSet<EitEventKey> = previous_keys
            .difference(&new_keys)
            .copied()
            .chain(obsolete_event_keys.iter().copied())
            .collect();
        let removable_previous_keys: BTreeSet<_> = if deletion_authoritative {
            removal_candidates
                .into_iter()
                .filter(|old_key| !malformed_event_keys.contains(old_key))
                .filter(|old_key| !new_keys.contains(old_key))
                .filter(|old_key| !surviving_section_references.contains(old_key))
                .collect()
        } else {
            BTreeSet::new()
        };'''
if text.count(old) != 1:
    raise SystemExit("F16 removal blockが一意ではありません")
text = text.replace(old, new, 1)
old = '''        for old_key in &removable_previous_keys {
            self.events.remove(old_key);
        }
        self.diagnostic_section_events
            .insert(section_key, parsed.clone());'''
new = '''        for obsolete_section_key in &obsolete_section_keys {
            self.section_events.remove(obsolete_section_key);
            self.diagnostic_section_events.remove(obsolete_section_key);
        }
        for old_key in &removable_previous_keys {
            self.events.remove(old_key);
        }
        self.diagnostic_section_events
            .insert(section_key, parsed.clone());'''
if text.count(old) != 1:
    raise SystemExit("F16 obsolete section removal anchorが一意ではありません")
text = text.replace(old, new, 1)
# regressionは既存F05 testの直後に追加。
marker = '''    #[test]
    fn authoritative_valid_update_window_marks_obsolete_delete_allowed() {'''
test = '''    #[test]
    fn version_update_shrinking_last_section_number_reclaims_obsolete_sections() {
        let mut store = EitStore::default();
        let start0 = [0xee, 0x00, 0x12, 0x00, 0x00];
        let start1 = [0xee, 0x01, 0x13, 0x00, 0x00];
        let start2 = [0xee, 0x02, 0x14, 0x00, 0x00];
        for (section_number, event_id, start) in [
            (0_u8, 1_u16, start0),
            (1_u8, 2_u16, start1),
            (2_u8, 3_u16, start2),
        ] {
            let mut section = eit_body(1, &[(event_id, start)]);
            section[6] = section_number;
            section[7] = 2;
            store.upsert_section(&section_with_crc(section));
        }
        assert_eq!(store.section_count_for_diagnostic(), 3);
        let mut new_section0 = eit_body(2, &[(1, start0)]);
        new_section0[6] = 0;
        new_section0[7] = 1;
        store.upsert_section(&section_with_crc(new_section0));
        let events = store.snapshot_present_following_actual();
        assert_eq!(store.section_count_for_diagnostic(), 2);
        assert!(events.iter().any(|event| event.event_id == 1 && event.version == 2));
        assert!(events.iter().any(|event| event.event_id == 2 && event.version == 1));
        assert!(!events.iter().any(|event| event.event_id == 3));
    }

'''
if text.count(marker) != 1:
    raise SystemExit("F16 regression anchorが一意ではありません")
text = text.replace(marker, test + marker, 1)
p.write_text(text)

# F07: MediaSyncのPR追加Javadoc/comment/exceptionを日本語化する。
p = Path("tis/platform_patches/lineage-22.1/frameworks_base_mediasync_first_output.patch")
text = p.read_text()
repls = {
"Listener for the first video frame successfully queued by MediaSync to its output surface": "MediaSyncがoutput surfaceへ正常queueした最初のvideo frameを通知するlistener",
"after an explicit arm operation.": "明示arm後のeventだけを対象とする。",
"Arms a one-shot callback for the first video frame that MediaSync successfully queues to": "MediaSyncがcurrent output surfaceへ正常queueした最初のvideo frameについてone-shot callbackをarmする。",
"its current output surface. The arm sequence is opaque to MediaSync and is returned unchanged": "arm sequenceはMediaSyncではopaqueに扱い、callbackへそのまま返す。",
"with the callback so a platform-coupled client can reject a stale delayed event after re-arm.": "これによりplatform連携clientはre-arm前の遅延eventを拒否できる。",
"<p>This observes successful queueing to MediaSync's output surface. It does not indicate": "<p>観測対象はMediaSync output surfaceへのqueue成功であり、",
"compositor presentation or present-fence completion.</p>": "compositor表示完了やpresent-fence完了を意味しない。</p>",
"@param armSequence positive identifier scoped by the caller to this MediaSync instance": "@param armSequence callerがこのMediaSync instance内で割り当てる正の識別子",
"@param listener callback to arm, or {@code null} to disarm": "@param listener armするcallback。{@code null}ならdisarmする",
"@param handler handler used to deliver the callback, or {@code null} for the current/main": "@param handler callback配送用handler。{@code null}ならcurrent/main",
"looper": "looperを使う",
"armSequence must be positive when armed": "arm時のarmSequenceは正である必要があります",
"Disarm is part of release cleanup and remains harmless after native_release().": "disarmはrelease cleanupの一部であり、native_release()後でも無害に扱う。",
}
for a, b in repls.items():
    text = text.replace(a, b)
p.write_text(text)

# F09/F15: active design baselineとdynamic scan契約をAndroid 15/LineageOS 22.1へ揃える。
p = Path("tis/DESIGN_JA.md")
text = p.read_text()
for a, b in [
("Android 14 Tuner API builder", "Android 15 Tuner API builder"),
("Android 14の`MediaCodec.QueueRequest`", "Android 15の`MediaCodec.QueueRequest`"),
("stock Android 14 / LineageOS 21", "stock Android 15 / LineageOS 22.1"),
("LineageOS 21／Android 14の通常ライブセッション", "LineageOS 22.1／Android 15の通常ライブセッション"),
("LineageOS 21の通常経路", "LineageOS 22.1の通常経路"),
("Android 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`", "Android 15（API 35）環境の公開`AudioTrack.Builder.setContext(sessionContext)`"),
]:
    text = text.replace(a, b)
text = text.replace(
"scan callbackがstream IDを返せないfrontendに限りTISのversioned BS TSID表へfallbackするが、受信後のservice identityはPAT/NIT/SDT由来ONID/TSID/SIDを正とする。",
"`Tuner.scan()`が失敗した場合、timeoutした場合、または完了してもstream IDが0件の場合は診断を分離してfail-closedとし、versioned TSID表を成功候補へ代入しない。受信後のservice identityはPAT/NIT/SDT由来ONID/TSID/SIDを正とする。",
)
p.write_text(text)

# F14: integration文書を実在class名へ同期する。
p = Path("tis/INTEGRATION.md")
text = p.read_text().replace("BootReceiver", "EpgBootSyncReceiver").replace("DirectBootEpgPending", "DirectBootGuard").replace("BootEpgSyncCoordinator", "BootEpgSyncScheduler")
p.write_text(text)

# F12: host API surfaceをAndroid 15へ更新する。
sha = os.environ.get("ANDROID15_SHA256", "").strip()
if not re.fullmatch(r"[0-9a-f]{64}", sha):
    raise SystemExit("ANDROID15_SHA256が不正です")
p = Path(".github/workflows/tis-host-ci.yml")
text = p.read_text()
text = text.replace("固定KotlinとAndroid 14入力を準備", "固定KotlinとAndroid 15入力を準備")
text = text.replace("14-robolectric-10818077/android-all-14-robolectric-10818077.jar", "15-robolectric-13954326/android-all-15-robolectric-13954326.jar")
text = text.replace("6be2218c6a53fe3c57bc22ebdc723edcb7270a8a6f187545708aa5c0ed813977", sha)
p.write_text(text)

# core unit testsも通常ARIB CIに含める。
p = Path(".github/workflows/arib-si-engine-host-ci.yml")
text = p.read_text()
old = '      - name: ホスト互換単体テストを実行\n        run: cargo +"$RUST_TOOLCHAIN" test --locked\n'
new = '      - name: ホスト互換単体テストを実行\n        run: |\n          cargo +"$RUST_TOOLCHAIN" test --locked\n          cargo +"$RUST_TOOLCHAIN" test --manifest-path core/Cargo.toml\n'
if text.count(old) != 1:
    raise SystemExit("ARIB CI test stepが一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

# F11: Android 15 System TV App parental policy patchを生成する。
src = Path(os.environ["AOSP_PARENTAL_SOURCE"])
original = src.read_text()
modified = original
modified = modified.replace(
"    private final LegacyFlags mLegacyFlags;\n",
'''    private final LegacyFlags mLegacyFlags;
    private static final TvContentRating MALEICACID_ARIB_EXCEPTIONAL_RATING =
            TvContentRating.createRating(
                    "com.maleicacid.tv.ratings", "ARIB_EXCEPTIONAL", "BROADCASTER_DEFINED");
''', 1)
modified = modified.replace(
'''    public void setParentalControlsEnabled(boolean enabled) {
        mTvInputManager.setParentalControlsEnabled(enabled);
    }
''',
'''    public void setParentalControlsEnabled(boolean enabled) {
        mTvInputManager.setParentalControlsEnabled(enabled);
        if (mRatings == null) {
            loadRatings();
        } else {
            storeRatings();
        }
    }
''', 1)
modified = modified.replace(
'''    public void loadRatings() {
        mRatings = new HashSet<>(mTvInputManager.getBlockedRatings());
    }

    private void storeRatings() {
        Set<TvContentRating> removed = new HashSet<>(mTvInputManager.getBlockedRatings());''',
'''    public void loadRatings() {
        mRatings = new HashSet<>(mTvInputManager.getBlockedRatings());
        storeRatings();
    }

    private void storeRatings() {
        applyMaleicacidExceptionalRatingPolicy();
        Set<TvContentRating> removed = new HashSet<>(mTvInputManager.getBlockedRatings());''', 1)
anchor = "    private void updateRatingsForCurrentLevel(ContentRatingsManager manager) {"
helper = '''    private void applyMaleicacidExceptionalRatingPolicy() {
        boolean shouldBlock =
                mTvInputManager.isParentalControlsEnabled()
                        && getContentRatingLevel() != TvSettings.CONTENT_RATING_LEVEL_NONE;
        if (shouldBlock) {
            mRatings.add(MALEICACID_ARIB_EXCEPTIONAL_RATING);
        } else {
            mRatings.remove(MALEICACID_ARIB_EXCEPTIONAL_RATING);
        }
    }

'''
if anchor not in modified:
    raise SystemExit("System TV App patch anchorが見つかりません")
modified = modified.replace(anchor, helper + anchor, 1)
rel = "src/com/android/tv/parental/ParentalControlSettings.java"
diff = difflib.unified_diff(original.splitlines(True), modified.splitlines(True), fromfile=f"a/{rel}", tofile=f"b/{rel}")
out = Path("tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch")
out.write_text(f"diff --git a/{rel} b/{rel}\n" + "".join(diff))

p = Path("tis/INTEGRATION.md")
text = p.read_text()
anchor = "## MediaSync Exact-mode platform統合"
section = '''## System TV App exceptional rating policy統合

JPN parental rating raw `0x12..0xFF` はTISで `com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` として保持する。blocked-rating policyのownerはSystem TV Appなので、LineageOS 22.1 / Android 15 product treeの `packages/apps/TV` へ `tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch` を適用する。

```bash
cd packages/apps/TV
git apply --check "$TV_REPO/tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch"
git apply "$TV_REPO/tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch"
```

patchはparental controls有効かつglobal rating levelが`NONE`以外の場合だけ当該product固有ratingをblocked集合へ追加し、disabled/`NONE`では当該ratingだけを除去する。TISは`TvInputManager.isRatingBlocked()`をpolicy authorityとして使い続ける。製品統合ではpatch適用後のSystem TV App target compileとenable/disable、`NONE`/非`NONE`、PIN unblockを確認する。

'''
if anchor not in text:
    raise SystemExit("INTEGRATION MediaSync anchorがありません")
text = text.replace(anchor, section + anchor, 1)
p.write_text(text)

# F07: 具体的に指摘済みのTunerSelectionPolicy KDocも日本語化する。
p = Path("tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt")
text = p.read_text()
text = text.replace("/** Pure stream and track selection policy. Android Tuner resources remain owned by [TunerController]. */", "/** stream/track選択の純粋policy。Android Tuner資源は[TunerController]が所有する。 */")
text = text.replace("/** EIT component_descriptor facts that have a direct TvTrackInfo video representation. */", "/** EIT component_descriptorのうちTvTrackInfo videoへ直接表現できるfactだけを投影する。 */")
text = text.replace(" * ARIB audio_component_descriptor facts to Android live-track metadata.\n * PMT stream_type remains the codec authority. Descriptor facts are used only when valid and\n * directly representable by TvTrackInfo; reserved/ambiguous values are left unset.", " * ARIB audio_component_descriptorのfactをAndroid live-track metadataへ投影する。\n * codecの正本はPMT stream_typeとし、有効かつTvTrackInfoへ直接表現できるfactだけを使う。\n * reservedまたは曖昧な値は未設定のままにする。")
p.write_text(text)
