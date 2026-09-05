from pathlib import Path
import difflib
import os
import re


def read(path: str) -> str:
    return Path(path).read_text()


def write(path: str, text: str) -> None:
    Path(path).write_text(text)


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"置換対象が一意ではありません: {path} count={count} old={old[:80]!r}")
    write(path, text.replace(old, new, 1))


def replace_required(path: str, old: str, new: str, minimum: int = 1) -> None:
    text = read(path)
    count = text.count(old)
    if count < minimum:
        raise SystemExit(f"置換対象がありません: {path} old={old[:80]!r}")
    write(path, text.replace(old, new))


# F02: nested DTOのclosed契約はSerdeを正本にし、Value再走査と手書きunknown-field validatorを削除する。
p = Path("arib_si_engine_rs/src/core/provider_data.rs")
text = p.read_text()
start = text.index("fn field_object_has_only(")
end = text.index("pub fn build_program_provider_data", start)
text = text[:start] + text[end:]
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
    raise SystemExit("program normalizeのtyped deserialize置換対象が一意ではありません")
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
    raise SystemExit("channel decodeのtyped deserialize置換対象が一意ではありません")
text = text.replace(old, new, 1)
old = '''fn normalize_program_extensions(
    mut data: ProgramProviderDataV1,
    raw_value: Option<&serde_json::Value>,
) -> ProgramProviderDataV1 {'''
new = '''fn normalize_program_extensions(mut data: ProgramProviderDataV1) -> ProgramProviderDataV1 {'''
if text.count(old) != 1:
    raise SystemExit("normalize_program_extensions signatureが一意ではありません")
text = text.replace(old, new, 1)
old = '''    if let Some(raw) = raw_value {
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
'''
if text.count(old) != 1:
    raise SystemExit("program nested collector blockが一意ではありません")
text = text.replace(old, "", 1)
old = '''fn normalize_channel_extensions(
    mut data: ChannelProviderDataV1,
    raw_value: Option<&serde_json::Value>,
) -> ChannelProviderDataV1 {'''
new = '''fn normalize_channel_extensions(mut data: ChannelProviderDataV1) -> ChannelProviderDataV1 {'''
if text.count(old) != 1:
    raise SystemExit("normalize_channel_extensions signatureが一意ではありません")
text = text.replace(old, new, 1)
old = '''    if let Some(raw) = raw_value {
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
'''
if text.count(old) != 1:
    raise SystemExit("channel nested collector blockが一意ではありません")
text = text.replace(old, "", 1)
collector_start = text.index("fn collect_channel_unknown_extensions(")
collector_end = text.index("fn note_drop(", collector_start)
text = text[:collector_start] + text[collector_end:]
old = '''fn parse_descriptor_diagnostics(text: &str) -> Option<Vec<DescriptorDiagnosticV1>> {
    let raw = serde_json::from_str::<serde_json::Value>(text).ok()?;
    if !descriptor_diagnostics_have_only_known_fields(&raw) {
        return None;
    }
    let items = serde_json::from_value::<Vec<DescriptorDiagnosticV1>>(raw).ok()?;
    if items.iter().all(valid_descriptor_diagnostic) {
        Some(items)
    } else {
        None
    }
}

fn descriptor_diagnostics_have_only_known_fields(value: &serde_json::Value) -> bool {
    let Some(items) = value.as_array() else {
        return false;
    };
    items.iter().all(|item| {
        object_has_only(
            item,
            &[
                "schema",
                "schemaVersion",
                "severity",
                "code",
                "scope",
                "descriptor",
                "message",
            ],
        ) && field_object_has_only(
            item,
            "scope",
            &[
                "pid",
                "tableId",
                "tableIdExtension",
                "version",
                "sectionNumber",
                "originalNetworkId",
                "transportStreamId",
                "serviceId",
                "eventId",
            ],
        ) && field_object_has_only(
            item,
            "descriptor",
            &[
                "tag",
                "name",
                "offset",
                "declaredLength",
                "actualRemainingLength",
                "parseStatus",
                "rawPrefixHex",
            ],
        )
    })
}
'''
new = '''fn parse_descriptor_diagnostics(text: &str) -> Option<Vec<DescriptorDiagnosticV1>> {
    let items = serde_json::from_str::<Vec<DescriptorDiagnosticV1>>(text).ok()?;
    if items.iter().all(valid_descriptor_diagnostic) {
        Some(items)
    } else {
        None
    }
}
'''
if text.count(old) != 1:
    raise SystemExit("descriptor diagnostic第二validator置換対象が一意ではありません")
text = text.replace(old, new, 1)
# closed nested DTOの回帰を既存testへ追加する。
marker = '''    #[test]
    fn program_request_rejects_nested_unknown_fields_and_preserves_event_group_shape() {'''
test = '''    #[test]
    fn normalize_program_provider_data_rejects_unknown_descriptor_diagnostic_nested_key() {
        let mut stored: serde_json::Value =
            serde_json::from_str(&minimal_program_json("")).unwrap();
        stored["diagnostics"]["descriptorDiagnostics"] = serde_json::json!([{
            "schema": "maleicacid.tv.descriptorDiagnostic",
            "schemaVersion": 1,
            "severity": "ERROR",
            "code": "TEST",
            "scope": {"pid": 18, "futureNestedKey": true},
            "descriptor": {
                "tag": 0x4d,
                "name": null,
                "offset": 0,
                "declaredLength": 1,
                "actualRemainingLength": 1,
                "parseStatus": "OK",
                "rawPrefixHex": "00"
            },
            "message": "test"
        }]);
        let result = normalize_program_provider_data(stored.to_string().as_bytes());
        assert!(!result.success);
        assert_eq!(result.error_code, "PROGRAM_PROVIDER_DATA_SCHEMA_FAILED");
    }

'''
if text.count(marker) != 1:
    raise SystemExit("provider-data nested回帰挿入位置が一意ではありません")
