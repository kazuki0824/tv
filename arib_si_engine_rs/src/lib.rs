
mod arib_jis_x0208_table;
mod arib_string;
mod ca_descriptor;
mod descriptors;
mod discovery_requirements;
mod eit;
mod provider_data;
mod sections;
mod service_discovery;

use ca_descriptor::CaDescriptor;
use jni::objects::{JByteArray, JObject, JString};
use jni::sys::{jboolean, jbyteArray, jint, jlong, jstring};
use jni::JNIEnv;
use sections::{parse_section_header, section_crc_valid};
use eit::{EitEvent, EitUpdateWindow};
use descriptors::{event_descriptor_diagnostic, event_provider_fields, json_escape};
use provider_data as provider_data_api;
use service_discovery::{
    DiscoveredElementaryStream, DiscoveredService, DiscoveredTransport, EsCaMetadata,
    DiscoveryPublishStage,
    ServiceDiscoveryCollector, ServicePublishability,
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
const STATUS_INDEX_OUT_OF_RANGE: jint = -5;
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
    last_status: jint,
}

impl ParserState {
    fn is_section_for_discovery(&self, pid: u16, table_id: u8) -> bool {
        is_fixed_pid_si_table_for_discovery(pid, table_id) || (table_id == 0x02 && self.collector.is_known_pmt_pid(pid))
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
            let malformed_descriptor_loop = section_has_malformed_descriptor_loop(pid, table_id, section, self.collector.is_known_pmt_pid(pid));
            // 不正descriptor loopは診断付き入力として扱い、意味解析前に
            // section全体を破棄する理由にはしない。復旧不能なsection length / CRC errorは上で拒否する。
            self.collector.push_section(pid, section);
            self.last_status = if malformed_descriptor_loop { STATUS_MALFORMED_DESCRIPTOR } else { STATUS_OK };
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
        if self.private_sections.iter().any(|existing| existing == &record) {
            return;
        }
        if self.private_sections.len() >= MAX_RETAINED_PRIVATE_SECTIONS {
            self.private_sections.remove(0);
        }
        self.private_sections.push(record);
    }

    fn snapshot(&self) -> service_discovery::DiscoverySnapshot {
        self.collector.state().registration_ready_snapshot().unwrap_or_default()
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

    fn publishability(&self) -> Vec<ServicePublishability> {
        self.collector.state().publishability_by_service
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
    value.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string())
}

fn json_opt_u8(value: Option<u8>) -> String {
    value.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string())
}

fn json_bool(value: bool) -> &'static str { if value { "true" } else { "false" } }

fn json_array(items: Vec<String>) -> String { format!("[{}]", items.join(",")) }

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


fn stream_component_tag(stream: &DiscoveredElementaryStream) -> u8 { stream.component_tag.unwrap_or(0) }
fn stream_component_type(stream: &DiscoveredElementaryStream) -> u8 { stream.component_type.unwrap_or(0) }
fn stream_language(stream: &DiscoveredElementaryStream) -> String {
    stream.language_codes.iter().find(|v| !v.is_empty()).cloned().unwrap_or_else(|| "jpn".to_string())
}

fn video_codec_name(stream_type: u8) -> Option<(&'static str, bool)> {
    match stream_type {
        0x02 => Some(("MPEG-2", true)),
        0x1b => Some(("H.264", true)),
        0x24 => Some(("HEVC", false)),
        _ => None,
    }
}

fn audio_codec_name(stream_type: u8) -> Option<(&'static str, bool)> {
    match stream_type {
        0x03 | 0x04 => Some(("MPEG-Audio", true)),
        0x0f => Some(("AAC", true)),
        0x11 => Some(("MPEG-4-AAC-LATM", false)),
        _ => None,
    }
}

