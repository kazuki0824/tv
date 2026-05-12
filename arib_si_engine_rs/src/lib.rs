
mod arib_jis_x0208_table;
mod arib_string;
mod ca_descriptor;
mod descriptors;
mod discovery_requirements;
mod eit;
mod sections;
mod service_discovery;

use ca_descriptor::CaDescriptor;
use jni::objects::{JByteArray, JObject};
use jni::sys::{jbyteArray, jint, jlong, jstring};
use jni::JNIEnv;
use sections::{parse_section_header, section_crc_valid};
use eit::{EitEvent, EitUpdateWindow};
use descriptors::{event_descriptor_diagnostic, event_provider_fields};
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
            if section_has_malformed_descriptor_loop(pid, table_id, section, self.collector.is_known_pmt_pid(pid)) {
                self.last_status = STATUS_MALFORMED_DESCRIPTOR;
                return STATUS_MALFORMED_DESCRIPTOR;
            }
            self.collector.push_section(pid, section);
            self.last_status = STATUS_OK;
            STATUS_OK
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

    fn epg_update_windows(&self) -> Vec<EitUpdateWindow> {
        self.collector.epg_update_windows()
    }

    fn clear_epg_update_windows(&mut self) {
        self.collector.clear_epg_update_windows()
    }
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

fn bool_to_jint(value: Option<bool>) -> jint {
    match value {
        Some(true) => 1,
        Some(false) => 0,
        None => STATUS_INDEX_OUT_OF_RANGE,
    }
}

fn service_at(state: &ParserState, index: jint) -> Option<DiscoveredService> {
    if index < 0 { return None; }
    state.services().get(index as usize).cloned()
}

fn cas_discovery_service_at(state: &ParserState, index: jint) -> Option<DiscoveredService> {
    if index < 0 { return None; }
    state.cas_discovery_services().get(index as usize).cloned()
}

fn transport_at(state: &ParserState, index: jint) -> Option<DiscoveredTransport> {
    if index < 0 { return None; }
    state.transports().get(index as usize).cloned()
}

fn stream_at(state: &ParserState, service_index: jint, es_index: jint) -> Option<DiscoveredElementaryStream> {
    if es_index < 0 { return None; }
    service_at(state, service_index).and_then(|s| s.streams.get(es_index as usize).cloned())
}

fn program_ca_at(state: &ParserState, service_index: jint, ca_index: jint) -> Option<CaDescriptor> {
    if ca_index < 0 { return None; }
    service_at(state, service_index).and_then(|s| s.program_ca_descriptors.get(ca_index as usize).cloned())
}

fn es_ca_group_at(state: &ParserState, service_index: jint, es_ca_index: jint) -> Option<EsCaMetadata> {
    if es_ca_index < 0 { return None; }
    service_at(state, service_index).and_then(|s| s.es_ca_descriptors.get(es_ca_index as usize).cloned())
}

fn es_ca_at(state: &ParserState, service_index: jint, es_ca_index: jint, ca_index: jint) -> Option<CaDescriptor> {
    if ca_index < 0 { return None; }
    es_ca_group_at(state, service_index, es_ca_index).and_then(|g| g.descriptors.get(ca_index as usize).cloned())
}

fn cas_discovery_stream_at(state: &ParserState, service_index: jint, es_index: jint) -> Option<DiscoveredElementaryStream> {
    if es_index < 0 { return None; }
    cas_discovery_service_at(state, service_index).and_then(|s| s.streams.get(es_index as usize).cloned())
}

fn cas_discovery_program_ca_at(state: &ParserState, service_index: jint, ca_index: jint) -> Option<CaDescriptor> {
    if ca_index < 0 { return None; }
    cas_discovery_service_at(state, service_index).and_then(|s| s.program_ca_descriptors.get(ca_index as usize).cloned())
}

fn cas_discovery_es_ca_group_at(state: &ParserState, service_index: jint, es_ca_index: jint) -> Option<EsCaMetadata> {
    if es_ca_index < 0 { return None; }
    cas_discovery_service_at(state, service_index).and_then(|s| s.es_ca_descriptors.get(es_ca_index as usize).cloned())
}