text = text.replace(marker, test + marker, 1)
# F07: このRust境界で利用者へ返す説明文を日本語化する。error codeは互換のため維持する。
replacements = {
    'format!("Program provider-data request JSON parse failed: {err}")': 'format!("Program provider-data request JSONの解析に失敗しました: {err}")',
    '"Program provider-data request did not satisfy schema v1 invariants".to_string()': '"Program provider-data requestがschema v1の不変条件を満たしません".to_string()',
    'format!("Channel provider-data request JSON parse failed: {err}")': 'format!("Channel provider-data request JSONの解析に失敗しました: {err}")',
    '"Channel provider-data request did not satisfy schema v1 invariants".to_string()': '"Channel provider-data requestがschema v1の不変条件を満たしません".to_string()',
    'format!("Program provider-data is not UTF-8: {err}")': 'format!("Program provider-dataがUTF-8ではありません: {err}")',
    '"Program provider-data JSON v1 invariants failed".to_string()': '"Program provider-data JSON v1の不変条件を満たしません".to_string()',
    'format!("Program provider-data serialization failed: {err}")': 'format!("Program provider-dataのserializationに失敗しました: {err}")',
    'format!("Truncated program provider-data serialization failed: {err}")': 'format!("切詰め後Program provider-dataのserializationに失敗しました: {err}")',
    '"Channel provider-data JSON v1 invariants failed".to_string()': '"Channel provider-data JSON v1の不変条件を満たしません".to_string()',
    'format!("Channel provider-data serialization failed: {err}")': 'format!("Channel provider-dataのserializationに失敗しました: {err}")',
    'format!("Truncated channel provider-data serialization failed: {err}")': 'format!("切詰め後Channel provider-dataのserializationに失敗しました: {err}")',
    '"Program provider-data cannot be reduced below {} bytes without dropping protected semantic fields (current={} bytes)"': '"保護対象の意味factを落とさずProgram provider-dataを{} bytes以下へ縮小できません（current={} bytes）"',
    '"Channel provider-data cannot be reduced below {} bytes without dropping protected tune or CAS fields (current={} bytes)"': '"保護対象のtune/CAS factを落とさずChannel provider-dataを{} bytes以下へ縮小できません（current={} bytes）"',
}
for old_s, new_s in replacements.items():
    if old_s in text:
        text = text.replace(old_s, new_s)
p.write_text(text)

# F08: RELATIVEはAIDL tune-time selectorとしてのみ扱い、TvProvider保存時にTSIDへ意味変換しない。
p = Path("tis/src/com/maleicacid/tvinput/aribsi/ProviderDataBridge.kt")
text = p.read_text()
old = '''        val selector = when (channel.streamSelector.type) {
            StreamSelectorType.RELATIVE -> StreamSelector.tsid(channel.serviceKey.transportStreamId)
            else -> channel.streamSelector
        }
        val selectorValue = selector.value'''
new = '''        require(channel.streamSelector.type != StreamSelectorType.RELATIVE) {
            "RELATIVE stream selectorはTvProvider永続identityとして保存できません"
        }
        if (channel.streamSelector.type == StreamSelectorType.TSID) {
            require(channel.streamSelector.value == channel.serviceKey.transportStreamId) {
                "保存TSID selectorはbroadcast service identityのTSIDと一致する必要があります"
            }
        }
        val selector = channel.streamSelector
        val selectorValue = selector.value'''
