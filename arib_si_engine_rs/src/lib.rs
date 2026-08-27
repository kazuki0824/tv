mod arib_string;
mod ca_descriptor;
mod descriptors;
mod discovery_requirements;
mod eit;
pub(crate) mod provider_data;
mod sections;
mod service_discovery;

use ca_descriptor::{CaDescriptor, MalformedCaDescriptorDiagnostic};
use descriptors::{
    event_descriptor_diagnostic, event_descriptor_diagnostics_array_json_scoped,
    event_provider_fields, json_escape, DescriptorSectionScope,
};
use discovery_requirements::DiscoveryProfile;
use eit::{EitEvent, EitStableEventIdentity, EitUpdateWindow};
use jni::objects::{JByteArray, JObject, JString};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;
use provider_data as provider_data_api;
use serde::Serialize;
use sections::{parse_section_header, section_crc_valid};
use service_discovery::{
    DiscoveredElementaryStream, DiscoveredService, DiscoveryPublishStage, ServiceDiscoveryCollector,
    ServiceSemanticFacts, TableRequirementStatus,
};
use std::collections::BTreeMap;
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

const STATUS_OK: jint = 0;
const STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE: jint = 1;
const STATUS_INVALID_HANDLE: jint = -1;
const STATUS_INVALID_PID: jint = -2;
const STATUS_INVALID_SECTION: jint = -3;
const STATUS_MALFORMED_DESCRIPTOR: jint = -4;
const STATUS_JNI_ERROR: jint = -6;
const STATUS_INTERNAL_ERROR: jint = -7;
const STATUS_INVALID_DISCOVERY_PROFILE: jint = -8;

const DISCOVERY_STAGE_INCOMPLETE: jint = 0;
const DISCOVERY_STAGE_PARTIAL: jint = 1;
const DISCOVERY_STAGE_COMPLETE: jint = 2;

#[derive(Default)]
struct ParserState {
    collector: ServiceDiscoveryCollector,
    sections_seen: u64,
    last_status: jint,
}

impl ParserState {
    fn is_section_for_discovery(&self, pid: u16, table_id: u8) -> bool {
        is_fixed_pid_si_table_for_discovery(pid, table_id)
            || (table_id == 0x02 && self.collector.is_known_pmt_pid(pid))
    }

    fn ingest_section(&mut self, pid: u16, section: &[u8]) -> jint {
        let Some(header) = parse_section_header(section) else {
            self.last_status = STATUS_INVALID_SECTION;
            return STATUS_INVALID_SECTION;
        };
        if header.total_length != section.len() {
            self.last_status = STATUS_INVALID_SECTION;
            return STATUS_INVALID_SECTION;
        }

        self.sections_seen = self.sections_seen.saturating_add(1);
        let table_id = header.table_id;
        if self.is_section_for_discovery(pid, table_id) {
            if header.current_next_indicator == Some(false) {
                self.last_status = STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE;
                return STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE;
            }
            if header.syntax && !section_crc_valid(section) {
                self.last_status = STATUS_INVALID_SECTION;
                return STATUS_INVALID_SECTION;
            }
            let malformed_descriptor_loop = section_has_malformed_descriptor_loop(
                pid,
                table_id,
                section,
                self.collector.is_known_pmt_pid(pid),
            );
            // 不正descriptor loopは診断付き入力として扱い、意味解析前に
            // section全体を破棄する理由にはしない。復旧不能なsection length / CRC errorは上で拒否する。
            self.collector.push_section(pid, section);
            self.last_status = if malformed_descriptor_loop {
                STATUS_MALFORMED_DESCRIPTOR
            } else {
                STATUS_OK
            };
            self.last_status
        } else {
            self.last_status = STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE;
            STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE
        }
    }

    #[cfg(test)]
    fn snapshot(&self) -> service_discovery::DiscoverySnapshot {
        self.collector.state().snapshot
    }

    #[cfg(test)]
    fn services(&self) -> Vec<DiscoveredService> {
        self.snapshot().services
    }

    fn events(&self) -> Vec<EitEvent> {
        self.collector.events()
    }

    fn take_epg_update_windows(&mut self) -> Vec<EitUpdateWindow> {
        self.collector.take_epg_update_windows()
    }

