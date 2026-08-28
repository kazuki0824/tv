use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const PROVIDER_SCHEMA_VERSION: i64 = 1;
const PROGRAM_SCHEMA_NAME: &str = "maleicacid.tv.program";
const CHANNEL_SCHEMA_NAME: &str = "maleicacid.tv.channel";
const PROGRAM_REQUEST_SCHEMA_NAME: &str = "maleicacid.tv.programRequest";
const CHANNEL_REQUEST_SCHEMA_NAME: &str = "maleicacid.tv.channelRequest";
const CHANNEL_SCHEMA_VERSION: i64 = 1;
const SOFT_LIMIT_BYTES: usize = 16 * 1024;
const HARD_LIMIT_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDataResult {
    pub success: bool,
    pub json: String,
    pub schema_version: i64,
    pub truncated: bool,
    pub diagnostics_dropped_count: i64,
    pub error_code: String,
    pub error_message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramKeyResult {
    pub original_network_id: i64,
    pub transport_stream_id: i64,
    pub service_id: i64,
    pub event_id: i64,
    pub key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProgramKeyV1 {
    kind: String,
    original_network_id: i64,
    transport_stream_id: i64,
    service_id: i64,
    event_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServiceKeyV1 {
    original_network_id: i64,
    transport_stream_id: i64,
    service_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TimingV1 {
    start_utc_millis: i64,
    duration_millis: i64,
    #[serde(rename = "endUtcMillis", default, skip_serializing)]
    legacy_end_utc_millis: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProgramRequestTimingV1 {
    start_utc_millis: i64,
    duration_millis: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceV1 {
    pid: i64,
    table_id: i64,
    version: i64,
    section_number: i64,
    last_section_number: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CasV1 {
    requires_cas: bool,
    source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RatingV1 {
    country_code: String,
    raw_rating_byte: i64,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GenreV1 {
    level1: i64,
    level2: i64,
    user_nibble: i64,
    arib_name: String,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SeriesV1 {
    series_id: i64,
    repeat_label: i64,
    program_pattern: i64,
    expire_date_valid: bool,
    expire_date: Option<i64>,
    episode_number: i64,
    last_episode_number: i64,
    name: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FreeCaModeV1 {
    raw: i64,
    scrambled: bool,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExtendedItemV1 {
    language_code: String,
    description: String,
    text: String,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventGroupReferenceV1 {
    service_id: i64,
    event_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OtherNetworkEventReferenceV1 {
    original_network_id: i64,
    transport_stream_id: i64,
    service_id: i64,
    event_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventGroupV1 {
    group_type: i64,
    events: Vec<EventGroupReferenceV1>,
    other_network_events: Vec<OtherNetworkEventReferenceV1>,
    private_data_hex: String,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LinkageV1 {
    transport_stream_id: i64,
    original_network_id: i64,
    service_id: i64,
    linkage_type: i64,
    private_data_prefix_hex: String,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SectionScopeV1 {
    pub(crate) pid: Option<i64>,
    pub(crate) table_id: Option<i64>,
    pub(crate) table_id_extension: Option<i64>,
    pub(crate) version: Option<i64>,
    pub(crate) section_number: Option<i64>,
    pub(crate) original_network_id: Option<i64>,
    pub(crate) transport_stream_id: Option<i64>,
    pub(crate) service_id: Option<i64>,
    pub(crate) event_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DescriptorScopeV1 {
    pub(crate) tag: i64,
    pub(crate) name: Option<String>,
    pub(crate) offset: i64,
    pub(crate) declared_length: i64,
    pub(crate) actual_remaining_length: i64,
    pub(crate) parse_status: String,
    pub(crate) raw_prefix_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DescriptorDiagnosticV1 {
    pub(crate) schema: String,
    pub(crate) schema_version: i64,
    pub(crate) severity: String,
    pub(crate) code: String,
    pub(crate) scope: SectionScopeV1,
    pub(crate) descriptor: DescriptorScopeV1,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VideoComponentV1 {
    es_pid: i64,
    stream_type: i64,
    component_tag: Option<i64>,
    component_type: Option<i64>,
    codec: String,
    resolution: Option<String>,
    scan: Option<String>,
    aspect: Option<String>,
    profile_level: Option<String>,
    source_descriptor: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AudioComponentV1 {
    es_pid: i64,
    stream_type: i64,
    component_tag: Option<i64>,
    component_type: Option<i64>,
    codec: String,
    language: Option<String>,
    second_language: Option<String>,
    channel_configuration: Option<String>,
    sampling_info: Option<String>,
    source_descriptor: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubtitleComponentV1 {
    es_pid: i64,
    component_tag: Option<i64>,
    data_component_id: Option<i64>,
    language: Option<String>,
    caption_service_kind: String,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DataComponentV1 {
    es_pid: i64,
    component_tag: Option<i64>,
    data_component_id: Option<i64>,
    component_type: Option<i64>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ComponentsV1 {
    video: Vec<VideoComponentV1>,
    audio: Vec<AudioComponentV1>,
    subtitle: Vec<SubtitleComponentV1>,
    data: Vec<DataComponentV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DiagnosticItemV1 {
    code: String,
    message: String,
    severity: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawProviderDataExtensionV1 {
    key: String,
    value: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DiagnosticsV1 {
    descriptor_diagnostics: Vec<DescriptorDiagnosticV1>,
    publish_diagnostics: Vec<DiagnosticItemV1>,
    parser_diagnostics: Vec<DiagnosticItemV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_provider_data_extensions: Vec<RawProviderDataExtensionV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_hard_limit_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_soft_limit_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_dropped_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_dropped_counts: Option<BTreeMap<String, i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_original_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_final_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_truncation_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    malformed_ca_descriptor_count: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgramProviderDataV1 {
    schema: String,
    schema_version: i64,
    program_key: ProgramKeyV1,
    timing: TimingV1,
    source: SourceV1,
    cas: CasV1,
    ratings: Vec<RatingV1>,
    genres: Vec<GenreV1>,
    series: Option<SeriesV1>,
    event_groups: Vec<EventGroupV1>,
    linkage: Vec<LinkageV1>,
    free_ca_mode: Option<FreeCaModeV1>,
    extended_items: Vec<ExtendedItemV1>,
    components: ComponentsV1,
    diagnostics: DiagnosticsV1,
    #[serde(default, flatten)]
    extensions: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProgramRequestDiagnosticsV1 {
    descriptor_diagnostics_canonical_json: String,
    publish_diagnostics: Vec<DiagnosticItemV1>,
    parser_diagnostics: Vec<DiagnosticItemV1>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProgramProviderDataRequestV1 {
    schema: String,
    schema_version: i64,
    program_key: ProgramKeyV1,
    timing: ProgramRequestTimingV1,
    source: SourceV1,
    cas: CasV1,
    ratings: Vec<RatingV1>,
    genres: Vec<GenreV1>,
    series: Option<SeriesV1>,
    event_groups: Vec<EventGroupV1>,
    linkage: Vec<LinkageV1>,
    free_ca_mode: Option<FreeCaModeV1>,
    extended_items: Vec<ExtendedItemV1>,
    components: ComponentsV1,
    diagnostics: ProgramRequestDiagnosticsV1,
    #[serde(default)]
    malformed_ca_descriptor_count: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChannelTuneV1 {
    #[serde(rename = "displayName", default, skip_serializing)]
    legacy_display_name: Option<String>,
    delivery_system: String,
    frequency_hz: i64,
    stream_id: Option<i64>,
    stream_id_type: String,
    physical_channel: Option<i64>,
    satellite_band: Option<String>,
    remote_control_key_id: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChannelRequestTuneV1 {
    delivery_system: String,
    frequency_hz: i64,
    stream_id: Option<i64>,
    stream_id_type: String,
    physical_channel: Option<i64>,
    satellite_band: Option<String>,
    remote_control_key_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChannelCasV1 {
    requires_cas: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChannelDiagnosticsV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_provider_data_extensions: Vec<RawProviderDataExtensionV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_hard_limit_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_soft_limit_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_dropped_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_dropped_counts: Option<BTreeMap<String, i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_original_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_final_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_truncation_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelProviderDataV1 {
    schema: String,
    schema_version: i64,
    service_key: ServiceKeyV1,
    tune: ChannelTuneV1,
    cas: ChannelCasV1,
    diagnostics: ChannelDiagnosticsV1,
    #[serde(default, flatten)]
    extensions: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelRequestDiagnosticsV1 {}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChannelProviderDataRequestV1 {
    schema: String,
    schema_version: i64,
    service_key: ServiceKeyV1,
    tune: ChannelRequestTuneV1,
    cas: ChannelCasV1,
    diagnostics: ChannelRequestDiagnosticsV1,
}

pub fn build_program_key(onid: i32, tsid: i32, sid: i32, event_id: i32) -> String {
    serde_json::to_string(&ProgramKeyV1 {
        kind: "arib-event-v1".to_string(),
        original_network_id: i64::from(onid),
        transport_stream_id: i64::from(tsid),
        service_id: i64::from(sid),
        event_id: i64::from(event_id),
    })
    .unwrap_or_default()
}

fn field_object_has_only(parent: &serde_json::Value, field: &str, known_keys: &[&str]) -> bool {
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

pub fn build_program_provider_data(request_json: &str) -> ProviderDataResult {
    let request = match serde_json::from_str::<ProgramProviderDataRequestV1>(request_json) {
        Ok(request) => request,
        Err(err) => {
            return failure_result(
                "PROGRAM_REQUEST_PARSE_FAILED",
                format!("Program provider-data request JSON parse failed: {err}"),
                PROVIDER_SCHEMA_VERSION,
            )
        }
    };
    let Some(data) = program_data_from_request(request) else {
        return failure_result(
            "PROGRAM_REQUEST_INVALID",
            "Program provider-data request did not satisfy schema v1 invariants".to_string(),
            PROVIDER_SCHEMA_VERSION,
        );
    };
    finalize_program(data)
}

pub fn build_channel_provider_data(request_json: &str) -> ProviderDataResult {
    let request = match serde_json::from_str::<ChannelProviderDataRequestV1>(request_json) {
        Ok(request) => request,
        Err(err) => {
            return failure_result(
                "CHANNEL_REQUEST_PARSE_FAILED",
                format!("Channel provider-data request JSON parse failed: {err}"),
                CHANNEL_SCHEMA_VERSION,
            )
        }
    };
    let Some(data) = channel_data_from_request(request) else {
        return failure_result(
            "CHANNEL_REQUEST_INVALID",
            "Channel provider-data request did not satisfy schema v1 invariants".to_string(),
            CHANNEL_SCHEMA_VERSION,
        );
    };
    finalize_channel(data)
}

pub fn normalize_program_provider_data(raw_bytes: &[u8]) -> ProviderDataResult {
    let text = match std::str::from_utf8(raw_bytes) {
        Ok(text) => text,
        Err(err) => {
            return failure_result(
                "PROGRAM_PROVIDER_DATA_UTF8_FAILED",
                format!("Program provider-data is not UTF-8: {err}"),
                PROVIDER_SCHEMA_VERSION,
            )
        }
    };
    let raw_value = match serde_json::from_str::<serde_json::Value>(text.trim()) {
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
    let data = normalize_program_extensions(data, Some(&raw_value));
    if !valid_program_provider_data(&data) {
        return failure_result(
            "PROGRAM_PROVIDER_DATA_INVALID",
            "Program provider-data JSON v1 invariants failed".to_string(),
            PROVIDER_SCHEMA_VERSION,
        );
    }
    finalize_program(data)
}

pub fn extract_program_key_result(raw_bytes: &[u8]) -> Option<ProgramKeyResult> {
    let text = std::str::from_utf8(raw_bytes).ok()?;
    let data = serde_json::from_str::<ProgramProviderDataV1>(text.trim()).ok()?;
    if !valid_program_provider_data(&data) {
        return None;
    }
    Some(ProgramKeyResult {
        original_network_id: data.program_key.original_network_id,
        transport_stream_id: data.program_key.transport_stream_id,
        service_id: data.program_key.service_id,
        event_id: data.program_key.event_id,
        key: serde_json::to_string(&data.program_key).unwrap_or_default(),
    })
}

#[cfg(test)]
pub fn extract_program_key(raw_bytes: &[u8]) -> Option<String> {
    extract_program_key_result(raw_bytes).map(|v| v.key)
}

pub fn decode_channel_provider_data(provider_data: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(provider_data) else {
        return String::new();
    };
    let Ok(raw_value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return String::new();
    };
    let Ok(data) = serde_json::from_value::<ChannelProviderDataV1>(raw_value.clone()) else {
        return String::new();
    };
    let data = normalize_channel_extensions(data, Some(&raw_value));
    if !valid_channel_provider_data(&data) {
        return String::new();
    }
    let canonical_result = finalize_channel(data.clone());
    if !canonical_result.success {
        return String::new();
    }
    serde_json::json!({
        "canonical": canonical_result.json,
        "schemaVersion": data.schema_version,
        "serviceKey": {
            "originalNetworkId": data.service_key.original_network_id,
            "transportStreamId": data.service_key.transport_stream_id,
            "serviceId": data.service_key.service_id,
        },
        "tune": {
            "deliverySystem": data.tune.delivery_system,
            "frequencyHz": data.tune.frequency_hz,
            "streamId": data.tune.stream_id,
            "streamIdType": data.tune.stream_id_type,
            "physicalChannel": data.tune.physical_channel,
            "satelliteBand": data.tune.satellite_band,
            "remoteControlKeyId": data.tune.remote_control_key_id,
        },
        "cas": { "requiresCas": data.cas.requires_cas },
    })
    .to_string()
}

fn program_data_from_request(
    request: ProgramProviderDataRequestV1,
) -> Option<ProgramProviderDataV1> {
    if request.schema != PROGRAM_REQUEST_SCHEMA_NAME
        || request.schema_version != PROVIDER_SCHEMA_VERSION
    {
        return None;
    }
    if request.program_key.kind != "arib-event-v1" {
        return None;
    }
    let descriptor_diagnostics =
        parse_descriptor_diagnostics(&request.diagnostics.descriptor_diagnostics_canonical_json)?;
    let data = ProgramProviderDataV1 {
        schema: PROGRAM_SCHEMA_NAME.to_string(),
        schema_version: PROVIDER_SCHEMA_VERSION,
        program_key: request.program_key,
        timing: TimingV1 {
            start_utc_millis: request.timing.start_utc_millis,
            duration_millis: request.timing.duration_millis,
            legacy_end_utc_millis: None,
        },
        source: request.source,
        cas: request.cas,
        ratings: request.ratings,
        genres: request.genres,
        series: request.series,
        event_groups: request.event_groups,
        linkage: request.linkage,
        free_ca_mode: request.free_ca_mode,
        extended_items: request.extended_items,
        components: request.components,
        diagnostics: DiagnosticsV1 {
            descriptor_diagnostics,
            publish_diagnostics: request.diagnostics.publish_diagnostics,
            parser_diagnostics: request.diagnostics.parser_diagnostics,
            raw_provider_data_extensions: Vec::new(),
            provider_data_truncated: None,
            provider_data_hard_limit_bytes: None,
            provider_data_soft_limit_bytes: None,
            provider_data_dropped_count: None,
            provider_data_dropped_counts: None,
            provider_data_original_bytes: None,
            provider_data_final_bytes: None,
            provider_data_truncation_code: None,
            malformed_ca_descriptor_count: (request.malformed_ca_descriptor_count > 0)
                .then_some(request.malformed_ca_descriptor_count),
        },
        extensions: serde_json::Map::new(),
    };
    valid_program_provider_data(&data).then_some(data)
}

fn channel_data_from_request(
    request: ChannelProviderDataRequestV1,
) -> Option<ChannelProviderDataV1> {
    if request.schema != CHANNEL_REQUEST_SCHEMA_NAME
        || request.schema_version != CHANNEL_SCHEMA_VERSION
    {
        return None;
    }
    let _request_diagnostics = request.diagnostics;
    let data = ChannelProviderDataV1 {
        schema: CHANNEL_SCHEMA_NAME.to_string(),
        schema_version: CHANNEL_SCHEMA_VERSION,
        service_key: request.service_key,
        tune: ChannelTuneV1 {
            legacy_display_name: None,
            delivery_system: request.tune.delivery_system,
            frequency_hz: request.tune.frequency_hz,
            stream_id: request.tune.stream_id,
            stream_id_type: request.tune.stream_id_type,
            physical_channel: request.tune.physical_channel,
            satellite_band: request.tune.satellite_band,
            remote_control_key_id: request.tune.remote_control_key_id,
        },
        cas: request.cas,
        diagnostics: ChannelDiagnosticsV1::default(),
        extensions: serde_json::Map::new(),
    };
    valid_channel_provider_data(&data).then_some(data)
}

fn normalize_program_extensions(
    mut data: ProgramProviderDataV1,
    raw_value: Option<&serde_json::Value>,
) -> ProgramProviderDataV1 {
    data.diagnostics
        .raw_provider_data_extensions
        .retain(|extension| !forbidden_program_extension(&extension.key));
    let extensions = std::mem::take(&mut data.extensions);
    for (key, value) in extensions {
        if forbidden_program_extension(&key) {
            continue;
        }
        data.diagnostics
            .raw_provider_data_extensions
            .push(RawProviderDataExtensionV1 { key, value });
    }
    if let Some(raw) = raw_value {
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
    data
}

fn normalize_channel_extensions(
    mut data: ChannelProviderDataV1,
    raw_value: Option<&serde_json::Value>,
) -> ChannelProviderDataV1 {
    data.tune.legacy_display_name = None;
    data.diagnostics
        .raw_provider_data_extensions
        .retain(|extension| !forbidden_channel_extension(&extension.key));
    let extensions = std::mem::take(&mut data.extensions);
    for (key, value) in extensions {
        if forbidden_channel_extension(&key) {
            continue;
        }
        data.diagnostics
            .raw_provider_data_extensions
            .push(RawProviderDataExtensionV1 { key, value });
    }
    if let Some(raw) = raw_value {
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
    data
}

fn forbidden_program_extension(key: &str) -> bool {
    matches!(
        key,
        "serviceKey"
            | "audioLanguages"
            | "audio"
            | "video"
            | "eventGroupText"
            | "freeCaText"
            | "seriesName"
            | "canonicalGenres"
            | "signature"
            | "contentDigest"
            | "cas.unsupportedCas"
            | "cas.clearLivePlaybackSupported"
            | "cas.channelRegistrationReady"
            | "cas.epgPublishable"
            | "cas.publishStateSource"
            | "diagnostics.currentProgram"
    ) || indexed_field_is_forbidden(key, "ratings", &["supported", "mappedTvContentRating"])
        || indexed_field_is_forbidden(
            key,
            "genres",
            &["unmappedReason", "canonicalGenre", "mappedCanonicalGenre"],
        )
        || indexed_field_is_forbidden(
            key,
            "components.video",
            &["r51PlaybackSupported", "liveViewableClaim"],
        )
        || indexed_field_is_forbidden(
            key,
            "components.audio",
            &["r51PlaybackSupported", "liveViewableClaim"],
        )
        || indexed_field_is_forbidden(key, "components.subtitle", &["trackId"])
}

fn forbidden_channel_extension(key: &str) -> bool {
    matches!(
        key,
        "inputId"
            | "backend"
            | "backendHint"
            | "driver"
            | "driverName"
            | "px4RelativeSlot"
            | "relativeSlot"
            | "signature"
            | "contentDigest"
            | "tune.inputId"
            | "tune.backend"
            | "tune.backendHint"
            | "tune.driver"
            | "tune.driverName"
            | "tune.px4RelativeSlot"
            | "tune.relativeSlot"
            | "cas.unsupportedCas"
            | "cas.clearLivePlaybackSupported"
            | "cas.channelRegistrationReady"
            | "cas.epgPublishable"
            | "cas.publishStateSource"
            | "diagnostics.channelRegistrationReady"
            | "diagnostics.epgPublishable"
            | "diagnostics.publishStateSource"
            | "diagnostics.unsupportedCas"
            | "diagnostics.clearLivePlaybackSupported"
    )
}

fn indexed_field_is_forbidden(key: &str, collection: &str, fields: &[&str]) -> bool {
    let Some(rest) = key
        .strip_prefix(collection)
        .and_then(|value| value.strip_prefix('['))
    else {
        return false;
    };
    let Some((index, field)) = rest.split_once("].") else {
        return false;
    };
    !index.is_empty()
        && index.bytes().all(|byte| byte.is_ascii_digit())
        && fields.iter().any(|forbidden| *forbidden == field)
}

fn collect_channel_unknown_extensions(
    raw: &serde_json::Value,
    out: &mut Vec<RawProviderDataExtensionV1>,
) {
    collect_object_unknown(
        raw,
        "",
        &[
            "schema",
            "schemaVersion",
            "serviceKey",
            "tune",
            "cas",
            "diagnostics",
        ],
        out,
    );
    collect_object_unknown(
        raw.get("serviceKey").unwrap_or(&serde_json::Value::Null),
        "serviceKey",
        &["originalNetworkId", "transportStreamId", "serviceId"],
        out,
    );
    collect_object_unknown(
        raw.get("tune").unwrap_or(&serde_json::Value::Null),
        "tune",
        &[
            "displayName",
            "deliverySystem",
            "frequencyHz",
            "streamId",
            "streamIdType",
            "physicalChannel",
            "satelliteBand",
            "remoteControlKeyId",
        ],
        out,
    );
    collect_object_unknown(
        raw.get("cas").unwrap_or(&serde_json::Value::Null),
        "cas",
        &["requiresCas"],
        out,
    );
    if let Some(diagnostics) = raw.get("diagnostics") {
        collect_object_unknown(
            diagnostics,
            "diagnostics",
            &[
                "rawProviderDataExtensions",
                "providerDataTruncated",
                "providerDataHardLimitBytes",
                "providerDataSoftLimitBytes",
                "providerDataDroppedCount",
                "providerDataDroppedCounts",
                "providerDataOriginalBytes",
                "providerDataFinalBytes",
                "providerDataTruncationCode",
            ],
            out,
        );
        collect_array_unknown(
            diagnostics
                .get("rawProviderDataExtensions")
                .unwrap_or(&serde_json::Value::Null),
            "diagnostics.rawProviderDataExtensions",
            &["key", "value"],
            out,
        );
    }
}

fn collect_program_unknown_extensions(
    raw: &serde_json::Value,
    out: &mut Vec<RawProviderDataExtensionV1>,
) {
    collect_object_unknown(
        raw,
        "",
        &[
            "schema",
            "schemaVersion",
            "programKey",
            "serviceKey",
            "timing",
            "source",
            "cas",
            "ratings",
            "genres",
            "series",
            "eventGroups",
            "linkage",
            "freeCaMode",
            "audioLanguages",
            "extendedItems",
            "components",
            "diagnostics",
        ],
        out,
    );
    collect_object_unknown(
        raw.get("programKey").unwrap_or(&serde_json::Value::Null),
        "programKey",
        &[
            "kind",
            "originalNetworkId",
            "transportStreamId",
            "serviceId",
            "eventId",
        ],
        out,
    );
    collect_object_unknown(
        raw.get("serviceKey").unwrap_or(&serde_json::Value::Null),
        "serviceKey",
        &["originalNetworkId", "transportStreamId", "serviceId"],
        out,
    );
    collect_object_unknown(
        raw.get("timing").unwrap_or(&serde_json::Value::Null),
        "timing",
        &["startUtcMillis", "endUtcMillis", "durationMillis"],
        out,
    );
    collect_object_unknown(
        raw.get("source").unwrap_or(&serde_json::Value::Null),
        "source",
        &[
            "pid",
            "tableId",
            "version",
            "sectionNumber",
            "lastSectionNumber",
        ],
        out,
    );
    collect_object_unknown(
        raw.get("cas").unwrap_or(&serde_json::Value::Null),
        "cas",
        &["requiresCas", "source"],
        out,
    );
    collect_array_unknown(
        raw.get("ratings").unwrap_or(&serde_json::Value::Null),
        "ratings",
        &["countryCode", "rawRatingByte", "parseStatus"],
        out,
    );
    collect_array_unknown(
        raw.get("genres").unwrap_or(&serde_json::Value::Null),
        "genres",
        &["level1", "level2", "userNibble", "aribName", "parseStatus"],
        out,
    );
    collect_object_unknown(
        raw.get("series").unwrap_or(&serde_json::Value::Null),
        "series",
        &[
            "seriesId",
            "repeatLabel",
            "programPattern",
            "expireDateValid",
            "expireDate",
            "episodeNumber",
            "lastEpisodeNumber",
            "name",
            "parseStatus",
        ],
        out,
    );
    collect_event_group_unknown_extensions(
        raw.get("eventGroups").unwrap_or(&serde_json::Value::Null),
        out,
    );
    collect_array_unknown(
        raw.get("linkage").unwrap_or(&serde_json::Value::Null),
        "linkage",
        &[
            "transportStreamId",
            "originalNetworkId",
            "serviceId",
            "linkageType",
            "privateDataPrefixHex",
            "parseStatus",
        ],
        out,
    );
    collect_object_unknown(
        raw.get("freeCaMode").unwrap_or(&serde_json::Value::Null),
        "freeCaMode",
        &["raw", "scrambled", "parseStatus"],
        out,
    );
    collect_array_unknown(
        raw.get("audioLanguages")
            .unwrap_or(&serde_json::Value::Null),
        "audioLanguages",
        &["language", "source", "parseStatus"],
        out,
    );
    collect_array_unknown(
        raw.get("extendedItems").unwrap_or(&serde_json::Value::Null),
        "extendedItems",
        &["languageCode", "description", "text", "parseStatus"],
        out,
    );
    if let Some(components) = raw.get("components") {
        collect_object_unknown(
            components,
            "components",
            &["video", "audio", "subtitle", "data"],
            out,
        );
        collect_array_unknown(
            components.get("video").unwrap_or(&serde_json::Value::Null),
            "components.video",
            &[
                "esPid",
                "streamType",
                "componentTag",
                "componentType",
                "codec",
                "resolution",
                "scan",
                "aspect",
                "profileLevel",
                "sourceDescriptor",
                "parseStatus",
            ],
            out,
        );
        collect_array_unknown(
            components.get("audio").unwrap_or(&serde_json::Value::Null),
            "components.audio",
            &[
                "esPid",
                "streamType",
                "componentTag",
                "componentType",
                "codec",
                "language",
                "secondLanguage",
                "channelConfiguration",
                "samplingInfo",
                "sourceDescriptor",
                "parseStatus",
            ],
            out,
        );
        collect_array_unknown(
            components
                .get("subtitle")
                .unwrap_or(&serde_json::Value::Null),
            "components.subtitle",
            &[
                "esPid",
                "componentTag",
                "dataComponentId",
                "language",
                "captionServiceKind",
                "parseStatus",
            ],
            out,
        );
        collect_array_unknown(
            components.get("data").unwrap_or(&serde_json::Value::Null),
            "components.data",
            &[
                "esPid",
                "componentTag",
                "dataComponentId",
                "componentType",
                "parseStatus",
            ],
            out,
        );
    }
    if let Some(diagnostics) = raw.get("diagnostics") {
        collect_object_unknown(
            diagnostics,
            "diagnostics",
            &[
                "descriptorDiagnostics",
                "publishDiagnostics",
                "parserDiagnostics",
                "rawProviderDataExtensions",
                "providerDataTruncated",
                "providerDataHardLimitBytes",
                "providerDataSoftLimitBytes",
                "providerDataDroppedCount",
                "providerDataDroppedCounts",
                "providerDataOriginalBytes",
                "providerDataFinalBytes",
                "providerDataTruncationCode",
                "malformedCaDescriptorCount",
            ],
            out,
        );
        collect_descriptor_diagnostic_unknown(
            diagnostics
                .get("descriptorDiagnostics")
                .unwrap_or(&serde_json::Value::Null),
            out,
        );
        collect_array_unknown(
            diagnostics
                .get("publishDiagnostics")
                .unwrap_or(&serde_json::Value::Null),
            "diagnostics.publishDiagnostics",
            &["code", "message", "severity"],
            out,
        );
        collect_array_unknown(
            diagnostics
                .get("parserDiagnostics")
                .unwrap_or(&serde_json::Value::Null),
            "diagnostics.parserDiagnostics",
            &["code", "message", "severity"],
            out,
        );
        collect_array_unknown(
            diagnostics
                .get("rawProviderDataExtensions")
                .unwrap_or(&serde_json::Value::Null),
            "diagnostics.rawProviderDataExtensions",
            &["key", "value"],
            out,
        );
    }
}

fn collect_event_group_unknown_extensions(
    value: &serde_json::Value,
    out: &mut Vec<RawProviderDataExtensionV1>,
) {
    let Some(groups) = value.as_array() else {
        return;
    };
    for (index, group) in groups.iter().enumerate() {
        let base = format!("eventGroups[{}]", index);
        collect_object_unknown(
            group,
            &base,
            &[
                "groupType",
                "events",
                "otherNetworkEvents",
                "privateDataHex",
                "parseStatus",
            ],
            out,
        );
        collect_array_unknown(
            group.get("events").unwrap_or(&serde_json::Value::Null),
            &format!("{}.events", base),
            &["serviceId", "eventId"],
            out,
        );
        collect_array_unknown(
            group
                .get("otherNetworkEvents")
                .unwrap_or(&serde_json::Value::Null),
            &format!("{}.otherNetworkEvents", base),
            &[
                "originalNetworkId",
                "transportStreamId",
                "serviceId",
                "eventId",
            ],
            out,
        );
    }
}

fn collect_descriptor_diagnostic_unknown(
    value: &serde_json::Value,
    out: &mut Vec<RawProviderDataExtensionV1>,
) {
    let Some(items) = value.as_array() else {
        return;
    };
    for (index, item) in items.iter().enumerate() {
        let base = format!("diagnostics.descriptorDiagnostics[{}]", index);
        collect_object_unknown(
            item,
            &base,
            &[
                "schema",
                "schemaVersion",
                "severity",
                "code",
                "scope",
                "descriptor",
                "message",
            ],
            out,
        );
        if let Some(scope) = item.get("scope") {
            collect_object_unknown(
                scope,
                &format!("{}.scope", base),
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
                out,
            );
        }
        if let Some(descriptor) = item.get("descriptor") {
            collect_object_unknown(
                descriptor,
                &format!("{}.descriptor", base),
                &[
                    "tag",
                    "name",
                    "offset",
                    "declaredLength",
                    "actualRemainingLength",
                    "parseStatus",
                    "rawPrefixHex",
                ],
                out,
            );
        }
    }
}

fn collect_array_unknown(
    value: &serde_json::Value,
    path: &str,
    known_keys: &[&str],
    out: &mut Vec<RawProviderDataExtensionV1>,
) {
    let Some(items) = value.as_array() else {
        return;
    };
    for (index, item) in items.iter().enumerate() {
        collect_object_unknown(item, &format!("{}[{}]", path, index), known_keys, out);
    }
}

fn collect_object_unknown(
    value: &serde_json::Value,
    path: &str,
    known_keys: &[&str],
    out: &mut Vec<RawProviderDataExtensionV1>,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (key, value) in object {
        if !known_keys.iter().any(|known| *known == key) {
            let full_key = if path.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", path, key)
            };
            out.push(RawProviderDataExtensionV1 {
                key: full_key,
                value: value.clone(),
            });
        }
    }
}

fn note_drop(counts: &mut BTreeMap<String, i64>, kind: &str, count: i64) {
    let current = counts.get(kind).copied().unwrap_or(0);
    counts.insert(kind.to_string(), current.saturating_add(count));
}

fn total_dropped(counts: &BTreeMap<String, i64>) -> i64 {
    counts
        .values()
        .copied()
        .fold(0i64, |total, value| total.saturating_add(value))
}

fn shorten_utf8_tail(value: &mut String, requested_bytes: usize, keep_one_scalar: bool) -> usize {
    let minimum = if keep_one_scalar {
        value.chars().next().map(char::len_utf8).unwrap_or(0)
    } else {
        0
    };
    if value.len() <= minimum {
        return 0;
    }
    let mut target = value
        .len()
        .saturating_sub(requested_bytes.max(1))
        .max(minimum);
    while target > minimum && !value.is_char_boundary(target) {
        target -= 1;
    }
    if target == value.len() {
        target = value[..target]
            .char_indices()
            .next_back()
            .map(|(index, _)| index.max(minimum))
            .unwrap_or(minimum);
    }
    let removed = value.len().saturating_sub(target);
    value.truncate(target);
    removed
}

fn shorten_program_long_text(data: &mut ProgramProviderDataV1, requested_bytes: usize) -> usize {
    if let Some(item) = data
        .diagnostics
        .parser_diagnostics
        .iter_mut()
        .rev()
        .find(|item| item.message.chars().count() > 1)
    {
        return shorten_utf8_tail(&mut item.message, requested_bytes, true);
    }
    if let Some(item) = data
        .genres
        .iter_mut()
        .rev()
        .find(|item| !item.arib_name.is_empty())
    {
        return shorten_utf8_tail(&mut item.arib_name, requested_bytes, false);
    }
    if let Some(name) = data.series.as_mut().and_then(|series| series.name.as_mut()) {
        if !name.is_empty() {
            return shorten_utf8_tail(name, requested_bytes, false);
        }
    }
    if let Some(item) = data
        .linkage
        .iter_mut()
        .rev()
        .find(|item| !item.private_data_prefix_hex.is_empty())
    {
        let removed = shorten_utf8_tail(&mut item.private_data_prefix_hex, requested_bytes, false);
        if item.private_data_prefix_hex.len() % 2 != 0 {
            item.private_data_prefix_hex.pop();
            return removed.saturating_add(1);
        }
        return removed;
    }
    for item in data.components.video.iter_mut().rev() {
        for value in [
            &mut item.source_descriptor,
            &mut item.profile_level,
            &mut item.aspect,
            &mut item.scan,
            &mut item.resolution,
        ] {
            if let Some(text) = value.as_mut() {
                if !text.is_empty() {
                    return shorten_utf8_tail(text, requested_bytes, false);
                }
            }
        }
    }
    for item in data.components.audio.iter_mut().rev() {
        for value in [
            &mut item.source_descriptor,
            &mut item.sampling_info,
            &mut item.channel_configuration,
        ] {
            if let Some(text) = value.as_mut() {
                if !text.is_empty() {
                    return shorten_utf8_tail(text, requested_bytes, false);
                }
            }
        }
    }
    0
}

fn serialize_program_with_truncation_metadata(
    data: &mut ProgramProviderDataV1,
    counts: &BTreeMap<String, i64>,
) -> Result<String, serde_json::Error> {
    data.diagnostics.provider_data_dropped_count = Some(total_dropped(counts));
    data.diagnostics.provider_data_dropped_counts = Some(counts.clone());
    let mut text = serde_json::to_string(data)?;
    for _ in 0..16 {
        data.diagnostics.provider_data_final_bytes = Some(text.len() as i64);
        let next = serde_json::to_string(data)?;
        if next.len() == text.len() {
            return Ok(next);
        }
        text = next;
    }
    Ok(text)
}

fn serialize_channel_with_truncation_metadata(
    data: &mut ChannelProviderDataV1,
    counts: &BTreeMap<String, i64>,
) -> Result<String, serde_json::Error> {
    data.diagnostics.provider_data_dropped_count = Some(total_dropped(counts));
    data.diagnostics.provider_data_dropped_counts = Some(counts.clone());
    let mut text = serde_json::to_string(data)?;
    for _ in 0..16 {
        data.diagnostics.provider_data_final_bytes = Some(text.len() as i64);
        let next = serde_json::to_string(data)?;
        if next.len() == text.len() {
            return Ok(next);
        }
        text = next;
    }
    Ok(text)
}

fn parse_descriptor_diagnostics(text: &str) -> Option<Vec<DescriptorDiagnosticV1>> {
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

fn valid_descriptor_diagnostic(item: &DescriptorDiagnosticV1) -> bool {
    item.schema == "maleicacid.tv.descriptorDiagnostic"
        && item.schema_version == 1
        && !item.severity.is_empty()
        && !item.code.is_empty()
        && item.descriptor.tag >= 0
        && item.descriptor.tag <= 255
        && item.descriptor.offset >= 0
        && item.descriptor.declared_length >= 0
        && item.descriptor.declared_length <= 255
        && item.descriptor.actual_remaining_length >= 0
        && !item.descriptor.parse_status.is_empty()
}

fn valid_program_provider_data(data: &ProgramProviderDataV1) -> bool {
    data.schema == PROGRAM_SCHEMA_NAME
        && data.schema_version == PROVIDER_SCHEMA_VERSION
        && data.program_key.kind == "arib-event-v1"
        && in_u16(data.program_key.original_network_id)
        && in_u16(data.program_key.transport_stream_id)
        && in_u16(data.program_key.service_id)
        && in_u16(data.program_key.event_id)
        && data.timing.start_utc_millis >= 0
        && data.timing.duration_millis >= 0
        && data
            .timing
            .start_utc_millis
            .checked_add(data.timing.duration_millis)
            .is_some()
        && data
            .timing
            .legacy_end_utc_millis
            .map(|end| {
                data.timing
                    .start_utc_millis
                    .checked_add(data.timing.duration_millis)
                    == Some(end)
            })
            .unwrap_or(true)
        && data.source.pid >= 0
        && data.source.pid <= 8191
        && data.source.table_id >= 0
        && data.source.table_id <= 255
        && data.source.version >= 0
        && data.source.version <= 31
        && data.source.section_number >= 0
        && data.source.section_number <= 255
        && data.source.last_section_number >= data.source.section_number
        && data.source.last_section_number <= 255
        && !data.cas.source.is_empty()
        && data.ratings.iter().all(valid_rating)
        && data.genres.iter().all(valid_genre)
        && data.series.as_ref().map(valid_series).unwrap_or(true)
        && data.event_groups.iter().all(valid_event_group)
        && data.linkage.iter().all(valid_linkage)
        && data
            .free_ca_mode
            .as_ref()
            .map(valid_free_ca_mode)
            .unwrap_or(true)
        && data.extended_items.iter().all(valid_extended_item)
        && valid_components(&data.components)
        && data
            .diagnostics
            .descriptor_diagnostics
            .iter()
            .all(valid_descriptor_diagnostic)
        && data
            .diagnostics
            .publish_diagnostics
            .iter()
            .all(valid_diagnostic_item)
        && data
            .diagnostics
            .parser_diagnostics
            .iter()
            .all(valid_diagnostic_item)
}

fn valid_channel_provider_data(data: &ChannelProviderDataV1) -> bool {
    data.schema == CHANNEL_SCHEMA_NAME
        && data.schema_version == CHANNEL_SCHEMA_VERSION
        && in_u16(data.service_key.original_network_id)
        && in_u16(data.service_key.transport_stream_id)
        && in_u16(data.service_key.service_id)
        && !data.tune.delivery_system.is_empty()
        && data.tune.frequency_hz > 0
        && matches!(data.tune.stream_id_type.as_str(), "NONE" | "TSID")
        && (if data.tune.stream_id_type == "NONE" {
            data.tune.stream_id.is_none()
        } else {
            data.tune.stream_id.map(valid_stream_id).unwrap_or(false)
        })
}

fn in_u16(v: i64) -> bool {
    (0..=0xffff).contains(&v)
}
fn valid_stream_id(v: i64) -> bool {
    (0..=0xfffe).contains(&v)
}
fn nonempty(s: &str) -> bool {
    !s.is_empty()
}
fn valid_iso639(s: &str) -> bool {
    s.len() == 3 && s.bytes().all(|byte| byte.is_ascii_alphabetic())
}
fn valid_rating(v: &RatingV1) -> bool {
    valid_iso639(&v.country_code)
        && (0..=255).contains(&v.raw_rating_byte)
        && nonempty(&v.parse_status)
}
fn valid_genre(v: &GenreV1) -> bool {
    (0..=15).contains(&v.level1)
        && (0..=15).contains(&v.level2)
        && (0..=255).contains(&v.user_nibble)
        && nonempty(&v.parse_status)
}
fn valid_series(v: &SeriesV1) -> bool {
    in_u16(v.series_id)
        && (0..=15).contains(&v.repeat_label)
        && (0..=7).contains(&v.program_pattern)
        && v.expire_date_valid == v.expire_date.is_some()
        && v.expire_date.map(in_u16).unwrap_or(true)
        && (0..=4095).contains(&v.episode_number)
        && (0..=4095).contains(&v.last_episode_number)
        && nonempty(&v.parse_status)
}
fn valid_event_group_reference(v: &EventGroupReferenceV1) -> bool {
    in_u16(v.service_id) && in_u16(v.event_id)
}
fn valid_other_network_event_reference(v: &OtherNetworkEventReferenceV1) -> bool {
    in_u16(v.original_network_id)
        && in_u16(v.transport_stream_id)
        && in_u16(v.service_id)
        && in_u16(v.event_id)
}
fn valid_hex(value: &str) -> bool {
    value.len() % 2 == 0 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn valid_event_group(v: &EventGroupV1) -> bool {
    (0..=15).contains(&v.group_type)
        && v.events.iter().all(valid_event_group_reference)
        && v.other_network_events
            .iter()
            .all(valid_other_network_event_reference)
        && valid_hex(&v.private_data_hex)
        && nonempty(&v.parse_status)
        && if matches!(v.group_type, 4 | 5) {
            v.private_data_hex.is_empty()
        } else {
            v.other_network_events.is_empty()
        }
}
fn valid_linkage(v: &LinkageV1) -> bool {
    in_u16(v.original_network_id)
        && in_u16(v.transport_stream_id)
        && in_u16(v.service_id)
        && (0..=255).contains(&v.linkage_type)
        && nonempty(&v.parse_status)
}
fn valid_free_ca_mode(v: &FreeCaModeV1) -> bool {
    (0..=1).contains(&v.raw) && nonempty(&v.parse_status)
}
fn valid_extended_item(v: &ExtendedItemV1) -> bool {
    valid_iso639(&v.language_code)
        && (nonempty(&v.description) || nonempty(&v.text))
        && nonempty(&v.parse_status)
}
fn valid_diagnostic_item(v: &DiagnosticItemV1) -> bool {
    nonempty(&v.code) && nonempty(&v.message)
}
fn valid_optional_u8(v: Option<i64>) -> bool {
    v.map(|value| (0..=255).contains(&value)).unwrap_or(true)
}
fn valid_optional_u16(v: Option<i64>) -> bool {
    v.map(in_u16).unwrap_or(true)
}
fn valid_optional_iso639(v: &Option<String>) -> bool {
    v.as_deref().map(valid_iso639).unwrap_or(true)
}
fn valid_components(v: &ComponentsV1) -> bool {
    v.video.iter().all(valid_video_component)
        && v.audio.iter().all(valid_audio_component)
        && v.subtitle.iter().all(valid_subtitle_component)
        && v.data.iter().all(valid_data_component)
}
fn valid_video_component(v: &VideoComponentV1) -> bool {
    v.es_pid > 0
        && v.es_pid <= 8191
        && (0..=255).contains(&v.stream_type)
        && valid_optional_u8(v.component_tag)
        && valid_optional_u8(v.component_type)
        && nonempty(&v.codec)
        && nonempty(&v.parse_status)
}
fn valid_audio_component(v: &AudioComponentV1) -> bool {
    v.es_pid > 0
        && v.es_pid <= 8191
        && (0..=255).contains(&v.stream_type)
        && valid_optional_u8(v.component_tag)
        && valid_optional_u8(v.component_type)
        && nonempty(&v.codec)
        && valid_optional_iso639(&v.language)
        && valid_optional_iso639(&v.second_language)
        && nonempty(&v.parse_status)
}
fn valid_subtitle_component(v: &SubtitleComponentV1) -> bool {
    v.es_pid > 0
        && v.es_pid <= 8191
        && valid_optional_u8(v.component_tag)
        && valid_optional_u16(v.data_component_id)
        && valid_optional_iso639(&v.language)
        && nonempty(&v.caption_service_kind)
        && nonempty(&v.parse_status)
}
fn valid_data_component(v: &DataComponentV1) -> bool {
    v.es_pid > 0
        && v.es_pid <= 8191
        && valid_optional_u8(v.component_tag)
        && valid_optional_u16(v.data_component_id)
        && valid_optional_u8(v.component_type)
        && nonempty(&v.parse_status)
}

fn failure_result(code: &str, message: String, schema_version: i64) -> ProviderDataResult {
    ProviderDataResult {
        success: false,
        json: String::new(),
        schema_version,
        truncated: false,
        diagnostics_dropped_count: 0,
        error_code: code.to_string(),
        error_message: message,
    }
}

fn success_result(
    json: String,
    schema_version: i64,
    truncated: bool,
    diagnostics_dropped_count: i64,
) -> ProviderDataResult {
    ProviderDataResult {
        success: true,
        json,
        schema_version,
        truncated,
        diagnostics_dropped_count,
        error_code: String::new(),
        error_message: String::new(),
    }
}

fn finalize_program(mut data: ProgramProviderDataV1) -> ProviderDataResult {
    if !valid_program_provider_data(&data) {
        return failure_result(
            "PROGRAM_PROVIDER_DATA_INVALID",
            "Program provider-data JSON v1 invariants failed".to_string(),
            PROVIDER_SCHEMA_VERSION,
        );
    }
    let text = match serde_json::to_string(&data) {
        Ok(text) => text,
        Err(err) => {
            return failure_result(
                "PROGRAM_PROVIDER_DATA_SERIALIZE_FAILED",
                format!("Program provider-data serialization failed: {err}"),
                PROVIDER_SCHEMA_VERSION,
            )
        }
    };
    if text.len() <= HARD_LIMIT_BYTES {
        return success_result(text, PROVIDER_SCHEMA_VERSION, false, 0);
    }

    let original_bytes = text.len() as i64;
    data.diagnostics.provider_data_truncated = Some(true);
    data.diagnostics.provider_data_hard_limit_bytes = Some(HARD_LIMIT_BYTES as i64);
    data.diagnostics.provider_data_soft_limit_bytes = Some(SOFT_LIMIT_BYTES as i64);
    data.diagnostics.provider_data_original_bytes = Some(original_bytes);
    data.diagnostics.provider_data_truncation_code = Some("PROVIDER_DATA_TRUNCATED".to_string());
    let mut counts = BTreeMap::new();

    loop {
        let encoded = match serialize_program_with_truncation_metadata(&mut data, &counts) {
            Ok(text) => text,
            Err(err) => {
                return failure_result(
                    "PROGRAM_PROVIDER_DATA_SERIALIZE_FAILED",
                    format!("Truncated program provider-data serialization failed: {err}"),
                    PROVIDER_SCHEMA_VERSION,
                )
            }
        };
        if encoded.len() <= HARD_LIMIT_BYTES {
            return success_result(
                encoded,
                PROVIDER_SCHEMA_VERSION,
                true,
                total_dropped(&counts),
            );
        }

        if data
            .diagnostics
            .raw_provider_data_extensions
            .pop()
            .is_some()
        {
            note_drop(&mut counts, "rawProviderDataExtensions", 1);
            continue;
        }
        if data.diagnostics.descriptor_diagnostics.pop().is_some() {
            note_drop(&mut counts, "descriptorDiagnostics", 1);
            continue;
        }
        if data.diagnostics.publish_diagnostics.pop().is_some() {
            note_drop(&mut counts, "publishDiagnostics", 1);
            continue;
        }
        if data.extended_items.pop().is_some() {
            note_drop(&mut counts, "extendedItems", 1);
            continue;
        }
        let removed =
            shorten_program_long_text(&mut data, encoded.len().saturating_sub(HARD_LIMIT_BYTES));
        if removed > 0 {
            note_drop(&mut counts, "longTextUtf8Bytes", removed as i64);
            continue;
        }
        return failure_result(
            "PROGRAM_PROVIDER_DATA_HARD_LIMIT_EXCEEDED",
            format!(
                "Program provider-data cannot be reduced below {} bytes without dropping protected semantic fields (current={} bytes)",
                HARD_LIMIT_BYTES,
                encoded.len()
            ),
            PROVIDER_SCHEMA_VERSION,
        );
    }
}

fn finalize_channel(mut data: ChannelProviderDataV1) -> ProviderDataResult {
    if !valid_channel_provider_data(&data) {
        return failure_result(
            "CHANNEL_PROVIDER_DATA_INVALID",
            "Channel provider-data JSON v1 invariants failed".to_string(),
            CHANNEL_SCHEMA_VERSION,
        );
    }
    let text = match serde_json::to_string(&data) {
        Ok(text) => text,
        Err(err) => {
            return failure_result(
                "CHANNEL_PROVIDER_DATA_SERIALIZE_FAILED",
                format!("Channel provider-data serialization failed: {err}"),
                CHANNEL_SCHEMA_VERSION,
            )
        }
    };
    if text.len() <= HARD_LIMIT_BYTES {
        return success_result(text, CHANNEL_SCHEMA_VERSION, false, 0);
    }

    let original_bytes = text.len() as i64;
    data.diagnostics.provider_data_truncated = Some(true);
    data.diagnostics.provider_data_hard_limit_bytes = Some(HARD_LIMIT_BYTES as i64);
    data.diagnostics.provider_data_soft_limit_bytes = Some(SOFT_LIMIT_BYTES as i64);
    data.diagnostics.provider_data_original_bytes = Some(original_bytes);
    data.diagnostics.provider_data_truncation_code = Some("PROVIDER_DATA_TRUNCATED".to_string());
    let mut counts = BTreeMap::new();

    loop {
        let encoded = match serialize_channel_with_truncation_metadata(&mut data, &counts) {
            Ok(text) => text,
            Err(err) => {
                return failure_result(
                    "CHANNEL_PROVIDER_DATA_SERIALIZE_FAILED",
                    format!("Truncated channel provider-data serialization failed: {err}"),
                    CHANNEL_SCHEMA_VERSION,
                )
            }
        };
        if encoded.len() <= HARD_LIMIT_BYTES {
            return success_result(
                encoded,
                CHANNEL_SCHEMA_VERSION,
                true,
                total_dropped(&counts),
            );
        }
        if data
            .diagnostics
            .raw_provider_data_extensions
            .pop()
            .is_some()
        {
            note_drop(&mut counts, "rawProviderDataExtensions", 1);
            continue;
        }
        return failure_result(
            "CHANNEL_PROVIDER_DATA_HARD_LIMIT_EXCEEDED",
            format!(
                "Channel provider-data cannot be reduced below {} bytes without dropping protected tune or CAS fields (current={} bytes)",
                HARD_LIMIT_BYTES,
                encoded.len()
            ),
            CHANNEL_SCHEMA_VERSION,
        );
    }
}

#[cfg(test)]
mod provider_data_tests {
    use super::*;

    fn minimal_channel_request(extra_top_level: &str, stream_id: i64) -> String {
        format!(
            r#"{{
            "schema":"maleicacid.tv.channelRequest",
            "schemaVersion":1,
            "serviceKey":{{"originalNetworkId":4,"transportStreamId":16400,"serviceId":101}},
            "tune":{{"deliverySystem":"ISDB_T","frequencyHz":473142857,"streamId":{},"streamIdType":"TSID","physicalChannel":13,"satelliteBand":null,"remoteControlKeyId":1}},
            "cas":{{"requiresCas":false}},
            "diagnostics":{{}}
            {}
        }}"#,
            stream_id, extra_top_level
        )
    }

    fn minimal_program_json(extra_top_level: &str) -> String {
        format!(
            r#"{{
            "schema":"maleicacid.tv.program",
            "schemaVersion":1,
            "programKey":{{"kind":"arib-event-v1","originalNetworkId":4,"transportStreamId":16400,"serviceId":101,"eventId":12345}},
            "timing":{{"startUtcMillis":1730000000000,"durationMillis":1800000}},
            "source":{{"pid":18,"tableId":78,"version":12,"sectionNumber":0,"lastSectionNumber":1}},
            "cas":{{"requiresCas":false,"source":"SI_SEMANTICS"}},
            "ratings":[],
            "genres":[],
            "series":null,
            "eventGroups":[],
            "linkage":[],
            "freeCaMode":{{"raw":0,"scrambled":false,"parseStatus":"OK"}},
            "extendedItems":[],
            "components":{{"video":[],"audio":[],"subtitle":[],"data":[]}},
            "diagnostics":{{"descriptorDiagnostics":[],"publishDiagnostics":[],"parserDiagnostics":[]}}
            {}
        }}"#,
            extra_top_level
        )
    }

    fn minimal_program_request_value() -> serde_json::Value {
        let mut value =
            serde_json::from_str::<serde_json::Value>(&minimal_program_json("")).unwrap();
        value["schema"] = serde_json::json!("maleicacid.tv.programRequest");
        value["diagnostics"] = serde_json::json!({
            "descriptorDiagnosticsCanonicalJson": "[]",
            "publishDiagnostics": [],
            "parserDiagnostics": [],
        });
        value["malformedCaDescriptorCount"] = serde_json::json!(0);
        value
    }

    #[test]
    fn production_builder_output_matches_schema_validated_fixture() {
        let mut request = minimal_program_request_value();
        request["genres"] = serde_json::json!([{
            "level1": 0,
            "level2": 0,
            "userNibble": 0,
            "aribName": "ニュース／報道",
            "parseStatus": "OK",
        }]);

        let result = build_program_provider_data(&request.to_string());
        assert!(result.success, "{}", result.error_message);
        let actual: serde_json::Value = serde_json::from_str(&result.json).unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../testdata/program_provider_data_v1/minimal_clear_program.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn program_request_rejects_derived_free_ca_text() {
        let mut request = minimal_program_request_value();
        request["freeCaMode"]["text"] = serde_json::json!("無料放送");

        let result = build_program_provider_data(&request.to_string());
        assert!(!result.success);
        assert_eq!(result.error_code, "PROGRAM_REQUEST_PARSE_FAILED");
    }

    #[test]
    fn series_expire_date_requires_matching_validity_flag() {
        let mut request = minimal_program_request_value();
        request["series"] = serde_json::json!({
            "seriesId": 0x1234,
            "repeatLabel": 2,
            "programPattern": 3,
            "expireDateValid": true,
            "expireDate": 0xe123,
            "episodeNumber": 3,
            "lastEpisodeNumber": 12,
            "name": "シリーズ",
            "parseStatus": "OK",
        });

        let valid = build_program_provider_data(&request.to_string());
        assert!(valid.success, "{}", valid.error_message);
        let canonical: serde_json::Value = serde_json::from_str(&valid.json).unwrap();
        assert_eq!(canonical["series"]["expireDate"], 0xe123);

        request["series"]["expireDateValid"] = serde_json::json!(false);
        let invalid = build_program_provider_data(&request.to_string());
        assert!(!invalid.success);
        assert_eq!(invalid.error_code, "PROGRAM_REQUEST_INVALID");
    }

    #[test]
    fn extended_event_item_allows_empty_description_when_text_exists() {
        let item = ExtendedItemV1 {
            language_code: "jpn".to_string(),
            description: String::new(),
            text: "本文".to_string(),
            parse_status: "OK".to_string(),
        };
        assert!(valid_extended_item(&item));
    }

    #[test]
    fn normalize_program_provider_data_preserves_top_level_unknown_key() {
        let result = normalize_program_provider_data(
            minimal_program_json(",\"futureVendorKey\":{\"x\":1}").as_bytes(),
        );
        assert!(result.success, "{}", result.error_message);
        let value: serde_json::Value = serde_json::from_str(&result.json).unwrap();
        let extensions = value["diagnostics"]["rawProviderDataExtensions"]
            .as_array()
            .unwrap();
        assert_eq!(extensions[0]["key"], "futureVendorKey");
        assert_eq!(extensions[0]["value"]["x"], 1);
        assert!(value.get("futureVendorKey").is_none());
    }

    #[test]
    fn normalize_program_provider_data_migrates_legacy_duplicate_fields() {
        let mut stored: serde_json::Value =
            serde_json::from_str(&minimal_program_json("")).unwrap();
        stored["serviceKey"] = serde_json::json!({
            "originalNetworkId": 4,
            "transportStreamId": 16400,
            "serviceId": 101,
        });
        stored["timing"]["endUtcMillis"] = serde_json::json!(1_730_001_800_000_i64);
        stored["audioLanguages"] = serde_json::json!([]);

        let result = normalize_program_provider_data(stored.to_string().as_bytes());
        assert!(result.success, "{}", result.error_message);
        let canonical: serde_json::Value = serde_json::from_str(&result.json).unwrap();
        assert!(canonical.get("serviceKey").is_none());
        assert!(canonical.get("audioLanguages").is_none());
        assert!(canonical["timing"].get("endUtcMillis").is_none());
    }

    #[test]
    fn normalize_program_provider_data_rejects_nested_policy_extensions() {
        let mut stored: serde_json::Value =
            serde_json::from_str(&minimal_program_json("")).unwrap();
        stored["cas"]["unsupportedCas"] = serde_json::json!(true);
        let result = normalize_program_provider_data(stored.to_string().as_bytes());
        assert!(!result.success);
        assert_eq!(result.error_code, "PROGRAM_PROVIDER_DATA_SCHEMA_FAILED");
    }

    #[test]
    fn program_request_rejects_nested_unknown_fields_and_preserves_event_group_shape() {
        let mut request = minimal_program_request_value();
        request["eventGroups"] = serde_json::json!([
              {
        "groupType": 2,
        "events": [{"serviceId": 102, "eventId": 456}],
        "otherNetworkEvents": [],
        "privateDataHex": "dead",
        "parseStatus": "OK"
              },
              {
        "groupType": 4,
        "events": [{"serviceId": 103, "eventId": 457}],
        "otherNetworkEvents": [{
            "originalNetworkId": 6,
            "transportStreamId": 16500,
            "serviceId": 104,
            "eventId": 458
        }],
        "privateDataHex": "",
        "parseStatus": "OK"
              }
          ]);
        let valid = build_program_provider_data(&request.to_string());
        assert!(valid.success, "{}", valid.error_message);
        let canonical: serde_json::Value = serde_json::from_str(&valid.json).unwrap();
        assert!(canonical.get("skippedUnresolvedTransport").is_none());
        assert!(canonical.get("relatedItems").is_none());
        assert_eq!(canonical["eventGroups"][0]["events"][0]["serviceId"], 102);
        assert_eq!(canonical["eventGroups"][0]["privateDataHex"], "dead");
        assert_eq!(
            canonical["eventGroups"][1]["otherNetworkEvents"][0]["originalNetworkId"],
            6
        );

        let mut invalid = minimal_program_request_value();
        invalid["eventGroups"] = serde_json::json!([{
              "groupType": 2,
              "events": [{"serviceId": 102, "eventId": 456}],
              "otherNetworkEvents": [{
        "originalNetworkId": 6,
        "transportStreamId": 16500,
        "serviceId": 104,
        "eventId": 458
              }],
              "privateDataHex": "",
              "parseStatus": "OK"
          }]);
        let rejected = build_program_provider_data(&invalid.to_string());
        assert!(!rejected.success);
        assert_eq!(rejected.error_code, "PROGRAM_REQUEST_INVALID");

        let mut legacy = minimal_program_request_value();
        legacy["skippedUnresolvedTransport"] = serde_json::json!(false);
        let rejected = build_program_provider_data(&legacy.to_string());
        assert!(!rejected.success);
        assert_eq!(rejected.error_code, "PROGRAM_REQUEST_PARSE_FAILED");
    }

    #[test]
    fn generated_program_key_matches_key_extracted_from_provider_data() {
        let result = build_program_provider_data(&minimal_program_request_value().to_string());
        assert!(result.success, "{}", result.error_message);
        assert_eq!(
            build_program_key(4, 16400, 101, 12345),
            extract_program_key(result.json.as_bytes()).unwrap()
        );
    }

    #[test]
    fn program_request_preserves_combined_user_nibble_byte() {
        let mut request = minimal_program_request_value();
        request["genres"] = serde_json::json!([{
            "level1": 1,
            "level2": 2,
            "userNibble": 0x34,
            "aribName": "試験分類",
            "parseStatus": "OK",
        }]);

        let result = build_program_provider_data(&request.to_string());
        assert!(result.success, "{}", result.error_message);
        let canonical: serde_json::Value = serde_json::from_str(&result.json).unwrap();
        assert_eq!(canonical["genres"][0]["userNibble"], 0x34);
    }

    #[test]
    fn oversized_program_provider_data_records_truncation_diagnostic() {
        let mut data: ProgramProviderDataV1 =
            serde_json::from_str(&minimal_program_json("")).unwrap();
        data.extended_items = (0..200)
            .map(|i| ExtendedItemV1 {
                language_code: "jpn".to_string(),
                description: String::new(),
                text: format!("{}{}", i, "x".repeat(512)),
                parse_status: "OK".to_string(),
            })
            .collect();
        let result = finalize_program(data);
        assert!(result.success, "{}", result.error_message);
        assert!(result.json.len() <= HARD_LIMIT_BYTES);
        let value: serde_json::Value = serde_json::from_str(&result.json).unwrap();
        assert_eq!(value["diagnostics"]["providerDataTruncated"], true);
        assert_eq!(
            value["diagnostics"]["providerDataTruncationCode"],
            "PROVIDER_DATA_TRUNCATED"
        );
        assert!(
            value["diagnostics"]["providerDataOriginalBytes"]
                .as_i64()
                .unwrap()
                > HARD_LIMIT_BYTES as i64
        );
        assert_eq!(
            value["diagnostics"]["providerDataFinalBytes"],
            result.json.len() as i64
        );
        assert!(
            value["diagnostics"]["providerDataDroppedCounts"]["extendedItems"]
                .as_i64()
                .unwrap()
                > 0
        );
        assert!(
            value["diagnostics"]["providerDataDroppedCount"]
                .as_i64()
                .unwrap()
                > 0
        );
    }

    #[test]
    fn invalid_program_provider_data_returns_failure_not_empty_json() {
        let result = build_program_provider_data("{not-json");
        assert!(!result.success);
        assert!(result.json.is_empty());
        assert_eq!(result.error_code, "PROGRAM_REQUEST_PARSE_FAILED");
    }

    #[test]
    fn channel_request_rejects_unknown_and_reserved_stream_id() {
        let unknown =
            build_channel_provider_data(&minimal_channel_request(",\"inputId\":\"legacy\"", 1));
        assert!(!unknown.success);
        assert_eq!(unknown.error_code, "CHANNEL_REQUEST_PARSE_FAILED");

        let reserved = build_channel_provider_data(&minimal_channel_request("", 65_535));
        assert!(!reserved.success);
        assert_eq!(reserved.error_code, "CHANNEL_REQUEST_INVALID");
    }

    #[test]
    fn channel_decode_returns_canonical_bytes_and_typed_tune_only() {
        let built = build_channel_provider_data(&minimal_channel_request("", 16_400));
        assert!(built.success, "{}", built.error_message);
        let decoded = decode_channel_provider_data(built.json.as_bytes());
        let value: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["serviceKey"]["originalNetworkId"], 4);
        assert_eq!(value["serviceKey"]["transportStreamId"], 16400);
        assert_eq!(value["serviceKey"]["serviceId"], 101);
        assert_eq!(value["tune"]["streamId"], 16400);
        assert_eq!(value["tune"]["streamIdType"], "TSID");
        let canonical: serde_json::Value =
            serde_json::from_str(value["canonical"].as_str().unwrap()).unwrap();
        assert_eq!(canonical["cas"]["requiresCas"], false);
        assert!(decode_channel_provider_data(&[0xff, 0xfe]).is_empty());
    }

    #[test]
    fn channel_decode_migrates_legacy_display_name() {
        let built = build_channel_provider_data(&minimal_channel_request("", 16_400));
        assert!(built.success, "{}", built.error_message);
        let mut stored: serde_json::Value = serde_json::from_str(&built.json).unwrap();
        stored["tune"]["displayName"] = serde_json::json!("legacy duplicate");

        let decoded = decode_channel_provider_data(stored.to_string().as_bytes());
        let value: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        let canonical: serde_json::Value =
            serde_json::from_str(value["canonical"].as_str().unwrap()).unwrap();
        assert!(canonical["tune"].get("displayName").is_none());
    }

    #[test]
    fn channel_decode_rejects_nested_unknown_fields() {
        let built = build_channel_provider_data(&minimal_channel_request("", 16_400));
        assert!(built.success, "{}", built.error_message);
        let mut stored: serde_json::Value = serde_json::from_str(&built.json).unwrap();
        stored["tune"]["futureSelector"] = serde_json::json!({"kind": "future"});

        assert!(decode_channel_provider_data(stored.to_string().as_bytes()).is_empty());
    }

    #[test]
    fn channel_decode_drops_forbidden_legacy_policy_extensions() {
        let built = build_channel_provider_data(&minimal_channel_request("", 16_400));
        assert!(built.success, "{}", built.error_message);
        let mut stored: serde_json::Value = serde_json::from_str(&built.json).unwrap();
        stored["cas"]["unsupportedCas"] = serde_json::json!(true);
        assert!(decode_channel_provider_data(stored.to_string().as_bytes()).is_empty());
    }
}

#[cfg(test)]
mod nullable_component_fact_tests {
    use super::*;

    #[test]
    fn component_descriptor_absence_is_a_valid_fact() {
        assert!(valid_video_component(&VideoComponentV1 {
            es_pid: 0x120,
            stream_type: 0x24,
            component_tag: None,
            component_type: None,
            codec: "HEVC".to_string(),
            resolution: None,
            scan: None,
            aspect: None,
            profile_level: None,
            source_descriptor: None,
            parse_status: "OK".to_string(),
        }));
        assert!(valid_audio_component(&AudioComponentV1 {
            es_pid: 0x110,
            stream_type: 0x0f,
            component_tag: None,
            component_type: None,
            codec: "AAC".to_string(),
            language: None,
            second_language: None,
            channel_configuration: None,
            sampling_info: None,
            source_descriptor: None,
            parse_status: "OK".to_string(),
        }));
    }
}