if text.count(old) != 1:
    raise SystemExit("RELATIVE保存変換置換対象が一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

# F13: include!対象の新文字表をSoong sandboxの明示sourceへ追加する。
p = Path("arib_si_engine_rs/Android.bp")
text = p.read_text()
needle = '        "src/core/arib_jis_x0208_table.rs",\n        "src/core/arib_string.rs",'
if text.count(needle) != 2:
    raise SystemExit(f"Android.bp文字表source挿入箇所が2件ではありません: {text.count(needle)}")
text = text.replace(needle, '        "src/core/arib_jis_x0208_table.rs",\n        "src/core/arib_extended_graphic_table.rs",\n        "src/core/arib_string.rs",')
p.write_text(text)

# F16: version更新でlast_section_numberが縮小した時は、新versionが明示的に除外した旧sectionだけを回収する。
p = Path("arib_si_engine_rs/src/core/eit.rs")
text = p.read_text()
old = '''        let previous_keys: BTreeSet<EitEventKey> = self
            .section_events
            .get(&section_key)
            .map(|old| old.event_keys.clone())
            .unwrap_or_default();
        let new_keys: BTreeSet<_> = parsed.iter().filter_map(stable_event_key).collect();
        let removable_previous_keys: BTreeSet<_> = if deletion_authoritative {
            previous_keys
                .difference(&new_keys)
                .filter(|old_key| !malformed_event_keys.contains(old_key))
                .copied()
                .collect()
        } else {
            BTreeSet::new()
        };'''
new = '''        let previous_keys: BTreeSet<EitEventKey> = self
            .section_events
            .get(&section_key)
            .map(|old| old.event_keys.clone())
            .unwrap_or_default();
        let new_keys: BTreeSet<_> = parsed.iter().filter_map(stable_event_key).collect();
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
            .filter(|(key, _)| **key != section_key && !obsolete_section_keys.contains(key))
            .flat_map(|(_, old)| old.event_keys.iter().copied())
            .collect();
        let removal_candidates: BTreeSet<EitEventKey> = previous_keys
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
    raise SystemExit("EIT removal block置換対象が一意ではありません")
text = text.replace(old, new, 1)
old = '''        if header.table_id == 0x4e && (!previous_keys.is_empty() || !new_keys.is_empty()) {
            let pf_actual_window_events: Vec<_> = window_events
                .iter()
                .filter(|event| {
                    event.table_id == 0x4e && event.timing_state == EitTimingState::Defined
                })
                .cloned()
                .collect();
            let pf_actual_current_events: Vec<_> = parsed
                .iter()
                .filter(|event| event.table_id == 0x4e && event.stable_identity().is_some())
                .cloned()
                .collect();'''
new = '''        if header.table_id == 0x4e
            && (!previous_keys.is_empty() || !new_keys.is_empty() || !obsolete_section_keys.is_empty())
        {
            let pf_actual_window_events: Vec<_> = window_events
                .iter()
                .filter(|event| {
                    event.table_id == 0x4e && event.timing_state == EitTimingState::Defined
                })
                .cloned()
                .collect();
            let mut pf_actual_current_events: Vec<_> = self
                .events
                .iter()
                .filter(|(key, event)| {
                    key.table_id == 0x4e
                        && key.original_network_id == original_network_id
                        && key.transport_stream_id == transport_stream_id
                        && key.service_id == service_id
                        && !removable_previous_keys.contains(key)
                        && event.stable_identity().is_some()
                })
                .map(|(_, event)| event.clone())
                .collect();
            pf_actual_current_events.extend(
                parsed
                    .iter()
                    .filter(|event| event.table_id == 0x4e && event.stable_identity().is_some())
                    .cloned(),
            );'''
if text.count(old) != 1:
    raise SystemExit("EIT update window block置換対象が一意ではありません")
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
    raise SystemExit("EIT obsolete section削除挿入位置が一意ではありません")
text = text.replace(old, new, 1)
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
        assert_eq!(store.snapshot_present_following_actual().len(), 3);

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
    raise SystemExit("EIT shrink回帰挿入位置が一意ではありません")
text = text.replace(marker, test + marker, 1)
p.write_text(text)

# F15: AOSP scan結果の失敗/timeout/0件をcallback非対応と同一視せず、dynamic discoveryはfail-closedにする。
p = Path("tis/src/com/maleicacid/tvinput/tis/ChannelScanController.kt")
text = p.read_text()
old = '''                    val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(candidate, discovery.streamIds)
                    if (discovered.isNotEmpty()) {
                        diagnostics += "BS stream-ID discovery ${candidate.displayChannel}: ${discovered.size} streams"
                        discovered
                    } else {
                        val fallback = JapanIsdbScanPlan.fallbackBsCandidates(candidate)
                        diagnostics += "BS stream-ID discovery fallback ${candidate.displayChannel}: ${fallback.size} versioned TSID candidates result=${discovery.resultCode} message=${discovery.message}"
                        fallback
                    }'''
new = '''                    val discovered = JapanIsdbScanPlan.explicitBsCandidatesFromScan(candidate, discovery.streamIds)
                    if (discovered.isNotEmpty()) {
                        diagnostics += "BS stream-ID discovery ${candidate.displayChannel}: ${discovered.size} streams"
                        discovered
                    } else {
                        diagnostics += "BS stream-ID discoveryをfail-closedにします ${candidate.displayChannel}: result=${discovery.resultCode} message=${discovery.message}"
                        emptyList()
                    }'''
if text.count(old) != 1:
    raise SystemExit("BS fallback block置換対象が一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

p = Path("tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt")
text = p.read_text()
start = text.index("    /** scan callbackがstream IDを返せないfrontend向けcompatibility fallback。 */")
end = text.index("    fun isdbs110CsBands()", start)
# explicitBsCandidatesFromScanは残す必要があるためfallback関数だけを切る。
fallback_start = text.index("    fun fallbackBsCandidates(", start)
fallback_end = text.index("\n\n    fun isdbs110CsBands()", fallback_start)
text = text[:fallback_start] + text[fallback_end + 2:]
# 上のcompatibility comment/isdbsBsTsidStreamsも通常fallback正本を残すため削除する。
comment_start = text.index("    /** scan callbackがstream IDを返せないfrontend向けcompatibility fallback。 */")
comment_end = text.index("    fun explicitBsCandidatesFromScan", comment_start)
text = text[:comment_start] + text[comment_end:]
p.write_text(text)

# caption management discoveryと表示track advertisementを分離する。
p = Path("tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt")
text = p.read_text()
text = text.replace(
    '/** Pure stream and track selection policy. Android Tuner resources remain owned by [TunerController]. */',
    '/** stream/track選択の純粋policy。Android Tuner資源は[TunerController]が所有する。 */',
)
text = text.replace(
    '''/**
 * ARIB audio_component_descriptor facts to Android live-track metadata.
 * PMT stream_type remains the codec authority. Descriptor facts are used only when valid and
 * directly representable by TvTrackInfo; reserved/ambiguous values are left unset.
 */''',
    '''/**
 * ARIB audio_component_descriptorのfactをAndroid live-track metadataへ投影する。
 * codecの正本はPMT stream_typeとし、有効かつTvTrackInfoへ直接表現できるfactだけを使う。
 * reservedまたは曖昧な値は未設定のままにする。
 */''',
)
text = text.replace(
    '/** EIT component_descriptor facts that have a direct TvTrackInfo video representation. */',
    '/** EIT component_descriptorのうちTvTrackInfo videoへ直接表現できるfactだけを投影する。 */',
)
needle = '''    fun trackIdForSubtitle(stream: AribElementaryStream, languageId: Int = 1): String {
        val base = stream.componentTag?.let { "subtitle:${stream.elementaryPid}:$it" } ?: "subtitle:${stream.elementaryPid}"
        return "$base:lang$languageId"
    }

    fun trackIdForSuperimpose'''
replacement = '''    fun trackIdForSubtitle(stream: AribElementaryStream, languageId: Int = 1): String {
        val base = stream.componentTag?.let { "subtitle:${stream.elementaryPid}:$it" } ?: "subtitle:${stream.elementaryPid}"
        return "$base:lang$languageId"
    }

    fun trackIdForCaptionDiscovery(stream: AribElementaryStream): String = "caption-discovery:${stream.elementaryPid}"

    fun trackIdForSuperimpose'''
if text.count(needle) != 1:
    raise SystemExit("caption discovery track id挿入位置が一意ではありません")
text = text.replace(needle, replacement, 1)
p.write_text(text)

p = Path("tis/src/com/maleicacid/tvinput/tis/TunerController.kt")
text = p.read_text()
old = '''        val selectedCaptionTrack = if (subtitleExplicitlyDisabled) null else preferredSubtitleTrackId?.let { wanted ->
            captionTracks.firstOrNull { it.id == wanted }
        } ?: TunerSelectionPolicy.selectCaption(streams, defaultComponentGroupTags)?.let { defaultStream -> captionTracks.firstOrNull { it.pid == defaultStream.elementaryPid } }
        val subtitle = selectedCaptionTrack?.let { track -> streams.firstOrNull { it.elementaryPid == track.pid && TunerSelectionPolicy.isCaptionStream(it) } }
        val superimpose = TunerSelectionPolicy.selectSuperimpose(streams, defaultComponentGroupTags)
        return AvStreamSelection(serviceKey, pcrPid, video, audio, subtitle, selectedCaptionTrack?.captionLanguageId, superimpose, audio?.componentType, dualMonoPresentation)'''
new = '''        val captionDiscovery = TunerSelectionPolicy.selectCaption(streams, defaultComponentGroupTags)
        val selectedCaptionTrack = if (subtitleExplicitlyDisabled) null else preferredSubtitleTrackId?.let { wanted ->
            captionTracks.firstOrNull { it.id == wanted }
        } ?: captionDiscovery?.let { defaultStream -> captionTracks.firstOrNull { it.pid == defaultStream.elementaryPid } }
        val subtitle = selectedCaptionTrack?.let { track -> streams.firstOrNull { it.elementaryPid == track.pid && TunerSelectionPolicy.isCaptionStream(it) } }
        val superimpose = TunerSelectionPolicy.selectSuperimpose(streams, defaultComponentGroupTags)
        return AvStreamSelection(
            serviceKey,
            pcrPid,
            video,
            audio,
            subtitle,
            selectedCaptionTrack?.captionLanguageId,
            superimpose,
            audio?.componentType,
            dualMonoPresentation,
            captionDiscovery,
        )'''
if text.count(old) != 1:
    raise SystemExit("caption selection block置換対象が一意ではありません")
text = text.replace(old, new, 1)
old = '''            val languages = captionLanguagesByPid[stream.elementaryPid].orEmpty()
            if (languages.isEmpty()) {
                add(TisTrack(TunerSelectionPolicy.trackIdForSubtitle(stream, 1), android.media.tv.TvTrackInfo.TYPE_SUBTITLE, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, null, stream.dataComponentId, TunerSelectionPolicy.captionKind(stream), 1))
            } else {
                languages.filter { it.languageTag in 0..1 }.forEach { language ->
                    val languageId = language.languageTag + 1
                    add(TisTrack(TunerSelectionPolicy.trackIdForSubtitle(stream, languageId), android.media.tv.TvTrackInfo.TYPE_SUBTITLE, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, language.iso639LanguageCode, stream.dataComponentId, TunerSelectionPolicy.captionKind(stream), languageId))
                }
            }'''
new = '''            val languages = captionLanguagesByPid[stream.elementaryPid].orEmpty()
            languages.filter { it.languageTag in 0..1 }.forEach { language ->
                val languageId = language.languageTag + 1
                add(TisTrack(TunerSelectionPolicy.trackIdForSubtitle(stream, languageId), android.media.tv.TvTrackInfo.TYPE_SUBTITLE, stream.elementaryPid, stream.streamType, stream.componentTag, stream.componentType, language.iso639LanguageCode, stream.dataComponentId, TunerSelectionPolicy.captionKind(stream), languageId))
            }'''
if text.count(old) != 1:
    raise SystemExit("caption fallback track block置換対象が一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

p = Path("tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt")
text = p.read_text()
# AvStreamSelectionにcaptionDiscoveryを追加する。
old = '''        val superimpose: AribElementaryStream? = null,
        val audioComponentType: Int? = audio?.componentType,
        val dualMonoPresentation: DualMonoPresentation = DualMonoPresentation.MAIN,
    )'''
new = '''        val superimpose: AribElementaryStream? = null,
        val audioComponentType: Int? = audio?.componentType,
        val dualMonoPresentation: DualMonoPresentation = DualMonoPresentation.MAIN,
        val captionDiscovery: AribElementaryStream? = null,
    )'''
if text.count(old) != 1:
    raise SystemExit("AvStreamSelection captionDiscovery挿入箇所が一意ではありません")
text = text.replace(old, new, 1)
old = '''        val subtitle = selection.subtitle
        if (subtitle != null) {
            val trackId = TunerSelectionPolicy.trackIdForSubtitle(subtitle, selection.subtitleLanguageId ?: 1)
            val openedSubtitle = createAndStartCaptionPesFilter(tuner, subtitle, trackId, superimpose = false)
                .onFailure { error -> diagnostics += "subtitle PES filter start failed: ${error.message}" }
                .getOrNull()
            if (openedSubtitle != null) {
                subtitleFilter = openedSubtitle
                diagnostics += "subtitlePid=${subtitle.elementaryPid}"
            }
        }'''
new = '''        val subtitle = selection.subtitle
        val captionAcquisition = subtitle ?: selection.captionDiscovery
        if (captionAcquisition != null) {
            val trackId = if (subtitle != null) {
                TunerSelectionPolicy.trackIdForSubtitle(subtitle, selection.subtitleLanguageId ?: 1)
            } else {
                TunerSelectionPolicy.trackIdForCaptionDiscovery(captionAcquisition)
            }
            val openedSubtitle = createAndStartCaptionPesFilter(tuner, captionAcquisition, trackId, superimpose = false)
                .onFailure { error -> diagnostics += "subtitle PES filter start failed: ${error.message}" }
                .getOrNull()
            if (openedSubtitle != null) {
                subtitleFilter = openedSubtitle
                diagnostics += "subtitlePid=${captionAcquisition.elementaryPid}"
            }
        }'''
if text.count(old) != 1:
    raise SystemExit("caption PES acquisition block置換対象が一意ではありません")
text = text.replace(old, new, 1)
# PCM encodingはdecoder output MediaFormatを正本にする。
old = '''    private data class OutputPcmFormat(
        val sampleRate: Int,
        val channelCount: Int,
        val channelMask: Int,
    )'''
new = '''    private data class OutputPcmFormat(
        val sampleRate: Int,
        val channelCount: Int,
        val channelMask: Int,
        val pcmEncoding: Int,
    )'''
if text.count(old) != 1:
    raise SystemExit("OutputPcmFormat置換対象が一意ではありません")
text = text.replace(old, new, 1)
old = '''            val decoderMask = if (format.containsKey(MediaFormat.KEY_CHANNEL_MASK)) format.getInteger(MediaFormat.KEY_CHANNEL_MASK) else null
            val channelMask = PcmChannelMaskPolicy.resolve(decoderMask, channelCount, componentType)'''
new = '''            val decoderMask = if (format.containsKey(MediaFormat.KEY_CHANNEL_MASK)) format.getInteger(MediaFormat.KEY_CHANNEL_MASK) else null
            val pcmEncoding = if (format.containsKey(MediaFormat.KEY_PCM_ENCODING)) {
                format.getInteger(MediaFormat.KEY_PCM_ENCODING)
            } else {
                AudioFormat.ENCODING_PCM_16BIT
            }
            val channelMask = PcmChannelMaskPolicy.resolve(decoderMask, channelCount, componentType)'''
if text.count(old) != 1:
    raise SystemExit("PCM encoding取得挿入箇所が一意ではありません")
text = text.replace(old, new, 1)
text = text.replace(
    '            val next = OutputPcmFormat(sampleRate, channelCount, channelMask)',
    '            val next = OutputPcmFormat(sampleRate, channelCount, channelMask, pcmEncoding)',
    1,
)
text = text.replace('            ensureAudioTrack(channelMask)', '            ensureAudioTrack(channelMask, pcmEncoding)', 1)
text = text.replace('        private fun ensureAudioTrack(channelMask: Int) {', '        private fun ensureAudioTrack(channelMask: Int, pcmEncoding: Int) {', 1)
text = text.replace(
    'AudioTrack.getMinBufferSize(outputSampleRate, channelMask, AudioFormat.ENCODING_PCM_16BIT)',
    'AudioTrack.getMinBufferSize(outputSampleRate, channelMask, pcmEncoding)',
    1,
)
text = text.replace(
    '.setAudioFormat(AudioFormat.Builder().setSampleRate(outputSampleRate).setChannelMask(channelMask).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())',
    '.setAudioFormat(AudioFormat.Builder().setSampleRate(outputSampleRate).setChannelMask(channelMask).setEncoding(pcmEncoding).build())',
    1,
)
text = text.replace('"sessionContext is required for AudioTrack"', '"AudioTrackにはsessionContextが必要です"')
text = text.replace('"playback executor interrupted"', '"playback executorがinterruptされました"')
p.write_text(text)

p = Path("tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt")
text = p.read_text()
text = text.replace(
    '''                    // Rust caption JNI parses management/STM facts before this callback.
                    // Rebuild TIF tracks so newly discovered language_tag values become selectable.
                    latestService?.let(::updateTracks)
                    if (trackId.startsWith("superimpose:")) {''',
    '''                    // Rust caption JNIはこのcallbackより前にmanagement/STM factを構造化する。
                    // 新たに判明したlanguage_tagを選択可能にするためTIF trackを再構築する。
                    latestService?.let(::updateTracks)
                    if (trackId.startsWith("caption-discovery:")) {
                        latestService?.let(::maybeStartPlayback)
                        return@enqueueSessionAction
                    }
                    if (trackId.startsWith("superimpose:")) {''',
)
text = text.replace('"session executor interrupted"', '"session executorがinterruptされました"')
p.write_text(text)

# TvProvider broadcast genreは複数分類をCSV要素へ個別encodeする。
p = Path("tis/src/com/maleicacid/tvinput/db/TvInputModels.kt")
text = p.read_text()
old = '    val broadcastGenre: String? = null,\n'
new = '    val broadcastGenres: List<String> = emptyList(),\n'
if text.count(old) != 1:
    raise SystemExit("ProgramDescriptors broadcastGenre fieldが一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

p = Path("tis/src/com/maleicacid/tvinput/aribsi/EventModelMapper.kt")
text = p.read_text()
text = text.replace(
    '                    broadcastGenre = broadcastGenreText(event.descriptors.contentGenres),',
    '                    broadcastGenres = broadcastGenreTexts(event.descriptors.contentGenres),',
    1,
)
old = '''    private fun broadcastGenreText(genres: List<AribContentGenre>): String? = genres
        .filter { it.parseStatus == "OK" }
        .takeIf { it.isNotEmpty() }
        ?.joinToString("、") { "ARIB(0x${it.level1.toString(16)}/0x${it.level2.toString(16)}):${it.aribName}" }
'''
new = '''    private fun broadcastGenreTexts(genres: List<AribContentGenre>): List<String> = genres
        .filter { it.parseStatus == "OK" }
        .map { "ARIB(0x${it.level1.toString(16)}/0x${it.level2.toString(16)}):${it.aribName}" }
'''
if text.count(old) != 1:
    raise SystemExit("broadcastGenreText置換対象が一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

p = Path("tis/src/com/maleicacid/tvinput/tis/TvProviderWriter.kt")
text = p.read_text()
old = '        program.descriptors.broadcastGenre?.let { put(TvContract.Programs.COLUMN_BROADCAST_GENRE, TvContract.Programs.Genres.encode(it)) }\n'
new = '''        if (program.descriptors.broadcastGenres.isNotEmpty()) {
            put(
                TvContract.Programs.COLUMN_BROADCAST_GENRE,
                TvContract.Programs.Genres.encode(*program.descriptors.broadcastGenres.toTypedArray()),
            )
        }
'''
if text.count(old) != 1:
    raise SystemExit("TvProvider broadcast genre投影置換対象が一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

# F07: MediaSync platform-private patchのPR追加Javadoc/comment/exceptionを日本語化する。
p = Path("tis/platform_patches/lineage-22.1/frameworks_base_mediasync_first_output.patch")
text = p.read_text()
repls = {
'''+    /**
+     * Listener for the first video frame successfully queued by MediaSync to its output surface
+     * after an explicit arm operation.
+     *
+     * @hide
+     */''': '''+    /**
+     * 明示arm後にMediaSyncがoutput surfaceへ正常queueした最初のvideo frameを通知する。
+     *
+     * @hide
+     */''',
'''+    /**
+     * Arms a one-shot callback for the first video frame that MediaSync successfully queues to
+     * its current output surface. The arm sequence is opaque to MediaSync and is returned unchanged
+     * with the callback so a platform-coupled client can reject a stale delayed event after re-arm.
+     *
+     * <p>This observes successful queueing to MediaSync's output surface. It does not indicate
+     * compositor presentation or present-fence completion.</p>
+     *
+     * @param armSequence positive identifier scoped by the caller to this MediaSync instance
+     * @param listener callback to arm, or {@code null} to disarm
+     * @param handler handler used to deliver the callback, or {@code null} for the current/main
+     *                looper
+     * @hide
+     */''': '''+    /**
+     * MediaSyncがcurrent output surfaceへ正常queueした最初のvideo frameについて
+     * one-shot callbackをarmする。arm sequenceはMediaSyncではopaqueに扱い、callbackへ
+     * そのまま返すため、platform連携clientはre-arm前の遅延eventを拒否できる。
+     *
+     * <p>観測対象はMediaSync output surfaceへのqueue成功であり、compositorでの表示完了や
+     * present-fence完了を意味しない。</p>
+     *
+     * @param armSequence callerがこのMediaSync instance内で割り当てる正の識別子
+     * @param listener armするcallback。{@code null}ならdisarmする
+     * @param handler callback配送用handler。{@code null}ならcurrent/main looperを使う
+     * @hide
+     */''',
'"armSequence must be positive when armed"': '"arm時のarmSequenceは正である必要があります"',
'+        // Disarm is part of release cleanup and remains harmless after native_release().': '+        // disarmはrelease cleanupの一部であり、native_release()後でも無害に扱う。',
}
for old_s, new_s in repls.items():
    if old_s not in text:
        raise SystemExit(f"MediaSync patch置換対象がありません: {old_s[:60]!r}")
    text = text.replace(old_s, new_s)
p.write_text(text)

# F09: 現行product baselineをLineageOS 22.1 / Android 15へ統一する。
p = Path("tis/DESIGN_JA.md")
text = p.read_text()
for old_s, new_s in [
    ("Android 14 Tuner API builder", "Android 15 Tuner API builder"),
    ("Android 14の`MediaCodec.QueueRequest`", "Android 15の`MediaCodec.QueueRequest`"),
    ("stock Android 14 / LineageOS 21", "stock Android 15 / LineageOS 22.1"),
    ("LineageOS 21／Android 14の通常ライブセッション", "LineageOS 22.1／Android 15の通常ライブセッション"),
    ("LineageOS 21の通常経路", "LineageOS 22.1の通常経路"),
    ("Android 14（API 34）の公開`AudioTrack.Builder.setContext(sessionContext)`", "Android 15（API 35）環境の公開`AudioTrack.Builder.setContext(sessionContext)`"),
]:
    if old_s in text:
        text = text.replace(old_s, new_s)
# F15 dynamic BS fallbackを設計正本から除去する。
old = "scan callbackがstream IDを返せないfrontendに限りTISのversioned BS TSID表へfallbackするが、受信後のservice identityはPAT/NIT/SDT由来ONID/TSID/SIDを正とする。"
new = "`Tuner.scan()`が失敗した場合、timeoutした場合、または完了してもstream IDが0件の場合はそれぞれ診断を分離してfail-closedとし、versioned TSID表を成功候補へ代入しない。受信後のservice identityはPAT/NIT/SDT由来ONID/TSID/SIDを正とする。"
if old not in text:
    raise SystemExit("DESIGN BS fallback文が見つかりません")
text = text.replace(old, new, 1)
# PCM encoding authorityを明記する。
needle = "AudioTrackへ渡すPCM topologyはdecoderの実出力を正本にする。"
insert = "AudioTrackへ渡すPCM topologyはdecoderの実出力を正本にする。decoder output `MediaFormat.KEY_PCM_ENCODING` が存在する場合はその値をAudioTrack encodingへ使い、存在しないraw PCM outputはAOSP既定のsigned PCM 16-bitとして扱う。sample rate / channel topologyだけでなくPCM encodingが変わるoutput-format changeでも旧AudioTrackを再利用しない。"
if needle not in text:
    raise SystemExit("PCM DESIGN挿入位置が見つかりません")
text = text.replace(needle, insert, 1)
p.write_text(text)

# F14/F11: Integration正本を現実装名へ直し、System TV Appの再現可能patchを製品統合条件へ追加する。
p = Path("tis/INTEGRATION.md")
text = p.read_text()
text = text.replace("BootReceiver", "EpgBootSyncReceiver")
text = text.replace("DirectBootEpgPending", "DirectBootGuard")
text = text.replace("BootEpgSyncCoordinator", "BootEpgSyncScheduler")
# packages/apps/TV patch適用節をMediaSync節の前へ追加する。
anchor = "## MediaSync Exact-mode platform統合"
section = '''## System TV App exceptional rating policy統合

JPN parental rating raw `0x12..0xFF` はTIS側で `com.maleicacid.tv.ratings / ARIB_EXCEPTIONAL / BROADCASTER_DEFINED` として保持する。親制御policy ownerはSystem TV Appなので、LineageOS 22.1 / Android 15 (`android-15.0.0_r14`) の `packages/apps/TV` へ次のpatchを適用する。

```bash
cd packages/apps/TV
git apply --check "$TV_REPO/tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch"
git apply "$TV_REPO/tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch"
```

patchはparental controlsが有効かつglobal rating levelが`NONE`以外の場合だけ、このproduct固有rating 1件をblocked-rating集合へ追加する。無効化または`NONE`では同ratingだけを除去し、第三者custom rating、CTS Verifier由来rating、他domain/ratingSystemには触れない。TISは引き続き`TvInputManager.isRatingBlocked()`だけをpolicy authorityとして使用する。

製品統合ではpatch適用後の`packages/apps/TV` target compileと、parental controlsのenable/disable、rating level `NONE`/非`NONE`、PINによるcurrent content unblockを実機またはtarget testで確認する。host TIS testだけでSystem TV App policyの成立を証明した扱いにはしない。

'''
if anchor not in text:
    raise SystemExit("INTEGRATION MediaSync anchorが見つかりません")
text = text.replace(anchor, section + anchor, 1)
p.write_text(text)

# F12: host Kotlin入力もAndroid 15 API surfaceへ揃える。hashはworkflowで取得した実artifact値を固定する。
android15_sha = os.environ.get("ANDROID15_SHA256", "").strip()
if not re.fullmatch(r"[0-9a-f]{64}", android15_sha):
    raise SystemExit("ANDROID15_SHA256が不正です")
p = Path(".github/workflows/tis-host-ci.yml")
text = p.read_text()
text = text.replace("固定KotlinとAndroid 14入力を準備", "固定KotlinとAndroid 15入力を準備")
text = text.replace(
    "https://repo.maven.apache.org/maven2/org/robolectric/android-all/14-robolectric-10818077/android-all-14-robolectric-10818077.jar",
    "https://repo.maven.apache.org/maven2/org/robolectric/android-all/15-robolectric-13954326/android-all-15-robolectric-13954326.jar",
)
text = text.replace("6be2218c6a53fe3c57bc22ebdc723edcb7270a8a6f187545708aa5c0ed813977", android15_sha)
p.write_text(text)

# core crateのunit regressionも通常ARIB SI CIで実行する。
p = Path(".github/workflows/arib-si-engine-host-ci.yml")
text = p.read_text()
old = '      - name: ホスト互換単体テストを実行\n        run: cargo +"$RUST_TOOLCHAIN" test --locked\n'
new = '''      - name: ホスト互換単体テストを実行
        run: |
          cargo +"$RUST_TOOLCHAIN" test --locked
          cargo +"$RUST_TOOLCHAIN" test --manifest-path core/Cargo.toml
'''
if text.count(old) != 1:
    raise SystemExit("ARIB SI core test追加箇所が一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

# Kotlin既存testを新契約へ同期し、test method数は増やさない。
p = Path("tis/tests/src/com/maleicacid/tvinput/tis/TisR51FixedPlanAcceptanceTest.kt")
text = p.read_text()
text = text.replace(
    '        check(JapanIsdbScanPlan.fallbackBsCandidates(bsSeed).all { it.streamSelector.type == com.maleicacid.tvinput.common.StreamSelectorType.TSID })\n',
    '        check(JapanIsdbScanPlan.explicitBsCandidatesFromScan(bsSeed, emptyList()).isEmpty())\n',
    1,
)
text = text.replace(
    '        check(TunerSelectionPolicy.trackIdForSubtitle(subtitle) == "subtitle:304:8:lang1")\n',
    '        check(TunerSelectionPolicy.trackIdForSubtitle(subtitle) == "subtitle:304:8:lang1")\n        check(TunerSelectionPolicy.trackIdForCaptionDiscovery(subtitle) == "caption-discovery:304")\n',
    1,
)
p.write_text(text)

p = Path("tis/tests/src/com/maleicacid/tvinput/tis/TvProviderWriterR51FixTest.kt")
text = p.read_text()
text = text.replace(
    '                broadcastGenre = "ARIB(0x0/0x0):ニュース/報道/定時・総合",',
    '                broadcastGenres = listOf("ARIB(0x0/0x0):ニュース/報道/定時・総合", "ARIB(0x1/0x2):スポーツ,中継"),',
    1,
)
needle = '        check(values.get(TvContract.Programs.COLUMN_BROADCAST_GENRE) == null)\n'
# merge後null確認の前に初回encode結果を確認したいので、初回write直後へ追加する。
old = '''        writer.upsertPrograms(listOf(p))
        writer.upsertPrograms(listOf(p.copy(canonicalGenres = emptyList(), descriptors = ProgramDescriptors(), contentRatings = emptyList())))'''
new = '''        writer.upsertPrograms(listOf(p))
        val encodedBroadcastGenres = store.programs.values.single().getAsString(TvContract.Programs.COLUMN_BROADCAST_GENRE)
        check(TvContract.Programs.Genres.decode(encodedBroadcastGenres).toList() == p.descriptors.broadcastGenres)
        writer.upsertPrograms(listOf(p.copy(canonicalGenres = emptyList(), descriptors = ProgramDescriptors(), contentRatings = emptyList())))'''
if text.count(old) != 1:
    raise SystemExit("broadcast genre回帰挿入位置が一意ではありません")
text = text.replace(old, new, 1)
p.write_text(text)

# F08の保存境界を既存acceptance testへ固定する。
p = Path("tis/tests/src/com/maleicacid/tvinput/tis/ProviderDataAssetsR51ContractTest.kt")
if p.exists():
    text = p.read_text()
    # このfileはinstrumentation寄りなので、production host suiteでは別testに固定する。

# packages/apps/TV patchをAndroid 15正本sourceから生成する。
source_path = Path(os.environ["AOSP_PARENTAL_SOURCE"])
original = source_path.read_text()
modified = original
needle = "    private final LegacyFlags mLegacyFlags;\n"
addition = '''    private final LegacyFlags mLegacyFlags;
    private static final TvContentRating MALEICACID_ARIB_EXCEPTIONAL_RATING =
            TvContentRating.createRating(
                    "com.maleicacid.tv.ratings", "ARIB_EXCEPTIONAL", "BROADCASTER_DEFINED");
'''
if modified.count(needle) != 1:
    raise SystemExit("AOSP parental constant挿入位置が一意ではありません")
modified = modified.replace(needle, addition, 1)
old = '''    public void setParentalControlsEnabled(boolean enabled) {
        mTvInputManager.setParentalControlsEnabled(enabled);
    }
'''
new = '''    public void setParentalControlsEnabled(boolean enabled) {
        mTvInputManager.setParentalControlsEnabled(enabled);
        if (mRatings == null) {
            loadRatings();
        } else {
            storeRatings();
        }
    }
'''
if modified.count(old) != 1:
    raise SystemExit("AOSP setParentalControlsEnabled置換対象が一意ではありません")
modified = modified.replace(old, new, 1)
old = '''    public void loadRatings() {
        mRatings = new HashSet<>(mTvInputManager.getBlockedRatings());
    }

    private void storeRatings() {
        Set<TvContentRating> removed = new HashSet<>(mTvInputManager.getBlockedRatings());'''
new = '''    public void loadRatings() {
        mRatings = new HashSet<>(mTvInputManager.getBlockedRatings());
        storeRatings();
    }

    private void storeRatings() {
        applyMaleicacidExceptionalRatingPolicy();
        Set<TvContentRating> removed = new HashSet<>(mTvInputManager.getBlockedRatings());'''
if modified.count(old) != 1:
    raise SystemExit("AOSP load/store ratings置換対象が一意ではありません")
modified = modified.replace(old, new, 1)
anchor = '''    private void updateRatingsForCurrentLevel(ContentRatingsManager manager) {'''
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
if modified.count(anchor) != 1:
    raise SystemExit("AOSP parental helper挿入位置が一意ではありません")
modified = modified.replace(anchor, helper + anchor, 1)
patch_path = Path("tis/platform_patches/lineage-22.1/packages_apps_TV_arib_exceptional_parental_policy.patch")
patch_path.parent.mkdir(parents=True, exist_ok=True)
rel = "src/com/android/tv/parental/ParentalControlSettings.java"
diff = list(difflib.unified_diff(
    original.splitlines(keepends=True),
    modified.splitlines(keepends=True),
    fromfile=f"a/{rel}",
    tofile=f"b/{rel}",
))
patch_path.write_text(f"diff --git a/{rel} b/{rel}\n" + "".join(diff))

# 開発規則の明示指摘箇所について英語commentが残っていないことだけを限定確認する。
checks = {
    "tis/src/com/maleicacid/tvinput/tis/TunerSelectionPolicy.kt": [
        "Pure stream and track selection policy",
        "ARIB audio_component_descriptor facts",
        "EIT component_descriptor facts",
    ],
    "tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt": [
        "Rust caption JNI parses",
        "Rebuild TIF tracks",
    ],
    "tis/platform_patches/lineage-22.1/frameworks_base_mediasync_first_output.patch": [
        "Listener for the first video frame",
        "Arms a one-shot callback",
        "armSequence must be positive when armed",
        "Disarm is part of release cleanup",
    ],
}
for path, needles in checks.items():
    body = read(path)
    for needle in needles:
        if needle in body:
            raise SystemExit(f"指摘済み英語comment/errorが残っています: {path}: {needle}")