    fn sdt_actual_transport_keys(&self) -> Vec<(u16, u16)> {
        self.collector.sdt_actual_transport_keys()
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ElementaryStreamDto {
    elementary_pid: u16,
    stream_type: u8,
    component_tag: Option<u8>,
    component_type: Option<u8>,
    stream_content: Option<u8>,
    language_codes: Vec<String>,
    data_component_id: Option<u16>,
    is_caption: bool,
    is_superimpose: bool,
}

impl From<&DiscoveredElementaryStream> for ElementaryStreamDto {
    fn from(stream: &DiscoveredElementaryStream) -> Self {
        Self {
            elementary_pid: stream.elementary_pid,
            stream_type: stream.stream_type,
            component_tag: stream.component_tag,
            component_type: stream.component_type,
            stream_content: stream.stream_content,
            language_codes: stream.language_codes.clone(),
            data_component_id: stream.data_component_id,
            is_caption: stream.is_caption,
            is_superimpose: stream.is_superimpose,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceCaDescriptorDto {
    ca_system_id: u16,
    ca_pid: u16,
    scope: &'static str,
    es_pid: Option<u16>,
    raw_descriptor_hex: String,
    private_data_hex: String,
}

fn service_ca_descriptor_dto(
    ca: &CaDescriptor,
    scope: &'static str,
    es_pid: Option<u16>,
) -> ServiceCaDescriptorDto {
    ServiceCaDescriptorDto {
        ca_system_id: ca.ca_system_id,
        ca_pid: ca.ca_pid,
        scope,
        es_pid,
        raw_descriptor_hex: hex_lower(&ca.raw_descriptor),
        private_data_hex: hex_lower(&ca.private_data),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceDto {
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
    name: String,
    provider_name: String,
    service_type: Option<u8>,
    pmt_pid: Option<u16>,
    pcr_pid: Option<u16>,
    free_ca_mode: Option<bool>,
    streams: Vec<ElementaryStreamDto>,
    service_scoped_ca_descriptors: Vec<ServiceCaDescriptorDto>,
}

impl From<&DiscoveredService> for ServiceDto {
    fn from(service: &DiscoveredService) -> Self {
        let mut ca = service
            .program_ca_descriptors
            .iter()
            .map(|descriptor| service_ca_descriptor_dto(descriptor, "PROGRAM", None))
            .collect::<Vec<_>>();
        for group in &service.es_ca_descriptors {
            ca.extend(group.descriptors.iter().map(|descriptor| {
                service_ca_descriptor_dto(descriptor, "ES", Some(group.elementary_pid))
            }));
        }
        Self {
            original_network_id: service.original_network_id,
            transport_stream_id: service.transport_stream_id,
            service_id: service.service_id,
            name: service.service_name.clone().unwrap_or_default(),
            provider_name: service.provider_name.clone().unwrap_or_default(),
            service_type: service.service_type,
            pmt_pid: service.pmt_pid,
            pcr_pid: service.pcr_pid,
            free_ca_mode: service.free_ca_mode,
            streams: service.streams.iter().map(ElementaryStreamDto::from).collect(),
            service_scoped_ca_descriptors: ca,
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceKeyDto {
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TransportKeyDto {
    original_network_id: u16,
    transport_stream_id: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PmtPidMappingDto {
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
    pmt_pid: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaMetadataDto {
    service_key: Option<ServiceKeyDto>,
    ca_system_id: u16,
    ecm_pid: Option<u16>,
    emm_pid: Option<u16>,
    elementary_pid: Option<u16>,
    private_data_hex: String,
    source: &'static str,
}

fn ca_metadata_dto(
    service_key: Option<ServiceKeyDto>,
    ca: &CaDescriptor,
    ecm_pid: Option<u16>,
    emm_pid: Option<u16>,
    elementary_pid: Option<u16>,
    source: &'static str,
) -> CaMetadataDto {
    CaMetadataDto {
        service_key,
        ca_system_id: ca.ca_system_id,
        ecm_pid,
        emm_pid,
        elementary_pid,
        private_data_hex: hex_lower(&ca.private_data),
        source,
    }
}

fn ca_metadata_from_services(
    services: &[DiscoveredService],
    cat: &[CaDescriptor],
) -> Vec<CaMetadataDto> {
    let mut out = Vec::new();
    for service in services {
        let key = Some(ServiceKeyDto {
            original_network_id: service.original_network_id,
            transport_stream_id: service.transport_stream_id,
            service_id: service.service_id,
        });
        out.extend(service.program_ca_descriptors.iter().map(|ca| {
            ca_metadata_dto(key, ca, Some(ca.ca_pid), None, None, "PROGRAM")
        }));
        for group in &service.es_ca_descriptors {
            out.extend(group.descriptors.iter().map(|ca| {
                ca_metadata_dto(
                    key,
                    ca,
                    Some(ca.ca_pid),
                    None,
                    Some(group.elementary_pid),
                    "ELEMENTARY_STREAM",
                )
            }));
        }
    }
    out.extend(
        cat.iter()
            .map(|ca| ca_metadata_dto(None, ca, None, Some(ca.ca_pid), None, "CAT")),
    );
    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MalformedCaDescriptorDiagnosticDto {
    pid: u16,
    table_id: u8,
    table_id_extension: Option<u16>,
    service_id: Option<u16>,
    elementary_pid: Option<u16>,
    scope: &'static str,
    offset: usize,
    declared_length: usize,
    actual_remaining_length: usize,
    reason: &'static str,
    raw_prefix_hex: String,
}

impl From<&MalformedCaDescriptorDiagnostic> for MalformedCaDescriptorDiagnosticDto {
    fn from(diagnostic: &MalformedCaDescriptorDiagnostic) -> Self {
        Self {
            pid: diagnostic.pid,
            table_id: diagnostic.table_id,
            table_id_extension: diagnostic.table_id_extension,
            service_id: diagnostic.service_id,
            elementary_pid: diagnostic.elementary_pid,
            scope: diagnostic.scope,
            offset: diagnostic.offset,
            declared_length: diagnostic.declared_length,
            actual_remaining_length: diagnostic.actual_remaining_length,
            reason: diagnostic.reason,
            raw_prefix_hex: diagnostic.raw_prefix_hex.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MalformedCaDescriptorCountDto {
    service_id: u16,
    count: usize,
}

fn malformed_ca_descriptor_counts(
    diagnostics: &[MalformedCaDescriptorDiagnostic],
) -> Vec<MalformedCaDescriptorCountDto> {
    let mut counts: BTreeMap<u16, usize> = BTreeMap::new();
    for d in diagnostics.iter().filter(|d| d.service_id.is_some()) {
        *counts.entry(d.service_id.unwrap_or_default()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(service_id, count)| MalformedCaDescriptorCountDto { service_id, count })
        .collect()
}

fn extended_items_value(event: &EitEvent) -> serde_json::Value {
    serde_json::Value::Array(
        event
            .descriptors
            .extended_items
            .iter()
            .map(|item| {
                serde_json::json!({
                    "languageCode": item.language_code,
                    "description": item.item_description,
                    "text": item.item_text,
                })
            })
            .collect(),
    )
}

fn event_component_text(event: &EitEvent) -> String {
    event
        .descriptors
        .components
        .iter()
        .map(|c| c.text.clone())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn event_audio_component_text(event: &EitEvent) -> String {
    event
        .descriptors
        .audio_components
        .iter()
        .map(|a| a.text.clone())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn event_audio_language(event: &EitEvent) -> String {
    let mut langs = Vec::new();
    for audio in &event.descriptors.audio_components {
        if !audio.language_code.is_empty() && !langs.contains(&audio.language_code) {
            langs.push(audio.language_code.clone());
        }
        if let Some(second) = &audio.language_code_2 {
            if !second.is_empty() && !langs.contains(second) {
                langs.push(second.clone());
            }
        }
    }
    langs.join(",")
}

fn event_primary_series_value(event: &EitEvent) -> serde_json::Value {
    let Some(series) = event.descriptors.series.first() else {
        return serde_json::Value::Null;
    };
    let expire_date_valid = series.expire_date != 0x1fff;
    serde_json::json!({
        "seriesId": series.series_id,
        "repeatLabel": series.repeat_label,
        "programPattern": series.program_pattern,
        "expireDateValid": expire_date_valid,
        "expireDate": serde_json::Value::Null,
        "episodeNumber": series.episode_number,
        "lastEpisodeNumber": series.last_episode_number,
        "name": if series.series_name.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(series.series_name.clone())
        },
        "parseStatus": "OK",
    })
}

fn event_groups_value(event: &EitEvent) -> serde_json::Value {
    serde_json::Value::Array(
        event
            .descriptors
            .event_groups
            .iter()
            .map(|group| {
                let events = group
                    .events
                    .iter()
                    .map(|related| {
                        serde_json::json!({
                            "serviceId": related.service_id,
                            "eventId": related.event_id,
                        })
                    })
                    .collect::<Vec<_>>();
                let other_network_events = group
                    .other_network_events
                    .iter()
                    .map(|related| {
                        serde_json::json!({
                            "originalNetworkId": related.original_network_id,
                            "transportStreamId": related.transport_stream_id,
                            "serviceId": related.service_id,
                            "eventId": related.event_id,
                        })
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({
                    "groupType": group.group_type,
                    "events": events,
                    "otherNetworkEvents": other_network_events,
                    "privateDataHex": hex_lower(&group.private_data),
                    "parseStatus": "OK",
                })
            })
            .collect(),
    )
}

fn event_linkage_value(event: &EitEvent) -> serde_json::Value {
    serde_json::Value::Array(
        event
            .descriptors
            .linkages
            .iter()
            .map(|linkage| {
                serde_json::json!({
                    "transportStreamId": linkage.transport_stream_id,
                    "originalNetworkId": linkage.original_network_id,
                    "serviceId": linkage.service_id,
                    "linkageType": linkage.linkage_type,
                    "privateDataPrefixHex": hex_prefix(&linkage.private_data, 16),
                    "parseStatus": "OK",
                })
            })
            .collect(),
    )
}

fn hex_prefix(bytes: &[u8], max_len: usize) -> String {
    bytes
        .iter()
        .take(max_len)
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join("")
}

fn event_content_genres_value(event: &EitEvent) -> serde_json::Value {
    serde_json::Value::Array(
        event
            .descriptors
            .contents
            .iter()
            .map(|content| {
                serde_json::json!({
                    "level1": content.content_nibble_level_1,
                    "level2": content.content_nibble_level_2,
                    "userNibble": ((content.user_nibble_1 as u16) << 4) | content.user_nibble_2 as u16,
                    "aribName": content.arib_display_name,
                    "parseStatus": "OK",
                })
            })
            .collect(),
    )
}

fn event_genre_supplement_text(event: &EitEvent) -> String {
    event
        .descriptors
        .contents
        .iter()
        .map(|c| c.arib_display_name.clone())
        .collect::<Vec<_>>()
        .join("、")
}

fn event_diagnostic_text(event: &EitEvent) -> String {
    let d = event.descriptors.clone();
    let diagnostic = event_descriptor_diagnostic(&d);
    format!(
        "content={:?} component={:?} audio={:?} parental={:?} series={:?} eventGroupCount={} linkageCount={} unknownCount={}",
        d.contents.iter().map(|c| (c.content_nibble_level_1, c.content_nibble_level_2)).collect::<Vec<_>>(),
        d.components.iter().map(|c| (c.stream_content, c.component_type, c.component_tag, c.language_code.clone())).collect::<Vec<_>>(),
        d.audio_components.iter().map(|a| (a.stream_content, a.component_type, a.component_tag, a.stream_type, a.language_code.clone(), a.language_code_2.clone())).collect::<Vec<_>>(),
        d.parental_ratings.iter().map(|r| (r.country_code.clone(), r.raw_rating_byte)).collect::<Vec<_>>(),
        d.series.iter().map(|s| (s.series_id, s.episode_number, s.last_episode_number, s.series_name.clone())).collect::<Vec<_>>(),
        diagnostic.event_group_count,
        diagnostic.linkage_count,
        diagnostic.unknown_count,
    )
}

fn parental_ratings_value(event: &EitEvent) -> serde_json::Value {
    serde_json::Value::Array(
        event
            .descriptors
            .parental_ratings
            .iter()
            .map(|rating| {
                serde_json::json!({
                    "countryCode": rating.country_code,
                    "rawRatingByte": rating.raw_rating_byte,
                    "parseStatus": "OK",
                })
            })
            .collect(),
    )
}

fn event_components_value() -> serde_json::Value {
    serde_json::json!({
        "video": [],
        "audio": [],
        "subtitle": [],
        "data": [],
    })
}

fn stable_identity_string(id: EitStableEventIdentity) -> String {
    provider_data_api::build_program_key(
        i32::from(id.original_network_id),
        i32::from(id.transport_stream_id),
        i32::from(id.service_id),
        i32::from(id.event_id),
    )
}

fn json_value(text: String) -> serde_json::Value {
    serde_json::from_str(&text).unwrap_or(serde_json::Value::Null)
}

fn event_value(event: &EitEvent) -> serde_json::Value {
    let provider = event_provider_fields(&event.descriptors);
    let descriptor_diagnostics = event_descriptor_diagnostics_array_json_scoped(
        &event.descriptors,
        Some(DescriptorSectionScope {
            pid: Some(18),
            table_id: Some(event.table_id),
            table_id_extension: Some(event.service_id),
            version: Some(event.version),
            section_number: Some(event.section_number),
            original_network_id: Some(event.original_network_id),
            transport_stream_id: Some(event.transport_stream_id),
            service_id: Some(event.service_id),
            event_id: Some(event.event_id),
        }),
    );
    let stable_identity = event.stable_identity();
    let program_key = stable_identity.map(|_| {
        serde_json::json!({
            "kind": "arib-event-v1",
            "originalNetworkId": event.original_network_id,
            "transportStreamId": event.transport_stream_id,
            "serviceId": event.service_id,
            "eventId": event.event_id,
        })
    });
    serde_json::json!({
        "programKey": program_key,
        "eventId": event.event_id,
        "serviceKey": {
            "originalNetworkId": event.original_network_id,
            "transportStreamId": event.transport_stream_id,
            "serviceId": event.service_id,
        },
        "stableIdentity": stable_identity.map(stable_identity_string),
        "timing": {
            "state": event.timing_state.as_str(),
            "rawStartTimeHex": hex_lower(&event.raw_start_time),
            "rawDurationHex": hex_lower(&event.raw_duration),
            "startUtcMillis": event.start_time_millis,
            "endUtcMillis": event.start_time_millis.saturating_add(event.duration_millis),
            "durationMillis": event.duration_millis,
        },
        "title": provider.title,
        "description": provider.description,
        "extendedDescription": provider.extended_description,
        "eventScope": event.scope.as_str(),
        "source": {
            "pid": 18,
            "tableId": event.table_id,
            "version": event.version,
            "sectionNumber": event.section_number,
            "lastSectionNumber": event.last_section_number,
        },
        "descriptors": {
            "extendedItems": extended_items_value(event),
            "component": { "text": event_component_text(event) },
            "audio": { "componentText": event_audio_component_text(event), "language": event_audio_language(event) },
            "genres": { "content": event_content_genres_value(event), "genreSupplementText": event_genre_supplement_text(event) },
            "eventGroups": event_groups_value(event),
            "linkage": event_linkage_value(event),
            "freeCaMode": {
                "raw": if event.free_ca_mode { 1 } else { 0 },
                "scrambled": event.free_ca_mode,
                "text": if event.free_ca_mode { "有料放送" } else { "無料放送" },
                "parseStatus": "OK",
            },
            "series": event_primary_series_value(event),
            "components": event_components_value(),
            "diagnostics": {
                "summary": event_diagnostic_text(event),
                "descriptorDiagnostics": json_value(descriptor_diagnostics.clone()),
                "descriptorDiagnosticsCanonicalJson": descriptor_diagnostics,
            },
            "parentalRatings": parental_ratings_value(event),
        }
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EpgUpdateWindowDto {
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
    window_start_millis: i64,
    window_end_millis: i64,
    valid_program_stable_identities: Vec<String>,
    deletion_authoritative: bool,
}

impl From<&EitUpdateWindow> for EpgUpdateWindowDto {
    fn from(window: &EitUpdateWindow) -> Self {
        Self {
            original_network_id: window.original_network_id,
            transport_stream_id: window.transport_stream_id,
            service_id: window.service_id,
            window_start_millis: window.window_start_millis,
            window_end_millis: window.window_end_millis,
            valid_program_stable_identities: window
                .valid_event_identities
                .iter()
                .map(|identity| stable_identity_string(*identity))
                .collect(),
            deletion_authoritative: window.deletion_authoritative,
        }
    }
}

#[cfg(test)]
fn epg_update_window_json(window: &EitUpdateWindow) -> String {
    serde_json::to_string(&EpgUpdateWindowDto::from(window)).unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemManagementFactsDto {
    descriptor_present: bool,
    syntax_valid: bool,
    system_management_id: Option<u16>,
    broadcasting_flag: Option<u8>,
    broadcasting_identifier: Option<u8>,
    additional_broadcasting_identification: Option<u8>,
    additional_identification_info_hex: String,
    semantic_state: &'static str,
    diagnostic: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceSemanticFactsDto {
    original_network_id: u16,
    transport_stream_id: u16,
    service_id: u16,
    service_type: Option<u8>,
    pmt_pid_resolved: bool,
    pmt_parsed: bool,
    pcr_pid_resolved: bool,
    elementary_streams: Vec<ElementaryStreamDto>,
    requires_cas: bool,
    ca_descriptors_resolved: bool,
    free_ca_mode: Option<bool>,
    smd: SystemManagementFactsDto,
    missing_components: Vec<&'static str>,
    semantic_diagnostics: Vec<&'static str>,
}

impl From<&ServiceSemanticFacts> for ServiceSemanticFactsDto {
    fn from(facts: &ServiceSemanticFacts) -> Self {
        Self {
            original_network_id: facts.original_network_id,
            transport_stream_id: facts.transport_stream_id,
            service_id: facts.service_id,
            service_type: facts.service_type,
            pmt_pid_resolved: facts.pmt_pid_resolved,
            pmt_parsed: facts.pmt_parsed,
            pcr_pid_resolved: facts.pcr_pid_resolved,
            elementary_streams: facts
                .elementary_streams
                .iter()
                .map(ElementaryStreamDto::from)
                .collect(),
            requires_cas: facts.requires_cas,
            ca_descriptors_resolved: facts.ca_descriptors_resolved,
            free_ca_mode: facts.free_ca_mode,
            smd: SystemManagementFactsDto {
                descriptor_present: facts.system_management.descriptor_present,
                syntax_valid: facts.system_management.syntax_valid,
                system_management_id: facts.system_management.system_management_id,
                broadcasting_flag: facts.system_management.broadcasting_flag,
                broadcasting_identifier: facts.system_management.broadcasting_identifier,
                additional_broadcasting_identification: facts
                    .system_management
                    .additional_broadcasting_identification,
                additional_identification_info_hex: hex_lower(
                    &facts.system_management.additional_identification_info,
                ),
                semantic_state: facts.system_management.semantic_state.as_str(),
                diagnostic: facts.system_management.diagnostic,
            },
            missing_components: facts.missing_components.clone(),
            semantic_diagnostics: facts.semantic_diagnostics.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableRequirementStatusDto {
    component: &'static str,
    original_network_id: Option<u16>,
    transport_stream_id: Option<u16>,
    service_id: Option<u16>,
    required: bool,
    complete: bool,
}

impl From<&TableRequirementStatus> for TableRequirementStatusDto {
    fn from(status: &TableRequirementStatus) -> Self {
        Self {
            component: status.component,
            original_network_id: status.original_network_id,
            transport_stream_id: status.transport_stream_id,
            service_id: status.service_id,
            required: status.required,
            complete: status.complete,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BulkSnapshot {
    ingest_sequence: u64,
    discovery_stage: jint,
    table_requirements: Vec<TableRequirementStatusDto>,
    services: Vec<ServiceDto>,
    ca_metadata: Vec<CaMetadataDto>,
    malformed_ca_descriptor_diagnostics: Vec<MalformedCaDescriptorDiagnosticDto>,
    malformed_ca_descriptor_counts: Vec<MalformedCaDescriptorCountDto>,
    pmt_pid_mappings: Vec<PmtPidMappingDto>,
    sdt_actual_transports: Vec<TransportKeyDto>,
    events: Vec<serde_json::Value>,
    epg_update_windows: Vec<EpgUpdateWindowDto>,
    service_semantic_facts: Vec<ServiceSemanticFactsDto>,
    parser_diagnostics: Vec<ParserDiagnosticDto>,
}

fn bulk_snapshot_json(state: &mut ParserState, take_update_windows: bool) -> String {
    let ingest_sequence = state.sections_seen;
    let last_status = state.last_status;
    let collection_state = state.collector.state();
    let discovery_stage = collection_state.publish_stage();
    let table_requirements = &collection_state.table_requirements;
    let semantic_facts = &collection_state.semantic_facts_by_service;
    let snapshot = &collection_state.snapshot;
    let parser_diagnostics = parser_diagnostics(ingest_sequence, last_status, snapshot);
    let services = &snapshot.services;
    let pmt_mappings = &snapshot.pmt_pids_by_service;
    let cat_ca = &snapshot.cat_ca.descriptors;
    // 更新区間は排出型一括APIだけで公開する。
    // 非排出型一括snapshotはEPG更新区間を返さない。これにより本番呼び出し側が
    // 同じ廃止削除区間を誤って再公開することを防ぐ。
    let epg_windows = if take_update_windows {
        state.take_epg_update_windows()
    } else {
        Vec::new()
    };
    serde_json::to_string(&BulkSnapshot {
        ingest_sequence,
        discovery_stage: discovery_stage_to_jint(discovery_stage),
        table_requirements: table_requirements
            .iter()
            .map(TableRequirementStatusDto::from)
            .collect(),
        services: services.iter().map(ServiceDto::from).collect(),
        ca_metadata: ca_metadata_from_services(services, cat_ca),
        malformed_ca_descriptor_diagnostics: snapshot
            .malformed_ca_descriptor_diagnostics
            .iter()
            .map(MalformedCaDescriptorDiagnosticDto::from)
            .collect(),
        malformed_ca_descriptor_counts: malformed_ca_descriptor_counts(
            &snapshot.malformed_ca_descriptor_diagnostics,
        ),
        pmt_pid_mappings: pmt_mappings
            .iter()
            .map(|mapping| PmtPidMappingDto {
                original_network_id: mapping.original_network_id,
                transport_stream_id: mapping.transport_stream_id,
                service_id: mapping.service_id,
                pmt_pid: mapping.pmt_pid,
            })
            .collect(),
        sdt_actual_transports: state
            .sdt_actual_transport_keys()
            .iter()
            .map(|(tsid, onid)| TransportKeyDto {
                original_network_id: *onid,
                transport_stream_id: *tsid,
            })
            .collect(),
        events: state.events().iter().map(event_value).collect(),
        epg_update_windows: epg_windows.iter().map(EpgUpdateWindowDto::from).collect(),
        service_semantic_facts: semantic_facts
            .iter()
            .map(ServiceSemanticFactsDto::from)
            .collect(),
        parser_diagnostics,
    })
    .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ParserDiagnosticDto {
    code: &'static str,
    message: String,
    severity: &'static str,
}

fn parser_diagnostics(
    sections_seen: u64,
    last_status: jint,
    snapshot: &service_discovery::DiscoverySnapshot,
) -> Vec<ParserDiagnosticDto> {
    let message = format!(
        "sectionsSeen={} lastStatus={}",
        sections_seen, last_status
    );
    let mut diagnostics = vec![ParserDiagnosticDto {
        code: "PARSER_STATE",
        message,
        severity: "info",
    }];
    let mut text_diagnostics = snapshot
        .services
        .iter()
        .flat_map(|service| {
            service.text_decode_diagnostics.iter().map(|diagnostic| {
                format!(
                    "service={}/{}/{} {}",
                    service.original_network_id,
                    service.transport_stream_id,
                    service.service_id,
                    diagnostic,
                )
            })
        })
        .chain(snapshot.transports.iter().flat_map(|transport| {
            transport.text_decode_diagnostics.iter().map(|diagnostic| {
                format!(
                    "transport={}/{} {}",
                    transport.original_network_id, transport.transport_stream_id, diagnostic,
                )
            })
        }))
        .collect::<Vec<_>>();
    text_diagnostics.sort();
    text_diagnostics.dedup();
    diagnostics.extend(text_diagnostics.into_iter().map(|message| ParserDiagnosticDto {
        code: "ARIB_SI_TEXT_REPLACED",
        message,
        severity: "warning",
    }));
    diagnostics
}

fn section_body_end(section: &[u8]) -> Option<usize> {
    let header = parse_section_header(section)?;
    if header.section_length < 4 || header.total_length > section.len() {
        return None;
    }
    Some(3 + header.section_length - 4)
}

fn descriptor_loop_well_formed(bytes: &[u8]) -> bool {
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if cursor + 2 > bytes.len() {
            return false;
        }
        let len = bytes[cursor + 1] as usize;
        let Some(next) = cursor.checked_add(2).and_then(|v| v.checked_add(len)) else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        cursor = next;
    }
    true
}

fn section_has_malformed_descriptor_loop(
    pid: u16,
    table_id: u8,
    section: &[u8],
    known_pmt_pid: bool,
) -> bool {
    let Some(body_end) = section_body_end(section) else {
        return true;
    };
    match (pid, table_id) {
        (0x0001, 0x01) => {
            section.len() < 8 || body_end < 8 || !descriptor_loop_well_formed(&section[8..body_end])
        }
        (_, 0x02) if known_pmt_pid => {
            if section.len() < 12 || body_end < 12 || body_end > section.len() {
                return true;
            }
            let program_info_length = (((section[10] & 0x0f) as usize) << 8) | section[11] as usize;
            let Some(program_info_end) = 12usize.checked_add(program_info_length) else {
                return true;
            };
            if program_info_end > body_end
                || !descriptor_loop_well_formed(&section[12..program_info_end])
            {
                return true;
            }
            let mut cursor = program_info_end;
            while cursor < body_end {
                if cursor + 5 > body_end {
                    return true;
                }
                let es_info_length =
                    (((section[cursor + 3] & 0x0f) as usize) << 8) | section[cursor + 4] as usize;
                let Some(desc_start) = cursor.checked_add(5) else {
                    return true;
                };
                let Some(desc_end) = desc_start.checked_add(es_info_length) else {
                    return true;
                };
                if desc_end > body_end
                    || !descriptor_loop_well_formed(&section[desc_start..desc_end])
                {
                    return true;
                }
                cursor = desc_end;
            }
            false
        }
        (0x0010, 0x40) | (0x0010, 0x41) => {
            if section.len() < 10 || body_end < 10 {
                return true;
            }
            let descriptors_length = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
            let Some(network_desc_end) = 10usize.checked_add(descriptors_length) else {
                return true;
            };
            if network_desc_end > body_end
                || !descriptor_loop_well_formed(&section[10..network_desc_end])
            {
                return true;
            }
            if network_desc_end + 2 > body_end {
                return true;
            }
            let transport_loop_length = (((section[network_desc_end] & 0x0f) as usize) << 8)
                | section[network_desc_end + 1] as usize;
            let mut cursor = network_desc_end + 2;
            let Some(transport_end) = cursor.checked_add(transport_loop_length) else {
                return true;
            };
            if transport_end > body_end {
                return true;
            }
            while cursor < transport_end {
                if cursor + 6 > transport_end {
                    return true;
                }
                let desc_len =
                    (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
                let desc_start = cursor + 6;
                let Some(desc_end) = desc_start.checked_add(desc_len) else {
                    return true;
                };
                if desc_end > transport_end
                    || !descriptor_loop_well_formed(&section[desc_start..desc_end])
                {
                    return true;
                }
                cursor = desc_end;
            }
            false
        }
        (0x0011, 0x42) | (0x0011, 0x46) => {
            if section.len() < 11 || body_end < 11 {
                return true;
            }
            let mut cursor = 11usize;
            while cursor < body_end {
                if cursor + 5 > body_end {
                    return true;
                }
                let desc_len =
                    (((section[cursor + 3] & 0x0f) as usize) << 8) | section[cursor + 4] as usize;
                let desc_start = cursor + 5;
                let Some(desc_end) = desc_start.checked_add(desc_len) else {
                    return true;
                };
                if desc_end > body_end
                    || !descriptor_loop_well_formed(&section[desc_start..desc_end])
                {
                    return true;
                }
                cursor = desc_end;
            }
            false
        }
        (0x0011, 0x4a) => {
            if section.len() < 10 || body_end < 10 {
                return true;
            }
            let bouquet_desc_len = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
            let Some(bouquet_desc_end) = 10usize.checked_add(bouquet_desc_len) else {
                return true;
            };
            if bouquet_desc_end > body_end
                || !descriptor_loop_well_formed(&section[10..bouquet_desc_end])
            {
                return true;
            }
            if bouquet_desc_end + 2 > body_end {
                return true;
            }
            let transport_loop_length = (((section[bouquet_desc_end] & 0x0f) as usize) << 8)
                | section[bouquet_desc_end + 1] as usize;
            let mut cursor = bouquet_desc_end + 2;
            let Some(transport_end) = cursor.checked_add(transport_loop_length) else {
                return true;
            };
            if transport_end > body_end {
                return true;
            }
            while cursor < transport_end {
                if cursor + 6 > transport_end {
                    return true;
                }
                let desc_len =
                    (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
                let desc_start = cursor + 6;
                let Some(desc_end) = desc_start.checked_add(desc_len) else {
                    return true;
                };
                if desc_end > transport_end
                    || !descriptor_loop_well_formed(&section[desc_start..desc_end])
                {
                    return true;
                }
                cursor = desc_end;
            }
            false
        }
        _ => false,
    }
}

fn is_fixed_pid_si_table_for_discovery(pid: u16, table_id: u8) -> bool {
    matches!(
        (pid, table_id),
        (0x0000, 0x00)
            | (0x0001, 0x01)
            | (0x0010, 0x40 | 0x41)
            | (0x0011, 0x42 | 0x46 | 0x4a)
            | (0x0012, 0x4e..=0x6f)
    )
}

#[derive(Default)]
struct ParserRegistry {
    next_handle: jlong,
    parsers: BTreeMap<jlong, Arc<Mutex<ParserState>>>,
}

impl ParserRegistry {
    fn create(&mut self) -> jlong {
        self.next_handle = self.next_handle.saturating_add(1).max(1);
        let handle = self.next_handle;
        self.parsers
            .insert(handle, Arc::new(Mutex::new(ParserState::default())));
        handle
    }

    fn remove(&mut self, handle: jlong) -> bool {
        self.parsers.remove(&handle).is_some()
    }

    fn get(&self, handle: jlong) -> Option<Arc<Mutex<ParserState>>> {
        self.parsers.get(&handle).cloned()
    }
}

static REGISTRY: OnceLock<Mutex<ParserRegistry>> = OnceLock::new();

fn registry() -> &'static Mutex<ParserRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(ParserRegistry::default()))
}

fn with_state<T>(handle: jlong, default_value: T, f: impl FnOnce(&ParserState) -> T) -> T {
    let parser = match registry().lock() {
        Ok(guard) => guard.get(handle),
        Err(_) => return default_value,
    };
    let Some(parser) = parser else {
        return default_value;
    };
    let result = match parser.lock() {
        Ok(guard) => f(&guard),
        Err(_) => default_value,
    };
    result
}

fn with_state_mut(
    handle: jlong,
    default_value: jint,
    f: impl FnOnce(&mut ParserState) -> jint,
) -> jint {
    let parser = match registry().lock() {
        Ok(guard) => guard.get(handle),
        Err(_) => return STATUS_INTERNAL_ERROR,
    };
    let Some(parser) = parser else {
        return default_value;
    };
    let result = match parser.lock() {
        Ok(mut guard) => f(&mut guard),
        Err(_) => STATUS_INTERNAL_ERROR,
    };
    result
}

fn java_string(env: &mut JNIEnv<'_>, value: Option<String>) -> jstring {
    match env.new_string(value.unwrap_or_default()) {
        Ok(s) => s.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

fn discovery_stage_to_jint(stage: DiscoveryPublishStage) -> jint {
    match stage {
        DiscoveryPublishStage::Incomplete => DISCOVERY_STAGE_INCOMPLETE,
        DiscoveryPublishStage::Partial => DISCOVERY_STAGE_PARTIAL,
        DiscoveryPublishStage::Complete => DISCOVERY_STAGE_COMPLETE,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeSnapshotBulkJson(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    take_update_windows: jint,
) -> jstring {
    let parser = match registry().lock() {
        Ok(guard) => guard.get(handle),
        Err(_) => return java_string(&mut env, Some("{}".to_string())),
    };
    let Some(parser) = parser else {
        return java_string(&mut env, Some("{}".to_string()));
    };
    let json = match parser.lock() {
        Ok(mut guard) => bulk_snapshot_json(&mut guard, take_update_windows != 0),
        Err(_) => "{}".to_string(),
    };
    java_string(&mut env, Some(json))
}

fn jstring_to_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Option<String> {
    env.get_string(&value).ok().map(|s| s.into())
}

fn jbytearray_to_vec(env: &mut JNIEnv<'_>, value: JByteArray<'_>) -> Vec<u8> {
    env.convert_byte_array(value).unwrap_or_default()
}

fn provider_result_json(result: provider_data_api::ProviderDataResult) -> String {
    format!(
        "{{\"success\":{},\"bytes\":{},\"schemaVersion\":{},\"truncated\":{},\"diagnosticsDroppedCount\":{},\"errorCode\":{},\"errorMessage\":{}}}",
        if result.success { "true" } else { "false" },
        json_string(&result.json),
        result.schema_version,
        if result.truncated { "true" } else { "false" },
        result.diagnostics_dropped_count,
        json_string(&result.error_code),
        json_string(&result.error_message),
    )
}

fn program_key_result_json(result: provider_data_api::ProgramKeyResult) -> String {
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"eventId\":{},\"key\":{}}}",
        result.original_network_id,
        result.transport_stream_id,
        result.service_id,
        result.event_id,
        json_string(&result.key),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeBuildChannelProviderData(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    request_json: JString<'_>,
) -> jstring {
    let request = jstring_to_string(&mut env, request_json).unwrap_or_default();
    let result = provider_data_api::build_channel_provider_data(&request);
    java_string(&mut env, Some(provider_result_json(result)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeBuildProgramKey(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    onid: jint,
    tsid: jint,
    sid: jint,
    event_id: jint,
) -> jstring {
    java_string(
        &mut env,
        Some(provider_data_api::build_program_key(
            onid, tsid, sid, event_id,
        )),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeBuildProgramProviderData(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    request_json: JString<'_>,
) -> jstring {
    let request = jstring_to_string(&mut env, request_json).unwrap_or_default();
    let result = provider_data_api::build_program_provider_data(&request);
    java_string(&mut env, Some(provider_result_json(result)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeNormalizeProgramProviderData(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JByteArray<'_>,
) -> jstring {
    let data = jbytearray_to_vec(&mut env, provider_data);
    let result = provider_data_api::normalize_program_provider_data(&data);
    java_string(&mut env, Some(provider_result_json(result)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeExtractProgramKeyResult(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JByteArray<'_>,
) -> jstring {
    let data = jbytearray_to_vec(&mut env, provider_data);
    let json = provider_data_api::extract_program_key_result(&data).map(program_key_result_json);
    java_string(&mut env, json)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeDecodeChannelProviderData(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JByteArray<'_>,
) -> jstring {
    let data = jbytearray_to_vec(&mut env, provider_data);
    java_string(
        &mut env,
        Some(provider_data_api::decode_channel_provider_data(
            data.as_slice(),
        )),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeCreate(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jlong {
    match registry().lock() {
        Ok(mut guard) => guard.create(),
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeDestroy(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    if handle == 0 {
        return STATUS_INVALID_HANDLE;
    }
    match registry().lock() {
        Ok(mut guard) => {
            if guard.remove(handle) {
                STATUS_OK
            } else {
                STATUS_INVALID_HANDLE
            }
        }
        Err(_) => STATUS_INTERNAL_ERROR,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeIngestSection(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    pid: jint,
    section: JByteArray<'_>,
) -> jint {
    if !(0..=0x1fff).contains(&pid) {
        return STATUS_INVALID_PID;
    }
    let section = match env.convert_byte_array(section) {
        Ok(v) => v,
        Err(_) => return STATUS_JNI_ERROR,
    };
    with_state_mut(handle, STATUS_INVALID_HANDLE, |state| {
        state.ingest_section(pid as u16, &section)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeLastStatus(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| state.last_status)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeSetDiscoveryProfile(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    profile: jint,
) -> jint {
    let profile = match profile {
        0 => DiscoveryProfile::IsdbT,
        1 => DiscoveryProfile::Bs,
        2 => DiscoveryProfile::Cs110,
        _ => return STATUS_INVALID_DISCOVERY_PROFILE,
    };
    with_state_mut(handle, STATUS_INVALID_HANDLE, |state| {
        state.collector.set_discovery_profile(profile);
        STATUS_OK
    })
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeDecodeAribString(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    bytes: JByteArray<'_>,
) -> jstring {
    let decoded = match env.convert_byte_array(bytes) {
        Ok(v) => arib_string::decode_arib_string_lossy(&v).0,
        Err(_) => String::new(),
    };
    java_string(&mut env, Some(decoded))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeDecodeAribStringDiagnosticSummary(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    bytes: JByteArray<'_>,
) -> jstring {
    let summary = match env.convert_byte_array(bytes) {
        Ok(v) => arib_string::decode_arib_string_lossy(&v).1.summary(),
        Err(_) => String::from("scope=mirakc_scope_non_caption_si_epg_only replacement_count=0 unsupported_escape_count=0 truncated_escape_count=0 truncated_graphic_count=0 entries=[]"),
    };
    java_string(&mut env, Some(summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::crc32_mpeg;

    fn section_with_crc(mut body: Vec<u8>) -> Vec<u8> {
        let crc = crc32_mpeg(&body);
        body.extend_from_slice(&crc.to_be_bytes());
        body
    }

    fn minimal_event_for_related_items(group_type: u8, event_id: u16) -> EitEvent {
        EitEvent {
            diagnostics: Vec::new(),
            table_id: 0x4e,
            version: 0,
            section_number: 0,
            last_section_number: 0,
            scope: crate::eit::EitScope::PresentFollowingActual,
            service_id: 101,
            transport_stream_id: 16625,
            original_network_id: 4,
            event_id: 300,
            timing_state: crate::eit::EitTimingState::Defined,
            raw_start_time: [0; 5],
            raw_duration: [0; 3],
            start_time_millis: 1,
            duration_millis: 1,
            free_ca_mode: false,
            descriptors: crate::descriptors::EventDescriptors {
                event_groups: vec![crate::descriptors::EventGroupDescriptor {
                    group_type,
                    events: vec![crate::descriptors::EventGroupReference {
                        service_id: 101,
                        event_id,
                    }],
                    other_network_events: Vec::new(),
                    private_data: vec![],
                }],
                ..crate::descriptors::EventDescriptors::default()
            },
        }
    }

    #[test]
    fn event_group_json_preserves_raw_group_type_without_derived_kind() {
        for group_type in 1u8..=5 {
            let value = event_groups_value(&minimal_event_for_related_items(
                group_type,
                0x0100 + group_type as u16,
            ));
            let group = &value[0];
            assert_eq!(group["groupType"].as_u64(), Some(u64::from(group_type)));
            assert!(group["events"].is_array());
            assert!(group.get("kind").is_none());
        }
    }

    #[test]
    fn epg_update_window_json_exports_deletion_authoritative_for_tis() {
        let window = EitUpdateWindow {
            original_network_id: 4,
            transport_stream_id: 16625,
            service_id: 101,
            window_start_millis: 1_700_000_000_000,
            window_end_millis: 1_700_001_800_000,
            valid_event_identities: Vec::new(),
            deletion_authoritative: true,
        };
        let json = epg_update_window_json(&window);
        assert!(json.contains("\"deletionAuthoritative\":true"), "{}", json);
    }

    #[test]
    fn event_identity_uses_the_same_canonical_key_as_provider_data() {
        let identity = EitStableEventIdentity {
            original_network_id: 4,
            transport_stream_id: 16625,
            service_id: 101,
            event_id: 10,
        };
        assert_eq!(
            stable_identity_string(identity),
            provider_data_api::build_program_key(4, 16625, 101, 10)
        );
    }

    #[test]
    fn ingest_pat_updates_service_count_without_pointer_handles() {
        let mut state = ParserState::default();
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        assert_eq!(state.ingest_section(0x0000, &pat), STATUS_OK);
        assert_eq!(state.sections_seen, 1);
        assert_eq!(state.snapshot().services.len(), 0);
    }

    #[test]
    fn bulk_snapshot_exposes_arib_si_text_replacement_diagnostic() {
        let mut state = ParserState::default();
        let sdt = section_with_crc(vec![
            0x42, 0xf0, 0x19, 0x00, 0x11, 0xc1, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x01, 0xfc,
            0xe0, 0x08, 0x48, 0x06, 0x01, 0x00, 0x03, 0x1b, b'$', b'X',
        ]);
        assert_eq!(state.ingest_section(0x0011, &sdt), STATUS_OK);
        let snapshot: serde_json::Value =
            serde_json::from_str(&bulk_snapshot_json(&mut state, false)).unwrap();
        let diagnostics = snapshot["parserDiagnostics"].as_array().unwrap();
        let text_diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic["code"] == "ARIB_SI_TEXT_REPLACED")
            .expect("ARIB SI text diagnostic");
        let message = text_diagnostic["message"].as_str().unwrap();
        assert!(message.contains("field=serviceName"), "{}", message);
        assert!(message.contains("input_prefix_hex:1b2458"), "{}", message);
    }

    #[test]
    fn unsupported_private_section_is_ignored_without_parallel_storage() {
        let mut state = ParserState::default();
        let section = vec![0x80, 0x00, 0x03, 0xaa, 0xbb, 0xcc];
        assert_eq!(
            state.ingest_section(0x0123, &section),
            STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE
        );
    }

    #[test]
    fn next_section_is_ignored_not_published() {
        let mut state = ParserState::default();
        let pat_next = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc0, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        assert_eq!(
            state.ingest_section(0x0000, &pat_next),
            STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE
        );
        assert_eq!(state.services().len(), 0);
    }

    #[test]
    fn bad_crc_si_section_is_rejected() {
        let mut state = ParserState::default();
        let mut pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        let last = pat.len() - 1;
        pat[last] ^= 0xff;
        assert_eq!(state.ingest_section(0x0000, &pat), STATUS_INVALID_SECTION);
        assert_eq!(state.services().len(), 0);
    }

    #[test]
    fn registry_rejects_destroyed_handles_without_raw_pointer_exposure() {
        let handle = registry().lock().unwrap().create();
        assert!(handle > 0);
        assert!(registry().lock().unwrap().get(handle).is_some());
        assert!(registry().lock().unwrap().remove(handle));
        assert!(registry().lock().unwrap().get(handle).is_none());
        assert!(!registry().lock().unwrap().remove(handle));
    }

    #[test]
    fn malformed_pmt_descriptor_loop_returns_status_only_on_known_pmt_pid() {
        let mut state = ParserState::default();
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00, 0x00, 0x01, 0xe1, 0x00,
        ]);
        assert_eq!(state.ingest_section(0x0000, &pat), STATUS_OK);
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x10, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x03, 0x09, 0x06,
            0x00,
        ]);
        assert_eq!(
            state.ingest_section(0x0100, &pmt),
            STATUS_MALFORMED_DESCRIPTOR
        );
    }

    #[test]
    fn table_id_0x02_on_unknown_pid_is_ignored_not_pmt() {
        let mut state = ParserState::default();
        let pmt_like = section_with_crc(vec![
            0x02, 0xb0, 0x10, 0x00, 0x01, 0xc1, 0x00, 0x00, 0xe1, 0x01, 0xf0, 0x03, 0x09, 0x06,
            0x00,
        ]);
        assert_eq!(
            state.ingest_section(0x0100, &pmt_like),
            STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE
        );
        assert_eq!(state.services().len(), 0);
    }
}