fn stream_video_component_json(stream: &DiscoveredElementaryStream, codec: &str, r51_supported: bool) -> String {
    format!(
        "{{\"esPid\":{},\"streamType\":{},\"componentTag\":{},\"componentType\":{},\"codec\":{},\"r51PlaybackSupported\":{},\"liveViewableClaim\":{},\"diagnosticCode\":{},\"parseStatus\":{}}}",
        stream.elementary_pid,
        stream.stream_type,
        stream_component_tag(stream),
        stream_component_type(stream),
        json_string(codec),
        json_bool(r51_supported),
        json_bool(r51_supported),
        json_string(if r51_supported { "OK" } else { "UNSUPPORTED_R51_CODEC" }),
        json_string(if r51_supported { "OK" } else { "UNSUPPORTED_R51" }),
    )
}

fn stream_audio_component_json(stream: &DiscoveredElementaryStream, codec: &str, r51_supported: bool) -> String {
    format!(
        "{{\"esPid\":{},\"streamType\":{},\"componentTag\":{},\"componentType\":{},\"codec\":{},\"language\":{},\"diagnosticCode\":{},\"parseStatus\":{}}}",
        stream.elementary_pid,
        stream.stream_type,
        stream_component_tag(stream),
        stream_component_type(stream),
        json_string(codec),
        json_string(&stream_language(stream)),
        json_string(if r51_supported { "OK" } else { "UNSUPPORTED_R51_CODEC" }),
        json_string(if r51_supported { "OK" } else { "UNSUPPORTED_R51" }),
    )
}

fn stream_subtitle_component_json(stream: &DiscoveredElementaryStream) -> String {
    let data_component_id = stream.data_component_id.unwrap_or(0x0008);
    let tag = stream_component_tag(stream);
    let kind = if stream.is_superimpose {
        "superimpose"
    } else if data_component_id == 0x0012 {
        "one-seg-caption"
    } else {
        "caption"
    };
    format!(
        "{{\"esPid\":{},\"componentTag\":{},\"dataComponentId\":{},\"language\":{},\"trackId\":{},\"captionServiceKind\":{},\"parseStatus\":\"OK\"}}",
        stream.elementary_pid,
        tag,
        data_component_id,
        json_string(&stream_language(stream)),
        json_string(&format!("subtitle:{}:{}", stream.elementary_pid, tag)),
        json_string(kind),
    )
}

fn stream_data_component_json(stream: &DiscoveredElementaryStream) -> String {
    format!(
        "{{\"esPid\":{},\"componentTag\":{},\"dataComponentId\":{},\"componentType\":{},\"parseStatus\":\"OK\"}}",
        stream.elementary_pid,
        stream_component_tag(stream),
        stream.data_component_id.unwrap_or(0),
        stream_component_type(stream),
    )
}

