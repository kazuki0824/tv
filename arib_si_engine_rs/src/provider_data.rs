use serde::{Deserialize, Serialize};

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
    pub json: String,
    pub signature: String,
    pub schema_version: i64,
    pub truncated: bool,
    pub diagnostics_dropped_count: i64,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    canonical_genres: Vec<String>,
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
    r51_playback_supported: Option<bool>,
    live_viewable_claim: Option<bool>,
    diagnostic_code: Option<String>,
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
    r51_playback_supported: Option<bool>,
    live_viewable_claim: Option<bool>,
    diagnostic_code: Option<String>,
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
    value: serde_json::Value,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_dropped_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    malformed_ca_descriptor_count: Option<i64>,
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
    #[serde(default, flatten)]
    extensions: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgramRequestDiagnosticsV1 {
    descriptor_diagnostics_canonical_json: String,
    publish_diagnostics: Vec<DiagnosticItemV1>,
    parser_diagnostics: Vec<DiagnosticItemV1>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgramProviderDataRequestV1 {
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
    diagnostics: ProgramRequestDiagnosticsV1,
    #[serde(default)]
    malformed_ca_descriptor_count: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelTuneV1 {
    input_id: String,
    display_name: String,
    delivery_system: String,
    frequency_hz: i64,
    stream_id: Option<i64>,
    stream_id_type: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_data_dropped_count: Option<i64>,
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
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelProviderDataRequestV1 {
    schema: String,
    schema_version: i64,
    service_key: ServiceKeyV1,
    tune: ChannelTuneV1,
    cas: ChannelCasV1,
    diagnostics: ChannelDiagnosticsV1,
}

pub fn build_program_key(onid: i32, tsid: i32, sid: i32, event_id: i32) -> String {
    serde_json::json!({
        "kind": "arib-event-v1",
        "originalNetworkId": onid,
        "transportStreamId": tsid,
        "serviceId": sid,
        "eventId": event_id,
    }).to_string()
}

pub fn build_program_provider_data(request_json: &str) -> ProviderDataResult {
    let Ok(request) = serde_json::from_str::<ProgramProviderDataRequestV1>(request_json) else { return empty_result(); };
    let Some(data) = program_data_from_request(request) else { return empty_result(); };
    finalize_program(data)
}

pub fn build_channel_provider_data(request_json: &str) -> ProviderDataResult {
    let Ok(request) = serde_json::from_str::<ChannelProviderDataRequestV1>(request_json) else { return empty_result(); };
    let Some(data) = channel_data_from_request(request) else { return empty_result(); };
    finalize_channel(data)
}

pub fn normalize_program_provider_data(raw_bytes: &[u8]) -> ProviderDataResult {
    let Ok(text) = std::str::from_utf8(raw_bytes) else { return empty_result(); };
    let Ok(raw_value) = serde_json::from_str::<serde_json::Value>(text.trim()) else { return empty_result(); };
    let Ok(data) = serde_json::from_value::<ProgramProviderDataV1>(raw_value.clone()) else { return empty_result(); };
    let data = normalize_program_extensions(data, Some(&raw_value));
    if !valid_program_provider_data(&data) { return empty_result(); }
    finalize_program(data)
}

pub fn append_current_program_diagnostics(raw_bytes: &[u8], overlap_count: i64, selected_program_id: i64, selection_rule: &str) -> ProviderDataResult {
    let Ok(text) = std::str::from_utf8(raw_bytes) else { return empty_result(); };
    let Ok(raw_value) = serde_json::from_str::<serde_json::Value>(text.trim()) else { return empty_result(); };
    let Ok(data) = serde_json::from_value::<ProgramProviderDataV1>(raw_value.clone()) else { return empty_result(); };
    let mut data = normalize_program_extensions(data, Some(&raw_value));
    if !valid_program_provider_data(&data) { return empty_result(); }
    data.diagnostics.current_program = Some(CurrentProgramDiagnosticsV1 { overlap_count: overlap_count.max(0), selected_program_id, selection_rule: selection_rule.to_string() });
    finalize_program(data)
}

pub fn program_provider_data_signature(raw_bytes: &[u8]) -> String {
    sha256_hex(raw_bytes)
}

pub fn extract_program_key_result(raw_bytes: &[u8]) -> Option<ProgramKeyResult> {
    let text = std::str::from_utf8(raw_bytes).ok()?;
    let data = serde_json::from_str::<ProgramProviderDataV1>(text.trim()).ok()?;
    if !valid_program_provider_data(&data) { return None; }
    Some(ProgramKeyResult {
        original_network_id: data.program_key.original_network_id,
        transport_stream_id: data.program_key.transport_stream_id,
        service_id: data.program_key.service_id,
        event_id: data.program_key.event_id,
        key: serde_json::to_string(&data.program_key).unwrap_or_default(),
    })
}

pub fn extract_program_key(raw_bytes: &[u8]) -> Option<String> {
    extract_program_key_result(raw_bytes).map(|v| v.key)
}

pub fn extract_channel_tune_key(provider_data: &str) -> String {
    let Ok(data) = serde_json::from_str::<ChannelProviderDataV1>(provider_data.trim()) else { return String::new(); };
    if !valid_channel_provider_data(&data) { return String::new(); }
    serde_json::to_string(&data).unwrap_or_default()
}

fn program_data_from_request(request: ProgramProviderDataRequestV1) -> Option<ProgramProviderDataV1> {
    if request.schema != PROGRAM_REQUEST_SCHEMA_NAME || request.schema_version != PROVIDER_SCHEMA_VERSION { return None; }
    if request.program_key.kind != "arib-event-v1" { return None; }
    if request.service_key.original_network_id != request.program_key.original_network_id || request.service_key.transport_stream_id != request.program_key.transport_stream_id || request.service_key.service_id != request.program_key.service_id { return None; }
    let descriptor_diagnostics = parse_descriptor_diagnostics(&request.diagnostics.descriptor_diagnostics_canonical_json)?;
    let data = ProgramProviderDataV1 {
        schema: PROGRAM_SCHEMA_NAME.to_string(),
        schema_version: PROVIDER_SCHEMA_VERSION,
        program_key: request.program_key,
        service_key: request.service_key,
        timing: request.timing,
        source: request.source,
        cas: request.cas,
        ratings: request.ratings,
        genres: request.genres,
        series: request.series,
        related_items: request.related_items,
        linkage: request.linkage,
        free_ca_mode: request.free_ca_mode,
        audio_languages: request.audio_languages,
        audio: request.audio,
        video: request.video,
        extended_items: request.extended_items,
        components: request.components,
        diagnostics: DiagnosticsV1 {
            descriptor_diagnostics,
            publish_diagnostics: request.diagnostics.publish_diagnostics,
            parser_diagnostics: request.diagnostics.parser_diagnostics,
            current_program: None,
            raw_provider_data_extensions: Vec::new(),
            provider_data_truncated: None,
            provider_data_hard_limit_bytes: None,
            provider_data_soft_limit_bytes: None,
            provider_data_dropped_count: None,
            malformed_ca_descriptor_count: (request.malformed_ca_descriptor_count > 0).then_some(request.malformed_ca_descriptor_count),
        },
        extensions: serde_json::Map::new(),
    };
    valid_program_provider_data(&data).then_some(data)
}

fn channel_data_from_request(request: ChannelProviderDataRequestV1) -> Option<ChannelProviderDataV1> {
    if request.schema != CHANNEL_REQUEST_SCHEMA_NAME || request.schema_version != CHANNEL_SCHEMA_VERSION { return None; }
    let data = ChannelProviderDataV1 {
        schema: CHANNEL_SCHEMA_NAME.to_string(),
        schema_version: CHANNEL_SCHEMA_VERSION,
        service_key: request.service_key,
        tune: request.tune,
        cas: request.cas,
        diagnostics: request.diagnostics,
    };
    valid_channel_provider_data(&data).then_some(data)
}

fn normalize_program_extensions(mut data: ProgramProviderDataV1, raw_value: Option<&serde_json::Value>) -> ProgramProviderDataV1 {
    let extensions = std::mem::take(&mut data.extensions);
    for (key, value) in extensions {
        data.diagnostics.raw_provider_data_extensions.push(RawProviderDataExtensionV1 { key, value });
    }
    if let Some(raw) = raw_value {
        let mut nested = Vec::new();
        collect_program_unknown_extensions(raw, &mut nested);
        for extension in nested {
            if !data.diagnostics.raw_provider_data_extensions.iter().any(|existing| existing.key == extension.key) {
                data.diagnostics.raw_provider_data_extensions.push(extension);
            }
        }
    }
    data
}

fn collect_program_unknown_extensions(raw: &serde_json::Value, out: &mut Vec<RawProviderDataExtensionV1>) {
    collect_object_unknown(raw, "", &["schema", "schemaVersion", "programKey", "serviceKey", "timing", "source", "cas", "ratings", "genres", "series", "relatedItems", "linkage", "freeCaMode", "audioLanguages", "audio", "video", "extendedItems", "components", "diagnostics"], out);
    collect_object_unknown(raw.get("programKey").unwrap_or(&serde_json::Value::Null), "programKey", &["kind", "originalNetworkId", "transportStreamId", "serviceId", "eventId"], out);
    collect_object_unknown(raw.get("serviceKey").unwrap_or(&serde_json::Value::Null), "serviceKey", &["originalNetworkId", "transportStreamId", "serviceId"], out);
    collect_object_unknown(raw.get("timing").unwrap_or(&serde_json::Value::Null), "timing", &["startUtcMillis", "endUtcMillis", "durationMillis"], out);
    collect_object_unknown(raw.get("source").unwrap_or(&serde_json::Value::Null), "source", &["pid", "tableId", "version", "sectionNumber", "lastSectionNumber"], out);
    collect_object_unknown(raw.get("cas").unwrap_or(&serde_json::Value::Null), "cas", &["requiresCas", "unsupportedCas", "clearLivePlaybackSupported", "source"], out);
    collect_array_unknown(raw.get("ratings").unwrap_or(&serde_json::Value::Null), "ratings", &["countryCode", "ratingValue", "rawRatingByte", "supported", "mappedTvContentRating", "parseStatus"], out);
    collect_array_unknown(raw.get("genres").unwrap_or(&serde_json::Value::Null), "genres", &["level1", "level2", "userNibble", "aribName", "unmappedReason", "canonicalGenres", "parseStatus"], out);
    collect_object_unknown(raw.get("series").unwrap_or(&serde_json::Value::Null), "series", &["seriesId", "repeatLabel", "programPattern", "expireDateValid", "expireDate", "episodeNumber", "lastEpisodeNumber", "name", "parseStatus"], out);
    collect_array_unknown(raw.get("relatedItems").unwrap_or(&serde_json::Value::Null), "relatedItems", &["kind", "groupType", "originalNetworkId", "transportStreamId", "serviceId", "eventId", "parseStatus"], out);
    collect_array_unknown(raw.get("linkage").unwrap_or(&serde_json::Value::Null), "linkage", &["transportStreamId", "originalNetworkId", "serviceId", "linkageType", "privateDataPrefixHex", "parseStatus"], out);
    collect_object_unknown(raw.get("freeCaMode").unwrap_or(&serde_json::Value::Null), "freeCaMode", &["raw", "scrambled", "text", "parseStatus"], out);
    collect_array_unknown(raw.get("audioLanguages").unwrap_or(&serde_json::Value::Null), "audioLanguages", &["language", "source", "parseStatus"], out);
    collect_object_unknown(raw.get("audio").unwrap_or(&serde_json::Value::Null), "audio", &["esPid", "componentTag", "codec", "language", "text", "parseStatus"], out);
    collect_object_unknown(raw.get("video").unwrap_or(&serde_json::Value::Null), "video", &["esPid", "componentTag", "codec", "format", "width", "height", "parseStatus"], out);
    collect_array_unknown(raw.get("extendedItems").unwrap_or(&serde_json::Value::Null), "extendedItems", &["description", "text", "parseStatus"], out);
    if let Some(components) = raw.get("components") {
        collect_object_unknown(components, "components", &["video", "audio", "subtitle", "data"], out);
        collect_array_unknown(components.get("video").unwrap_or(&serde_json::Value::Null), "components.video", &["esPid", "streamType", "componentTag", "componentType", "codec", "resolution", "scan", "aspect", "profileLevel", "sourceDescriptor", "r51PlaybackSupported", "liveViewableClaim", "diagnosticCode", "parseStatus"], out);
        collect_array_unknown(components.get("audio").unwrap_or(&serde_json::Value::Null), "components.audio", &["esPid", "streamType", "componentTag", "componentType", "codec", "language", "channelConfiguration", "samplingInfo", "sourceDescriptor", "r51PlaybackSupported", "liveViewableClaim", "diagnosticCode", "parseStatus"], out);
        collect_array_unknown(components.get("subtitle").unwrap_or(&serde_json::Value::Null), "components.subtitle", &["esPid", "componentTag", "dataComponentId", "language", "trackId", "captionServiceKind", "parseStatus"], out);
        collect_array_unknown(components.get("data").unwrap_or(&serde_json::Value::Null), "components.data", &["esPid", "componentTag", "dataComponentId", "componentType", "parseStatus"], out);
    }
    if let Some(diagnostics) = raw.get("diagnostics") {
        collect_object_unknown(diagnostics, "diagnostics", &["descriptorDiagnostics", "publishDiagnostics", "parserDiagnostics", "currentProgram", "rawProviderDataExtensions", "providerDataTruncated", "providerDataHardLimitBytes", "providerDataSoftLimitBytes", "providerDataDroppedCount", "malformedCaDescriptorCount"], out);
        collect_descriptor_diagnostic_unknown(diagnostics.get("descriptorDiagnostics").unwrap_or(&serde_json::Value::Null), out);
        collect_array_unknown(diagnostics.get("publishDiagnostics").unwrap_or(&serde_json::Value::Null), "diagnostics.publishDiagnostics", &["code", "message", "severity"], out);
        collect_array_unknown(diagnostics.get("parserDiagnostics").unwrap_or(&serde_json::Value::Null), "diagnostics.parserDiagnostics", &["code", "message", "severity"], out);
        collect_object_unknown(diagnostics.get("currentProgram").unwrap_or(&serde_json::Value::Null), "diagnostics.currentProgram", &["overlapCount", "selectedProgramId", "selectionRule"], out);
    }
}


fn collect_descriptor_diagnostic_unknown(value: &serde_json::Value, out: &mut Vec<RawProviderDataExtensionV1>) {
    let Some(items) = value.as_array() else { return; };
    for (index, item) in items.iter().enumerate() {
        let base = format!("diagnostics.descriptorDiagnostics[{}]", index);
        collect_object_unknown(item, &base, &["schema", "schemaVersion", "severity", "code", "scope", "descriptor", "message"], out);
        if let Some(scope) = item.get("scope") {
            collect_object_unknown(scope, &format!("{}.scope", base), &["pid", "tableId", "tableIdExtension", "version", "sectionNumber", "originalNetworkId", "transportStreamId", "serviceId", "eventId"], out);
        }
        if let Some(descriptor) = item.get("descriptor") {
            collect_object_unknown(descriptor, &format!("{}.descriptor", base), &["tag", "name", "offset", "declaredLength", "actualRemainingLength", "parseStatus", "rawPrefixHex"], out);
        }
    }
}

fn collect_array_unknown(value: &serde_json::Value, path: &str, known_keys: &[&str], out: &mut Vec<RawProviderDataExtensionV1>) {
    let Some(items) = value.as_array() else { return; };
    for (index, item) in items.iter().enumerate() {
        collect_object_unknown(item, &format!("{}[{}]", path, index), known_keys, out);
    }
}

fn collect_object_unknown(value: &serde_json::Value, path: &str, known_keys: &[&str], out: &mut Vec<RawProviderDataExtensionV1>) {
    let Some(object) = value.as_object() else { return; };
    for (key, value) in object {
        if !known_keys.iter().any(|known| *known == key) {
            let full_key = if path.is_empty() { key.clone() } else { format!("{}.{}", path, key) };
            out.push(RawProviderDataExtensionV1 { key: full_key, value: value.clone() });
        }
    }
}

fn program_truncation_dropped_count(data: &ProgramProviderDataV1) -> i64 {
    let mut count = 0i64;
    count += data.genres.len() as i64;
    count += data.series.as_ref().map(|_| 1).unwrap_or(0);
    count += data.related_items.len() as i64;
    count += data.linkage.len() as i64;
    count += data.audio_languages.len() as i64;
    count += data.audio.as_ref().map(|_| 1).unwrap_or(0);
    count += data.video.as_ref().map(|_| 1).unwrap_or(0);
    count += data.extended_items.len() as i64;
    count += data.components.video.len() as i64;
    count += data.components.audio.len() as i64;
    count += data.components.subtitle.len() as i64;
    count += data.components.data.len() as i64;
    count += data.diagnostics.descriptor_diagnostics.len() as i64;
    count += data.diagnostics.publish_diagnostics.len() as i64;
    count += data.diagnostics.parser_diagnostics.len() as i64;
    count += data.diagnostics.raw_provider_data_extensions.len() as i64;
    count.max(1)
}

fn channel_truncation_dropped_count(data: &ChannelProviderDataV1) -> i64 {
    (data.diagnostics.raw_provider_data_extensions.len() as i64).max(1)
}

fn provider_data_truncated_item(dropped_count: i64) -> DiagnosticItemV1 {
    DiagnosticItemV1 {
        code: "PROVIDER_DATA_TRUNCATED".to_string(),
        message: format!("provider-data hard limit exceeded; droppedCount={}", dropped_count),
        severity: Some("warning".to_string()),
    }
}

fn parse_descriptor_diagnostics(text: &str) -> Option<Vec<DescriptorDiagnosticV1>> {
    let items = serde_json::from_str::<Vec<DescriptorDiagnosticV1>>(text).ok()?;
    if items.iter().all(valid_descriptor_diagnostic) { Some(items) } else { None }
}

fn valid_descriptor_diagnostic(item: &DescriptorDiagnosticV1) -> bool {
    item.schema == "maleicacid.tv.descriptorDiagnostic" &&
        item.schema_version == 1 &&
        !item.severity.is_empty() &&
        !item.code.is_empty() &&
        item.descriptor.tag >= 0 && item.descriptor.tag <= 255 &&
        item.descriptor.offset >= 0 &&
        item.descriptor.declared_length >= 0 && item.descriptor.declared_length <= 255 &&
        item.descriptor.actual_remaining_length >= 0 &&
        !item.descriptor.parse_status.is_empty()
}

fn valid_program_provider_data(data: &ProgramProviderDataV1) -> bool {
    data.schema == PROGRAM_SCHEMA_NAME &&
        data.schema_version == PROVIDER_SCHEMA_VERSION &&
        data.program_key.kind == "arib-event-v1" &&
        in_u16(data.program_key.original_network_id) && in_u16(data.program_key.transport_stream_id) && in_u16(data.program_key.service_id) && in_u16(data.program_key.event_id) &&
        data.service_key.original_network_id == data.program_key.original_network_id &&
        data.service_key.transport_stream_id == data.program_key.transport_stream_id &&
        data.service_key.service_id == data.program_key.service_id &&
        data.timing.start_utc_millis >= 0 && data.timing.duration_millis >= 0 && data.timing.end_utc_millis == data.timing.start_utc_millis.saturating_add(data.timing.duration_millis) &&
        data.source.pid >= 0 && data.source.pid <= 8191 && data.source.table_id >= 0 && data.source.table_id <= 255 && data.source.version >= 0 && data.source.version <= 31 && data.source.section_number >= 0 && data.source.section_number <= 255 && data.source.last_section_number >= data.source.section_number && data.source.last_section_number <= 255 &&
        !data.cas.source.is_empty() &&
        data.ratings.iter().all(valid_rating) &&
        data.genres.iter().all(valid_genre) &&
        data.series.as_ref().map(valid_series).unwrap_or(true) &&
        data.related_items.iter().all(valid_related_item) &&
        data.linkage.iter().all(valid_linkage) &&
        data.free_ca_mode.as_ref().map(valid_free_ca_mode).unwrap_or(true) &&
        data.audio_languages.iter().all(valid_audio_language) &&
        data.audio.as_ref().map(valid_audio_metadata).unwrap_or(true) &&
        data.video.as_ref().map(valid_video_metadata).unwrap_or(true) &&
        data.extended_items.iter().all(valid_extended_item) &&
        valid_components(&data.components) &&
        data.diagnostics.descriptor_diagnostics.iter().all(valid_descriptor_diagnostic) &&
        data.diagnostics.publish_diagnostics.iter().all(valid_diagnostic_item) &&
        data.diagnostics.parser_diagnostics.iter().all(valid_diagnostic_item)
}

fn valid_channel_provider_data(data: &ChannelProviderDataV1) -> bool {
    data.schema == CHANNEL_SCHEMA_NAME && data.schema_version == CHANNEL_SCHEMA_VERSION &&
        in_u16(data.service_key.original_network_id) && in_u16(data.service_key.transport_stream_id) && in_u16(data.service_key.service_id) &&
        !data.tune.input_id.is_empty() && !data.tune.display_name.is_empty() && !data.tune.delivery_system.is_empty() && data.tune.frequency_hz > 0 &&
        matches!(data.tune.stream_id_type.as_str(), "NONE" | "TSID" | "RELATIVE") &&
        (if data.tune.stream_id_type == "NONE" { data.tune.stream_id.is_none() } else { data.tune.stream_id.map(in_u16).unwrap_or(false) }) &&
        !data.diagnostics.publish_state_source.is_empty()
}

fn in_u16(v: i64) -> bool { (0..=0xffff).contains(&v) }
fn nonempty(s: &str) -> bool { !s.is_empty() }
fn valid_rating(v: &RatingV1) -> bool { nonempty(&v.country_code) && (0..=255).contains(&v.rating_value) && (0..=255).contains(&v.raw_rating_byte) && nonempty(&v.parse_status) }
fn valid_genre(v: &GenreV1) -> bool { (0..=15).contains(&v.level1) && (0..=15).contains(&v.level2) && (0..=15).contains(&v.user_nibble) && nonempty(&v.parse_status) }
fn valid_series(v: &SeriesV1) -> bool { in_u16(v.series_id) && (0..=15).contains(&v.repeat_label) && (0..=7).contains(&v.program_pattern) && (0..=4095).contains(&v.episode_number) && (0..=4095).contains(&v.last_episode_number) && nonempty(&v.parse_status) }
fn valid_related_item(v: &RelatedItemV1) -> bool { nonempty(&v.kind) && in_u16(v.original_network_id) && in_u16(v.transport_stream_id) && in_u16(v.service_id) && in_u16(v.event_id) && nonempty(&v.parse_status) }
fn valid_linkage(v: &LinkageV1) -> bool { in_u16(v.original_network_id) && in_u16(v.transport_stream_id) && in_u16(v.service_id) && (0..=255).contains(&v.linkage_type) && nonempty(&v.parse_status) }
fn valid_free_ca_mode(v: &FreeCaModeV1) -> bool { (0..=1).contains(&v.raw) && nonempty(&v.parse_status) }
fn valid_audio_language(v: &AudioLanguageV1) -> bool { nonempty(&v.language) && nonempty(&v.source) && nonempty(&v.parse_status) }
fn valid_audio_metadata(v: &AudioMetadataV1) -> bool { nonempty(&v.codec) && nonempty(&v.parse_status) }
fn valid_video_metadata(v: &VideoMetadataV1) -> bool { nonempty(&v.codec) && nonempty(&v.parse_status) }
fn valid_extended_item(v: &ExtendedItemV1) -> bool { nonempty(&v.text) && nonempty(&v.parse_status) }
fn valid_diagnostic_item(v: &DiagnosticItemV1) -> bool { nonempty(&v.code) && nonempty(&v.message) }
fn valid_components(v: &ComponentsV1) -> bool { v.video.iter().all(valid_video_component) && v.audio.iter().all(valid_audio_component) && v.subtitle.iter().all(valid_subtitle_component) && v.data.iter().all(valid_data_component) }
fn valid_video_component(v: &VideoComponentV1) -> bool { v.es_pid > 0 && v.es_pid <= 8191 && (0..=255).contains(&v.stream_type) && (0..=255).contains(&v.component_tag) && (0..=255).contains(&v.component_type) && nonempty(&v.codec) && nonempty(&v.parse_status) }
fn valid_audio_component(v: &AudioComponentV1) -> bool { v.es_pid > 0 && v.es_pid <= 8191 && (0..=255).contains(&v.stream_type) && (0..=255).contains(&v.component_tag) && (0..=255).contains(&v.component_type) && nonempty(&v.codec) && nonempty(&v.language) && nonempty(&v.parse_status) }
fn valid_subtitle_component(v: &SubtitleComponentV1) -> bool { v.es_pid > 0 && v.es_pid <= 8191 && (0..=255).contains(&v.component_tag) && nonempty(&v.language) && nonempty(&v.track_id) && nonempty(&v.caption_service_kind) && nonempty(&v.parse_status) }
fn valid_data_component(v: &DataComponentV1) -> bool { v.es_pid > 0 && v.es_pid <= 8191 && (0..=255).contains(&v.component_tag) && (0..=65535).contains(&v.data_component_id) && (0..=255).contains(&v.component_type) && nonempty(&v.parse_status) }

fn empty_result() -> ProviderDataResult {
    ProviderDataResult { json: "{}".to_string(), signature: sha256_hex(b"{}"), schema_version: PROVIDER_SCHEMA_VERSION, truncated: false, diagnostics_dropped_count: 0 }
}

fn finalize_program(data: ProgramProviderDataV1) -> ProviderDataResult {
    if !valid_program_provider_data(&data) { return empty_result(); }
    let mut text = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
    let mut truncated_flag = false;
    let mut dropped_count = 0i64;
    if text.len() > HARD_LIMIT_BYTES {
        dropped_count = program_truncation_dropped_count(&data);
        let mut truncated = truncated_program_value(&data);
        truncated.diagnostics.provider_data_truncated = Some(true);
        truncated.diagnostics.provider_data_hard_limit_bytes = Some(HARD_LIMIT_BYTES as i64);
        truncated.diagnostics.provider_data_soft_limit_bytes = Some(SOFT_LIMIT_BYTES as i64);
        truncated.diagnostics.provider_data_dropped_count = Some(dropped_count);
        truncated.diagnostics.publish_diagnostics.push(provider_data_truncated_item(dropped_count));
        text = serde_json::to_string(&truncated).unwrap_or_else(|_| "{}".to_string());
        truncated_flag = true;
    }
    ProviderDataResult { signature: sha256_hex(text.as_bytes()), json: text, schema_version: PROVIDER_SCHEMA_VERSION, truncated: truncated_flag, diagnostics_dropped_count: dropped_count }
}

fn finalize_channel(data: ChannelProviderDataV1) -> ProviderDataResult {
    if !valid_channel_provider_data(&data) { return empty_result(); }
    let mut text = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string());
    let mut truncated_flag = false;
    let mut dropped_count = 0i64;
    if text.len() > HARD_LIMIT_BYTES {
        dropped_count = channel_truncation_dropped_count(&data);
        let mut truncated = truncated_channel_value(&data);
        truncated.diagnostics.provider_data_truncated = Some(true);
        truncated.diagnostics.provider_data_hard_limit_bytes = Some(HARD_LIMIT_BYTES as i64);
        truncated.diagnostics.provider_data_soft_limit_bytes = Some(SOFT_LIMIT_BYTES as i64);
        truncated.diagnostics.provider_data_dropped_count = Some(dropped_count);
        truncated.diagnostics.provider_data_truncation_code = Some("PROVIDER_DATA_TRUNCATED".to_string());
        text = serde_json::to_string(&truncated).unwrap_or_else(|_| "{}".to_string());
        truncated_flag = true;
    }
    ProviderDataResult { signature: sha256_hex(text.as_bytes()), json: text, schema_version: CHANNEL_SCHEMA_VERSION, truncated: truncated_flag, diagnostics_dropped_count: dropped_count }
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
        diagnostics: DiagnosticsV1 { malformed_ca_descriptor_count: data.diagnostics.malformed_ca_descriptor_count, ..DiagnosticsV1::default() },
        extensions: serde_json::Map::new(),
    }
}

fn truncated_channel_value(data: &ChannelProviderDataV1) -> ChannelProviderDataV1 {
    ChannelProviderDataV1 {
        schema: CHANNEL_SCHEMA_NAME.to_string(),
        schema_version: CHANNEL_SCHEMA_VERSION,
        service_key: data.service_key.clone(),
        tune: data.tune.clone(),
        cas: data.cas.clone(),
        diagnostics: ChannelDiagnosticsV1 {
            channel_registration_ready: data.diagnostics.channel_registration_ready,
            epg_publishable: data.diagnostics.epg_publishable,
            publish_state_source: "TRUNCATED_WITH_CAS_STATE".to_string(),
            raw_provider_data_extensions: Vec::new(),
            provider_data_truncated: None,
            provider_data_hard_limit_bytes: None,
            provider_data_soft_limit_bytes: None,
            provider_data_dropped_count: None,
            provider_data_truncation_code: None,
        },
    }
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

#[cfg(test)]
mod provider_data_tests {
    use super::*;

    fn minimal_program_json(extra_top_level: &str) -> String {
        format!(r#"{{
            "schema":"maleicacid.tv.program",
            "schemaVersion":1,
            "programKey":{{"kind":"arib-event-v1","originalNetworkId":4,"transportStreamId":16400,"serviceId":101,"eventId":12345}},
            "serviceKey":{{"originalNetworkId":4,"transportStreamId":16400,"serviceId":101}},
            "timing":{{"startUtcMillis":1730000000000,"endUtcMillis":1730001800000,"durationMillis":1800000}},
            "source":{{"pid":18,"tableId":78,"version":12,"sectionNumber":0,"lastSectionNumber":1}},
            "cas":{{"requiresCas":false,"unsupportedCas":false,"clearLivePlaybackSupported":true,"source":"CURRENT_DIAGNOSTIC"}},
            "ratings":[],
            "genres":[],
            "series":null,
            "relatedItems":[],
            "linkage":[],
            "freeCaMode":{{"raw":0,"scrambled":false,"parseStatus":"OK"}},
            "audioLanguages":[],
            "audio":null,
            "video":null,
            "extendedItems":[],
            "components":{{"video":[],"audio":[],"subtitle":[],"data":[]}},
            "diagnostics":{{"descriptorDiagnostics":[],"publishDiagnostics":[],"parserDiagnostics":[]}}
            {}
        }}"#, extra_top_level)
    }

    #[test]
    fn extended_event_item_allows_empty_description_when_text_exists() {
        let item = ExtendedItemV1 { description: String::new(), text: "本文".to_string(), parse_status: "OK".to_string() };
        assert!(valid_extended_item(&item));
    }

    #[test]
    fn normalize_program_provider_data_preserves_top_level_unknown_key() {
        let result = normalize_program_provider_data(minimal_program_json(",\"futureVendorKey\":{\"x\":1}").as_bytes());
        let value: serde_json::Value = serde_json::from_str(&result.json).unwrap();
        let extensions = value["diagnostics"]["rawProviderDataExtensions"].as_array().unwrap();
        assert_eq!(extensions[0]["key"], "futureVendorKey");
        assert_eq!(extensions[0]["value"]["x"], 1);
        assert!(value.get("futureVendorKey").is_none());
    }

    #[test]
    fn oversized_program_provider_data_records_truncation_diagnostic() {
        let mut data: ProgramProviderDataV1 = serde_json::from_str(&minimal_program_json("")).unwrap();
        data.extended_items = (0..200)
            .map(|i| ExtendedItemV1 { description: String::new(), text: format!("{}{}", i, "x".repeat(512)), parse_status: "OK".to_string() })
            .collect();
        let result = finalize_program(data);
        let value: serde_json::Value = serde_json::from_str(&result.json).unwrap();
        assert_eq!(value["diagnostics"]["providerDataTruncated"], true);
        assert_eq!(value["diagnostics"]["publishDiagnostics"][0]["code"], "PROVIDER_DATA_TRUNCATED");
        assert!(value["diagnostics"]["providerDataDroppedCount"].as_i64().unwrap() > 0);
    }
}
