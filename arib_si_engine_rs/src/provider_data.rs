use serde::{Deserialize, Serialize};
use serde_json::Value;

const PROVIDER_SCHEMA_VERSION: i64 = 1;
const PROGRAM_SCHEMA_NAME: &str = "maleicacid.tv.program";
const CHANNEL_SCHEMA_NAME: &str = "maleicacid.tv.channel";
const CHANNEL_SCHEMA_VERSION: i64 = 1;
const SOFT_LIMIT_BYTES: usize = 16 * 1024;
const HARD_LIMIT_BYTES: usize = 32 * 1024;

const PROGRAM_KNOWN_TOP_LEVEL_KEYS: &[&str] = &[
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
    "relatedItems",
    "linkage",
    "freeCaMode",
    "audioLanguages",
    "audio",
    "video",
    "extendedItems",
    "components",
    "diagnostics",
    "descriptorDiagnostics",
    "parserDiagnostics",
    "publishDiagnostics",
    "parentalRatings",
    "audioLanguage",
    "eventId",
    "originalNetworkId",
    "transportStreamId",
    "serviceId",
    "startTimeMillis",
    "endUtcMillis",
    "durationMillis",
];

const CHANNEL_KNOWN_TOP_LEVEL_KEYS: &[&str] = &[
    "schema",
    "schemaVersion",
    "serviceKey",
    "tune",
    "cas",
    "diagnostics",
    "originalNetworkId",
    "transportStreamId",
    "serviceId",
    "inputId",
    "displayName",
    "system",
    "frequencyHz",
    "streamSelector",
    "streamSelectorType",
    "streamSelectorValue",
    "physicalChannel",
    "backendHint",
    "satelliteBand",
    "remoteControlKeyId",
    "requiresCas",
    "unsupportedCas",
    "clearLivePlaybackSupported",
    "channelRegistrationReady",
    "epgPublishable",
    "publishStateSource",
];


