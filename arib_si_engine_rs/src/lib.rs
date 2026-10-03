mod arib_string;
mod broadcast_clock;
mod ca_descriptor;
mod descriptors;
mod discovery_requirements;
mod eit;
mod jvm_snapshot_generated;
pub(crate) mod provider_data;
mod sections;
mod service_discovery;

use broadcast_clock::{parse_broadcast_clock, BroadcastClockFact};
use ca_descriptor::MalformedCaDescriptorDiagnostic;
use descriptors::json_escape;
use discovery_requirements::DiscoveryProfile;
use eit::EitEvent;
use jni::objects::{JByteArray, JClass, JObject, JString, JThrowable, JValue};
use jni::sys::{jint, jintArray, jlong, jobject, jstring};
use jni::JNIEnv;
use maleicacid_arib_si_engine_core::codec_probe_dto::AacConfigurationProbeDto;
use maleicacid_arib_si_engine_core::eit_instances::EitInstances;
use maleicacid_arib_si_engine_core::runtime_snapshot_build;
use maleicacid_arib_si_engine_core::runtime_snapshot_dto::{
    BroadcastClockDto, BulkSnapshotDto, MalformedCaDescriptorCountDto, ParserDiagnosticDto,
    ServiceRegistrationSnapshotDto,
};
use provider_data as provider_data_api;
use sections::{
    parse_section_header, section_crc_valid_with_header, section_has_malformed_descriptor_loop,
};
use service_discovery::{DiscoveryPublishStage, ServiceDiscoveryCollector};
use std::collections::BTreeMap;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const STATUS_OK: jint = 0;
const STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE: jint = 1;
const STATUS_INVALID_HANDLE: jint = -1;
const STATUS_INVALID_PID: jint = -2;
const STATUS_INVALID_SECTION: jint = -3;
const STATUS_MALFORMED_DESCRIPTOR: jint = -4;
const STATUS_JNI_ERROR: jint = -6;
const STATUS_INTERNAL_ERROR: jint = -7;
const STATUS_INVALID_DISCOVERY_PROFILE: jint = -8;
const STATUS_COLLECTION_LIMIT_EXCEEDED: jint = -9;
const MAX_COLLECTION_BYTES: usize = 4 * 1024 * 1024;
const MAX_COLLECTION_SECTIONS: usize = 8192;
const MAX_COLLECTION_AGE: Duration = Duration::from_secs(60);

const DISCOVERY_STAGE_INCOMPLETE: jint = 0;
const DISCOVERY_STAGE_PARTIAL: jint = 1;
const DISCOVERY_STAGE_COMPLETE: jint = 2;

const SI_REGISTRY_LOCK_NAME: &str = "arib_si_registry";
const SI_PARSER_LOCK_NAME: &str = "arib_si_parser_state";
static SI_MODULE_ABNORMAL: AtomicBool = AtomicBool::new(false);
static SI_MUTEX_POISON_COUNT: AtomicU64 = AtomicU64::new(0);

fn record_si_mutex_poison(lock_name: &'static str) {
    let count = SI_MUTEX_POISON_COUNT
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    SI_MODULE_ABNORMAL.store(true, Ordering::Release);
    eprintln!("ARIB SI mutex汚染: lock={lock_name} poison_count={count}");
}

fn si_module_is_healthy() -> bool {
    !SI_MODULE_ABNORMAL.load(Ordering::Acquire)
}

struct ParserState {
    collection_started_at: Instant,
    collection_bytes: usize,
    collection_sections: usize,
    collection_limit_exceeded: bool,
    collection_generation: u64,
    collector: ServiceDiscoveryCollector,
    eit_instances: EitInstances,
    sections_seen: u64,
    last_status: jint,
    invalid_section_reason: Option<InvalidSectionReason>,
    latest_broadcast_clock: Option<BroadcastClockFact>,
}

#[derive(Clone, Copy)]
enum InvalidSectionReason {
    Header,
    Length,
    Clock,
    Crc,
}

impl InvalidSectionReason {
    fn diagnostic(self) -> ParserDiagnosticDto {
        let (code, message) = match self {
            Self::Header => (
                "SECTION_HEADER_INVALID",
                "section headerが不正または途中で切れています",
            ),
            Self::Length => ("SECTION_LENGTH_MISMATCH", "section長が入力長と一致しません"),
            Self::Clock => ("BROADCAST_CLOCK_INVALID", "放送時刻sectionが不正です"),
            Self::Crc => ("SECTION_CRC_MISMATCH", "section CRCが一致しません"),
        };
        ParserDiagnosticDto {
            code: code.to_string(),
            message: message.to_string(),
            severity: Some("error".to_string()),
        }
    }
}

impl Default for ParserState {
    fn default() -> Self {
        Self {
            collection_started_at: Instant::now(),
            collection_bytes: 0,
            collection_sections: 0,
            collection_limit_exceeded: false,
            collection_generation: 0,
            eit_instances: EitInstances::default(),
            collector: ServiceDiscoveryCollector::default(),
            sections_seen: 0,
            last_status: STATUS_OK,
            invalid_section_reason: None,
            latest_broadcast_clock: None,
        }
    }
}