fn service_components_json(service: &DiscoveredService) -> String {
    let mut video = Vec::new();
    let mut audio = Vec::new();
    let mut subtitle = Vec::new();
    let mut data = Vec::new();
    for stream in &service.streams {
        if let Some((codec, supported)) = video_codec_name(stream.stream_type) {
            video.push(stream_video_component_json(stream, codec, supported));
        } else if let Some((codec, supported)) = audio_codec_name(stream.stream_type) {
            audio.push(stream_audio_component_json(stream, codec, supported));
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
    ca.extend(service.program_ca_descriptors.iter().map(|d| ca_descriptor_json(d, "PROGRAM", None)));
    for group in &service.es_ca_descriptors {
        ca.extend(group.descriptors.iter().map(|d| ca_descriptor_json(d, "ES", Some(group.elementary_pid))));
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
    format!("{{\"originalNetworkId\":{},\"transportStreamId\":{}}}", onid, tsid)
}

fn pmt_mapping_json(mapping: &crate::service_discovery::PmtPidMapping) -> String {
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"pmtPid\":{}}}",
        mapping.original_network_id, mapping.transport_stream_id, mapping.service_id, mapping.pmt_pid,
    )
}

fn ca_metadata_json(service_key: Option<(u16, u16, u16)>, ca: &CaDescriptor, ecm_pid: Option<u16>, emm_pid: Option<u16>, elementary_pid: Option<u16>, source: &str) -> String {
    let service_key_json = match service_key {
        Some((onid, tsid, sid)) => format!("{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{}}}", onid, tsid, sid),
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
        let key = Some((service.original_network_id, service.transport_stream_id, service.service_id));
        out.extend(service.program_ca_descriptors.iter().map(|ca| ca_metadata_json(key, ca, Some(ca.ca_pid), None, None, "PROGRAM")));
        for group in &service.es_ca_descriptors {
            out.extend(group.descriptors.iter().map(|ca| ca_metadata_json(key, ca, Some(ca.ca_pid), None, Some(group.elementary_pid), "ELEMENTARY_STREAM")));
        }
    }
    out.extend(cat.iter().map(|ca| ca_metadata_json(None, ca, None, Some(ca.ca_pid), None, "CAT")));
    json_array(out)
}

fn private_section_json(section: &PrivateSectionRecord) -> String {
    format!("{{\"pid\":{},\"tableId\":{},\"bytesHex\":{}}}", section.pid, section.table_id, json_string(&hex_lower(&section.bytes)))
}

fn extended_items_json(event: &EitEvent) -> String {
    json_array(event.descriptors.extended_items.iter().map(|item| format!("{{\"description\":{},\"text\":{}}}", json_string(&item.item_description), json_string(&item.item_text))).collect())
}

fn event_component_text(event: &EitEvent) -> String {
    event.descriptors.components.iter().map(|c| c.text.clone()).filter(|v| !v.is_empty()).collect::<Vec<_>>().join("\n")
}

fn event_audio_component_text(event: &EitEvent) -> String {
    event.descriptors.audio_components.iter().map(|a| a.text.clone()).filter(|v| !v.is_empty()).collect::<Vec<_>>().join("\n")
}

fn event_audio_language(event: &EitEvent) -> String {
    let mut langs = Vec::new();
    for audio in &event.descriptors.audio_components {
        if !audio.language_code.is_empty() && !langs.contains(&audio.language_code) { langs.push(audio.language_code.clone()); }
        if let Some(second) = &audio.language_code_2 { if !second.is_empty() && !langs.contains(second) { langs.push(second.clone()); } }
    }
    langs.join(",")
}

fn event_primary_series_json(event: &EitEvent) -> String {
    let Some(series) = event.descriptors.series.first() else { return "null".to_string(); };
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

fn event_related_items_json(event: &EitEvent) -> String {
    let mut items = Vec::new();
    for group in &event.descriptors.event_groups {
        let kind = match group.group_type {
            0x2 | 0x4 => "relay",
            0x3 | 0x5 => "movement",
            _ => "shared",
        };
        for related in &group.events {
            items.push(format!(
                "{{\"kind\":{},\"groupType\":{},\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"eventId\":{},\"parseStatus\":\"OK\"}}",
                json_string(kind),
                group.group_type,
                event.original_network_id,
                event.transport_stream_id,
                related.service_id,
                related.event_id,
            ));
        }
        for related in &group.other_network_events {
            items.push(format!(
                "{{\"kind\":{},\"groupType\":{},\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"eventId\":{},\"parseStatus\":\"OK\"}}",
                json_string(kind),
                group.group_type,
                related.original_network_id.unwrap_or(event.original_network_id),
                related.transport_stream_id.unwrap_or(event.transport_stream_id),
                related.service_id,
                related.event_id,
            ));
        }
    }
    json_array(items)
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
    bytes.iter().take(max_len).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join("")
}

fn event_broadcast_genre(event: &EitEvent) -> String {
    event.descriptors.contents.iter().map(|c| format!("ARIB(0x{:x}/0x{:x}):{}", c.content_nibble_level_1, c.content_nibble_level_2, c.arib_display_name)).collect::<Vec<_>>().join("、")
}

fn event_genre_supplement_text(event: &EitEvent) -> String {
    event.descriptors.contents.iter().map(|c| arib_content_to_ui_text(c.content_nibble_level_1, c.content_nibble_level_2)).collect::<Vec<_>>().join("、")
}

fn event_group_text(event: &EitEvent) -> String {
    let mut parts = Vec::new();
    for group in &event.descriptors.event_groups {
        for related in &group.events { parts.push(format!("sid={} event={}", related.service_id, related.event_id)); }
        for related in &group.other_network_events { parts.push(format!("onid={} tsid={} sid={} event={}", related.original_network_id.unwrap_or(0), related.transport_stream_id.unwrap_or(0), related.service_id, related.event_id)); }
    }
    parts.join("、")
}

fn event_series_name(event: &EitEvent) -> String {
    event.descriptors.series.iter().map(|s| s.series_name.clone()).filter(|v| !v.is_empty()).collect::<Vec<_>>().join("\n")
}

fn event_diagnostic_text(event: &EitEvent) -> String {
    let d = event.descriptors.clone();
    let diagnostic = event_descriptor_diagnostic(&d);
    format!(
        "content={:?} component={:?} audio={:?} parental={:?} series={:?} eventGroupCount={} linkageCount={} unknownCount={} json={}",
        d.contents.iter().map(|c| (c.content_nibble_level_1, c.content_nibble_level_2)).collect::<Vec<_>>(),
        d.components.iter().map(|c| (c.stream_content, c.component_type, c.component_tag, c.language_code.clone())).collect::<Vec<_>>(),
        d.audio_components.iter().map(|a| (a.stream_content, a.component_type, a.component_tag, a.stream_type, a.language_code.clone(), a.language_code_2.clone())).collect::<Vec<_>>(),
        d.parental_ratings.iter().map(|r| (r.country_code.clone(), r.rating_value, r.raw_rating_byte)).collect::<Vec<_>>(),
        d.series.iter().map(|s| (s.series_id, s.episode_number, s.last_episode_number, s.series_name.clone())).collect::<Vec<_>>(),
        diagnostic.event_group_count,
        diagnostic.linkage_count,
        diagnostic.unknown_count,
        diagnostic.descriptor_json,
    )
}

fn parental_ratings_json(event: &EitEvent) -> String {
    json_array(event.descriptors.parental_ratings.iter().map(|r| format!(
        "{{\"countryCode\":{},\"rating\":{},\"rawRating\":{},\"supported\":{}}}",
        json_string(&r.country_code), r.rating_value, r.raw_rating_byte, json_bool(r.country_code == "JPN" && r.rating_value <= 20)
    )).collect())
}

fn event_json(event: &EitEvent) -> String {
    let provider = event_provider_fields(&event.descriptors);
    let descriptor_diagnostic = event_descriptor_diagnostic(&event.descriptors);
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"stableIdentity\":{},\"eventId\":{},\"startTimeMillis\":{},\"durationMillis\":{},\"title\":{},\"description\":{},\"extendedDescription\":{},\"eventScope\":{},\"descriptors\":{{\"extendedItems\":{},\"component\":{{\"text\":{}}},\"audio\":{{\"componentText\":{},\"language\":{}}},\"genres\":{{\"broadcastGenre\":{},\"genreSupplementText\":{}}},\"relatedItems\":{},\"linkage\":{},\"freeCaMode\":{{\"scrambled\":{},\"text\":{}}},\"series\":{},\"components\":{},\"diagnostics\":{{\"summary\":{},\"descriptorDiagnostics\":{}}},\"parentalRatings\":{}}}}}}",
        event.original_network_id,
        event.transport_stream_id,
        event.service_id,
        json_string(&stable_identity_string(event.stable_identity())),
        event.event_id,
        event.start_time_millis,
        event.duration_millis,
        json_string(&provider.title),
        json_string(&provider.description),
        json_string(&provider.extended_description),
        json_string(event.scope.as_str()),
        extended_items_json(event),
        json_opt_string(Some(&event_component_text(event))),
        json_opt_string(Some(&event_audio_component_text(event))),
        json_opt_string(Some(&event_audio_language(event))),
        json_opt_string(Some(&event_broadcast_genre(event))),
        json_opt_string(Some(&event_genre_supplement_text(event))),
        event_related_items_json(event),
        event_linkage_json(event),
        json_bool(event.free_ca_mode),
        json_string(if event.free_ca_mode { "有料放送" } else { "無料放送" }),
        event_primary_series_json(event),
        "{\"video\":[],\"audio\":[],\"subtitle\":[],\"data\":[]}",
        json_string(&event_diagnostic_text(event)),
        descriptor_diagnostic.descriptor_json,
        parental_ratings_json(event),
    )
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

fn publishability_json(p: &ServicePublishability) -> String {
    format!(
        "{{\"originalNetworkId\":{},\"transportStreamId\":{},\"serviceId\":{},\"publishable\":{},\"channelRegistrationReady\":{},\"epgPublishable\":{},\"clearLivePlaybackSupported\":{},\"requiresCas\":{},\"unsupportedCas\":{},\"pmtPidResolved\":{},\"pmtParsed\":{},\"caStateResolved\":{},\"freeCaModeResolved\":{},\"missingComponents\":{},\"reasons\":{},\"registrationReasons\":{},\"epgReasons\":{}}}",
        p.original_network_id, p.transport_stream_id, p.service_id,
        json_bool(p.publishable), json_bool(p.channel_registration_ready), json_bool(p.epg_publishable), json_bool(p.clear_live_playback_supported), json_bool(p.requires_cas), json_bool(p.unsupported_cas), json_bool(p.pmt_pid_resolved), json_bool(p.pmt_parsed), json_bool(p.ca_state_resolved), json_bool(p.free_ca_mode_resolved),
        str_array_json(&p.missing_components), str_array_json(&p.reasons), str_array_json(&p.registration_reasons), str_array_json(&p.epg_reasons),
    )
}

fn bulk_snapshot_json(state: &mut ParserState, take_update_windows: bool) -> String {
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
        "{{\"services\":{},\"servicesForCasDiscovery\":{},\"caMetadata\":{},\"caMetadataForCasDiscovery\":{},\"pmtPidMappings\":{},\"pmtPidsForSectionFilters\":{},\"transports\":{},\"sdtActualTransports\":{},\"privateSections\":{},\"events\":{},\"epgUpdateWindows\":{},\"publishabilityDiagnostics\":{}}}",
        json_array(services.iter().map(service_json).collect()),
        json_array(cas_services.iter().map(service_json).collect()),
        ca_metadata_from_services_json(&services, &cat_ca),
        ca_metadata_from_services_json(&cas_services, &cas_cat_ca),
        json_array(pmt_mappings.iter().map(pmt_mapping_json).collect()),
        json_array(state.pmt_pids_for_section_filters().iter().map(|pid| pid.to_string()).collect()),
        json_array(transports.iter().map(transport_json).collect()),
        json_array(state.sdt_actual_transport_keys().iter().map(|(tsid, onid)| transport_key_json(*onid, *tsid)).collect()),
        json_array(state.private_sections.iter().map(private_section_json).collect()),
        json_array(state.events().iter().map(event_json).collect()),
        json_array(epg_windows.iter().map(epg_update_window_json).collect()),
        json_array(state.publishability().iter().map(publishability_json).collect()),
    )
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

fn section_has_malformed_descriptor_loop(pid: u16, table_id: u8, section: &[u8], known_pmt_pid: bool) -> bool {
    let Some(body_end) = section_body_end(section) else { return true; };
    match (pid, table_id) {
        (0x0001, 0x01) => section.len() < 8 || body_end < 8 || !descriptor_loop_well_formed(&section[8..body_end]),
        (_, 0x02) if known_pmt_pid => {
            if section.len() < 12 || body_end < 12 || body_end > section.len() { return true; }
            let program_info_length = (((section[10] & 0x0f) as usize) << 8) | section[11] as usize;
            let Some(program_info_end) = 12usize.checked_add(program_info_length) else { return true; };
            if program_info_end > body_end || !descriptor_loop_well_formed(&section[12..program_info_end]) {
                return true;
            }
            let mut cursor = program_info_end;
            while cursor < body_end {
                if cursor + 5 > body_end { return true; }
                let es_info_length = (((section[cursor + 3] & 0x0f) as usize) << 8) | section[cursor + 4] as usize;
                let Some(desc_start) = cursor.checked_add(5) else { return true; };
                let Some(desc_end) = desc_start.checked_add(es_info_length) else { return true; };
                if desc_end > body_end || !descriptor_loop_well_formed(&section[desc_start..desc_end]) {
                    return true;
                }
                cursor = desc_end;
            }
            false
        }
        (0x0010, 0x40) | (0x0010, 0x41) => {
            if section.len() < 10 || body_end < 10 { return true; }
            let descriptors_length = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
            let Some(network_desc_end) = 10usize.checked_add(descriptors_length) else { return true; };
            if network_desc_end > body_end || !descriptor_loop_well_formed(&section[10..network_desc_end]) { return true; }
            if network_desc_end + 2 > body_end { return true; }
            let transport_loop_length = (((section[network_desc_end] & 0x0f) as usize) << 8) | section[network_desc_end + 1] as usize;
            let mut cursor = network_desc_end + 2;
            let Some(transport_end) = cursor.checked_add(transport_loop_length) else { return true; };
            if transport_end > body_end { return true; }
            while cursor < transport_end {
                if cursor + 6 > transport_end { return true; }
                let desc_len = (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
                let desc_start = cursor + 6;
                let Some(desc_end) = desc_start.checked_add(desc_len) else { return true; };
                if desc_end > transport_end || !descriptor_loop_well_formed(&section[desc_start..desc_end]) { return true; }
                cursor = desc_end;
            }
            false
        }
        (0x0011, 0x42) | (0x0011, 0x46) => {
            if section.len() < 11 || body_end < 11 { return true; }
            let mut cursor = 11usize;
            while cursor < body_end {
                if cursor + 5 > body_end { return true; }
                let desc_len = (((section[cursor + 3] & 0x0f) as usize) << 8) | section[cursor + 4] as usize;
                let desc_start = cursor + 5;
                let Some(desc_end) = desc_start.checked_add(desc_len) else { return true; };
                if desc_end > body_end || !descriptor_loop_well_formed(&section[desc_start..desc_end]) { return true; }
                cursor = desc_end;
            }
            false
        }
        (0x0011, 0x4a) => {
            if section.len() < 10 || body_end < 10 { return true; }
            let bouquet_desc_len = (((section[8] & 0x0f) as usize) << 8) | section[9] as usize;
            let Some(bouquet_desc_end) = 10usize.checked_add(bouquet_desc_len) else { return true; };
            if bouquet_desc_end > body_end || !descriptor_loop_well_formed(&section[10..bouquet_desc_end]) { return true; }
            if bouquet_desc_end + 2 > body_end { return true; }
            let transport_loop_length = (((section[bouquet_desc_end] & 0x0f) as usize) << 8) | section[bouquet_desc_end + 1] as usize;
            let mut cursor = bouquet_desc_end + 2;
            let Some(transport_end) = cursor.checked_add(transport_loop_length) else { return true; };
            if transport_end > body_end { return true; }
            while cursor < transport_end {
                if cursor + 6 > transport_end { return true; }
                let desc_len = (((section[cursor + 4] & 0x0f) as usize) << 8) | section[cursor + 5] as usize;
                let desc_start = cursor + 6;
                let Some(desc_end) = desc_start.checked_add(desc_len) else { return true; };
                if desc_end > transport_end || !descriptor_loop_well_formed(&section[desc_start..desc_end]) { return true; }
                cursor = desc_end;
            }
            false
        }
        _ => false,
    }
}

fn is_fixed_pid_si_table_for_discovery(pid: u16, table_id: u8) -> bool {
    matches!((pid, table_id),
        (0x0000, 0x00) |
        (0x0001, 0x01) |
        (0x0010, 0x40 | 0x41) |
        (0x0011, 0x42 | 0x46 | 0x4a) |
        (0x0012, 0x4e..=0x6f)
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
        self.parsers.insert(handle, Arc::new(Mutex::new(ParserState::default())));
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
    let Some(parser) = parser else { return default_value; };
    match parser.lock() {
        Ok(guard) => f(&guard),
        Err(_) => default_value,
    }
}

fn with_state_mut(handle: jlong, default_value: jint, f: impl FnOnce(&mut ParserState) -> jint) -> jint {
    let parser = match registry().lock() {
        Ok(guard) => guard.get(handle),
        Err(_) => return STATUS_INTERNAL_ERROR,
    };
    let Some(parser) = parser else { return default_value; };
    match parser.lock() {
        Ok(mut guard) => f(&mut guard),
        Err(_) => STATUS_INTERNAL_ERROR,
    }
}

fn java_string(env: &mut JNIEnv<'_>, value: Option<String>) -> jstring {
    match env.new_string(value.unwrap_or_default()) {
        Ok(s) => s.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

fn java_byte_array(env: &mut JNIEnv<'_>, value: &[u8]) -> jbyteArray {
    match env.byte_array_from_slice(value) {
        Ok(array) => array.into_raw(),
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
    let Some(parser) = parser else { return java_string(&mut env, Some("{}".to_string())); };
    let json = match parser.lock() {
        Ok(mut guard) => bulk_snapshot_json(&mut guard, take_update_windows != 0),
        Err(_) => "{}".to_string(),
    };
    java_string(&mut env, Some(json))
}


fn jstring_to_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Option<String> {
    env.get_string(&value).ok().map(|s| s.into())
}

fn provider_result_json(result: provider_data_api::ProviderDataResult) -> String {
    format!(
        "{{\"json\":{},\"signature\":{},\"extractedKey\":{}}}",
        json_string(&result.json),
        json_string(&result.signature),
        json_string(&result.extracted_key),
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
    provider_data: JString<'_>,
) -> jstring {
    let data = jstring_to_string(&mut env, provider_data).unwrap_or_default();
    let result = provider_data_api::normalize_program_provider_data(&data);
    java_string(&mut env, Some(provider_result_json(result)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeProgramProviderDataSignature(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JString<'_>,
) -> jstring {
    let data = jstring_to_string(&mut env, provider_data).unwrap_or_default();
    java_string(&mut env, Some(provider_data_api::program_provider_data_signature(&data)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeExtractProgramKeyResult(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JString<'_>,
) -> jstring {
    let data = jstring_to_string(&mut env, provider_data).unwrap_or_default();
    let json = provider_data_api::extract_program_key_result(&data).map(program_key_result_json);
    java_string(&mut env, json)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeExtractChannelTuneKey(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JString<'_>,
) -> jstring {
    let data = jstring_to_string(&mut env, provider_data).unwrap_or_default();
    java_string(&mut env, Some(provider_data_api::extract_channel_tune_key(&data)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeAppendCurrentProgramDiagnostics(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JString<'_>,
    overlap_count: jlong,
    selected_program_id: jlong,
    selection_rule: JString<'_>,
) -> jstring {
    let data = jstring_to_string(&mut env, provider_data).unwrap_or_default();
    let rule = jstring_to_string(&mut env, selection_rule).unwrap_or_default();
    let result = provider_data_api::append_current_program_diagnostics(&data, overlap_count, selected_program_id, &rule);
    java_string(&mut env, Some(provider_result_json(result)))
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
    if handle == 0 { return STATUS_INVALID_HANDLE; }
    match registry().lock() {
        Ok(mut guard) => if guard.remove(handle) { STATUS_OK } else { STATUS_INVALID_HANDLE },
        Err(_) => STATUS_INTERNAL_ERROR,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeIngestSection(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    pid: jint,
    section: JByteArray<'_>,
) -> jint {
    if !(0..=0x1fff).contains(&pid) { return STATUS_INVALID_PID; }
    let section = match env.convert_byte_array(section) {
        Ok(v) => v,
        Err(_) => return STATUS_JNI_ERROR,
    };
    with_state_mut(handle, STATUS_INVALID_HANDLE, |state| state.ingest_section(pid as u16, &section))
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
    with_state(handle, STATUS_INVALID_HANDLE, |state| discovery_stage_to_jint(state.discovery_stage()))
}

#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]



#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]


#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeDecodeAribString(
    mut env: JNIEnv<'_>, _this: JObject<'_>, bytes: JByteArray<'_>,
) -> jstring {
    let decoded = match env.convert_byte_array(bytes) {
        Ok(v) => arib_string::decode_arib_string_lossy(&v),
        Err(_) => String::new(),
    };
    java_string(&mut env, Some(decoded))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeDecodeAribStringDiagnosticSummary(
    mut env: JNIEnv<'_>, _this: JObject<'_>, bytes: JByteArray<'_>,
) -> jstring {
    let summary = match env.convert_byte_array(bytes) {
        Ok(v) => arib_string::decode_arib_string_lossy_with_diagnostic(&v).1.summary(),
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
            scope: crate::eit::EitScope::PresentFollowing,
            service_id: 101,
            transport_stream_id: 16625,
            original_network_id: 4,
            event_id: 300,
            start_time_millis: 1,
            duration_millis: 1,
            free_ca_mode: false,
            descriptors: crate::descriptors::EventDescriptors {
                event_groups: vec![crate::descriptors::EventGroupDescriptor {
                    group_type,
                    events: vec![crate::descriptors::EventGroupReference {
                        service_id: 101,
                        event_id,
                        original_network_id: None,
                        transport_stream_id: None,
                    }],
                    other_network_events: Vec::new(),
                }],
                ..crate::descriptors::EventDescriptors::default()
            },
        }
    }

    #[test]
    fn event_group_kind_mapping_matches_r51_design() {
        let cases = [(0x1, "shared"), (0x2, "relay"), (0x3, "movement"), (0x4, "relay"), (0x5, "movement")];
        for (group_type, kind) in cases {
            let json = event_related_items_json(&minimal_event_for_related_items(group_type, 0x0100 + group_type as u16));
            assert!(json.contains(&format!("\"kind\":\"{}\"", kind)), "{} missing in {}", kind, json);
            assert!(json.contains(&format!("\"groupType\":{}", group_type)), "groupType missing in {}", json);
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
    fn ingest_pat_updates_service_count_without_pointer_handles() {
        let mut state = ParserState::default();
        let pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00,
            0x00, 0x01, 0xe1, 0x00,
        ]);
        assert_eq!(state.ingest_section(0x0000, &pat), STATUS_OK);
        assert_eq!(state.sections_seen, 1);
        assert_eq!(state.raw_snapshot_for_debug().services.len(), 0);
    }

    #[test]
    fn unsupported_private_section_is_retained_for_cas_path() {
        let mut state = ParserState::default();
        let section = vec![0x80, 0x00, 0x03, 0xaa, 0xbb, 0xcc];
        assert_eq!(state.ingest_section(0x0123, &section), STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE);
        assert_eq!(state.private_sections.len(), 1);
        assert_eq!(state.private_sections[0].pid, 0x0123);
        assert_eq!(state.private_sections[0].table_id, 0x80);
    }

    #[test]
    fn next_section_is_ignored_not_published() {
        let mut state = ParserState::default();
        let pat_next = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc0, 0x00, 0x00,
            0x00, 0x01, 0xe1, 0x00,
        ]);
        assert_eq!(state.ingest_section(0x0000, &pat_next), STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE);
        assert_eq!(state.services().len(), 0);
    }

    #[test]
    fn bad_crc_si_section_is_rejected() {
        let mut state = ParserState::default();
        let mut pat = section_with_crc(vec![
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00,
            0x00, 0x01, 0xe1, 0x00,
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
            0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1, 0x00, 0x00,
            0x00, 0x01, 0xe1, 0x00,
        ]);
        assert_eq!(state.ingest_section(0x0000, &pat), STATUS_OK);
        let pmt = section_with_crc(vec![
            0x02, 0xb0, 0x10, 0x00, 0x01, 0xc1, 0x00, 0x00,
            0xe1, 0x01, 0xf0, 0x03,
            0x09, 0x06, 0x00,
        ]);
        assert_eq!(state.ingest_section(0x0100, &pmt), STATUS_MALFORMED_DESCRIPTOR);
    }

    #[test]
    fn table_id_0x02_on_unknown_pid_is_ignored_not_pmt() {
        let mut state = ParserState::default();
        let pmt_like = section_with_crc(vec![
            0x02, 0xb0, 0x10, 0x00, 0x01, 0xc1, 0x00, 0x00,
            0xe1, 0x01, 0xf0, 0x03,
            0x09, 0x06, 0x00,
        ]);
        assert_eq!(state.ingest_section(0x0100, &pmt_like), STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE);
        assert_eq!(state.services().len(), 0);
    }
}