fn cas_discovery_es_ca_at(state: &ParserState, service_index: jint, es_ca_index: jint, ca_index: jint) -> Option<CaDescriptor> {
    if ca_index < 0 { return None; }
    cas_discovery_es_ca_group_at(state, service_index, es_ca_index).and_then(|g| g.descriptors.get(ca_index as usize).cloned())
}

fn cat_ca_at(state: &ParserState, ca_index: jint) -> Option<CaDescriptor> {
    if ca_index < 0 { return None; }
    state.snapshot().cat_ca.descriptors.get(ca_index as usize).cloned()
}

fn cas_discovery_cat_ca_at(state: &ParserState, ca_index: jint) -> Option<CaDescriptor> {
    if ca_index < 0 { return None; }
    state.raw_cat_ca_descriptors().get(ca_index as usize).cloned()
}

fn private_section_at(state: &ParserState, index: jint) -> Option<PrivateSectionRecord> {
    if index < 0 { return None; }
    state.private_sections.get(index as usize).cloned()
}

fn pmt_mapping_at(state: &ParserState, index: jint) -> Option<crate::service_discovery::PmtPidMapping> {
    if index < 0 { return None; }
    state.snapshot().pmt_pids_by_service.get(index as usize).cloned()
}


fn publishability_at(state: &ParserState, index: jint) -> Option<ServicePublishability> {
    if index < 0 { return None; }
    state.publishability().get(index as usize).cloned()
}