impl ParserState {
    fn clear_collection_facts(&mut self) {
        self.collector.reset_collection();
        self.eit_instances = EitInstances::default();
        self.collection_generation = self.collection_generation.saturating_add(1);
        self.latest_broadcast_clock = None;
        self.invalid_section_reason = None;
    }

    fn expire_collection_at(&mut self, now: Instant) {
        if now.saturating_duration_since(self.collection_started_at) >= MAX_COLLECTION_AGE {
            self.clear_collection_facts();
            self.collection_started_at = now;
            self.collection_bytes = 0;
            self.collection_sections = 0;
            self.collection_limit_exceeded = false;
            self.last_status = STATUS_OK;
        }
    }

    fn admit_section(&mut self, length: usize) -> bool {
        if self.collection_limit_exceeded {
            return false;
        }
        let Some(total_bytes) = self
            .collection_bytes
            .checked_add(length)
            .filter(|total| *total <= MAX_COLLECTION_BYTES)
            .filter(|_| self.collection_sections < MAX_COLLECTION_SECTIONS)
        else {
            self.collection_limit_exceeded = true;
            self.clear_collection_facts();
            return false;
        };
        self.collection_bytes = total_bytes;
        self.collection_sections += 1;
        true
    }

    fn is_section_for_discovery(&self, pid: u16, table_id: u8) -> bool {
        is_fixed_pid_si_table_for_discovery(pid, table_id)
            || (table_id == 0x02 && self.collector.is_known_pmt_pid(pid))
    }