#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDataResult {
    pub json: String,
    pub signature: String,
    pub extracted_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramKeyResult {
    pub original_network_id: i64,
    pub transport_stream_id: i64,
    pub service_id: i64,
    pub event_id: i64,
    pub key: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProgramIdentity {
    onid: i64,
    tsid: i64,
    sid: i64,
    event_id: i64,
    start_utc_millis: i64,
    duration_millis: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CurrentProgramDiagnostics {
    overlap_count: i64,
    selected_program_id: i64,
    selection_rule: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgramKeyV1 {
    kind: String,
    original_network_id: i64,
    transport_stream_id: i64,
    service_id: i64,
    event_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceKeyV1 {
    original_network_id: i64,
    transport_stream_id: i64,
    service_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimingV1 {
    start_utc_millis: i64,
    end_utc_millis: i64,
    duration_millis: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceV1 {
    pid: i64,
    table_id: i64,
    version: i64,
    section_number: i64,
    last_section_number: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CasV1 {
    requires_cas: bool,
    unsupported_cas: bool,
    clear_live_playback_supported: bool,
    source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RatingV1 {
    country_code: String,
    rating_value: i64,
    raw_rating_byte: i64,
    supported: bool,
    mapped_tv_content_rating: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenreV1 {
    level1: i64,
    level2: i64,
    user_nibble: i64,
    arib_name: String,
    unmapped_reason: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeriesV1 {
    series_id: i64,
    repeat_label: i64,
    program_pattern: i64,
    expire_date_valid: bool,
    expire_date: Option<String>,
    episode_number: i64,
    last_episode_number: i64,
    name: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FreeCaModeV1 {
    raw: i64,
    scrambled: bool,
    text: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioLanguageV1 {
    language: String,
    source: String,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioMetadataV1 {
    es_pid: Option<i64>,
    component_tag: Option<i64>,
    codec: String,
    language: Option<String>,
    text: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VideoMetadataV1 {
    es_pid: Option<i64>,
    component_tag: Option<i64>,
    codec: String,
    format: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtendedItemV1 {
    description: String,
    text: String,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelatedItemV1 {
    kind: String,
    group_type: i64,
    original_network_id: i64,
    transport_stream_id: i64,
    service_id: i64,
    event_id: i64,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
struct SectionScopeV1 {
    pid: Option<i64>,
    table_id: Option<i64>,
    table_id_extension: Option<i64>,
    version: Option<i64>,
    section_number: Option<i64>,
    original_network_id: Option<i64>,
    transport_stream_id: Option<i64>,
    service_id: Option<i64>,
    event_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DescriptorScopeV1 {
    tag: i64,
    name: Option<String>,
    offset: i64,
    declared_length: i64,
    actual_remaining_length: i64,
    parse_status: Option<String>,
    raw_prefix_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DescriptorDiagnosticV1 {
    schema: String,
    schema_version: i64,
    severity: String,
    code: String,
    scope: SectionScopeV1,
    descriptor: DescriptorScopeV1,
    message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VideoComponentV1 {
    es_pid: i64,
    stream_type: i64,
    component_tag: i64,
    component_type: i64,
    codec: String,
    resolution: Option<String>,
    scan: Option<String>,
    aspect: Option<String>,
    profile_level: Option<String>,
    source_descriptor: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioComponentV1 {
    es_pid: i64,
    stream_type: i64,
    component_tag: i64,
    component_type: i64,
    codec: String,
    language: String,
    channel_configuration: Option<String>,
    sampling_info: Option<String>,
    source_descriptor: Option<String>,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleComponentV1 {
    es_pid: i64,
    component_tag: i64,
    data_component_id: i64,
    language: String,
    track_id: String,
    caption_service_kind: String,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataComponentV1 {
    es_pid: i64,
    component_tag: i64,
    data_component_id: i64,
    component_type: i64,
    parse_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ComponentsV1 {
    video: Vec<VideoComponentV1>,
    audio: Vec<AudioComponentV1>,
    subtitle: Vec<SubtitleComponentV1>,
    data: Vec<DataComponentV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticItemV1 {
    code: String,
    message: String,
    severity: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProviderDataExtensionV1 {
    key: String,
    value: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentProgramDiagnosticsV1 {
    overlap_count: i64,
    selected_program_id: i64,
    selection_rule: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DiagnosticsV1 {
    descriptor_diagnostics: Vec<DescriptorDiagnosticV1>,
    publish_diagnostics: Vec<DiagnosticItemV1>,
    parser_diagnostics: Vec<DiagnosticItemV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_program: Option<CurrentProgramDiagnosticsV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_provider_data_extensions: Vec<RawProviderDataExtensionV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_hard_limit_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_soft_limit_bytes: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgramProviderDataV1 {
    schema: String,
    schema_version: i64,
    program_key: ProgramKeyV1,
    service_key: ServiceKeyV1,
    timing: TimingV1,
    source: SourceV1,
    cas: CasV1,
    ratings: Vec<RatingV1>,
    genres: Vec<GenreV1>,
    series: Option<SeriesV1>,
    related_items: Vec<RelatedItemV1>,
    linkage: Vec<LinkageV1>,
    free_ca_mode: Option<FreeCaModeV1>,
    audio_languages: Vec<AudioLanguageV1>,
    audio: Option<AudioMetadataV1>,
    video: Option<VideoMetadataV1>,
    extended_items: Vec<ExtendedItemV1>,
    components: ComponentsV1,
    diagnostics: DiagnosticsV1,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamSelectorV1 {
    r#type: String,
    value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelTuneV1 {
    input_id: Option<String>,
    display_name: Option<String>,
    system: String,
    frequency_hz: i64,
    stream_selector: StreamSelectorV1,
    physical_channel: Option<i64>,
    backend_hint: Option<String>,
    satellite_band: Option<String>,
    remote_control_key_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelCasV1 {
    requires_cas: bool,
    unsupported_cas: bool,
    clear_live_playback_supported: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelDiagnosticsV1 {
    channel_registration_ready: bool,
    epg_publishable: bool,
    publish_state_source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_provider_data_extensions: Vec<RawProviderDataExtensionV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_hard_limit_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_soft_limit_bytes: Option<i64>,
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
}

pub fn build_program_key(onid: i32, tsid: i32, sid: i32, event_id: i32) -> String {
    format!("onid={};tsid={};sid={};event={}", onid, tsid, sid, event_id)
}

pub fn build_program_provider_data(request_json: &str) -> ProviderDataResult {
    let value = parse_input(request_json);
    let identity = ProgramIdentity::from_value(&value);
    finalize_program(program_from_value(&value, identity, None))
}

pub fn build_channel_provider_data(request_json: &str) -> ProviderDataResult {
    let value = parse_input(request_json);
    finalize_channel(channel_from_value(&value))
}

pub fn normalize_program_provider_data(provider_data: &str) -> ProviderDataResult {
    let value = parse_input(provider_data.trim());
    let identity = ProgramIdentity::from_value_or_legacy(&value, provider_data);
    finalize_program(program_from_value(&value, identity, None))
}

pub fn append_current_program_diagnostics(provider_data: &str, overlap_count: i64, selected_program_id: i64, selection_rule: &str) -> ProviderDataResult {
    let value = parse_input(provider_data.trim());
    let identity = ProgramIdentity::from_value_or_legacy(&value, provider_data);
    let current = CurrentProgramDiagnostics {
        overlap_count: overlap_count.max(0),
        selected_program_id,
        selection_rule: selection_rule.to_string(),
    };
    finalize_program(program_from_value(&value, identity, Some(current)))
}

pub fn program_provider_data_signature(provider_data: &str) -> String {
    sha256_hex(provider_data.as_bytes())
}

pub fn extract_program_key_result(provider_data: &str) -> Option<ProgramKeyResult> {
    let raw = provider_data.trim();
    if raw.is_empty() { return None; }
    let identity = parse_legacy_key(raw).or_else(|| {
        let value = parse_input(raw);
        ProgramIdentity::from_value_optional(&value).or_else(|| {
            value.get("programKey")
                .and_then(Value::as_str)
                .and_then(parse_legacy_key)
        })
    })?;
    Some(ProgramKeyResult {
        original_network_id: identity.onid,
        transport_stream_id: identity.tsid,
        service_id: identity.sid,
        event_id: identity.event_id,
        key: identity.legacy_key(),
    })
}

pub fn extract_program_key(provider_data: &str) -> Option<String> {
    extract_program_key_result(provider_data).map(|v| v.key)
}

pub fn extract_channel_tune_key(provider_data: &str) -> String {
    let raw = provider_data.trim();
    if raw.is_empty() { return String::new(); }
    let value = parse_input(raw);
    let data = channel_from_value(&value);
    format!(
        "originalNetworkId={};transportStreamId={};serviceId={};system={};frequencyHz={};streamSelectorType={};streamSelectorValue={};physicalChannel={};backendHint={};satelliteBand={};remoteControlKeyId={};requiresCas={};unsupportedCas={};clearLivePlaybackSupported={};channelRegistrationReady={};epgPublishable={}",
        data.service_key.original_network_id,
        data.service_key.transport_stream_id,
        data.service_key.service_id,
        data.tune.system,
        data.tune.frequency_hz,
        data.tune.stream_selector.r#type,
        data.tune.stream_selector.value,
        data.tune.physical_channel.map(|v| v.to_string()).unwrap_or_default(),
        data.tune.backend_hint.unwrap_or_default(),
        data.tune.satellite_band.unwrap_or_default(),
        data.tune.remote_control_key_id.map(|v| v.to_string()).unwrap_or_default(),
        data.cas.requires_cas,
        data.cas.unsupported_cas,
        data.cas.clear_live_playback_supported,
        data.diagnostics.channel_registration_ready,
        data.diagnostics.epg_publishable,
    )
}

impl ProgramIdentity {
    fn from_value(value: &Value) -> Self {
        Self::from_value_optional(value).unwrap_or(Self {
            onid: 0,
            tsid: 0,
            sid: 0,
            event_id: 0,
            start_utc_millis: time_from(value),
            duration_millis: duration_from(value),
        }).clamped()
    }

    fn from_value_or_legacy(value: &Value, raw: &str) -> Self {
        if let Some(id) = Self::from_value_optional(value) { return id; }
        if let Some(id) = parse_legacy_key(raw) { return id; }
        Self::from_value(value)
    }

    fn from_value_optional(value: &Value) -> Option<Self> {
        if let Some(key) = value.get("programKey").and_then(Value::as_str) {
            if let Some(id) = parse_legacy_key(key) {
                return Some(Self { start_utc_millis: time_from(value), duration_millis: duration_from(value), ..id }.clamped());
            }
        }
        let program_key = value.get("programKey").and_then(Value::as_object);
        let service = value.get("serviceKey").unwrap_or(value);
        let timing = value.get("timing").unwrap_or(value);
        let onid = program_key.and_then(|obj| obj.get("originalNetworkId")).and_then(Value::as_i64)
            .unwrap_or_else(|| i64_field(service, "originalNetworkId", i64_field(value, "originalNetworkId", 0)));
        let tsid = program_key.and_then(|obj| obj.get("transportStreamId")).and_then(Value::as_i64)
            .unwrap_or_else(|| i64_field(service, "transportStreamId", i64_field(value, "transportStreamId", 0)));
        let sid = program_key.and_then(|obj| obj.get("serviceId")).and_then(Value::as_i64)
            .unwrap_or_else(|| i64_field(service, "serviceId", i64_field(value, "serviceId", 0)));
        let event_id = program_key.and_then(|obj| obj.get("eventId")).and_then(Value::as_i64)
            .unwrap_or_else(|| i64_field(value, "eventId", 0));
        if onid == 0 && tsid == 0 && sid == 0 && event_id == 0 { return None; }
        Some(Self {
            onid,
            tsid,
            sid,
            event_id,
            start_utc_millis: i64_field(timing, "startUtcMillis", time_from(value)),
            duration_millis: i64_field(timing, "durationMillis", duration_from(value)),
        }.clamped())
    }

    fn clamped(self) -> Self {
        Self {
            onid: clamp_i64(self.onid, 0, 0xffff),
            tsid: clamp_i64(self.tsid, 0, 0xffff),
            sid: clamp_i64(self.sid, 0, 0xffff),
            event_id: clamp_i64(self.event_id, 0, 0xffff),
            start_utc_millis: self.start_utc_millis.max(0),
            duration_millis: self.duration_millis.max(0),
        }
    }

    fn legacy_key(&self) -> String {
        format!("onid={};tsid={};sid={};event={}", self.onid, self.tsid, self.sid, self.event_id)
    }
}

fn program_from_value(value: &Value, identity: ProgramIdentity, current: Option<CurrentProgramDiagnostics>) -> ProgramProviderDataV1 {
    let diagnostics = diagnostics_from(value, current, PROGRAM_KNOWN_TOP_LEVEL_KEYS);
    let related_items = related_items_from(value);
    let linkage = linkage_from(value);
    let components = components_from(value);
    ProgramProviderDataV1 {
        schema: PROGRAM_SCHEMA_NAME.to_string(),
        schema_version: PROVIDER_SCHEMA_VERSION,
        program_key: ProgramKeyV1 {
            kind: "arib-event-v1".to_string(),
            original_network_id: identity.onid,
            transport_stream_id: identity.tsid,
            service_id: identity.sid,
            event_id: identity.event_id,
        },
        service_key: ServiceKeyV1 {
            original_network_id: identity.onid,
            transport_stream_id: identity.tsid,
            service_id: identity.sid,
        },
        timing: TimingV1 {
            start_utc_millis: identity.start_utc_millis,
            end_utc_millis: identity.start_utc_millis.saturating_add(identity.duration_millis),
            duration_millis: identity.duration_millis,
        },
        source: source_from(value),
        cas: cas_from(value),
        ratings: ratings_from(value),
        genres: genres_from(value),
        series: series_from(value),
        related_items,
        linkage,
        free_ca_mode: free_ca_mode_from(value),
        audio_languages: audio_languages_from(value),
        audio: audio_from(value),
        video: video_from(value),
        extended_items: extended_items_from(value),
        components,
        diagnostics,
    }
}

fn channel_from_value(value: &Value) -> ChannelProviderDataV1 {
    let service_source = value.get("serviceKey").unwrap_or(value);
    let tune_source = value.get("tune").unwrap_or(value);
    let selector_source = tune_source.get("streamSelector").unwrap_or(tune_source);
    let cas_source = value.get("cas").unwrap_or(value);
    let diag_source = value.get("diagnostics").unwrap_or(value);
    ChannelProviderDataV1 {
        schema: CHANNEL_SCHEMA_NAME.to_string(),
        schema_version: CHANNEL_SCHEMA_VERSION,
        service_key: ServiceKeyV1 {
            original_network_id: i64_field(service_source, "originalNetworkId", i64_field(value, "originalNetworkId", -1)),
            transport_stream_id: i64_field(service_source, "transportStreamId", i64_field(value, "transportStreamId", -1)),
            service_id: i64_field(service_source, "serviceId", i64_field(value, "serviceId", -1)),
        },
        tune: ChannelTuneV1 {
            input_id: string_opt(tune_source, "inputId"),
            display_name: string_opt(tune_source, "displayName"),
            system: string_field(tune_source, "system", ""),
            frequency_hz: i64_field(tune_source, "frequencyHz", 0),
            stream_selector: StreamSelectorV1 {
                r#type: string_field(selector_source, "type", string_field(tune_source, "streamSelectorType", "NONE")),
                value: string_field(selector_source, "value", string_field(tune_source, "streamSelectorValue", "")),
            },
            physical_channel: optional_i64(tune_source, "physicalChannel"),
            backend_hint: string_opt(tune_source, "backendHint"),
            satellite_band: string_opt(tune_source, "satelliteBand"),
            remote_control_key_id: optional_i64(tune_source, "remoteControlKeyId"),
        },
        cas: ChannelCasV1 {
            requires_cas: bool_field(cas_source, "requiresCas", false),
            unsupported_cas: bool_field(cas_source, "unsupportedCas", false),
            clear_live_playback_supported: bool_field(cas_source, "clearLivePlaybackSupported", false),
        },
        diagnostics: ChannelDiagnosticsV1 {
            channel_registration_ready: bool_field(diag_source, "channelRegistrationReady", bool_field(value, "channelRegistrationReady", false)),
            epg_publishable: bool_field(diag_source, "epgPublishable", bool_field(value, "epgPublishable", false)),
            publish_state_source: string_field(diag_source, "publishStateSource", string_field(value, "publishStateSource", "current")),
            raw_provider_data_extensions: raw_extensions_from(value, CHANNEL_KNOWN_TOP_LEVEL_KEYS),
            provider_data_truncated: None,
            provider_data_hard_limit_bytes: None,
            provider_data_soft_limit_bytes: None,
        },
    }
}

fn source_from(value: &Value) -> SourceV1 {
    let src = value.get("source").unwrap_or(value);
    SourceV1 {
        pid: clamp_i64(i64_field(src, "pid", 18), 0, 8191),
        table_id: clamp_i64(i64_field(src, "tableId", 78), 0, 255),
        version: clamp_i64(i64_field(src, "version", 0), 0, 31),
        section_number: clamp_i64(i64_field(src, "sectionNumber", 0), 0, 255),
        last_section_number: clamp_i64(i64_field(src, "lastSectionNumber", 0), 0, 255),
    }
}

fn cas_from(value: &Value) -> CasV1 {
    let cas = value.get("cas").unwrap_or(value);
    let requires_cas = bool_field(cas, "requiresCas", bool_field(value, "requiresCas", false));
    let unsupported_cas = bool_field(cas, "unsupportedCas", bool_field(value, "unsupportedCas", false));
    CasV1 {
        requires_cas,
        unsupported_cas,
        clear_live_playback_supported: bool_field(cas, "clearLivePlaybackSupported", !requires_cas && !unsupported_cas),
        source: string_field(cas, "source", string_field(value, "publishStateSource", "CURRENT_DIAGNOSTIC")),
    }
}

fn ratings_from(value: &Value) -> Vec<RatingV1> {
    let array = value.get("ratings").and_then(Value::as_array)
        .cloned()
        .or_else(|| value.get("parentalRatings").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    if !array.is_empty() {
        return array.into_iter().filter_map(|entry| {
            let obj = entry.as_object()?;
            Some(RatingV1 {
                country_code: object_string(obj, "countryCode", "JPN"),
                rating_value: clamp_i64(object_i64(obj, "ratingValue", object_i64(obj, "rating", 0)), 0, 255),
                raw_rating_byte: clamp_i64(object_i64(obj, "rawRatingByte", object_i64(obj, "rawRating", 0)), 0, 255),
                supported: object_bool(obj, "supported", false),
                mapped_tv_content_rating: obj.get("mappedTvContentRating").and_then(Value::as_str).map(ToString::to_string),
                parse_status: object_string(obj, "parseStatus", "OK"),
            })
        }).collect();
    }
    Vec::new()
}

fn genres_from(value: &Value) -> Vec<GenreV1> {
    if let Some(array) = value.get("genres").and_then(Value::as_array) {
        return array.iter().filter_map(|entry| {
            let obj = entry.as_object()?;
            Some(GenreV1 {
                level1: clamp_i64(object_i64(obj, "level1", 0), 0, 15),
                level2: clamp_i64(object_i64(obj, "level2", 0), 0, 15),
                user_nibble: clamp_i64(object_i64(obj, "userNibble", 0), 0, 15),
                arib_name: object_string(obj, "aribName", ""),
                unmapped_reason: obj.get("unmappedReason").and_then(Value::as_str).map(ToString::to_string),
                parse_status: object_string(obj, "parseStatus", "OK"),
            })
        }).collect();
    }
    Vec::new()
}

fn series_from(value: &Value) -> Option<SeriesV1> {
    let src = value.get("series")?;
    let obj = src.as_object()?;
    Some(SeriesV1 {
        series_id: clamp_i64(object_i64(obj, "seriesId", 0), 0, 0xffff),
        repeat_label: clamp_i64(object_i64(obj, "repeatLabel", 0), 0, 15),
        program_pattern: clamp_i64(object_i64(obj, "programPattern", 0), 0, 7),
        expire_date_valid: object_bool(obj, "expireDateValid", false),
        expire_date: obj.get("expireDate").and_then(Value::as_str).map(ToString::to_string),
        episode_number: clamp_i64(object_i64(obj, "episodeNumber", 0), 0, 4095),
        last_episode_number: clamp_i64(object_i64(obj, "lastEpisodeNumber", 0), 0, 4095),
        name: obj.get("name").and_then(Value::as_str).map(ToString::to_string),
        parse_status: object_string(obj, "parseStatus", "OK"),
    })
}

fn free_ca_mode_from(value: &Value) -> Option<FreeCaModeV1> {
    let src = value.get("freeCaMode")?;
    let obj = src.as_object()?;
    Some(FreeCaModeV1 {
        raw: clamp_i64(object_i64(obj, "raw", 0), 0, 1),
        scrambled: object_bool(obj, "scrambled", false),
        text: obj.get("text").and_then(Value::as_str).map(ToString::to_string),
        parse_status: object_string(obj, "parseStatus", "OK"),
    })
}

fn audio_languages_from(value: &Value) -> Vec<AudioLanguageV1> {
    if let Some(array) = value.get("audioLanguages").and_then(Value::as_array) {
        return array.iter().filter_map(|entry| {
            let obj = entry.as_object()?;
            Some(AudioLanguageV1 {
                language: object_string(obj, "language", "jpn"),
                source: object_string(obj, "source", "AUDIO_COMPONENT"),
                parse_status: object_string(obj, "parseStatus", "OK"),
            })
        }).collect();
    }
    if let Some(lang) = value.get("audioLanguage").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        return vec![AudioLanguageV1 {
            language: lang.to_string(),
            source: "AUDIO_COMPONENT".to_string(),
            parse_status: "OK".to_string(),
        }];
    }
    Vec::new()
}

fn audio_from(value: &Value) -> Option<AudioMetadataV1> {
    let src = value.get("audio")?.as_object()?;
    Some(AudioMetadataV1 {
        es_pid: src.get("esPid").and_then(Value::as_i64),
        component_tag: src.get("componentTag").and_then(Value::as_i64),
        codec: object_string(src, "codec", object_string(src, "format", "UNKNOWN_AUDIO")),
        language: src.get("language").and_then(Value::as_str).map(ToString::to_string),
        text: src.get("text").or_else(|| src.get("componentText")).and_then(Value::as_str).map(ToString::to_string),
        parse_status: object_string(src, "parseStatus", "OK"),
    })
}

fn video_from(value: &Value) -> Option<VideoMetadataV1> {
    let src = value.get("video")?.as_object()?;
    Some(VideoMetadataV1 {
        es_pid: src.get("esPid").and_then(Value::as_i64),
        component_tag: src.get("componentTag").and_then(Value::as_i64),
        codec: object_string(src, "codec", object_string(src, "format", object_string(src, "videoFormat", "UNKNOWN_VIDEO"))),
        format: src.get("format").or_else(|| src.get("videoFormat")).and_then(Value::as_str).map(ToString::to_string),
        width: src.get("width").and_then(Value::as_i64),
        height: src.get("height").and_then(Value::as_i64),
        parse_status: object_string(src, "parseStatus", "OK"),
    })
}

fn extended_items_from(value: &Value) -> Vec<ExtendedItemV1> {
    value.get("extendedItems").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(|entry| {
        let obj = entry.as_object()?;
        Some(ExtendedItemV1 {
            description: object_string(obj, "description", object_string(obj, "itemDescription", "")),
            text: object_string(obj, "text", object_string(obj, "itemText", "")),
            parse_status: object_string(obj, "parseStatus", "OK"),
        })
    }).collect()
}

fn diagnostics_from(value: &Value, current: Option<CurrentProgramDiagnostics>, known_keys: &[&str]) -> DiagnosticsV1 {
    let src = value.get("diagnostics").unwrap_or(value);
    let mut out = DiagnosticsV1::default();
    out.descriptor_diagnostics = descriptor_diagnostics_from(value, src);
    out.publish_diagnostics = publish_diagnostics_from(src);
    out.parser_diagnostics = parser_diagnostics_from(src);
    out.raw_provider_data_extensions = raw_extensions_from(value, known_keys);
    if let Some(current) = current {
        out.current_program = Some(CurrentProgramDiagnosticsV1 {
            overlap_count: current.overlap_count,
            selected_program_id: current.selected_program_id,
            selection_rule: current.selection_rule,
        });
    } else if let Some(existing) = src.get("currentProgram").and_then(Value::as_object) {
        out.current_program = Some(CurrentProgramDiagnosticsV1 {
            overlap_count: object_i64(existing, "overlapCount", 0),
            selected_program_id: object_i64(existing, "selectedProgramId", 0),
            selection_rule: object_string(existing, "selectionRule", "UNKNOWN"),
        });
    }
    out
}

fn descriptor_diagnostics_from(root: &Value, diagnostics: &Value) -> Vec<DescriptorDiagnosticV1> {
    let array = diagnostics.get("descriptorDiagnostics").and_then(Value::as_array).cloned()
        .or_else(|| root.get("descriptorDiagnostics").and_then(Value::as_array).cloned())
        .or_else(|| root.get("descriptorDiagnostics").and_then(Value::as_object).and_then(|obj| obj.get("diagnostics")).and_then(Value::as_array).cloned())
        .unwrap_or_default();
    array.into_iter().filter_map(|entry| descriptor_diagnostic_from_value(&entry, root)).collect()
}

fn publish_diagnostics_from(diagnostics: &Value) -> Vec<DiagnosticItemV1> {
    diagnostics.get("publishDiagnostics").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(|entry| {
        let obj = entry.as_object()?;
        Some(DiagnosticItemV1 {
            code: object_string(obj, "code", "UNKNOWN"),
            message: object_string(obj, "message", ""),
            severity: obj.get("severity").and_then(Value::as_str).map(ToString::to_string),
        })
    }).collect()
}

fn parser_diagnostics_from(diagnostics: &Value) -> Vec<DiagnosticItemV1> {
    diagnostics.get("parserDiagnostics").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(|entry| {
        let obj = entry.as_object()?;
        Some(DiagnosticItemV1 {
            code: object_string(obj, "code", "UNKNOWN"),
            message: object_string(obj, "message", ""),
            severity: obj.get("severity").and_then(Value::as_str).map(ToString::to_string),
        })
    }).collect()
}

fn raw_extensions_from(value: &Value, known_keys: &[&str]) -> Vec<RawProviderDataExtensionV1> {
    let Some(obj) = value.as_object() else { return Vec::new(); };
    obj.iter()
        .filter(|(k, _)| !known_keys.contains(&k.as_str()))
        .map(|(k, v)| RawProviderDataExtensionV1 { key: k.clone(), value: v.clone() })
        .collect()
}


fn related_items_from(value: &Value) -> Vec<RelatedItemV1> {
    value.get("relatedItems").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(|entry| {
        let obj = entry.as_object()?;
        Some(RelatedItemV1 {
            kind: object_string(obj, "kind", "shared"),
            group_type: clamp_i64(object_i64(obj, "groupType", 0), 0, 15),
            original_network_id: clamp_i64(object_i64(obj, "originalNetworkId", 0), 0, 0xffff),
            transport_stream_id: clamp_i64(object_i64(obj, "transportStreamId", 0), 0, 0xffff),
            service_id: clamp_i64(object_i64(obj, "serviceId", 0), 0, 0xffff),
            event_id: clamp_i64(object_i64(obj, "eventId", 0), 0, 0xffff),
            parse_status: object_string(obj, "parseStatus", "OK"),
        })
    }).collect()
}

fn linkage_from(value: &Value) -> Vec<LinkageV1> {
    value.get("linkage").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(|entry| {
        let obj = entry.as_object()?;
        Some(LinkageV1 {
            transport_stream_id: clamp_i64(object_i64(obj, "transportStreamId", 0), 0, 0xffff),
            original_network_id: clamp_i64(object_i64(obj, "originalNetworkId", 0), 0, 0xffff),
            service_id: clamp_i64(object_i64(obj, "serviceId", 0), 0, 0xffff),
            linkage_type: clamp_i64(object_i64(obj, "linkageType", 0), 0, 0xff),
            private_data_prefix_hex: object_string(obj, "privateDataPrefixHex", object_string(obj, "privateDataHex", "")),
            parse_status: object_string(obj, "parseStatus", "OK"),
        })
    }).collect()
}

fn components_from(value: &Value) -> ComponentsV1 {
    let Some(obj) = value.get("components").and_then(Value::as_object) else { return ComponentsV1::default(); };
    ComponentsV1 {
        video: obj.get("video").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(video_component_from_value).collect(),
        audio: obj.get("audio").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(audio_component_from_value).collect(),
        subtitle: obj.get("subtitle").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(subtitle_component_from_value).collect(),
        data: obj.get("data").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(data_component_from_value).collect(),
    }
}

fn video_component_from_value(value: Value) -> Option<VideoComponentV1> {
    let obj = value.as_object()?;
    Some(VideoComponentV1 {
        es_pid: clamp_i64(object_i64(obj, "esPid", 0), 0, 8191),
        stream_type: clamp_i64(object_i64(obj, "streamType", 0), 0, 255),
        component_tag: clamp_i64(object_i64(obj, "componentTag", 0), 0, 255),
        component_type: clamp_i64(object_i64(obj, "componentType", 0), 0, 255),
        codec: object_string(obj, "codec", "UNKNOWN_VIDEO"),
        resolution: obj.get("resolution").and_then(Value::as_str).map(ToString::to_string),
        scan: obj.get("scan").and_then(Value::as_str).map(ToString::to_string),
        aspect: obj.get("aspect").and_then(Value::as_str).map(ToString::to_string),
        profile_level: obj.get("profileLevel").and_then(Value::as_str).map(ToString::to_string),
        source_descriptor: obj.get("sourceDescriptor").and_then(Value::as_str).map(ToString::to_string),
        parse_status: object_string(obj, "parseStatus", "OK"),
    })
}

fn audio_component_from_value(value: Value) -> Option<AudioComponentV1> {
    let obj = value.as_object()?;
    Some(AudioComponentV1 {
        es_pid: clamp_i64(object_i64(obj, "esPid", 0), 0, 8191),
        stream_type: clamp_i64(object_i64(obj, "streamType", 0), 0, 255),
        component_tag: clamp_i64(object_i64(obj, "componentTag", 0), 0, 255),
        component_type: clamp_i64(object_i64(obj, "componentType", 0), 0, 255),
        codec: object_string(obj, "codec", "UNKNOWN_AUDIO"),
        language: object_string(obj, "language", "und"),
        channel_configuration: obj.get("channelConfiguration").and_then(Value::as_str).map(ToString::to_string),
        sampling_info: obj.get("samplingInfo").and_then(Value::as_str).map(ToString::to_string),
        source_descriptor: obj.get("sourceDescriptor").and_then(Value::as_str).map(ToString::to_string),
        parse_status: object_string(obj, "parseStatus", "OK"),
    })
}

fn subtitle_component_from_value(value: Value) -> Option<SubtitleComponentV1> {
    let obj = value.as_object()?;
    Some(SubtitleComponentV1 {
        es_pid: clamp_i64(object_i64(obj, "esPid", 0), 0, 8191),
        component_tag: clamp_i64(object_i64(obj, "componentTag", 0), 0, 255),
        data_component_id: clamp_i64(object_i64(obj, "dataComponentId", 0), 0, 0xffff),
        language: object_string(obj, "language", "und"),
        track_id: object_string(obj, "trackId", "subtitle-0"),
        caption_service_kind: object_string(obj, "captionServiceKind", "caption"),
        parse_status: object_string(obj, "parseStatus", "OK"),
    })
}

fn data_component_from_value(value: Value) -> Option<DataComponentV1> {
    let obj = value.as_object()?;
    Some(DataComponentV1 {
        es_pid: clamp_i64(object_i64(obj, "esPid", 0), 0, 8191),
        component_tag: clamp_i64(object_i64(obj, "componentTag", 0), 0, 255),
        data_component_id: clamp_i64(object_i64(obj, "dataComponentId", 0), 0, 0xffff),
        component_type: clamp_i64(object_i64(obj, "componentType", 0), 0, 255),
        parse_status: object_string(obj, "parseStatus", "OK"),
    })
}

fn descriptor_diagnostic_from_value(value: &Value, root: &Value) -> Option<DescriptorDiagnosticV1> {
    let obj = value.as_object()?;
    let scope_obj = obj.get("scope").and_then(Value::as_object);
    let desc_obj = obj.get("descriptor").and_then(Value::as_object).or(Some(obj))?;
    Some(DescriptorDiagnosticV1 {
        schema: object_string(obj, "schema", "maleicacid.tv.descriptorDiagnostic"),
        schema_version: object_i64(obj, "schemaVersion", 1),
        severity: object_string(obj, "severity", "warning"),
        code: object_string(obj, "code", object_string(desc_obj, "parseStatus", "DESCRIPTOR_DIAGNOSTIC")),
        scope: SectionScopeV1 {
            pid: scope_obj.and_then(|o| o.get("pid")).and_then(Value::as_i64).or_else(|| root.get("source").and_then(|s| s.get("pid")).and_then(Value::as_i64)),
            table_id: scope_obj.and_then(|o| o.get("tableId")).and_then(Value::as_i64).or_else(|| root.get("source").and_then(|s| s.get("tableId")).and_then(Value::as_i64)),
            table_id_extension: scope_obj.and_then(|o| o.get("tableIdExtension")).and_then(Value::as_i64),
            version: scope_obj.and_then(|o| o.get("version")).and_then(Value::as_i64).or_else(|| root.get("source").and_then(|s| s.get("version")).and_then(Value::as_i64)),
            section_number: scope_obj.and_then(|o| o.get("sectionNumber")).and_then(Value::as_i64).or_else(|| root.get("source").and_then(|s| s.get("sectionNumber")).and_then(Value::as_i64)),
            original_network_id: scope_obj.and_then(|o| o.get("originalNetworkId")).and_then(Value::as_i64).or_else(|| root.get("serviceKey").and_then(|s| s.get("originalNetworkId")).and_then(Value::as_i64)),
            transport_stream_id: scope_obj.and_then(|o| o.get("transportStreamId")).and_then(Value::as_i64).or_else(|| root.get("serviceKey").and_then(|s| s.get("transportStreamId")).and_then(Value::as_i64)),
            service_id: scope_obj.and_then(|o| o.get("serviceId")).and_then(Value::as_i64).or_else(|| root.get("serviceKey").and_then(|s| s.get("serviceId")).and_then(Value::as_i64)),
            event_id: scope_obj.and_then(|o| o.get("eventId")).and_then(Value::as_i64).or_else(|| root.get("eventId").and_then(Value::as_i64)),
        },
        descriptor: DescriptorScopeV1 {
            tag: clamp_i64(object_i64(desc_obj, "tag", 0), 0, 255),
            name: desc_obj.get("name").and_then(Value::as_str).map(ToString::to_string),
            offset: object_i64(desc_obj, "offset", 0).max(0),
            declared_length: clamp_i64(object_i64(desc_obj, "declaredLength", 0), 0, 255),
            actual_remaining_length: object_i64(desc_obj, "actualRemainingLength", object_i64(desc_obj, "actualRemaining", 0)).max(0),
            parse_status: desc_obj.get("parseStatus").and_then(Value::as_str).map(ToString::to_string),
            raw_prefix_hex: object_string(desc_obj, "rawPrefixHex", object_string(desc_obj, "rawPrefix", "")),
        },
        message: object_string(obj, "message", "descriptor diagnostic"),
    })
}

fn parse_input(text: &str) -> Value {
    serde_json::from_str::<Value>(text).unwrap_or_else(|_| legacy_value(text))
}

fn legacy_value(text: &str) -> Value {
    let mut map = serde_json::Map::new();
    for part in text.split(';') {
        let Some((key, value)) = part.split_once('=') else { continue; };
        map.insert(key.trim().to_string(), Value::String(value.trim().to_string()));
    }
    Value::Object(map)
}

fn time_from(value: &Value) -> i64 { i64_field(value.get("timing").unwrap_or(value), "startUtcMillis", i64_field(value, "startTimeMillis", 0)) }
fn duration_from(value: &Value) -> i64 {
    let timing = value.get("timing").unwrap_or(value);
    let duration = i64_field(timing, "durationMillis", i64_field(value, "durationMillis", -1));
    if duration >= 0 { return duration; }
    let start = i64_field(timing, "startUtcMillis", time_from(value));
    i64_field(timing, "endUtcMillis", i64_field(value, "endUtcMillis", start)).saturating_sub(start)
}

fn finalize_program(data: ProgramProviderDataV1) -> ProviderDataResult {
    let mut text = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
    if text.len() > HARD_LIMIT_BYTES {
        let mut truncated = truncated_program_value(&data);
        truncated.diagnostics.provider_data_truncated = Some(true);
        truncated.diagnostics.provider_data_hard_limit_bytes = Some(HARD_LIMIT_BYTES as i64);
        truncated.diagnostics.provider_data_soft_limit_bytes = Some(SOFT_LIMIT_BYTES as i64);
        text = serde_json::to_string(&truncated).unwrap_or_else(|_| "{}".to_string());
    }
    ProviderDataResult { signature: sha256_hex(text.as_bytes()), json: text, extracted_key: format!("onid={};tsid={};sid={};event={}", data.program_key.original_network_id, data.program_key.transport_stream_id, data.program_key.service_id, data.program_key.event_id) }
}

fn finalize_channel(data: ChannelProviderDataV1) -> ProviderDataResult {
    let mut text = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
    if text.len() > HARD_LIMIT_BYTES {
        let mut truncated = truncated_channel_value(&data.service_key, &data.tune);
        truncated.diagnostics.provider_data_truncated = Some(true);
        truncated.diagnostics.provider_data_hard_limit_bytes = Some(HARD_LIMIT_BYTES as i64);
        truncated.diagnostics.provider_data_soft_limit_bytes = Some(SOFT_LIMIT_BYTES as i64);
        text = serde_json::to_string(&truncated).unwrap_or_else(|_| "{}".to_string());
    }
    ProviderDataResult { signature: sha256_hex(text.as_bytes()), json: text, extracted_key: format!("onid={};tsid={};sid={}", data.service_key.original_network_id, data.service_key.transport_stream_id, data.service_key.service_id) }
}

fn truncated_program_value(data: &ProgramProviderDataV1) -> ProgramProviderDataV1 {
    ProgramProviderDataV1 {
        schema: PROGRAM_SCHEMA_NAME.to_string(),
        schema_version: PROVIDER_SCHEMA_VERSION,
        program_key: data.program_key.clone(),
        service_key: data.service_key.clone(),
        timing: data.timing.clone(),
        source: data.source.clone(),
        cas: data.cas.clone(),
        ratings: data.ratings.clone(),
        genres: Vec::new(),
        series: None,
        related_items: Vec::new(),
        linkage: Vec::new(),
        free_ca_mode: data.free_ca_mode.clone(),
        audio_languages: Vec::new(),
        audio: None,
        video: None,
        extended_items: Vec::new(),
        components: ComponentsV1::default(),
        diagnostics: DiagnosticsV1::default(),
    }
}

fn truncated_channel_value(service_key: &ServiceKeyV1, tune: &ChannelTuneV1) -> ChannelProviderDataV1 {
    ChannelProviderDataV1 {
        schema: CHANNEL_SCHEMA_NAME.to_string(),
        schema_version: CHANNEL_SCHEMA_VERSION,
        service_key: service_key.clone(),
        tune: tune.clone(),
        cas: ChannelCasV1 { requires_cas: false, unsupported_cas: false, clear_live_playback_supported: false },
        diagnostics: ChannelDiagnosticsV1 {
            channel_registration_ready: false,
            epg_publishable: false,
            publish_state_source: "TRUNCATED_IDENTITY_ONLY".to_string(),
            raw_provider_data_extensions: Vec::new(),
            provider_data_truncated: None,
            provider_data_hard_limit_bytes: None,
            provider_data_soft_limit_bytes: None,
        },
    }
}

fn i64_field(value: &Value, key: &str, default: i64) -> i64 { value.get(key).and_then(Value::as_i64).unwrap_or(default) }
fn bool_field(value: &Value, key: &str, default: bool) -> bool { value.get(key).and_then(Value::as_bool).unwrap_or(default) }
fn string_field<S: AsRef<str>>(value: &Value, key: &str, default: S) -> String { value.get(key).and_then(Value::as_str).map(ToString::to_string).unwrap_or_else(|| default.as_ref().to_string()) }
fn string_opt(value: &Value, key: &str) -> Option<String> { value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(ToString::to_string) }
fn optional_i64(value: &Value, key: &str) -> Option<i64> { value.get(key).and_then(Value::as_i64) }
fn object_i64(value: &serde_json::Map<String, Value>, key: &str, default: i64) -> i64 { value.get(key).and_then(Value::as_i64).unwrap_or(default) }
fn object_bool(value: &serde_json::Map<String, Value>, key: &str, default: bool) -> bool { value.get(key).and_then(Value::as_bool).unwrap_or(default) }
fn object_string<S: AsRef<str>>(value: &serde_json::Map<String, Value>, key: &str, default: S) -> String { value.get(key).and_then(Value::as_str).map(ToString::to_string).unwrap_or_else(|| default.as_ref().to_string()) }
fn clamp_i64(value: i64, min: i64, max: i64) -> i64 { value.max(min).min(max) }

fn parse_legacy_key(input: &str) -> Option<ProgramIdentity> {
    let mut onid = None;
    let mut tsid = None;
    let mut sid = None;
    let mut event_id = None;
    for part in input.split(';') {
        let Some((key, value)) = part.split_once('=') else { continue; };
        match key.trim() {
            "onid" | "originalNetworkId" => onid = value.parse::<i64>().ok(),
            "tsid" | "transportStreamId" => tsid = value.parse::<i64>().ok(),
            "sid" | "serviceId" => sid = value.parse::<i64>().ok(),
            "event" | "eventId" => event_id = value.parse::<i64>().ok(),
            _ => {}
        }
    }
    Some(ProgramIdentity { onid: onid?, tsid: tsid?, sid: sid?, event_id: event_id?, start_utc_millis: 0, duration_millis: 0 }.clamped())
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256(data);
    let mut out = String::with_capacity(64);
    for b in digest { out.push_str(&format!("{:02x}", b)); }
    out
}

fn sha256(data: &[u8]) -> [u8; 32] {
    const H0: [u32; 8] = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    const K: [u32; 64] = [
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
        0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
        0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
        0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
        0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
        0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
    let mut msg = data.to_vec();
    let bit_len = (msg.len() as u64) * 8;
    msg.push(0x80);
    while (msg.len() % 64) != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    let mut h = H0;
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(temp1); d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }
        h[0]=h[0].wrapping_add(a); h[1]=h[1].wrapping_add(b); h[2]=h[2].wrapping_add(c); h[3]=h[3].wrapping_add(d);
        h[4]=h[4].wrapping_add(e); h[5]=h[5].wrapping_add(f); h[6]=h[6].wrapping_add(g); h[7]=h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() { out[i*4..i*4+4].copy_from_slice(&v.to_be_bytes()); }
    out
}