fn discovery_stage_to_jint(stage: DiscoveryPublishStage) -> jint {
    match stage {
        DiscoveryPublishStage::Incomplete => DISCOVERY_STAGE_INCOMPLETE,
        DiscoveryPublishStage::Partial => DISCOVERY_STAGE_PARTIAL,
        DiscoveryPublishStage::Complete => DISCOVERY_STAGE_COMPLETE,
    }
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
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.publishability().len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityOriginalNetworkId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| p.original_network_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityTransportStreamId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| p.transport_stream_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityServiceId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| p.service_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityIsPublishable(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.publishable { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityIsChannelRegistrationReady(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.channel_registration_ready { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityIsClearLivePlaybackSupported(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.clear_live_playback_supported { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityIsEpgPublishable(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.epg_publishable { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityRequiresCas(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.requires_cas { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityUnsupportedCas(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.unsupported_cas { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityPmtPidResolved(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.pmt_pid_resolved { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityPmtParsed(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.pmt_parsed { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityCaStateResolved(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.ca_state_resolved { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityFreeCaModeResolved(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| publishability_at(state, index).map(|p| if p.free_ca_mode_resolved { 1 } else { 0 }).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityMissingComponents(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| publishability_at(state, index).map(|p| { let mut v = p.missing_components; v.sort_unstable(); v.dedup(); v.join(",") }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityReasons(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| publishability_at(state, index).map(|p| { let mut v = p.reasons; v.sort_unstable(); v.dedup(); v.join(",") }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityRegistrationReasons(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| publishability_at(state, index).map(|p| { let mut v = p.registration_reasons; v.sort_unstable(); v.dedup(); v.join(",") }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPublishabilityEpgReasons(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| publishability_at(state, index).map(|p| { let mut v = p.epg_reasons; v.sort_unstable(); v.dedup(); v.join(",") }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPmtPidMappingCount(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.snapshot().pmt_pids_by_service.len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetSectionFilterPmtPidCount(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.pmt_pids_for_section_filters().len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetSectionFilterPmtPid(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| {
        if index < 0 { return STATUS_INDEX_OUT_OF_RANGE; }
        state.pmt_pids_for_section_filters().get(index as usize).copied().map(|pid| pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE)
    })
}


#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPmtPidMappingTransportStreamId(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| pmt_mapping_at(state, index).map(|mapping| mapping.transport_stream_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPmtPidMappingOriginalNetworkId(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| pmt_mapping_at(state, index).map(|mapping| mapping.original_network_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPmtPidMappingServiceId(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| pmt_mapping_at(state, index).map(|mapping| mapping.service_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPmtPidMappingPmtPid(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| pmt_mapping_at(state, index).map(|mapping| mapping.pmt_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceCount(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.services().len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| service_at(state, index).map(|s| s.service_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetTransportStreamId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| service_at(state, index).map(|s| s.transport_stream_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetOriginalNetworkId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| service_at(state, index).map(|s| s.original_network_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceName(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| service_at(state, index).and_then(|s| s.service_name));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetProviderName(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| service_at(state, index).and_then(|s| s.provider_name));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceType(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| service_at(state, index).and_then(|s| s.service_type.map(|v| v as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPmtPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| {
        let Some(service) = service_at(state, index) else { return STATUS_INDEX_OUT_OF_RANGE; };
        service.pmt_pid.map(|p| p as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE)
    })
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPcrPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| service_at(state, index).and_then(|s| s.pcr_pid.map(|p| p as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetFreeCaMode(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| bool_to_jint(service_at(state, index).and_then(|s| s.free_ca_mode)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint,
) -> jint {
    with_state(handle, 0, |state| service_at(state, service_index).map(|s| s.streams.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsElementaryPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| stream_at(state, service_index, es_index).map(|s| s.elementary_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsStreamType(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| stream_at(state, service_index, es_index).map(|s| s.stream_type as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsComponentTag(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| stream_at(state, service_index, es_index).and_then(|s| s.component_tag.map(|v| v as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsComponentType(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| stream_at(state, service_index, es_index).and_then(|s| s.component_type.map(|v| v as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsStreamContent(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| stream_at(state, service_index, es_index).and_then(|s| s.stream_content.map(|v| v as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsLanguageCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, 0, |state| stream_at(state, service_index, es_index).map(|s| s.language_codes.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsLanguage(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint, lang_index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| {
        if lang_index < 0 { return None; }
        stream_at(state, service_index, es_index).and_then(|s| s.language_codes.get(lang_index as usize).cloned())
    });
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceProgramCaCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint,
) -> jint {
    with_state(handle, 0, |state| service_at(state, service_index).map(|s| s.program_ca_descriptors.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceProgramCaSystemId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| program_ca_at(state, service_index, ca_index).map(|ca| ca.ca_system_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceProgramCaPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| program_ca_at(state, service_index, ca_index).map(|ca| ca.ca_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceProgramCaPrivateData(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| program_ca_at(state, service_index, ca_index).map(|ca| ca.private_data).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceProgramCaRawDescriptor(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| program_ca_at(state, service_index, ca_index).map(|ca| ca.raw_descriptor).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsCaCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint,
) -> jint {
    with_state(handle, 0, |state| service_at(state, service_index).map(|s| s.es_ca_descriptors.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsCaElementaryPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| es_ca_group_at(state, service_index, es_ca_index).map(|g| g.elementary_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsCaDescriptorCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint,
) -> jint {
    with_state(handle, 0, |state| es_ca_group_at(state, service_index, es_ca_index).map(|g| g.descriptors.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsCaSystemId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| es_ca_at(state, service_index, es_ca_index, ca_index).map(|ca| ca.ca_system_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsCaPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| es_ca_at(state, service_index, es_ca_index, ca_index).map(|ca| ca.ca_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsCaPrivateData(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| es_ca_at(state, service_index, es_ca_index, ca_index).map(|ca| ca.private_data).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetServiceEsCaRawDescriptor(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| es_ca_at(state, service_index, es_ca_index, ca_index).map(|ca| ca.raw_descriptor).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCatCaCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.snapshot().cat_ca.descriptors.len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCatCaSystemId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cat_ca_at(state, ca_index).map(|ca| ca.ca_system_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCatCaPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cat_ca_at(state, ca_index).map(|ca| ca.ca_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCatCaPrivateData(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| cat_ca_at(state, ca_index).map(|ca| ca.private_data).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCatCaRawDescriptor(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| cat_ca_at(state, ca_index).map(|ca| ca.raw_descriptor).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.cas_discovery_services().len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_service_at(state, index).map(|s| s.service_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryTransportStreamId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_service_at(state, index).map(|s| s.transport_stream_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryOriginalNetworkId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_service_at(state, index).map(|s| s.original_network_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceName(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| cas_discovery_service_at(state, index).and_then(|s| s.service_name));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryProviderName(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| cas_discovery_service_at(state, index).and_then(|s| s.provider_name));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceType(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_service_at(state, index).and_then(|s| s.service_type.map(|v| v as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryPmtPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_service_at(state, index).and_then(|s| s.pmt_pid.map(|p| p as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryPcrPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_service_at(state, index).and_then(|s| s.pcr_pid.map(|p| p as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryFreeCaMode(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| bool_to_jint(cas_discovery_service_at(state, index).and_then(|s| s.free_ca_mode)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint,
) -> jint {
    with_state(handle, 0, |state| cas_discovery_service_at(state, service_index).map(|s| s.streams.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsElementaryPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_stream_at(state, service_index, es_index).map(|s| s.elementary_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsStreamType(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_stream_at(state, service_index, es_index).map(|s| s.stream_type as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsComponentTag(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_stream_at(state, service_index, es_index).and_then(|s| s.component_tag.map(|v| v as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsComponentType(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_stream_at(state, service_index, es_index).and_then(|s| s.component_type.map(|v| v as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsStreamContent(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_stream_at(state, service_index, es_index).and_then(|s| s.stream_content.map(|v| v as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsLanguageCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint,
) -> jint {
    with_state(handle, 0, |state| cas_discovery_stream_at(state, service_index, es_index).map(|s| s.language_codes.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsLanguage(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_index: jint, lang_index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| {
        if lang_index < 0 { return None; }
        cas_discovery_stream_at(state, service_index, es_index).and_then(|s| s.language_codes.get(lang_index as usize).cloned())
    });
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceProgramCaCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint,
) -> jint {
    with_state(handle, 0, |state| cas_discovery_service_at(state, service_index).map(|s| s.program_ca_descriptors.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceProgramCaSystemId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_program_ca_at(state, service_index, ca_index).map(|ca| ca.ca_system_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceProgramCaPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_program_ca_at(state, service_index, ca_index).map(|ca| ca.ca_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceProgramCaPrivateData(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| cas_discovery_program_ca_at(state, service_index, ca_index).map(|ca| ca.private_data).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceProgramCaRawDescriptor(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| cas_discovery_program_ca_at(state, service_index, ca_index).map(|ca| ca.raw_descriptor).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsCaCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint,
) -> jint {
    with_state(handle, 0, |state| cas_discovery_service_at(state, service_index).map(|s| s.es_ca_descriptors.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsCaElementaryPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_es_ca_group_at(state, service_index, es_ca_index).map(|g| g.elementary_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsCaDescriptorCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint,
) -> jint {
    with_state(handle, 0, |state| cas_discovery_es_ca_group_at(state, service_index, es_ca_index).map(|g| g.descriptors.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsCaSystemId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_es_ca_at(state, service_index, es_ca_index, ca_index).map(|ca| ca.ca_system_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsCaPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_es_ca_at(state, service_index, es_ca_index, ca_index).map(|ca| ca.ca_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsCaPrivateData(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| cas_discovery_es_ca_at(state, service_index, es_ca_index, ca_index).map(|ca| ca.private_data).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryServiceEsCaRawDescriptor(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, service_index: jint, es_ca_index: jint, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| cas_discovery_es_ca_at(state, service_index, es_ca_index, ca_index).map(|ca| ca.raw_descriptor).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryCatCaCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.raw_cat_ca_descriptors().len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryCatCaSystemId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_cat_ca_at(state, ca_index).map(|ca| ca.ca_system_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryCatCaPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, ca_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| cas_discovery_cat_ca_at(state, ca_index).map(|ca| ca.ca_pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryCatCaPrivateData(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| cas_discovery_cat_ca_at(state, ca_index).map(|ca| ca.private_data).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetCasDiscoveryCatCaRawDescriptor(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, ca_index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| cas_discovery_cat_ca_at(state, ca_index).map(|ca| ca.raw_descriptor).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPrivateSectionCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.private_sections.len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPrivateSectionPid(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| private_section_at(state, index).map(|r| r.pid as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPrivateSectionTableId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| private_section_at(state, index).map(|r| r.table_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetPrivateSectionBytes(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jbyteArray {
    let bytes = with_state(handle, Vec::new(), |state| private_section_at(state, index).map(|r| r.bytes).unwrap_or_default());
    java_byte_array(&mut env, &bytes)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetTransportCount(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.transports().len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetTransportStreamIdByIndex(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| transport_at(state, index).map(|t| t.transport_stream_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetTransportOriginalNetworkIdByIndex(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| transport_at(state, index).map(|t| t.original_network_id as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetNetworkName(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| transport_at(state, index).and_then(|t| t.network_name));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetTransportStreamName(
    mut env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| transport_at(state, index).and_then(|t| t.ts_name));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetRemoteControlKeyId(
    _env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong, index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| transport_at(state, index).and_then(|t| t.remote_control_key_id.map(|v| v as jint)).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

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

fn event_at(state: &ParserState, index: jint) -> Option<EitEvent> {
    if index < 0 { return None; }
    state.events().get(index as usize).cloned()
}

fn epg_update_window_at(state: &ParserState, index: jint) -> Option<EitUpdateWindow> {
    if index < 0 { return None; }
    state.epg_update_windows().get(index as usize).cloned()
}

fn stable_identity_string(stable: eit::EitStableEventIdentity) -> String {
    format!(
        "onid={};tsid={};sid={};event={}",
        stable.original_network_id, stable.transport_stream_id, stable.service_id, stable.event_id
    )
}


#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEpgUpdateWindowCount(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_state(handle, 0, |state| state.epg_update_windows().len() as jint)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeClearEpgUpdateWindows(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_state_mut(handle, STATUS_INVALID_HANDLE, |state| { state.clear_epg_update_windows(); STATUS_OK })
}

macro_rules! epg_update_window_int_getter {
    ($name:ident, $field:expr) => {
        #[no_mangle]
        pub extern "system" fn $name(
            _env: JNIEnv<'_>,
            _this: JObject<'_>,
            handle: jlong,
            index: jint,
        ) -> jint {
            with_state(handle, STATUS_INVALID_HANDLE, |state| epg_update_window_at(state, index).map($field).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
        }
    };
}

epg_update_window_int_getter!(Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEpgUpdateWindowOriginalNetworkId, |w: EitUpdateWindow| w.original_network_id as jint);
epg_update_window_int_getter!(Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEpgUpdateWindowTransportStreamId, |w: EitUpdateWindow| w.transport_stream_id as jint);
epg_update_window_int_getter!(Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEpgUpdateWindowServiceId, |w: EitUpdateWindow| w.service_id as jint);

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEpgUpdateWindowStartMillis(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jlong {
    with_state(handle, STATUS_INVALID_HANDLE as jlong, |state| epg_update_window_at(state, index).map(|w| w.window_start_millis as jlong).unwrap_or(STATUS_INDEX_OUT_OF_RANGE as jlong))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEpgUpdateWindowEndMillis(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jlong {
    with_state(handle, STATUS_INVALID_HANDLE as jlong, |state| epg_update_window_at(state, index).map(|w| w.window_end_millis as jlong).unwrap_or(STATUS_INDEX_OUT_OF_RANGE as jlong))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEpgUpdateWindowValidProgramKeyCount(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jint {
    with_state(handle, 0, |state| epg_update_window_at(state, index).map(|w| w.valid_event_identities.len() as jint).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEpgUpdateWindowValidProgramKey(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
    key_index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| {
        if key_index < 0 { return None; }
        epg_update_window_at(state, index)
            .and_then(|w| w.valid_event_identities.get(key_index as usize).copied())
            .map(stable_identity_string)
    });
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventCount(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| state.events().len() as jint)
}

macro_rules! event_int_getter {
    ($name:ident, $field:expr) => {
        #[no_mangle]
        pub extern "system" fn $name(
            _env: JNIEnv<'_>,
            _this: JObject<'_>,
            handle: jlong,
            index: jint,
        ) -> jint {
            with_state(handle, STATUS_INVALID_HANDLE, |state| event_at(state, index).map($field).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
        }
    };
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventStableIdentity(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).map(|e| {
        stable_identity_string(e.stable_identity())
    }));
    java_string(&mut env, value)
}

event_int_getter!(Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventServiceId, |e: EitEvent| e.service_id as jint);
event_int_getter!(Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventTransportStreamId, |e: EitEvent| e.transport_stream_id as jint);
event_int_getter!(Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventOriginalNetworkId, |e: EitEvent| e.original_network_id as jint);
event_int_getter!(Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventId, |e: EitEvent| e.event_id as jint);

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventStartTimeMillis(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jlong {
    with_state(handle, STATUS_INVALID_HANDLE as jlong, |state| event_at(state, index).map(|e| e.start_time_millis as jlong).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventDurationMillis(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jlong {
    with_state(handle, STATUS_INVALID_HANDLE as jlong, |state| event_at(state, index).map(|e| e.duration_millis as jlong).unwrap_or(0))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventTitle(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).map(|e| event_provider_fields(&e.descriptors).title));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventDescription(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).map(|e| event_provider_fields(&e.descriptors).description));
    java_string(&mut env, value)
}




#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventExtendedDescription(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).map(|e| event_provider_fields(&e.descriptors).extended_description));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventExtendedItemsJson(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).map(|e| {
        format!(
            "[{}]",
            e.descriptors.extended_items.iter().map(|item| format!(
                "{{\"description\":\"{}\",\"text\":\"{}\"}}",
                descriptors::json_escape(&item.item_description),
                descriptors::json_escape(&item.item_text)
            )).collect::<Vec<_>>().join(",")
        )
    }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventComponentText(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).and_then(|e| {
        let text = e.descriptors.components.iter().map(|c| c.text.clone()).filter(|v| !v.is_empty()).collect::<Vec<_>>().join("\n");
        (!text.is_empty()).then_some(text)
    }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventAudioComponentText(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).and_then(|e| {
        let text = e.descriptors.audio_components.iter().map(|a| a.text.clone()).filter(|v| !v.is_empty()).collect::<Vec<_>>().join("\n");
        (!text.is_empty()).then_some(text)
    }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventSeriesName(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).and_then(|e| {
        let text = e.descriptors.series.iter().map(|s| s.series_name.clone()).filter(|v| !v.is_empty()).collect::<Vec<_>>().join("\n");
        (!text.is_empty()).then_some(text)
    }));
    java_string(&mut env, value)
}


#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventAudioLanguage(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).and_then(|e| {
        let mut langs = Vec::new();
        for audio in &e.descriptors.audio_components {
            if !audio.language_code.is_empty() && !langs.contains(&audio.language_code) {
                langs.push(audio.language_code.clone());
            }
            if let Some(second) = &audio.language_code_2 {
                if !second.is_empty() && !langs.contains(second) {
                    langs.push(second.clone());
                }
            }
        }
        (!langs.is_empty()).then_some(langs.join(","))
    }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventCanonicalGenre(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    _handle: jlong,
    _index: jint,
) -> jstring {
    // 非推奨互換シンボル。r51 では ARIB content_descriptor を
    // arib_si_engine_rs 内で Android canonical genre へ写像しない。
    java_string(&mut env, Some(String::new()))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventDiagnosticDescriptorJson(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).map(|e| event_descriptor_diagnostic(&e.descriptors).descriptor_json));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventScope(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).map(|e| e.scope.as_str().to_string()));
    java_string(&mut env, value)
}


fn arib_content_major_name(level1: u8) -> &'static str {
    match level1 {
        0x0 => "ニュース/報道",
        0x1 => "スポーツ",
        0x2 => "情報/ワイドショー",
        0x3 => "ドラマ",
        0x4 => "音楽",
        0x5 => "バラエティ",
        0x6 => "映画",
        0x7 => "アニメ/特撮",
        0x8 => "ドキュメンタリー/教養",
        0x9 => "劇場/公演",
        0xa => "趣味/教育",
        0xb => "福祉",
        _ => "その他",
    }
}

fn arib_content_minor_name(level1: u8, level2: u8) -> &'static str {
    match (level1, level2) {
        (0x0, 0x0) => "定時・総合",
        (0x0, 0x1) => "天気",
        (0x0, 0x2) => "特集・ドキュメント",
        (0x0, 0x3) => "政治・国会",
        (0x0, 0x4) => "経済・市況",
        (0x0, 0x5) => "海外・国際",
        (0x0, 0x6) => "解説",
        (0x0, 0x7) => "討論・会談",
        (0x0, 0x8) => "報道特番",
        (0x0, 0x9) => "ローカル・地域",
        (0x0, 0xa) => "交通",
        (0x1, 0x0) => "スポーツニュース",
        (0x1, 0x1) => "野球",
        (0x1, 0x2) => "サッカー",
        (0x1, 0x3) => "ゴルフ",
        (0x1, 0x4) => "その他の球技",
        (0x1, 0x5) => "相撲・格闘技",
        (0x1, 0x6) => "オリンピック・国際大会",
        (0x1, 0x7) => "マラソン・陸上・水泳",
        (0x1, 0x8) => "モータースポーツ",
        (0x1, 0x9) => "マリン・ウィンタースポーツ",
        (0x1, 0xa) => "競馬・公営競技",
        (0x2, 0x0) => "芸能・ワイドショー",
        (0x2, 0x1) => "ファッション",
        (0x2, 0x2) => "暮らし・住まい",
        (0x2, 0x3) => "健康・医療",
        (0x2, 0x4) => "ショッピング・通販",
        (0x2, 0x5) => "グルメ・料理",
        (0x2, 0x6) => "イベント",
        (0x2, 0x7) => "番組紹介・お知らせ",
        (0x3, 0x0) => "国内ドラマ",
        (0x3, 0x1) => "海外ドラマ",
        (0x3, 0x2) => "時代劇",
        (0x4, 0x0) => "国内ロック・ポップス",
        (0x4, 0x1) => "海外ロック・ポップス",
        (0x4, 0x2) => "クラシック・オペラ",
        (0x4, 0x3) => "ジャズ・フュージョン",
        (0x4, 0x4) => "歌謡曲・演歌",
        (0x4, 0x5) => "ライブ・コンサート",
        (0x4, 0x6) => "ランキング・リクエスト",
        (0x4, 0x7) => "カラオケ・のど自慢",
        (0x4, 0x8) => "民謡・邦楽",
        (0x4, 0x9) => "童謡・キッズ",
        (0x4, 0xa) => "民族音楽・ワールドミュージック",
        (0x5, 0x0) => "クイズ",
        (0x5, 0x1) => "ゲーム",
        (0x5, 0x2) => "トークバラエティ",
        (0x5, 0x3) => "お笑い・コメディ",
        (0x5, 0x4) => "音楽バラエティ",
        (0x5, 0x5) => "旅バラエティ",
        (0x5, 0x6) => "料理バラエティ",
        (0x6, 0x0) => "洋画",
        (0x6, 0x1) => "邦画",
        (0x6, 0x2) => "アニメ",
        (0x7, 0x0) => "国内アニメ",
        (0x7, 0x1) => "海外アニメ",
        (0x7, 0x2) => "特撮",
        (0x8, 0x0) => "社会・時事",
        (0x8, 0x1) => "歴史・紀行",
        (0x8, 0x2) => "自然・動物・環境",
        (0x8, 0x3) => "宇宙・科学・医学",
        (0x8, 0x4) => "カルチャー・伝統文化",
        (0x8, 0x5) => "文学・文芸",
        (0x8, 0x6) => "スポーツ",
        (0x8, 0x7) => "ドキュメンタリー全般",
        (0x8, 0x8) => "インタビュー・討論",
        (0x9, 0x0) => "現代劇・新劇",
        (0x9, 0x1) => "ミュージカル",
        (0x9, 0x2) => "ダンス・バレエ",
        (0x9, 0x3) => "落語・演芸",
        (0x9, 0x4) => "歌舞伎・古典",
        (0xa, 0x0) => "旅・釣り・アウトドア",
        (0xa, 0x1) => "園芸・ペット・手芸",
        (0xa, 0x2) => "音楽・美術・工芸",
        (0xa, 0x3) => "囲碁・将棋",
        (0xa, 0x4) => "麻雀・パチンコ",
        (0xa, 0x5) => "車・オートバイ",
        (0xa, 0x6) => "コンピュータ・TVゲーム",
        (0xa, 0x7) => "会話・語学",
        (0xa, 0x8) => "幼児・小学生",
        (0xa, 0x9) => "中学生・高校生",
        (0xa, 0xa) => "大学生・受験",
        (0xa, 0xb) => "生涯教育・資格",
        (0xa, 0xc) => "教育問題",
        (0xb, 0x0) => "高齢者",
        (0xb, 0x1) => "障害者",
        (0xb, 0x2) => "社会福祉",
        (0xb, 0x3) => "ボランティア",
        (0xb, 0x4) => "手話",
        (0xb, 0x5) => "文字（字幕）",
        (0xb, 0x6) => "音声解説",
        (_, 0xf) => "その他",
        _ => "未定義",
    }
}

fn arib_content_to_ui_text(level1: u8, level2: u8) -> String {
    format!("{}/{}", arib_content_major_name(level1), arib_content_minor_name(level1, level2))
}



#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventBroadcastGenre(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).and_then(|e| {
        let text = e.descriptors.contents.iter()
            .map(|c| format!(
                "ARIB(0x{:x}/0x{:x}):{}",
                c.content_nibble_level_1,
                c.content_nibble_level_2,
                c.arib_display_name
            ))
            .collect::<Vec<_>>()
            .join("、");
        (!text.is_empty()).then_some(text)
    }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventGenreSupplementText(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).and_then(|e| {
        let text = e.descriptors.contents.iter()
            .map(|c| arib_content_to_ui_text(c.content_nibble_level_1, c.content_nibble_level_2))
            .collect::<Vec<_>>()
            .join("、");
        (!text.is_empty()).then_some(text)
    }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventGroupText(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).and_then(|e| {
        let mut parts = Vec::new();
        for group in &e.descriptors.event_groups {
            for related in &group.events {
                parts.push(format!("sid={} event={}", related.service_id, related.event_id));
            }
            for related in &group.other_network_events {
                parts.push(format!("onid={} tsid={} sid={} event={}", related.original_network_id.unwrap_or(0), related.transport_stream_id.unwrap_or(0), related.service_id, related.event_id));
            }
        }
        let text = parts.join("、");
        (!text.is_empty()).then_some(text)
    }));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventFreeCaText(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).map(|e| {
        if e.free_ca_mode { "有料放送".to_string() } else { "無料放送".to_string() }
    }));
    java_string(&mut env, value)
}


#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventParentalRatingCount(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    event_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| event_at(state, event_index).map(|e| e.descriptors.parental_ratings.len() as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

fn parental_rating_at(state: &ParserState, event_index: jint, rating_index: jint) -> Option<descriptors::ParentalRating> {
    if rating_index < 0 { return None; }
    event_at(state, event_index).and_then(|event| event.descriptors.parental_ratings.get(rating_index as usize).cloned())
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventParentalRatingCountryCode(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    event_index: jint,
    rating_index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| parental_rating_at(state, event_index, rating_index).map(|r| r.country_code));
    java_string(&mut env, value)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventParentalRatingValue(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    event_index: jint,
    rating_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| parental_rating_at(state, event_index, rating_index).map(|r| r.rating_value as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventParentalRatingRawValue(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    event_index: jint,
    rating_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| parental_rating_at(state, event_index, rating_index).map(|r| r.raw_rating_byte as jint).unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventParentalRatingSupported(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    event_index: jint,
    rating_index: jint,
) -> jint {
    with_state(handle, STATUS_INVALID_HANDLE, |state| parental_rating_at(state, event_index, rating_index)
        .map(|r| if r.country_code == "JPN" && r.rating_value <= 20 { 1 } else { 0 })
        .unwrap_or(STATUS_INDEX_OUT_OF_RANGE))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeGetEventDiagnosticText(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jstring {
    let value = with_state(handle, None, |state| event_at(state, index).map(|e| {
        let d = e.descriptors;
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
            diagnostic.descriptor_json
        )
    }));
    java_string(&mut env, value)
}

