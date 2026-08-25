mod arib_jis_x0208_table;
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
use eit::{EitEvent, EitStableEventIdentity, EitUpdateWindow};
use jni::objects::{JByteArray, JObject, JString};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;
use provider_data as provider_data_api;
use sections::{parse_section_header, section_crc_valid};
use service_discovery::{
    DiscoveredElementaryStream, DiscoveredService, DiscoveredTransport, DiscoveryPublishStage,
    ServiceDiscoveryCollector, ServiceSemanticFacts,
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

const DISCOVERY_STAGE_INCOMPLETE: jint = 0;
const DISCOVERY_STAGE_PARTIAL: jint = 1;
const DISCOVERY_STAGE_COMPLETE: jint = 2;

const MAX_RETAINED_PRIVATE_SECTIONS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
struct PrivateSectionRecord {
    pid: u16,
    table_id: u8,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct ParserState {
    collector: ServiceDiscoveryCollector,
    private_sections: Vec<PrivateSectionRecord>,
    sections_seen: u64,
    snapshot_generation: u64,
    last_status: jint,
}

impl ParserState {
    fn is_section_for_discovery(&self, pid: u16, table_id: u8) -> bool {
        is_fixed_pid_si_table_for_discovery(pid, table_id)
            || (table_id == 0x02 && self.collector.is_known_pmt_pid(pid))
    }

    fn ingest_section(&mut self, pid: u16, section: &[u8]) -> jint {
        let Some(header) = parse_section_header(section, 12) else {
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
            if header.syntax && !section_crc_valid(section, 12) {
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
            self.retain_private_section(PrivateSectionRecord {
                pid,
                table_id,
                bytes: section.to_vec(),
            });
            self.last_status = STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE;
            STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE
        }
    }

    fn retain_private_section(&mut self, record: PrivateSectionRecord) {
        if self
            .private_sections
            .iter()
            .any(|existing| existing == &record)
        {
            return;
        }
        if self.private_sections.len() >= MAX_RETAINED_PRIVATE_SECTIONS {
            self.private_sections.remove(0);
        }
        self.private_sections.push(record);
    }

    fn snapshot(&self) -> service_discovery::DiscoverySnapshot {
        self.collector.state().snapshot
    }

    fn raw_snapshot_for_debug(&self) -> service_discovery::DiscoverySnapshot {
        self.collector.state().snapshot
    }

    fn pmt_pids_for_section_filters(&self) -> Vec<u16> {
        self.collector.pmt_pids_for_section_filters()
    }

    fn cas_discovery_services(&self) -> Vec<DiscoveredService> {
        self.raw_snapshot_for_debug().services
    }

    fn raw_cat_ca_descriptors(&self) -> Vec<CaDescriptor> {
        self.raw_snapshot_for_debug().cat_ca.descriptors
    }

    fn semantic_facts(&self) -> Vec<ServiceSemanticFacts> {
        self.collector.state().semantic_facts_by_service
    }

    fn services(&self) -> Vec<DiscoveredService> {
        self.snapshot().services
    }

    fn transports(&self) -> Vec<DiscoveredTransport> {
        self.snapshot().transports
    }

    fn discovery_stage(&self) -> DiscoveryPublishStage {
        self.collector.state().publish_stage()
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

    fn clear_epg_update_windows(&mut self) {
        self.collector.clear_epg_update_windows()
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

fn json_opt_string(value: Option<&str>) -> String {
    match value {
        Some(v) if !v.is_empty() => json_string(v),
        _ => "null".to_string(),
    }
}

fn json_opt_u16(value: Option<u16>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_opt_u8(value: Option<u8>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_bool(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn json_array(items: Vec<String>) -> String {
    format!("[{}]", items.join(","))
}

fn string_array_json(items: &[String]) -> String {
    json_array(items.iter().map(|v| json_string(v)).collect())
}

fn str_array_json(items: &[&'static str]) -> String {
    json_array(items.iter().map(|v| json_string(v)).collect())
}

fn ca_descriptor_json(ca: &CaDescriptor, scope: &str, es_pid: Option<u16>) -> String {
    format!(
        "{{\"caSystemId\":{},\"caPid\":{},\"scope\":{},\"esPid\":{},\"rawDescriptorHex\":{},\"privateDataHex\":{}}}",
        ca.ca_system_id,
        ca.ca_pid,
        json_string(scope),
        json_opt_u16(es_pid),
        json_string(&hex_lower(&ca.raw_descriptor)),
        json_string(&hex_lower(&ca.private_data)),
    )
}

fn elementary_stream_json(stream: &DiscoveredElementaryStream) -> String {
    format!(
        "{{\"elementaryPid\":{},\"streamType\":{},\"componentTag\":{},\"componentType\":{},\"streamContent\":{},\"languageCodes\":{},\"dataComponentId\":{},\"isCaption\":{},\"isSuperimpose\":{}}}",
        stream.elementary_pid,
        stream.stream_type,
        json_opt_u8(stream.component_tag),
        json_opt_u8(stream.component_type),
        json_opt_u8(stream.stream_content),
        string_array_json(&stream.language_codes),
        json_opt_u16(stream.data_component_id),
        json_bool(stream.is_caption),
        json_bool(stream.is_superimpose),
    )
}

fn stream_language(stream: &DiscoveredElementaryStream) -> Option<&str> {
    stream
        .language_codes
        .iter()
        .find(|value| !value.is_empty())
        .map(String::as_str)
}

fn video_codec_name(stream_type: u8) -> Option<&'static str> {
    match stream_type {
        0x02 => Some("MPEG-2"),
        0x1b => Some("H.264"),
        0x24 => Some("HEVC"),
        _ => None,
    }
}

fn audio_codec_name(stream_type: u8) -> Option<&'static str> {
    match stream_type {
        0x03 | 0x04 => Some("MPEG-Audio"),
        0x0f => Some("AAC"),
        0x11 => Some("MPEG-4-AAC-LATM"),
        _ => None,
    }
}

fn stream_video_component_json(stream: &DiscoveredElementaryStream, codec: &str) -> String {
    format!(
        "{{\"esPid\":{},\"streamType\":{},\"componentTag\":{},\"componentType\":{},\"codec\":{},\"diagnosticCode\":\"CODEC_SIGNALING_OBSERVED\",\"parseStatus\":\"OK\"}}",
        stream.elementary_pid,
        stream.stream_type,
        json_opt_u8(stream.component_tag),
        json_opt_u8(stream.component_type),
        json_string(codec),
    )
}

fn stream_audio_component_json(stream: &DiscoveredElementaryStream, codec: &str) -> String {
    format!(
        "{{\"esPid\":{},\"streamType\":{},\"componentTag\":{},\"componentType\":{},\"codec\":{},\"language\":{},\"diagnosticCode\":\"CODEC_SIGNALING_OBSERVED\",\"parseStatus\":\"OK\"}}",
        stream.elementary_pid,
        stream.stream_type,
        json_opt_u8(stream.component_tag),
        json_opt_u8(stream.component_type),
        json_string(codec),
        json_opt_string(stream_language(stream)),
    )
}

fn stream_subtitle_component_json(stream: &DiscoveredElementaryStream) -> String {
    let kind = if stream.is_superimpose {
        "superimpose"
    } else if stream.data_component_id == Some(0x0012) {
        "one-seg-caption"
    } else {
        "caption"
    };
    format!(
        "{{\"esPid\":{},\"componentTag\":{},\"dataComponentId\":{},\"language\":{},\"captionServiceKind\":{},\"parseStatus\":\"OK\"}}",
        stream.elementary_pid,
        json_opt_u8(stream.component_tag),
        json_opt_u16(stream.data_component_id),
        json_opt_string(stream_language(stream)),
        json_string(kind),
    )
}

fn stream_data_component_json(stream: &DiscoveredElementaryStream) -> String {
    format!(
        "{{\"esPid\":{},\"componentTag\":{},\"dataComponentId\":{},\"componentType\":{},\"parseStatus\":\"OK\"}}",
        stream.elementary_pid,
        json_opt_u8(stream.component_tag),
        json_opt_u16(stream.data_component_id),
        json_opt_u8(stream.component_type),
    )
}

fn service_components_json(service: &DiscoveredService) -> String {
    let mut video = Vec::new();
    let mut audio = Vec::new();
    let mut subtitle = Vec::new();
    let mut data = Vec::new();
    for stream in &service.streams {
        if let Some(codec) = video_codec_name(stream.stream_type) {
            video.push(stream_video_component_json(stream, codec));
        } else if let Some(codec) = audio_codec_name(stream.stream_type) {
            audio.push(stream_audio_component_json(stream, codec));
        } else if stream.is_caption || matches!(stream.data_component_id, Some(0x0008 | 0x0012)) {
            subtitle.push(stream_subtitle_component_json(stream));
        } else if stream.data_component_id.is_some() {
            data.push(stream_data_component_json(stream));
        }
    }
    format!(
        "{{\"video\":{},\"audio\":{},\"subtitle\":{},\"data\":{}}}",
        json_array(video),
        json_array(audio),
        json_array(subtitle),
        json_array(data),
    )
}

fn service_json(service: &DiscoveredService) -> String {
    let mut ca = Vec::new();
    ca.extend(
        service
            .program_ca_descriptors
            .iter()
            .map(|d| ca_descriptor_json(d, "PROGRAM", None)),
    );
    for group in &service.es_ca_descriptors {
        ca.extend(
            group
                .descriptors
                .iter()
                .map(|d| ca_descriptor_json(d, "ES", Some(group.elementary_pid))),
        );
    }
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"name\":{},\"providerName\":{},\"serviceType\":{},\"pmtPid\":{},\"pcrPid\":{},\"freeCaMode\":{},\"streams\":{},\"components\":{},\"hasProgramCaDescriptor\":{},\"hasEsCaDescriptor\":{},\"serviceScopedCaDescriptors\":{}}}",
        service.original_network_id,
        service.transport_stream_id,
        service.service_id,
        json_string(service.service_name.as_deref().unwrap_or("")),
        json_string(service.provider_name.as_deref().unwrap_or("")),
        json_opt_u8(service.service_type),
        json_opt_u16(service.pmt_pid),
        json_opt_u16(service.pcr_pid),
        service.free_ca_mode.map(json_bool).unwrap_or("null"),
        json_array(service.streams.iter().map(elementary_stream_json).collect()),
        service_components_json(service),
        json_bool(!service.program_ca_descriptors.is_empty()),
        json_bool(!service.es_ca_descriptors.is_empty()),
        json_array(ca),
    )
}

fn transport_json(transport: &DiscoveredTransport) -> String {
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"networkName\":{},\"transportStreamName\":{},\"remoteControlKeyId\":{}}}",
        transport.original_network_id,
        transport.transport_stream_id,
        json_string(transport.network_name.as_deref().unwrap_or("")),
        json_string(transport.ts_name.as_deref().unwrap_or("")),
        json_opt_u8(transport.remote_control_key_id),
    )
}

fn transport_key_json(onid: u16, tsid: u16) -> String {
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{}}}",
        onid, tsid
    )
}

fn pmt_mapping_json(mapping: &crate::service_discovery::PmtPidMapping) -> String {
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"pmtPid\":{}}}",
        mapping.original_network_id,
        mapping.transport_stream_id,
        mapping.service_id,
        mapping.pmt_pid,
    )
}

fn ca_metadata_json(
    service_key: Option<(u16, u16, u16)>,
    ca: &CaDescriptor,
    ecm_pid: Option<u16>,
    emm_pid: Option<u16>,
    elementary_pid: Option<u16>,
    source: &str,
) -> String {
    let service_key_json = match service_key {
        Some((onid, tsid, sid)) => format!(
            "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{}}}",
            onid, tsid, sid
        ),
        None => "null".to_string(),
    };
    format!(
        "{{\"serviceKey\":{},\"caSystemId\":{},\"ecmPid\":{},\"emmPid\":{},\"elementaryPid\":{},\"privateDataHex\":{},\"source\":{}}}",
        service_key_json,
        ca.ca_system_id,
        json_opt_u16(ecm_pid),
        json_opt_u16(emm_pid),
        json_opt_u16(elementary_pid),
        json_string(&hex_lower(&ca.private_data)),
        json_string(source),
    )
}

fn ca_metadata_from_services_json(services: &[DiscoveredService], cat: &[CaDescriptor]) -> String {
    let mut out = Vec::new();
    for service in services {
        let key = Some((
            service.original_network_id,
            service.transport_stream_id,
            service.service_id,
        ));
        out.extend(
            service
                .program_ca_descriptors
                .iter()
                .map(|ca| ca_metadata_json(key, ca, Some(ca.ca_pid), None, None, "PROGRAM")),
        );
        for group in &service.es_ca_descriptors {
            out.extend(group.descriptors.iter().map(|ca| {
                ca_metadata_json(
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
            .map(|ca| ca_metadata_json(None, ca, None, Some(ca.ca_pid), None, "CAT")),
    );
    json_array(out)
}

fn malformed_ca_descriptor_diagnostic_json(d: &MalformedCaDescriptorDiagnostic) -> String {
    format!(
        "{{\"pid\":{},\"tableId\":{},\"tableIdExtension\":{},\"serviceId\":{},\"elementaryPid\":{},\"scope\":{},\"offset\":{},\"declaredLength\":{},\"actualRemainingLength\":{},\"reason\":{},\"rawPrefixHex\":{}}}",
        d.pid,
        d.table_id,
        json_opt_u16(d.table_id_extension),
        json_opt_u16(d.service_id),
        json_opt_u16(d.elementary_pid),
        json_string(d.scope),
        d.offset,
        d.declared_length,
        d.actual_remaining_length,
        json_string(d.reason),
        json_string(&d.raw_prefix_hex),
    )
}

fn malformed_ca_descriptor_diagnostics_json(
    diagnostics: &[MalformedCaDescriptorDiagnostic],
) -> String {
    json_array(
        diagnostics
            .iter()
            .map(malformed_ca_descriptor_diagnostic_json)
            .collect(),
    )
}

fn malformed_ca_descriptor_counts_json(diagnostics: &[MalformedCaDescriptorDiagnostic]) -> String {
    let mut counts: BTreeMap<u16, usize> = BTreeMap::new();
    for d in diagnostics.iter().filter(|d| d.service_id.is_some()) {
        *counts.entry(d.service_id.unwrap_or_default()).or_insert(0) += 1;
    }
    json_array(
        counts
            .into_iter()
            .map(|(sid, count)| format!("{{\"serviceId\":{},\"count\":{}}}", sid, count,))
            .collect(),
    )
}

fn private_section_json(section: &PrivateSectionRecord) -> String {
    format!(
        "{{\"pid\":{},\"tableId\":{},\"bytesHex\":{}}}",
        section.pid,
        section.table_id,
        json_string(&hex_lower(&section.bytes))
    )
}

fn extended_items_json(event: &EitEvent) -> String {
    json_array(
        event
            .descriptors
            .extended_items
            .iter()
            .map(|item| {
                format!(
                    "{{\"languageCode\":{},\"description\":{},\"text\":{}}}",
                    json_string(&item.language_code),
                    json_string(&item.item_description),
                    json_string(&item.item_text)
                )
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

fn event_primary_series_json(event: &EitEvent) -> String {
    let Some(series) = event.descriptors.series.first() else {
        return "null".to_string();
    };
    let expire_date_valid = series.expire_date != 0x1fff;
    format!(
        "{{\"seriesId\":{},\"repeatLabel\":{},\"programPattern\":{},\"expireDateValid\":{},\"expireDate\":null,\"episodeNumber\":{},\"lastEpisodeNumber\":{},\"name\":{},\"parseStatus\":\"OK\"}}",
        series.series_id,
        series.repeat_label,
        series.program_pattern,
        json_bool(expire_date_valid),
        series.episode_number,
        series.last_episode_number,
        if series.series_name.is_empty() { "null".to_string() } else { json_string(&series.series_name) },
    )
}

fn event_groups_json(event: &EitEvent) -> String {
    let groups = event
        .descriptors
        .event_groups
        .iter()
        .map(|group| {
  let events = group
      .events
      .iter()
      .map(|related| {
format!(
    r#"{{"serviceId":{},"eventId":{}}}"#,
    related.service_id, related.event_id,
)
      })
      .collect::<Vec<_>>();
  let other_network_events = group
      .other_network_events
      .iter()
      .map(|related| {
format!(
    r#"{{"originalNetworkId":{},"transportStreamId":{},"serviceId":{},"eventId":{}}}"#,
    related.original_network_id,
    related.transport_stream_id,
    related.service_id,
    related.event_id,
)
      })
      .collect::<Vec<_>>();
  format!(
      r#"{{"groupType":{},"events":{},"otherNetworkEvents":{},"privateDataHex":{},"parseStatus":"OK"}}"#,
      group.group_type,
      json_array(events),
      json_array(other_network_events),
      json_string(&hex_lower(&group.private_data)),
  )
        })
        .collect::<Vec<_>>();
    json_array(groups)
}

fn event_linkage_json(event: &EitEvent) -> String {
    json_array(event.descriptors.linkages.iter().map(|l| format!(
        "{{\"transportStreamId\":{},\"originalNetworkId\":{},\"serviceId\":{},\"linkageType\":{},\"privateDataPrefixHex\":{},\"parseStatus\":\"OK\"}}",
        l.transport_stream_id,
        l.original_network_id,
        l.service_id,
        l.linkage_type,
        json_string(&hex_prefix(&l.private_data, 16)),
    )).collect())
}

fn hex_prefix(bytes: &[u8], max_len: usize) -> String {
    bytes
        .iter()
        .take(max_len)
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join("")
}

fn event_content_genres_json(event: &EitEvent) -> String {
    json_array(
        event
            .descriptors
            .contents
            .iter()
            .map(|c| {
                format!(
        "{{\"level1\":{},\"level2\":{},\"userNibble\":{},\"aribName\":{},\"parseStatus\":\"OK\"}}",
        c.content_nibble_level_1,
        c.content_nibble_level_2,
        ((c.user_nibble_1 as u16) << 4) | c.user_nibble_2 as u16,
        json_string(&c.arib_display_name),
    )
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
        d.parental_ratings.iter().map(|r| (r.country_code.clone(), r.rating_value, r.raw_rating_byte)).collect::<Vec<_>>(),
        d.series.iter().map(|s| (s.series_id, s.episode_number, s.last_episode_number, s.series_name.clone())).collect::<Vec<_>>(),
        diagnostic.event_group_count,
        diagnostic.linkage_count,
        diagnostic.unknown_count,
    )
}

fn parental_ratings_json(event: &EitEvent) -> String {
    json_array(
        event
            .descriptors
            .parental_ratings
            .iter()
            .map(|r| {
                format!(
        "{{\"countryCode\":{},\"ratingValue\":{},\"rawRatingByte\":{},\"parseStatus\":\"OK\"}}",
        json_string(&r.country_code), r.rating_value, r.raw_rating_byte
    )
            })
            .collect(),
    )
}

fn event_video_components_json(_event: &EitEvent) -> String {
    "[]".to_string()
}

fn event_audio_components_json(_event: &EitEvent) -> String {
    "[]".to_string()
}

fn event_components_json(event: &EitEvent) -> String {
    format!(
        "{{\"video\":{},\"audio\":{},\"subtitle\":[],\"data\":[]}}",
        event_video_components_json(event),
        event_audio_components_json(event)
    )
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

fn event_json(event: &EitEvent) -> String {
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
    let model = serde_json::json!({
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
            "extendedItems": json_value(extended_items_json(event)),
            "component": { "text": event_component_text(event) },
            "audio": { "componentText": event_audio_component_text(event), "language": event_audio_language(event) },
            "genres": { "content": json_value(event_content_genres_json(event)), "genreSupplementText": event_genre_supplement_text(event) },
            "eventGroups": json_value(event_groups_json(event)),
            "linkage": json_value(event_linkage_json(event)),
            "freeCaMode": {
                "raw": if event.free_ca_mode { 1 } else { 0 },
                "scrambled": event.free_ca_mode,
                "text": if event.free_ca_mode { "有料放送" } else { "無料放送" },
                "parseStatus": "OK",
            },
            "series": json_value(event_primary_series_json(event)),
            "components": json_value(event_components_json(event)),
            "diagnostics": {
                "summary": event_diagnostic_text(event),
                "descriptorDiagnostics": json_value(descriptor_diagnostics.clone()),
                "descriptorDiagnosticsCanonicalJson": descriptor_diagnostics,
            },
            "parentalRatings": json_value(parental_ratings_json(event)),
        }
    });
    serde_json::to_string(&model).unwrap_or_else(|_| "{}".to_string())
}

fn epg_update_window_json(window: &EitUpdateWindow) -> String {
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"windowStartMillis\":{},\"windowEndMillis\":{},\"validProgramStableIdentities\":{},\"deletionAuthoritative\":{}}}",
        window.original_network_id,
        window.transport_stream_id,
        window.service_id,
        window.window_start_millis,
        window.window_end_millis,
        json_array(window.valid_event_identities.iter().map(|id| json_string(&stable_identity_string(*id))).collect()),
        json_bool(window.deletion_authoritative),
    )
}

fn semantic_facts_json(facts: &ServiceSemanticFacts) -> String {
    let elementary_streams = json_array(
        facts
            .elementary_streams
            .iter()
            .map(elementary_stream_json)
            .collect(),
    );
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"serviceType\":{},\"pmtPidResolved\":{},\"pmtParsed\":{},\"pcrPidResolved\":{},\"elementaryStreams\":{},\"requiresCas\":{},\"caDescriptorsResolved\":{},\"freeCaMode\":{},\"smd\":{{\"descriptorPresent\":{},\"syntaxValid\":{},\"systemManagementId\":{},\"broadcastingFlag\":{},\"broadcastingIdentifier\":{},\"additionalBroadcastingIdentification\":{},\"additionalIdentificationInfoHex\":{},\"semanticState\":{},\"diagnostic\":{}}},\"missingComponents\":{},\"semanticDiagnostics\":{}}}",
        facts.original_network_id,
        facts.transport_stream_id,
        facts.service_id,
        json_opt_u8(facts.service_type),
        json_bool(facts.pmt_pid_resolved),
        json_bool(facts.pmt_parsed),
        json_bool(facts.pcr_pid_resolved),
        elementary_streams,
        json_bool(facts.requires_cas),
        json_bool(facts.ca_descriptors_resolved),
        facts.free_ca_mode.map(json_bool).unwrap_or("null"),
        json_bool(facts.system_management.descriptor_present),
        json_bool(facts.system_management.syntax_valid),
        json_opt_u16(facts.system_management.system_management_id),
        json_opt_u8(facts.system_management.broadcasting_flag),
        json_opt_u8(facts.system_management.broadcasting_identifier),
        json_opt_u8(facts.system_management.additional_broadcasting_identification),
        json_string(&hex_lower(&facts.system_management.additional_identification_info)),
        json_string(facts.system_management.semantic_state.as_str()),
        json_opt_string(facts.system_management.diagnostic),
        str_array_json(&facts.missing_components),
        str_array_json(&facts.semantic_diagnostics),
    )
}

fn bulk_snapshot_json(state: &mut ParserState, take_update_windows: bool) -> String {
    state.snapshot_generation = state.snapshot_generation.saturating_add(1);
    let snapshot_generation = state.snapshot_generation;
    let ingest_sequence = state.sections_seen;
    let parser_diagnostics = parser_diagnostics_json(state);
    let snapshot = state.snapshot();
    let services = snapshot.services;
    let transports = snapshot.transports;
    let pmt_mappings = snapshot.pmt_pids_by_service;
    let cas_services = state.cas_discovery_services();
    let cat_ca = snapshot.cat_ca.descriptors;
    let cas_cat_ca = state.raw_cat_ca_descriptors();
    // 更新区間は排出型一括APIだけで公開する。
    // 非排出型一括snapshotはEPG更新区間を返さない。これにより本番呼び出し側が
    // 同じ廃止削除区間を誤って再公開することを防ぐ。
    let epg_windows = if take_update_windows {
        state.take_epg_update_windows()
    } else {
        Vec::new()
    };
    format!(
        r#"{{"snapshotGeneration":{},"ingestSequence":{},"services":{},"servicesForCasDiscovery":{},"caMetadata":{},"caMetadataForCasDiscovery":{},"malformedCaDescriptorDiagnostics":{},"malformedCaDescriptorCounts":{},"pmtPidMappings":{},"pmtPidsForSectionFilters":{},"transports":{},"sdtActualTransports":{},"privateSections":{},"events":{},"epgUpdateWindows":{},"serviceSemanticFacts":{},"parserDiagnostics":{}}}"#,
        snapshot_generation,
        ingest_sequence,
        json_array(services.iter().map(service_json).collect()),
        json_array(cas_services.iter().map(service_json).collect()),
        ca_metadata_from_services_json(&services, &cat_ca),
        ca_metadata_from_services_json(&cas_services, &cas_cat_ca),
        malformed_ca_descriptor_diagnostics_json(&snapshot.malformed_ca_descriptor_diagnostics),
        malformed_ca_descriptor_counts_json(&snapshot.malformed_ca_descriptor_diagnostics),
        json_array(pmt_mappings.iter().map(pmt_mapping_json).collect()),
        json_array(
            state
                .pmt_pids_for_section_filters()
                .iter()
                .map(|pid| pid.to_string())
                .collect()
        ),
        json_array(transports.iter().map(transport_json).collect()),
        json_array(
            state
                .sdt_actual_transport_keys()
                .iter()
                .map(|(tsid, onid)| transport_key_json(*onid, *tsid))
                .collect()
        ),
        json_array(
            state
                .private_sections
                .iter()
                .map(private_section_json)
                .collect()
        ),
        json_array(state.events().iter().map(event_json).collect()),
        json_array(epg_windows.iter().map(epg_update_window_json).collect()),
        json_array(
            state
                .semantic_facts()
                .iter()
                .map(semantic_facts_json)
                .collect()
        ),
        parser_diagnostics,
    )
}

fn parser_diagnostics_json(state: &ParserState) -> String {
    let message = format!(
        "sectionsSeen={} lastStatus={}",
        state.sections_seen, state.last_status
    );
    let mut diagnostics = vec![format!(
        "{{\"code\":\"PARSER_STATE\",\"message\":{},\"severity\":\"info\"}}",
        json_string(&message),
    )];
    let snapshot = state.raw_snapshot_for_debug();
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
    diagnostics.extend(text_diagnostics.into_iter().map(|diagnostic| {
        format!(
            "{{\"code\":\"ARIB_SI_TEXT_REPLACED\",\"message\":{},\"severity\":\"warning\"}}",
            json_string(&diagnostic),
        )
    }));
    json_array(diagnostics)
}

fn section_body_end(section: &[u8]) -> Option<usize> {
    let header = parse_section_header(section, 12)?;
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
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetDiscoveryStage(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| {
        discovery_stage_to_jint(state.discovery_stage())
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
            let json = event_groups_json(&minimal_event_for_related_items(
                group_type,
                0x0100 + group_type as u16,
            ));
            assert!(
                json.contains(&format!("\"groupType\":{}", group_type)),
                "{}",
                json
            );
            assert!(json.contains("\"events\":"), "{}", json);
            assert!(!json.contains("\"kind\":"), "{}", json);
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
        assert_eq!(state.raw_snapshot_for_debug().services.len(), 0);
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
    fn unsupported_private_section_is_retained_for_cas_path() {
        let mut state = ParserState::default();
        let section = vec![0x80, 0x00, 0x03, 0xaa, 0xbb, 0xcc];
        assert_eq!(
            state.ingest_section(0x0123, &section),
            STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE
        );
        assert_eq!(state.private_sections.len(), 1);
        assert_eq!(state.private_sections[0].pid, 0x0123);
        assert_eq!(state.private_sections[0].table_id, 0x80);
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

#[cfg(test)]
mod component_fact_serialization_tests {
    use super::{service_components_json, DiscoveredElementaryStream, DiscoveredService};

    #[test]
    fn hevc_and_absent_descriptors_are_serialized_as_observed_facts() {
        let mut service = DiscoveredService::default();
        service.streams.push(DiscoveredElementaryStream {
            elementary_pid: 0x120,
            stream_type: 0x24,
            component_tag: None,
            stream_content: None,
            component_type: None,
            data_component_id: None,
            language_codes: Vec::new(),
            is_caption: false,
            is_superimpose: false,
        });
        let json = service_components_json(&service);
        assert!(json.contains("\"codec\":\"HEVC\""));
        assert!(json.contains("\"diagnosticCode\":\"CODEC_SIGNALING_OBSERVED\""));
        assert!(json.contains("\"componentTag\":null"));
        assert!(json.contains("\"componentType\":null"));
        assert!(!json.contains("UNSUPPORTED_R51_CODEC_SIGNALING"));
    }
}
