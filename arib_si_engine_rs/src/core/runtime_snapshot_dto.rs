use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SiParseStatusDto {
    Ok,
    MalformedLength,
    TruncatedDescriptor,
    UnsupportedValue,
    InvalidSequence,
    Unresolved,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EitTimingStateDto {
    Defined,
    UndefinedTime,
    BothTimingUndefined,
    MalformedTiming,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ElementaryStreamKindDto {
    Video,
    Audio,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BroadcastSystemDto {
    IsdbT,
    IsdbSBs,
    IsdbS110Cs,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SmdSemanticStateDto {
    SupportedBroadcast,
    NonBroadcast,
    UndefinedBroadcastClass,
    UnsupportedBroadcastSystem,
    UndeterminedSmd,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CaDescriptorScopeDto {
    Program,
    Es,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CaMetadataSourceDto {
    Program,
    ElementaryStream,
    Cat,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceKeyDto {
    pub original_network_id: i32,
    pub transport_stream_id: i32,
    pub service_id: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastClockDto {
    pub table_id: i32,
    pub mjd: i32,
    pub millis_of_day: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableRequirementDto {
    pub component: String,
    pub original_network_id: Option<i32>,
    pub transport_stream_id: Option<i32>,
    pub service_id: Option<i32>,
    pub required: bool,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaMetadataDto {
    pub service_key: Option<ServiceKeyDto>,
    pub ca_system_id: i32,
    pub ecm_pid: Option<i32>,
    pub emm_pid: Option<i32>,
    pub elementary_pid: Option<i32>,
    pub private_data_hex: String,
    pub source: CaMetadataSourceDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MalformedCaDescriptorDiagnosticDto {
    pub pid: i32,
    pub table_id: i32,
    pub table_id_extension: Option<i32>,
    pub service_id: Option<i32>,
    pub elementary_pid: Option<i32>,
    pub scope: String,
    pub offset: i32,
    pub declared_length: i32,
    pub actual_remaining_length: i32,
    pub reason: String,
    pub raw_prefix_hex: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MalformedCaDescriptorCountDto {
    pub service_id: i32,
    pub count: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportSemanticFactsDto {
    pub original_network_id: i32,
    pub transport_stream_id: i32,
    pub network_name: Option<String>,
    pub transport_stream_name: Option<String>,
    pub remote_control_key_id: Option<i32>,
    pub sdt_actual: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvcSignalingDto {
    pub profile_idc: i32,
    pub constraint_flags: i32,
    pub level_idc: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioConfigHeaderDto {
    pub audio_object_type: i32,
    pub sampling_frequency: i32,
    pub channel_configuration: i32,
    pub extension_sampling_frequency: Option<i32>,
    pub core_audio_object_type: Option<i32>,
    pub channel_count: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecFactsDto {
    pub avc: Option<AvcSignalingDto>,
    pub audio_config_hex: Option<String>,
    pub audio_config_header: Option<AudioConfigHeaderDto>,
    pub raw_descriptors_hex: Option<String>,
    pub profile_level: Option<String>,
    pub resolved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementaryStreamDto {
    pub elementary_pid: i32,
    pub stream_type: i32,
    pub component_tag: Option<i32>,
    pub component_type: Option<i32>,
    pub stream_content: Option<i32>,
    pub language_codes: Vec<String>,
    pub data_component_id: Option<i32>,
    pub caption_dmf: Option<i32>,
    pub caption_timing: Option<i32>,
    pub automatic_presentation_on_reception: Option<bool>,
    pub is_caption: bool,
    pub is_superimpose: bool,
    pub codec: Option<String>,
    pub codec_kind: Option<ElementaryStreamKindDto>,
    pub codec_facts: CodecFactsDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceCaDescriptorDto {
    pub ca_system_id: i32,
    pub ca_pid: i32,
    pub scope: CaDescriptorScopeDto,
    pub es_pid: Option<i32>,
    pub raw_descriptor_hex: String,
    pub private_data_hex: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmdSemanticFactsDto {
    pub descriptor_present: bool,
    pub syntax_valid: bool,
    pub system_management_id: Option<i32>,
    pub broadcasting_flag: Option<i32>,
    pub broadcasting_identifier: Option<i32>,
    pub broadcast_system: Option<BroadcastSystemDto>,
    pub additional_broadcasting_identification: Option<i32>,
    pub additional_identification_info_hex: String,
    pub semantic_state: SmdSemanticStateDto,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSemanticFactsDto {
    pub original_network_id: i32,
    pub transport_stream_id: i32,
    pub service_id: i32,
    pub service_type: Option<i32>,
    pub partial_reception: bool,
    pub pmt_pid_resolved: bool,
    pub pmt_parsed: bool,
    pub pcr_pid_resolved: bool,
    pub elementary_streams: Vec<ElementaryStreamDto>,
    pub requires_cas: bool,
    pub cas_facts_canonical_json: Option<String>,
    pub ca_descriptors_resolved: bool,
    pub free_ca_mode: Option<bool>,
    pub smd: SmdSemanticFactsDto,
    pub missing_components: Vec<String>,
    pub semantic_diagnostics: Vec<String>,
    pub name: Option<String>,
    pub provider_name: Option<String>,
    pub pmt_pid: Option<i32>,
    pub pcr_pid: Option<i32>,
    pub service_scoped_ca_descriptors: Vec<ServiceCaDescriptorDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EitInstanceDto {
    pub original_network_id: i32,
    pub transport_stream_id: i32,
    pub service_id: i32,
    pub table_id: i32,
    pub version: i32,
    pub current_next_indicator: bool,
    pub last_section_number: i32,
    pub received_sections: Vec<i32>,
    pub missing_sections: Vec<i32>,
    pub safe_sections: Vec<i32>,
    pub complete: bool,
    pub inconsistent: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParserDiagnosticDto {
    pub code: String,
    pub message: String,
    pub severity: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortEventDto {
    pub language_code: String,
    pub title: String,
    pub text: String,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendedTextDto {
    pub language_code: String,
    pub text: String,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendedItemDto {
    pub language_code: String,
    pub description: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentGenreDto {
    pub level1: i32,
    pub level2: i32,
    pub user_nibble: i32,
    pub arib_name: String,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventGroupReferenceDto {
    pub service_id: i32,
    pub event_id: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherNetworkEventGroupReferenceDto {
    pub original_network_id: i32,
    pub transport_stream_id: i32,
    pub service_id: i32,
    pub event_id: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventGroupDto {
    pub group_type: i32,
    pub events: Vec<EventGroupReferenceDto>,
    pub other_network_events: Vec<OtherNetworkEventGroupReferenceDto>,
    pub private_data_hex: String,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentGroupDto {
    pub component_group_id: i32,
    pub component_tags: Vec<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentGroupDescriptorDto {
    pub component_group_type: i32,
    pub groups: Vec<ComponentGroupDto>,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkageDto {
    pub linkage_type: i32,
    pub original_network_id: i32,
    pub transport_stream_id: i32,
    pub service_id: i32,
    pub private_data_prefix_hex: String,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeCaModeDto {
    pub raw: Option<i32>,
    pub scrambled: Option<bool>,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesDto {
    pub series_id: Option<i32>,
    pub repeat_label: i32,
    pub program_pattern: i32,
    pub expire_date_valid: bool,
    pub expire_date: Option<i32>,
    pub episode_number: Option<i32>,
    pub last_episode_number: Option<i32>,
    pub name: Option<String>,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoComponentDto {
    pub stream_content: Option<i32>,
    pub component_tag: Option<i32>,
    pub component_type: Option<i32>,
    pub language: Option<String>,
    pub text: Option<String>,
    pub source_descriptor: Option<String>,
    pub resolution: Option<String>,
    pub scan: Option<String>,
    pub aspect: Option<String>,
    pub profile_level: Option<String>,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioComponentDto {
    pub stream_type: Option<i32>,
    pub stream_content: Option<i32>,
    pub component_tag: Option<i32>,
    pub component_type: Option<i32>,
    pub language: Option<String>,
    pub second_language: Option<String>,
    pub channel_configuration: Option<String>,
    pub simulcast_group_tag: Option<i32>,
    pub sampling_rate: Option<i32>,
    pub sampling_info: Option<String>,
    pub text: Option<String>,
    pub source_descriptor: Option<String>,
    pub main: Option<bool>,
    pub multi_lingual: Option<bool>,
    pub quality_indicator: Option<i32>,
    pub parse_status: SiParseStatusDto,
    pub channel_count: Option<i32>,
    pub sample_rate_hz: Option<i32>,
    pub audio_description: Option<bool>,
    pub hard_of_hearing: Option<bool>,
    pub dual_mono: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentsDto {
    pub video: Vec<VideoComponentDto>,
    pub audio: Vec<AudioComponentDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncatedDescriptorLoopDto {
    pub declared_length: i32,
    pub raw_bytes_hex: String,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptorDiagnosticScopeDto {
    pub pid: Option<i32>,
    pub table_id: Option<i32>,
    pub table_id_extension: Option<i32>,
    pub version: Option<i32>,
    pub section_number: Option<i32>,
    pub original_network_id: Option<i32>,
    pub transport_stream_id: Option<i32>,
    pub service_id: Option<i32>,
    pub event_id: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptorDiagnosticDescriptorDto {
    pub tag: i32,
    pub name: Option<String>,
    pub offset: i32,
    pub declared_length: i32,
    pub actual_remaining_length: i32,
    pub parse_status: String,
    pub raw_prefix_hex: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptorDiagnosticDto {
    pub schema: String,
    pub schema_version: i32,
    pub severity: String,
    pub code: String,
    pub scope: DescriptorDiagnosticScopeDto,
    pub descriptor: DescriptorDiagnosticDescriptorDto,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDiagnosticsDto {
    pub summary: String,
    pub descriptor_diagnostics: Vec<DescriptorDiagnosticDto>,
    pub descriptor_diagnostics_canonical_json: String,
    pub descriptor_facts_canonical_json: Option<String>,
    pub text_diagnostics: Vec<String>,
    pub truncated_descriptor_loop: Option<TruncatedDescriptorLoopDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDescriptorsDto {
    pub short_events: Vec<ShortEventDto>,
    pub extended_texts: Vec<ExtendedTextDto>,
    pub extended_items: Vec<ExtendedItemDto>,
    pub component_text: Option<String>,
    pub audio_component_text: Option<String>,
    pub content_genres: Vec<ContentGenreDto>,
    pub genre_supplement_text: Option<String>,
    pub event_groups: Vec<EventGroupDto>,
    pub component_groups: Vec<ComponentGroupDescriptorDto>,
    pub linkage: Vec<LinkageDto>,
    pub free_ca_mode: Option<FreeCaModeDto>,
    pub series: Option<SeriesDto>,
    pub series_candidates: Vec<SeriesDto>,
    pub series_candidates_canonical_json: Option<String>,
    pub parental_ratings: Vec<ParentalRatingDto>,
    pub components: ComponentsDto,
    pub diagnostics: EventDiagnosticsDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentalRatingDto {
    pub country_code: String,
    pub raw_rating_byte: i32,
    pub parse_status: SiParseStatusDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramSourceDto {
    pub pid: i32,
    pub table_id: i32,
    pub version: i32,
    pub section_number: i32,
    pub last_section_number: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDto {
    pub service_key: ServiceKeyDto,
    pub stable_identity: Option<String>,
    pub event_id: i32,
    pub timing_state: EitTimingStateDto,
    pub raw_start_time_hex: String,
    pub raw_duration_hex: String,
    pub start_time_millis: i64,
    pub duration_millis: i64,
    pub title: String,
    pub description: String,
    pub extended_description: String,
    pub event_scope: String,
    pub source: ProgramSourceDto,
    pub descriptors: EventDescriptorsDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSnapshotDto {
    pub collection_generation: i64,
    pub ingest_sequence: i64,
    pub discovery_stage: i32,
    pub broadcast_clock: Option<BroadcastClockDto>,
    pub table_requirements: Vec<TableRequirementDto>,
    pub cat_ca_metadata: Vec<CaMetadataDto>,
    pub malformed_ca_descriptor_diagnostics: Vec<MalformedCaDescriptorDiagnosticDto>,
    pub malformed_ca_descriptor_counts: Vec<MalformedCaDescriptorCountDto>,
    pub transport_semantic_facts: Vec<TransportSemanticFactsDto>,
    pub events: Vec<EventDto>,
    pub eit_instances: Vec<EitInstanceDto>,
    pub service_semantic_facts: Vec<ServiceSemanticFactsDto>,
    pub parser_diagnostics: Vec<ParserDiagnosticDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiCollectionSnapshotDto {
    pub discovery_stage: i32,
    pub table_requirements: Vec<TableRequirementDto>,
    pub transport_semantic_facts: Vec<TransportSemanticFactsDto>,
    pub eit_instances: Vec<EitInstanceDto>,
    pub service_semantic_facts: Vec<ServiceSemanticFactsDto>,
    pub parser_diagnostics: Vec<ParserDiagnosticDto>,
}