    fn ingest_section(&mut self, pid: u16, section: &[u8]) -> jint {
        self.expire_collection_at(Instant::now());
        self.invalid_section_reason = None;
        if !self.admit_section(section.len()) {
            self.last_status = STATUS_COLLECTION_LIMIT_EXCEEDED;
            return self.last_status;
        }
        let Some(header) = parse_section_header(section) else {
            self.last_status = STATUS_INVALID_SECTION;
            self.invalid_section_reason = Some(InvalidSectionReason::Header);
            return STATUS_INVALID_SECTION;
        };
        if header.total_length != section.len() {
            self.last_status = STATUS_INVALID_SECTION;
            self.invalid_section_reason = Some(InvalidSectionReason::Length);
            return STATUS_INVALID_SECTION;
        }

        if pid == 0x0012 {
            self.eit_instances.ingest(section);
        }
        self.sections_seen = self.sections_seen.saturating_add(1);
        let table_id = header.table_id;
        if pid == 0x0014 && matches!(table_id, 0x70 | 0x73) {
            let Some(clock) = parse_broadcast_clock(section, &header) else {
                self.last_status = STATUS_INVALID_SECTION;
                self.invalid_section_reason = Some(InvalidSectionReason::Clock);
                return STATUS_INVALID_SECTION;
            };
            self.latest_broadcast_clock = Some(clock);
            self.last_status = STATUS_OK;
            return STATUS_OK;
        }
        if self.is_section_for_discovery(pid, table_id) {
            if header.current_next_indicator == Some(false) {
                self.last_status = STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE;
                return STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE;
            }
            if header.syntax && !section_crc_valid_with_header(section, &header) {
                self.last_status = STATUS_INVALID_SECTION;
                self.invalid_section_reason = Some(InvalidSectionReason::Crc);
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
    fn services(&self) -> Vec<service_discovery::DiscoveredService> {
        self.snapshot().services
    }

    fn events(&self) -> Vec<EitEvent> {
        self.eit_instances.events()
    }

    fn sdt_actual_transport_keys(&self) -> Vec<(u16, u16)> {
        self.collector.sdt_actual_transport_keys()
    }
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn malformed_ca_descriptor_counts(
    diagnostics: &[MalformedCaDescriptorDiagnostic],
) -> Vec<MalformedCaDescriptorCountDto> {
    let mut counts: BTreeMap<u16, usize> = BTreeMap::new();
    for diagnostic in diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.service_id.is_some())
    {
        if let Some(service_id) = diagnostic.service_id {
            *counts.entry(service_id).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .map(|(service_id, count)| MalformedCaDescriptorCountDto {
            service_id: i32::from(service_id),
            count: i32::try_from(count).unwrap_or(i32::MAX),
        })
        .collect()
}

fn u64_to_i64_saturating(value: u64) -> i64 {
    if value > i64::MAX as u64 {
        i64::MAX
    } else {
        value as i64
    }
}

fn build_service_registration_snapshot(
    state: &mut ParserState,
) -> ServiceRegistrationSnapshotDto {
    state.expire_collection_at(Instant::now());
    let ingest_sequence = state.sections_seen;
    let last_status = state.last_status;
    let collection_state = state.collector.state();
    let discovery_stage = collection_state.publish_stage();
    let snapshot = &collection_state.snapshot;
    let mut parser_diagnostics = parser_diagnostics(ingest_sequence, last_status, snapshot);
    if let Some(reason) = state.invalid_section_reason {
        parser_diagnostics.push(reason.diagnostic());
    }
    let actual_transport_keys = state.sdt_actual_transport_keys();

    ServiceRegistrationSnapshotDto {
        discovery_stage: discovery_stage_to_jint(discovery_stage),
        table_requirements: collection_state
            .table_requirements
            .iter()
            .map(runtime_snapshot_build::table_requirement)
            .collect(),
        transport_semantic_facts: snapshot
            .transports
            .iter()
            .map(|transport| {
                runtime_snapshot_build::transport(
                    transport,
                    actual_transport_keys
                        .contains(&(transport.transport_stream_id, transport.original_network_id)),
                )
            })
            .collect(),
        eit_instances: state
            .eit_instances
            .states()
            .iter()
            .map(runtime_snapshot_build::eit_instance)
            .collect(),
        service_semantic_facts: collection_state
            .semantic_facts_by_service
            .iter()
            .map(runtime_snapshot_build::service_semantic_facts)
            .collect(),
        parser_diagnostics,
    }
}

fn build_bulk_snapshot(state: &mut ParserState) -> BulkSnapshotDto {
    state.expire_collection_at(Instant::now());
    let ingest_sequence = state.sections_seen;
    let last_status = state.last_status;
    let collection_state = state.collector.state();
    let discovery_stage = collection_state.publish_stage();
    let snapshot = &collection_state.snapshot;
    let mut parser_diagnostics = parser_diagnostics(ingest_sequence, last_status, snapshot);
    if let Some(reason) = state.invalid_section_reason {
        parser_diagnostics.push(reason.diagnostic());
    }
    let actual_transport_keys = state.sdt_actual_transport_keys();
    let events = state
        .events()
        .iter()
        .map(|event| {
            let stable_identity = event.timing_state.has_stable_identity().then(|| {
                provider_data_api::build_program_key(
                    i32::from(event.original_network_id),
                    i32::from(event.transport_stream_id),
                    i32::from(event.service_id),
                    i32::from(event.event_id),
                )
            });
            runtime_snapshot_build::event(event, stable_identity)
        })
        .collect();

    BulkSnapshotDto {
        collection_generation: u64_to_i64_saturating(state.collection_generation),
        ingest_sequence: u64_to_i64_saturating(ingest_sequence),
        discovery_stage: discovery_stage_to_jint(discovery_stage),
        broadcast_clock: state.latest_broadcast_clock.map(|clock| BroadcastClockDto {
            table_id: i32::from(clock.table_id),
            mjd: i32::from(clock.mjd),
            millis_of_day: i64::from(clock.millis_of_day),
        }),
        table_requirements: collection_state
            .table_requirements
            .iter()
            .map(runtime_snapshot_build::table_requirement)
            .collect(),
        cat_ca_metadata: snapshot
            .cat_ca
            .descriptors
            .iter()
            .map(|ca| {
                runtime_snapshot_build::ca_metadata(None, ca, None, Some(ca.ca_pid), None, "CAT")
            })
            .collect(),
        malformed_ca_descriptor_diagnostics: snapshot
            .malformed_ca_descriptor_diagnostics
            .iter()
            .map(runtime_snapshot_build::malformed_ca)
            .collect(),
        malformed_ca_descriptor_counts: malformed_ca_descriptor_counts(
            &snapshot.malformed_ca_descriptor_diagnostics,
        ),
        transport_semantic_facts: snapshot
            .transports
            .iter()
            .map(|transport| {
                runtime_snapshot_build::transport(
                    transport,
                    actual_transport_keys
                        .contains(&(transport.transport_stream_id, transport.original_network_id)),
                )
            })
            .collect(),
        events,
        eit_instances: state
            .eit_instances
            .states()
            .iter()
            .map(runtime_snapshot_build::eit_instance)
            .collect(),
        service_semantic_facts: collection_state
            .semantic_facts_by_service
            .iter()
            .map(runtime_snapshot_build::service_semantic_facts)
            .collect(),
        parser_diagnostics,
    }
}

fn parser_diagnostics(
    sections_seen: u64,
    last_status: jint,
    snapshot: &service_discovery::DiscoverySnapshot,
) -> Vec<ParserDiagnosticDto> {
    let message = format!("sectionsSeen={} lastStatus={}", sections_seen, last_status);
    let mut diagnostics = vec![ParserDiagnosticDto {
        code: "PARSER_STATE".to_string(),
        message,
        severity: Some("info".to_string()),
    }];
    if last_status == STATUS_COLLECTION_LIMIT_EXCEEDED {
        diagnostics.push(ParserDiagnosticDto {
            code: "COLLECTION_LIMIT_EXCEEDED".to_string(),
            severity: Some("error".to_string()),
            message:
                "SI収集の入力上限に達したため事実と更新区間を破棄しました。次の収集開始を待ちます"
                    .to_string(),
        });
    }
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
    diagnostics.extend(
        text_diagnostics
            .into_iter()
            .map(|message| ParserDiagnosticDto {
                code: "ARIB_SI_TEXT_REPLACED".to_string(),
                message,
                severity: Some("warning".to_string()),
            }),
    );
    diagnostics
}

fn is_fixed_pid_si_table_for_discovery(pid: u16, table_id: u8) -> bool {
    matches!(
        (pid, table_id),
        (0x0000, 0x00)
            | (0x0001, 0x01)
            | (0x0010, 0x40 | 0x41)
            | (0x0011, 0x42 | 0x46 | 0x4a)
            | (0x0012, 0x4e..=0x6f)
            | (0x0014, 0x70 | 0x73)
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

fn with_state_mut(
    handle: jlong,
    default_value: jint,
    f: impl FnOnce(&mut ParserState) -> jint,
) -> jint {
    if !si_module_is_healthy() {
        return STATUS_INTERNAL_ERROR;
    }
    let parser = match registry().lock() {
        Ok(guard) => guard.get(handle),
        Err(_) => {
            record_si_mutex_poison(SI_REGISTRY_LOCK_NAME);
            return STATUS_INTERNAL_ERROR;
        }
    };
    let Some(parser) = parser else {
        return default_value;
    };
    let result = match parser.lock() {
        Ok(mut guard) => f(&mut guard),
        Err(_) => {
            record_si_mutex_poison(SI_PARSER_LOCK_NAME);
            STATUS_INTERNAL_ERROR
        }
    };
    result
}

#[derive(Clone, Copy, Debug)]
enum SiJniFailureReason {
    ModuleAbnormal,
    RegistryPoisoned,
    ParserPoisoned,
    InvalidHandle,
    JniInput,
    JniOutput,
}

impl SiJniFailureReason {
    fn code(self) -> &'static str {
        match self {
            Self::ModuleAbnormal => "MODULE_ABNORMAL",
            Self::RegistryPoisoned => "REGISTRY_POISONED",
            Self::ParserPoisoned => "PARSER_POISONED",
            Self::InvalidHandle => "INVALID_HANDLE",
            Self::JniInput => "JNI_INPUT",
            Self::JniOutput => "JNI_OUTPUT",
        }
    }

    fn failure(self, detail: impl ToString) -> SiJniFailure {
        SiJniFailure {
            reason: self,
            detail: detail.to_string(),
        }
    }
}

#[derive(Debug)]
struct SiJniFailure {
    reason: SiJniFailureReason,
    detail: String,
}

fn throw_si_failure(env: &mut JNIEnv<'_>, failure: SiJniFailure) -> jstring {
    let thrown = (|| -> jni::errors::Result<()> {
        if env.exception_check()? {
            return Ok(());
        }
        let reason = JObject::from(env.new_string(failure.reason.code())?);
        let detail = JObject::from(env.new_string(&failure.detail)?);
        let exception = env.new_object(
            "com/maleicacid/tvinput/aribsi/NativeSiException",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&reason), JValue::Object(&detail)],
        )?;
        env.throw(JThrowable::from(exception))
    })();
    if let Err(error) = thrown {
        // VMの保留例外は消去しない。例外なしのnullもKotlinの受取入口が拒否する。
        eprintln!("SI JNI failure={failure:?}; exception delivery failure={error}");
    }
    ptr::null_mut()
}

fn java_string(env: &mut JNIEnv<'_>, value: Result<String, SiJniFailure>) -> jstring {
    let value = match value {
        Ok(value) => value,
        Err(failure) => return throw_si_failure(env, failure),
    };
    match env.new_string(value) {
        Ok(s) => s.into_raw(),
        Err(error) => throw_si_failure(env, SiJniFailureReason::JniOutput.failure(error)),
    }
}

fn discovery_stage_to_jint(stage: DiscoveryPublishStage) -> jint {
    match stage {
        DiscoveryPublishStage::Incomplete => DISCOVERY_STAGE_INCOMPLETE,
        DiscoveryPublishStage::Partial => DISCOVERY_STAGE_PARTIAL,
        DiscoveryPublishStage::Complete => DISCOVERY_STAGE_COMPLETE,
    }
}

fn snapshot_service_registration_typed(
    handle: jlong,
) -> Result<ServiceRegistrationSnapshotDto, SiJniFailure> {
    if !si_module_is_healthy() {
        return Err(SiJniFailureReason::ModuleAbnormal.failure("SI moduleが異常状態です"));
    }
    let parser = match registry().lock() {
        Ok(guard) => guard.get(handle),
        Err(_) => {
            record_si_mutex_poison(SI_REGISTRY_LOCK_NAME);
            return Err(SiJniFailureReason::RegistryPoisoned.failure(SI_REGISTRY_LOCK_NAME));
        }
    };
    let Some(parser) = parser else {
        return Err(SiJniFailureReason::InvalidHandle.failure(handle));
    };
    match parser.lock() {
        Ok(mut guard) => Ok(build_service_registration_snapshot(&mut guard)),
        Err(_) => {
            record_si_mutex_poison(SI_PARSER_LOCK_NAME);
            Err(SiJniFailureReason::ParserPoisoned.failure(SI_PARSER_LOCK_NAME))
        }
    }
}

fn snapshot_bulk_typed(handle: jlong) -> Result<BulkSnapshotDto, SiJniFailure> {
    if !si_module_is_healthy() {
        return Err(SiJniFailureReason::ModuleAbnormal.failure("SI moduleが異常状態です"));
    }
    let parser = match registry().lock() {
        Ok(guard) => guard.get(handle),
        Err(_) => {
            record_si_mutex_poison(SI_REGISTRY_LOCK_NAME);
            return Err(SiJniFailureReason::RegistryPoisoned.failure(SI_REGISTRY_LOCK_NAME));
        }
    };
    let Some(parser) = parser else {
        return Err(SiJniFailureReason::InvalidHandle.failure(handle));
    };
    let result = match parser.lock() {
        Ok(mut guard) => Ok(build_bulk_snapshot(&mut guard)),
        Err(_) => {
            record_si_mutex_poison(SI_PARSER_LOCK_NAME);
            Err(SiJniFailureReason::ParserPoisoned.failure(SI_PARSER_LOCK_NAME))
        }
    };
    result
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeSnapshotServiceRegistrationTyped(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jobject {
    let snapshot = match snapshot_service_registration_typed(handle) {
        Ok(snapshot) => snapshot,
        Err(failure) => return throw_si_failure(&mut env, failure) as jobject,
    };
    match jvm_snapshot_generated::service_registration_snapshot_to_java(&mut env, snapshot) {
        Ok(value) => value.into_raw(),
        Err(failure) => throw_si_failure(&mut env, failure) as jobject,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeSnapshotBulkTyped(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jobject {
    let snapshot = match snapshot_bulk_typed(handle) {
        Ok(snapshot) => snapshot,
        Err(failure) => return throw_si_failure(&mut env, failure) as jobject,
    };
    match jvm_snapshot_generated::snapshot_to_java(&mut env, snapshot) {
        Ok(value) => value.into_raw(),
        Err(failure) => throw_si_failure(&mut env, failure) as jobject,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeSnapshotPmtPidsForSectionFilters(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jintArray {
    let values = match snapshot_pmt_pids_for_section_filters(handle) {
        Ok(values) => values,
        Err(failure) => return throw_si_failure(&mut env, failure) as jintArray,
    };
    let length = match i32::try_from(values.len()) {
        Ok(length) => length,
        Err(error) => {
            return throw_si_failure(&mut env, SiJniFailureReason::JniOutput.failure(error))
                as jintArray
        }
    };
    let array = match env.new_int_array(length) {
        Ok(array) => array,
        Err(error) => {
            return throw_si_failure(&mut env, SiJniFailureReason::JniOutput.failure(error))
                as jintArray
        }
    };
    if let Err(error) = env.set_int_array_region(&array, 0, &values) {
        return throw_si_failure(&mut env, SiJniFailureReason::JniOutput.failure(error))
            as jintArray;
    }
    array.into_raw()
}

fn snapshot_pmt_pids_for_section_filters(handle: jlong) -> Result<Vec<jint>, SiJniFailure> {
    if !si_module_is_healthy() {
        return Err(SiJniFailureReason::ModuleAbnormal.failure("SI moduleが異常状態です"));
    }
    let parser = match registry().lock() {
        Ok(guard) => guard.get(handle),
        Err(_) => {
            record_si_mutex_poison(SI_REGISTRY_LOCK_NAME);
            return Err(SiJniFailureReason::RegistryPoisoned.failure(SI_REGISTRY_LOCK_NAME));
        }
    };
    let Some(parser) = parser else {
        return Err(SiJniFailureReason::InvalidHandle.failure(handle));
    };
    let result = match parser.lock() {
        Ok(guard) => Ok(guard
            .collector
            .pmt_pids_for_section_filters()
            .into_iter()
            .map(i32::from)
            .collect()),
        Err(_) => {
            record_si_mutex_poison(SI_PARSER_LOCK_NAME);
            Err(SiJniFailureReason::ParserPoisoned.failure(SI_PARSER_LOCK_NAME))
        }
    };
    result
}

fn jbytearray_to_vec(env: &JNIEnv<'_>, value: JByteArray<'_>) -> Result<Vec<u8>, SiJniFailure> {
    env.convert_byte_array(value)
        .map_err(|error| SiJniFailureReason::JniInput.failure(error))
}

enum CodecInputFailure {
    TooLarge,
    Jni(jni::errors::Error),
}

fn bounded_codec_bytes(
    env: &mut JNIEnv<'_>,
    value: &JByteArray<'_>,
    maximum: i32,
) -> Result<Vec<u8>, CodecInputFailure> {
    let length = env
        .get_array_length(value)
        .map_err(CodecInputFailure::Jni)?;
    if length > maximum {
        return Err(CodecInputFailure::TooLarge);
    }
    env.convert_byte_array(value)
        .map_err(CodecInputFailure::Jni)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeProbeAacConfiguration(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    adts: JByteArray<'_>,
    asc: JByteArray<'_>,
) -> jobject {
    use maleicacid_arib_si_engine_core::codec_signaling::{
        probe_adts_configuration, AacConfigurationProbe,
    };
    let input = bounded_codec_bytes(&mut env, &adts, 64 * 1024);
    let config = if asc.is_null() {
        Ok(None)
    } else {
        bounded_codec_bytes(&mut env, &asc, 255).map(Some)
    };
    let result = match (input, config) {
        (Ok(input), Ok(config)) => probe_adts_configuration(&input, config.as_deref()),
        (Err(CodecInputFailure::Jni(error)), _) | (_, Err(CodecInputFailure::Jni(error))) => {
            return throw_si_failure(&mut env, SiJniFailureReason::JniInput.failure(error))
                as jobject;
        }
        (Err(CodecInputFailure::TooLarge), _) | (_, Err(CodecInputFailure::TooLarge)) => {
            AacConfigurationProbe::Invalid {
                reason: "codec構成probeの入力上限を超過しました",
            }
        }
    };
    let transport = AacConfigurationProbeDto::from(result);
    match jvm_snapshot_generated::aac_probe_to_java(&mut env, &transport) {
        Ok(value) => value.into_raw(),
        Err(failure) => throw_si_failure(&mut env, failure) as jobject,
    }
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

fn provider_jni_failure(error: jni::errors::Error) -> provider_data_api::ProviderDataResult {
    provider_data_api::ProviderDataResult {
        success: false,
        json: String::new(),
        schema_version: 1,
        truncated: false,
        diagnostics_dropped_count: 0,
        error_code: "JNI_ERROR".to_string(),
        error_message: error.to_string(),
    }
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
    let result = match env.get_string(&request_json) {
        Ok(request) => provider_data_api::build_channel_provider_data(&String::from(request)),
        Err(error) => provider_jni_failure(error),
    };
    java_string(&mut env, Ok(provider_result_json(result)))
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
        Ok(provider_data_api::build_program_key(
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
    let result = match env.get_string(&request_json) {
        Ok(request) => provider_data_api::build_program_provider_data(&String::from(request)),
        Err(error) => provider_jni_failure(error),
    };
    java_string(&mut env, Ok(provider_result_json(result)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeNormalizeProgramProviderData(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JByteArray<'_>,
) -> jstring {
    let result = match env.convert_byte_array(provider_data) {
        Ok(data) => provider_data_api::normalize_program_provider_data(&data),
        Err(error) => provider_jni_failure(error),
    };
    java_string(&mut env, Ok(provider_result_json(result)))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeExtractProgramKeyResult(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JByteArray<'_>,
) -> jstring {
    let json = jbytearray_to_vec(&env, provider_data).map(|data| {
        // キー不在は正常な欠落表現を使い、JNI失敗はResultとして伝達する。
        provider_data_api::extract_program_key_result(&data)
            .map(program_key_result_json)
            .unwrap_or_default()
    });
    java_string(&mut env, json)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeDecodeChannelProviderData(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    provider_data: JByteArray<'_>,
) -> jstring {
    let json = jbytearray_to_vec(&env, provider_data)
        .map(|data| provider_data_api::decode_channel_provider_data(&data));
    java_string(&mut env, json)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribSiParser_nativeCreate(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jlong {
    if !si_module_is_healthy() {
        return 0;
    }
    match registry().lock() {
        Ok(mut guard) => guard.create(),
        Err(_) => {
            record_si_mutex_poison(SI_REGISTRY_LOCK_NAME);
            0
        }
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
        Err(_) => {
            record_si_mutex_poison(SI_REGISTRY_LOCK_NAME);
            STATUS_INTERNAL_ERROR
        }
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
    let decoded =
        jbytearray_to_vec(&env, bytes).map(|bytes| arib_string::decode_arib_string_lossy(&bytes).0);
    java_string(&mut env, decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::crc32_mpeg;

    #[test]
    fn invalid_section_reason_uses_existing_snapshot_diagnostics() {
        let cases: &[(u16, &[u8], &str)] = &[
            (0, &[0], "SECTION_HEADER_INVALID"),
            (0x14, &[0x70, 0x70, 0, 0], "SECTION_LENGTH_MISMATCH"),
            (
                0x14,
                &[0x70, 0x70, 5, 0xff, 0xff, 0xff, 0xff, 0xff],
                "BROADCAST_CLOCK_INVALID",
            ),
            (
                0,
                &[0, 0xb0, 9, 0, 1, 0xc1, 0, 0, 0, 0, 0, 0],
                "SECTION_CRC_MISMATCH",
            ),
        ];
        for &(pid, bytes, code) in cases {
            let mut state = ParserState::default();
            assert_eq!(state.ingest_section(pid, bytes), STATUS_INVALID_SECTION);
            let snapshot = build_bulk_snapshot(&mut state);
            assert!(snapshot
                .parser_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code));
            assert_eq!(
                state.ingest_section(0x10, &[0x7f, 0x30, 0]),
                STATUS_IGNORED_UNSUPPORTED_PID_OR_TABLE
            );
            let snapshot = build_bulk_snapshot(&mut state);
            assert!(!snapshot
                .parser_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code));
        }
    }

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
    fn multiple_series_keeps_every_fact_without_selecting_a_primary() {
        let mut event = minimal_event_for_related_items(1, 0x100);
        let bytes = [
            0xd5, 9, 0, 1, 0, 0, 0, 0, 1, 0, 2, 0xd5, 9, 0, 2, 0, 0, 0, 0, 3, 0, 4,
        ];
        event.descriptors = crate::descriptors::parse_event_descriptors(&bytes);
        let dto = runtime_snapshot_build::event(&event, None);
        assert!(dto.descriptors.series.is_none());
        assert_eq!(dto.descriptors.series_candidates.len(), 2);
        assert_eq!(dto.descriptors.series_candidates[0].series_id, Some(1));
        assert_eq!(dto.descriptors.series_candidates[1].series_id, Some(2));
        let canonical: Vec<maleicacid_arib_si_engine_core::runtime_snapshot_dto::SeriesDto> =
            serde_json::from_str(
                dto.descriptors
                    .series_candidates_canonical_json
                    .as_deref()
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(canonical, dto.descriptors.series_candidates);
        event.descriptors.series.pop();
        let dto = runtime_snapshot_build::event(&event, None);
        assert_eq!(
            dto.descriptors
                .series
                .as_ref()
                .and_then(|series| series.series_id),
            Some(1)
        );
        assert!(dto.descriptors.series_candidates_canonical_json.is_none());
    }

    #[test]
    fn event_group_json_preserves_raw_group_type_without_derived_kind() {
        for group_type in 1u8..=5 {
            let event = minimal_event_for_related_items(group_type, 0x0100 + group_type as u16);
            let dto = runtime_snapshot_build::event(&event, None);
            let group = &dto.descriptors.event_groups[0];
            assert_eq!(group.group_type, i32::from(group_type));
            assert!(!group.events.is_empty());
        }
    }

    #[test]
    fn event_component_json_keeps_arib_descriptor_facts_without_release_policy() {
        let mut event = minimal_event_for_related_items(1, 0x0101);
        event.descriptors.components = vec![crate::descriptors::ComponentDescriptor {
            stream_content: 0x01,
            component_type: 0xb3,
            component_tag: 0x10,
            language_code: "jpn".to_string(),
            text: String::new(),
        }];
        event.descriptors.audio_components = vec![crate::descriptors::AudioComponentDescriptor {
            stream_content: 0x02,
            component_type: 0x02,
            component_tag: 0x20,
            stream_type: 0x0f,
            simulcast_group_tag: 0xff,
            es_multi_lingual_flag: true,
            main_component_flag: true,
            quality_indicator: 2,
            sampling_rate: 7,
            language_code: "jpn".to_string(),
            language_code_2: Some("eng".to_string()),
            text: String::new(),
        }];

        let dto = runtime_snapshot_build::event(&event, None);
        let video = &dto.descriptors.components.video[0];
        assert_eq!(video.resolution.as_deref(), Some("1080"));
        assert_eq!(video.scan.as_deref(), Some("interlaced"));
        assert_eq!(video.aspect.as_deref(), Some("16:9"));
        assert_eq!(
            video.source_descriptor.as_deref(),
            Some("component_descriptor")
        );
        let audio = &dto.descriptors.components.audio[0];
        assert_eq!(audio.channel_configuration.as_deref(), Some("1/0+1/0"));
        assert_eq!(audio.channel_count, Some(2));
        assert_eq!(audio.sampling_info.as_deref(), Some("48kHz"));
        assert_eq!(audio.sample_rate_hz, Some(48_000));
        assert_eq!(audio.audio_description, Some(false));
        assert_eq!(audio.hard_of_hearing, Some(false));
        assert_eq!(audio.dual_mono, Some(true));
        assert_eq!(
            audio.source_descriptor.as_deref(),
            Some("audio_component_descriptor"),
        );
    }

    #[test]
    fn timing_state_controls_diagnostic_and_bulk_stable_identity() {
        use crate::eit::{parse_eit_section, EitTimingState};
        for state in [
            EitTimingState::Defined,
            EitTimingState::UndefinedTime,
            EitTimingState::BothTimingUndefined,
            EitTimingState::MalformedTiming,
        ] {
            let mut body = vec![
                0x4e, 0xf0, 34, 0, 1, 0xc1, 0, 0, 0, 0x11, 0, 0x22, 0, 0x4e, 0x12, 0x34, 0xee, 0,
                0x12, 0, 0, 0, 0x30, 0, 0x80, 7, 0x55, 5, 0x4a, 0x50, 0x4e, 12, 0xaa,
            ];
            match state {
                EitTimingState::UndefinedTime => body[16..21].fill(0xff),
                EitTimingState::BothTimingUndefined => body[16..24].fill(0xff),
                EitTimingState::MalformedTiming => body[18] = 0xfa,
                EitTimingState::Defined => {}
            }
            let events = parse_eit_section(&section_with_crc(body));
            assert_eq!(events.len(), 1);
            let event = &events[0];
            assert_eq!(event.timing_state, state);
            let expected = matches!(
                state,
                EitTimingState::Defined | EitTimingState::UndefinedTime
            );
            assert!(!event.diagnostics.is_empty());
            assert!(event
                .diagnostics
                .iter()
                .all(|d| d.event_identity.is_some() == expected));
            let stable_identity = expected.then(|| {
                provider_data_api::build_program_key(
                    i32::from(event.original_network_id),
                    i32::from(event.transport_stream_id),
                    i32::from(event.service_id),
                    i32::from(event.event_id),
                )
            });
            let value = runtime_snapshot_build::event(event, stable_identity);
            assert_eq!(value.stable_identity.is_some(), expected);
            assert_eq!(value.event_id, 0x1234);
            assert_eq!(value.service_key.service_id, 1);
            assert!(value
                .descriptors
                .diagnostics
                .descriptor_facts_canonical_json
                .is_some());
        }
    }

    #[test]
    fn service_registration_snapshot_keeps_scan_facts_without_program_event_projection() {
        let mut state = ParserState::default();
        state.collector.set_discovery_profile(DiscoveryProfile::Bs);
        let section = section_with_crc(vec![
            0x4e, 0xf0, 0x0f, 0, 1, 0xff, 0, 0, 0, 0x11, 0, 0x22, 0, 0x4e,
        ]);
        assert_eq!(state.ingest_section(0x0012, &section), STATUS_OK);

        let snapshot = build_service_registration_snapshot(&mut state);

        assert_eq!(snapshot.eit_instances.len(), 1);
        assert!(snapshot
            .parser_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "PARSER_STATE"));
    }

    #[test]
    fn collection_section_limit_clears_facts_and_refuses_following_input() {
        let mut state = ParserState::default();
        let tot = section_with_crc(vec![
            0x73, 0x70, 0x0b, 0xea, 0x60, 0x12, 0x34, 0x56, 0xf0, 0x00,
        ]);
        for _ in 0..MAX_COLLECTION_SECTIONS {
            assert_eq!(state.ingest_section(0x0014, &tot), STATUS_OK);
        }
        assert!(state.latest_broadcast_clock.is_some());
        assert_eq!(
            state.ingest_section(0x0014, &tot),
            STATUS_COLLECTION_LIMIT_EXCEEDED
        );
        assert_eq!(
            state.ingest_section(0x0014, &tot),
            STATUS_COLLECTION_LIMIT_EXCEEDED
        );
        let snapshot = build_bulk_snapshot(&mut state);
        assert!(snapshot.broadcast_clock.is_none());
        assert_eq!(snapshot.discovery_stage, DISCOVERY_STAGE_INCOMPLETE);
        assert!(snapshot
            .parser_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "COLLECTION_LIMIT_EXCEEDED"));
    }

    #[test]
    fn collection_byte_limit_also_counts_rejected_input() {
        let mut state = ParserState::default();
        let bytes = vec![0; 4096];
        for _ in 0..MAX_COLLECTION_BYTES / bytes.len() {
            assert_eq!(state.ingest_section(0x0012, &bytes), STATUS_INVALID_SECTION);
        }
        assert_eq!(
            state.ingest_section(0x0012, &bytes),
            STATUS_COLLECTION_LIMIT_EXCEEDED
        );
        assert!(state.collection_limit_exceeded);
        assert_eq!(state.collection_bytes, MAX_COLLECTION_BYTES);
    }

    #[test]
    fn collection_expiry_removes_stale_epg_and_resynchronizes_versions_without_losing_profile() {
        let mut state = ParserState::default();
        state.collector.set_discovery_profile(DiscoveryProfile::Bs);
        let section = section_with_crc(vec![
            0x4e, 0xf0, 0x0f, 0, 1, 0xff, 0, 0, 0, 0x11, 0, 0x22, 0, 0x4e,
        ]);
        assert_eq!(state.ingest_section(0x0012, &section), STATUS_OK);
        assert_eq!(state.eit_instances.states()[0].version, 31);
        state.expire_collection_at(state.collection_started_at + MAX_COLLECTION_AGE);
        assert!(state.eit_instances.states().is_empty());
        assert_eq!(state.collection_generation, 1);
        let next = section_with_crc(vec![
            0x4e, 0xf0, 0x0f, 0, 1, 0xe1, 0, 7, 0, 0x11, 0, 0x22, 7, 0x4e,
        ]);
        assert_eq!(state.ingest_section(0x0012, &next), STATUS_OK);
        let states = state.eit_instances.states();
        assert_eq!(states[0].version, 16);
        assert_eq!(states[0].last_section_number, 7);
    }

    #[test]
    fn ingest_tot_updates_typed_broadcast_clock_snapshot() {
        let mut state = ParserState::default();
        let tot = section_with_crc(vec![
            0x73, 0x70, 0x0b, 0xea, 0x60, 0x12, 0x34, 0x56, 0xf0, 0x00,
        ]);
        assert_eq!(state.ingest_section(0x0014, &tot), STATUS_OK);
        assert_eq!(
            state.latest_broadcast_clock,
            Some(BroadcastClockFact {
                table_id: 0x73,
                mjd: 0xea60,
                millis_of_day: (12 * 3_600 + 34 * 60 + 56) * 1_000,
            })
        );
        let snapshot = build_bulk_snapshot(&mut state);
        let clock = snapshot.broadcast_clock.expect("broadcast clock");
        assert_eq!(clock.table_id, 0x73);
        assert_eq!(clock.mjd, 0xea60);
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
        let snapshot = build_bulk_snapshot(&mut state);
        let text_diagnostic = snapshot
            .parser_diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "ARIB_SI_TEXT_REPLACED")
            .expect("ARIB SI text diagnostic");
        let message = text_diagnostic.message.as_str();
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
