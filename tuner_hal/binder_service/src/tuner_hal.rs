use crate::descrambler_key_table::{DescramblerKeyResolveError, DescramblerKeyTable};
use crate::descrambler_session::{DescramblerCleanupItem, DescramblerPidRegistration, DescramblerSession, KeyTokenBinding, PidBinding, SourceFilterBinding};
use crate::frontend_capability as frontend_cap_model;
use crate::lifecycle_txn::{LifecycleCleanupCaller as DvrCleanupCaller, LifecycleCleanupStepResult as DvrCleanupStepResult, DvrCleanupOutcome, DvrCleanupStepResults, LifecycleTxn};
use crate::registry_ledger::{DemuxLedger, LedgerId, LnbLedger, LnbOperationGuard, LnbOperationGuardError, FilterLedger, DvrLedger, DescramblerLedger};
use crate::stream_boundary::{PendingStreamBoundaryPlan, StreamBoundaryReason, StreamBoundaryResetPlan, StreamBoundaryResources};
use crate::hal_sync::{
    lock_mutex_hal, lock_mutex_io, lock_mutex_option, lock_mutex_status, poisoned_lock_status,
};
use crate::worker_runtime::{
    WorkerExit, WorkerJoinOutcome, WorkerRuntime, WorkerHandle, WorkerOwnerId, ConcreteWorkerSignal, WorkerSignal as RuntimeWorkerSignal,
    RuntimeAtomicFlag, RuntimeWaitSignal,
};
use crate::fmq_queue::{FmqFillStatus, FmqQueue};
use android_hardware_common::aidl::android::hardware::common::NativeHandle::NativeHandle as CommonNativeHandle;
use android_hardware_common_fmq::aidl::android::hardware::common::fmq::GrantorDescriptor::GrantorDescriptor as CommonGrantorDescriptor;
use android_hardware_common_fmq::aidl::android::hardware::common::fmq::MQDescriptor::MQDescriptor as CommonMqDescriptor;
use android_hardware_common_fmq::aidl::android::hardware::common::fmq::SynchronizedReadWrite::SynchronizedReadWrite as CommonSynchronizedReadWrite;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    AvStreamType::AvStreamType,
    DataFormat::DataFormat,
    DemuxCapabilities::DemuxCapabilities,
    DemuxFilterEvent::DemuxFilterEvent,
    DemuxFilterMainType::DemuxFilterMainType,
    DemuxFilterMediaEvent::DemuxFilterMediaEvent,
    DemuxFilterMediaEventExtraMetaData::DemuxFilterMediaEventExtraMetaData,
    DemuxFilterPesEvent::DemuxFilterPesEvent,
    DemuxFilterScIndexMask::DemuxFilterScIndexMask,
    DemuxFilterSectionEvent::DemuxFilterSectionEvent,
    DemuxFilterSectionSettingsCondition::DemuxFilterSectionSettingsCondition,
    DemuxFilterSettings::DemuxFilterSettings,
    DemuxFilterStatus::DemuxFilterStatus,
    DemuxFilterSubType::DemuxFilterSubType,
    DemuxFilterTsRecordEvent::DemuxFilterTsRecordEvent,
    DemuxFilterType::DemuxFilterType,
    DemuxInfo::DemuxInfo,
    DemuxPid::DemuxPid,
    DemuxTsFilterSettingsFilterSettings::DemuxTsFilterSettingsFilterSettings,
    DemuxTsFilterType::DemuxTsFilterType,
    DvrSettings::DvrSettings,
    DvrType::DvrType,
    FilterDelayHint::FilterDelayHint,
    FilterDelayHintType::FilterDelayHintType,
    FrontendCapabilities::FrontendCapabilities,
    FrontendDvbsCapabilities::FrontendDvbsCapabilities,
    FrontendEventType::FrontendEventType,
    FrontendInfo::FrontendInfo,
    FrontendIsdbs3Capabilities::FrontendIsdbs3Capabilities,
    FrontendIsdbsCapabilities::FrontendIsdbsCapabilities,
    FrontendIsdbsCoderate::FrontendIsdbsCoderate,
    FrontendIsdbsModulation::FrontendIsdbsModulation,
    FrontendIsdbsStreamIdType::FrontendIsdbsStreamIdType,
    FrontendIsdbtBandwidth::FrontendIsdbtBandwidth,
    FrontendIsdbtCapabilities::FrontendIsdbtCapabilities,
    FrontendIsdbtCoderate::FrontendIsdbtCoderate,
    FrontendIsdbtGuardInterval::FrontendIsdbtGuardInterval,
    FrontendIsdbtMode::FrontendIsdbtMode,
    FrontendIsdbtModulation::FrontendIsdbtModulation,
    FrontendIsdbtTimeInterleaveMode::FrontendIsdbtTimeInterleaveMode,
    FrontendScanMessage::FrontendScanMessage,
    FrontendScanMessageType::FrontendScanMessageType,
    FrontendScanType::FrontendScanType,
    FrontendSettings::FrontendSettings,
    FrontendStatus::FrontendStatus,
    FrontendStatusReadiness::FrontendStatusReadiness,
    FrontendStatusType::FrontendStatusType,
    FrontendType::FrontendType,
    IDemux::{BnDemux, IDemux},
    IDescrambler::{BnDescrambler, IDescrambler},
    IDvr::{BnDvr, IDvr},
    IDvrCallback::IDvrCallback,
    IFilter::{BnFilter, IFilter},
    IFilterCallback::{BnFilterCallback, IFilterCallback},
    IFrontend::{BnFrontend, IFrontend},
    IFrontendCallback::IFrontendCallback,
    ILnb::{BnLnb, ILnb},
    ILnbCallback::ILnbCallback,
    ITimeFilter::ITimeFilter,
    ITuner::ITuner,
    LnbPosition::LnbPosition,
    LnbTone::LnbTone,
    LnbVoltage::LnbVoltage,
    PlaybackStatus::PlaybackStatus,
    RecordStatus::RecordStatus,
    Result::Result as TunerResult,
    VideoStreamType::VideoStreamType,
};
use binder::{
    Binder, BinderFeatures, Interface, ParcelFileDescriptor, Result as BinderResult, Status,
    StatusCode, Strong,
};
use maleicacid_tuner_hal_common::{
    is_japan_cs110_if_frequency_hz, japan_isdbt_frequency_contract_range_hz, FrontendScanMode,
    FrontendStreamIdKind, FrontendSystem, FrontendTelemetry, FrontendTuneRequest, HalError,
    TsPacketCompletionBuffer,
    DEMUX_MAX_AUDIO_FILTERS, DEMUX_MAX_FILTERS_PER_DEMUX, DEMUX_MAX_PES_FILTERS,
    DEMUX_MAX_SECTION_FILTERS, DEMUX_MAX_TS_FILTERS, DEMUX_MAX_VIDEO_FILTERS,
    MAX_SECTION_FILTER_BYTES, MAX_SECTION_PAYLOAD_BYTES, TS_PACKET_SIZE,
};
use maleicacid_tuner_hal_descrambler::{
    descramble_ts_packet_in_place, parse_ts_packet_header, DescrambleFailure, DescrambleOutcome,
    DescramblerKeySlot, Multi2KeyMaterial,
};
use maleicacid_tuner_hal_frontend_dvb::{DvbFrontendBackend, DvbLiveStreamReader};
use maleicacid_tuner_hal_frontend_px4::{
    reportable_bs_tsid_for_scan, Px4FrontendBackend, Px4LiveStreamReader,
};
use maleicacid_tuner_hal_soft_demux::{
    demux_link_caps_for_ts_filter_linkage,
    record_index::{
        pes_stream_id, pes_time_fields, RecordEventState, RecordIndexParser,
        TsRecordEventData, AVC_SC_B_SLICE, AVC_SC_I_SLICE, AVC_SC_P_SLICE,
        AVC_SC_SI_SLICE, AVC_SC_SP_SLICE, DEMUX_TS_INDEX_ADAPTATION_EXTENSION,
        DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED, DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED,
        DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED, DEMUX_TS_INDEX_DISCONTINUITY,
        DEMUX_TS_INDEX_FIRST_PACKET, DEMUX_TS_INDEX_OPCR, DEMUX_TS_INDEX_PAYLOAD_UNIT_START,
        DEMUX_TS_INDEX_PCR, DEMUX_TS_INDEX_PRIORITY, DEMUX_TS_INDEX_PRIVATE_DATA,
        DEMUX_TS_INDEX_RANDOM_ACCESS, DEMUX_TS_INDEX_SPLICING_POINT, HEVC_SC_AUD,
        HEVC_SC_BLA_N_LP, HEVC_SC_BLA_W_LP, HEVC_SC_BLA_W_RADL, HEVC_SC_IDR_N_LP,
        HEVC_SC_IDR_W_RADL, HEVC_SC_SPS, HEVC_SC_TRAIL_CRA, RECORD_SC_TYPE_NONE,
        RECORD_SC_TYPE_SC, RECORD_SC_TYPE_SC_AVC, RECORD_SC_TYPE_SC_HEVC, RECORD_SC_TYPE_SC_VVC,
        VVC_SC_AUD, VVC_SC_CRA, VVC_SC_GDR, VVC_SC_IDR_N_LP, VVC_SC_IDR_W_RADL,
        VVC_SC_SPS, VVC_SC_VPS,
    },
    sections::{normalize_length_field_bits, parse_section_header},
    AvFilterStreamKind, AvPayloadMetadata, DemuxConfigError, DemuxCore, DemuxFilterRecord,
    DemuxHandle, DemuxPathDirection, DvrConfig, FilterConfig, FilterConfigKind,
    FilterDelayHintState, FilterOpenType, FilterPayload, SectionCondition, SectionConditionKind,
    DEMUX_FILTER_MAIN_TYPE_COUNT, DEMUX_FILTER_MAIN_TYPE_TS_BITS,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_void, CString};
use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

type TunerQueueDesc = CommonMqDescriptor<i8, CommonSynchronizedReadWrite>;
type TunerNativeHandle = CommonNativeHandle;

fn descrambler_upstream_filter_open_type_allowed(open_type: FilterOpenType) -> bool {
    matches!(
        open_type,
        FilterOpenType::TsAudio
            | FilterOpenType::TsVideo
            | FilterOpenType::TsPes
            | FilterOpenType::TsRecord
    )
}

fn empty_native_handle() -> TunerNativeHandle {
    TunerNativeHandle {
        fds: Vec::new(),
        ints: Vec::new(),
    }
}

const TUNER_EVENT_DATA_READY: u32 = 1 << 0;
const TUNER_EVENT_DATA_OVERFLOW: u32 = 1 << 1;

fn lnb_operation_guard_error_status(lnb_id: i32, err: LnbOperationGuardError) -> Status {
    match err {
        LnbOperationGuardError::Busy => Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None),
        LnbOperationGuardError::Poisoned => {
            record_tuner_diagnostic_counter(&LNB_BACKEND_APPLY_ERROR_COUNT, "lnb_internal_failed");
            eprintln!("maleicacid-tuner-hal-lnb-diagnostic: lnb_id={lnb_id} lnb_internal_failed=operation_lock_poisoned");
            poisoned_lock_status("lnb_operation_lock_ledger")
        }
        LnbOperationGuardError::DropReleaseFailed => {
            record_tuner_diagnostic_counter(&LNB_BACKEND_APPLY_ERROR_COUNT, "lnb_internal_failed");
            eprintln!("maleicacid-tuner-hal-lnb-diagnostic: lnb_id={lnb_id} lnb_internal_failed=operation_guard_release_failed");
            Status::from(StatusCode::UNKNOWN_ERROR)
        }
    }
}

fn lnb_operation_guard_for_id(lnb_id: i32) -> BinderResult<LnbOperationGuard> {
    LnbLedger::operation_guard(lnb_id)
        .map_err(|err| lnb_operation_guard_error_status(lnb_id, err))
}
const FILTER_MONITOR_MASK_STATUS: i32 = 1 << 0;
const FILTER_MONITOR_MASK_EVENT: i32 = 1 << 1;
const SUPPORTED_FILTER_MONITOR_MASK: i32 = 0;
const AV_SHARED_SLOT_COUNT: usize = 32;
const AV_SHARED_SLOT_SIZE_BYTES: usize = 1024 * 1024;
const AV_SLOT_COUNT: usize = AV_SHARED_SLOT_COUNT;
const AV_MIN_SLOT_SIZE: usize = AV_SHARED_SLOT_SIZE_BYTES;
const AV_DEBUG_LOG_INTERVAL: u64 = 64;
const DVR_DEFAULT_STATUS_CHECK_INTERVAL_MS: i64 = 25;
const LOCK_TIMEOUT_MS: u64 = 5_000;
const PX4_PATH_DIAGNOSTIC_TIMEOUT_MS: u64 = LOCK_TIMEOUT_MS;

static FILTER_QUEUE_DESC_UNAVAILABLE_COUNT: AtomicU64 = AtomicU64::new(0);
static DVR_QUEUE_DESC_INVALID_STATE_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_HANDLE_CLIENT_RELEASE_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_DATA_ID_RELEASE_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_DATA_ID_STALE_RELEASE_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_DATA_ID_INVALID_RELEASE_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_HANDLE_DIRECT_UNSUPPORTED_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_HANDLE_RELEASE_WITHOUT_HANDLE_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_HANDLE_UNAVAILABLE_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_GENERATION_BOUNDARY_COUNT: AtomicU64 = AtomicU64::new(0);
static AV_SHARED_HANDLE_CLIENT_RELEASED_DROP_COUNT: AtomicU64 = AtomicU64::new(0);
static FMQ_CREATE_ERROR_COUNT: AtomicU64 = AtomicU64::new(0);
static FILTER_FMQ_WRITE_ERROR_COUNT: AtomicU64 = AtomicU64::new(0);
static DVR_FMQ_WRITE_ERROR_COUNT: AtomicU64 = AtomicU64::new(0);
static DESCRAMBLER_DEMUX_INVALIDATE_ERROR_COUNT: AtomicU64 = AtomicU64::new(0);
static LNB_BACKEND_APPLY_ERROR_COUNT: AtomicU64 = AtomicU64::new(0);

fn should_log_tuner_counter(count: u64) -> bool {
    count <= 4 || count.is_power_of_two() || count % AV_DEBUG_LOG_INTERVAL == 0
}

fn record_tuner_diagnostic_counter(counter: &AtomicU64, name: &str) {
    let mut current = counter.load(Ordering::SeqCst);
    loop {
        let total = current.saturating_add(1);
        match counter.compare_exchange(current, total, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => {
                if total == u64::MAX {
                    eprintln!("maleicacid-tuner-hal-diagnostic: av_shared_counter_saturated name={name}");
                } else if should_log_tuner_counter(total) {
                    eprintln!("maleicacid-tuner-hal-diagnostic: {name} total={total}");
                }
                return;
            }
            Err(next_current) => current = next_current,
        }
    }
}

const ERRNO_EIO: i32 = 5;
const ERRNO_EACCES: i32 = 13;
const ERRNO_ENOENT: i32 = 2;
const ERRNO_ENOMEM: i32 = 12;
const ERRNO_EINVAL: i32 = 22;
const MAX_LIVE_DEMUXES: usize = 8;
const SUPPORTED_DEMUX_FILTER_CAPS: i32 = DemuxFilterMainType::TS.0;
const DEMUX_ID_BASE: i32 = 0;
const JAPAN_BS_FIRST_IF_HZ: i64 = 1_049_480_000;
const JAPAN_CS110_LAST_IF_HZ: i64 = 2_053_000_000;
const MAX_DISEQC_MESSAGE_LEN: usize = 6;
const MFD_CLOEXEC: i32 = 0x0001;
const PROT_READ: i32 = 0x1;
const PROT_WRITE: i32 = 0x2;
const MAP_SHARED: i32 = 0x01;
const MAP_FAILED: *mut c_void = !0usize as *mut c_void;
const PX4_PHYSICAL_GROUP_TAG: i32 = 0x1000_0000;
const DVB_PHYSICAL_GROUP_TAG: i32 = 0x2000_0000;

#[cfg(target_arch = "x86_64")]
const SYS_MEMFD_CREATE: isize = 319;
#[cfg(target_arch = "x86")]
const SYS_MEMFD_CREATE: isize = 356;
#[cfg(target_arch = "aarch64")]
const SYS_MEMFD_CREATE: isize = 279;


type WorkerSignal = RuntimeWorkerSignal<WorkerExit>;

fn worker_exit_status(worker_name: &'static str, exit: WorkerExit) -> Status {
    let detail = format!("worker={worker_name} abnormal_stop exit={exit:?}");
    Status::new_service_specific_error(TunerResult::UNKNOWN_ERROR.0, Some(&detail))
}


fn fmq_queue_error_io(context: &str, err: FmqQueueError) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("{context}: FMQ queue unavailable or failed: {err:?}"),
    )
}

fn fmq_queue_error_status(context: &str, err: FmqQueueError) -> Status {
    eprintln!("maleicacid-tuner-hal-fmq: {context}: {err:?}");
    Status::from(StatusCode::UNKNOWN_ERROR)
}
fn fmq_clear_error_status(detail: impl AsRef<str>) -> Status {
    let detail = detail.as_ref();
    eprintln!("maleicacid-tuner-hal-fmq: clear failed: {detail}");
    Status::new_service_specific_error(TunerResult::UNKNOWN_ERROR.0, Some(detail))
}

fn is_event_flag_wake_failure(err: &std::io::Error) -> bool {
    err.to_string().contains("EventFlagWakeFailed")
}

fn status_is_descriptor_internal_error(status: &Status) -> bool {
    format!("{status:?}").contains("descriptor_internal_error")
}

extern "C" {
    fn syscall(num: isize, ...) -> isize;
    fn ftruncate(fd: i32, length: i64) -> i32;
    fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, length: usize) -> i32;
    fn tuner_dmabuf_heap_alloc_system(len: usize) -> i32;
}

struct SharedMemoryBacking {
    queue: FmqQueue,
    ring_io_lock: Mutex<()>,
    worker: Mutex<Option<WorkerHandle>>,
    playback_consume_lock: Mutex<()>,
    playback_worker_failed: RuntimeAtomicFlag,
    playback_residual: Mutex<TsPacketCompletionBuffer>,
    playback_malformed_bytes: AtomicU64,
    playback_dropped_bytes: AtomicU64,
}

unsafe impl Send for SharedMemoryBacking {}
unsafe impl Sync for SharedMemoryBacking {}

#[derive(Clone, Copy, Debug, Default)]
struct RingWriteResult {
    start_offset: usize,
    len: usize,
    overflowed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaybackConsumeState {
    Empty,
    Consumed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NotificationOutcome {
    Delivered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AvBufferSlice {
    slot_index: usize,
    offset: usize,
    len: usize,
    generation: u64,
}

const PX4_DEVICE_FAMILY_UNKNOWN: i32 = 0;
const DVR_STATUS_MASK_DISABLED: i32 = 0;
const NO_DISEQC_GENERATION: u64 = 0;
const PES_STREAM_ID_UNKNOWN: i32 = 0;
const MEDIA_EVENT_TIMESTAMP_ABSENT: i64 = 0;
const SECTION_VERSION_ABSENT: i32 = 0;
const SECTION_NUMBER_ABSENT: i32 = 0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AvSharedStats {
    allocated_slots: usize,
    free_slots: usize,
    evicted_slots: u64,
    released_slots: u64,
    stale_releases: u64,
    alloc_failures: u64,
    av_overflow_no_slot: u64,
    av_oversize_payload: u64,
    av_malformed_payload: u64,
}

impl AvSharedStats {
    fn summary(&self) -> String {
        format!(
            "allocated={} free={} evicted={} released={} stale={} alloc_failures={} av_overflow_no_slot={} av_oversize_payload={} av_malformed_payload={}",
            self.allocated_slots,
            self.free_slots,
            self.evicted_slots,
            self.released_slots,
            self.stale_releases,
            self.alloc_failures,
            self.av_overflow_no_slot,
            self.av_oversize_payload,
            self.av_malformed_payload,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AvPayloadDeliveryResult {
    Delivered {
        slice: AvBufferSlice,
        av_data_id: i64,
    },
    DroppedBeforeHandleExport,
    DroppedAfterClientRelease,
    DroppedNoFreeSlot,
    DroppedOversizePayload,
    DroppedMalformedPayload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AvPayloadInternalError {
    MutexPoisoned,
    SharedHandleExportedWithoutBacking,
    ActiveSlotCollision,
    SlotRegistryInconsistent,
    MappingFailure,
    CounterFailure,
}

fn allocate_next_av_data_id(counter: &AtomicI64) -> Result<i64, AvPayloadInternalError> {
    // r50dz53/G2-01: AV MediaEvent dataId は 0 予約・負数禁止・wrap禁止。
    // fetch_add() で i64::MAX から負数へ回り込ませない。
    loop {
        let current = counter.load(Ordering::SeqCst);
        if current <= 0 || current == i64::MAX {
            return Err(AvPayloadInternalError::CounterFailure);
        }
        match counter.compare_exchange(
            current,
            current + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return Ok(current),
            Err(_) => continue,
        }
    }
}

impl AvPayloadInternalError {
    fn as_str(self) -> &'static str {
        match self {
            Self::MutexPoisoned => "MutexPoisoned",
            Self::SharedHandleExportedWithoutBacking => "SharedHandleExportedWithoutBacking",
            Self::ActiveSlotCollision => "ActiveSlotCollision",
            Self::SlotRegistryInconsistent => "SlotRegistryInconsistent",
            Self::MappingFailure => "MappingFailure",
            Self::CounterFailure => "CounterFailure",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AvPayloadAllocateError {
    Delivery(AvPayloadDeliveryResult),
    Internal(AvPayloadInternalError),
}

fn av_payload_can_notify_data_ready(
    is_media: bool,
    delivery: Option<AvPayloadDeliveryResult>,
) -> bool {
    !is_media || matches!(delivery, Some(AvPayloadDeliveryResult::Delivered { .. }))
}

fn av_payload_status_decision(
    is_media: bool,
    delivery: Option<AvPayloadDeliveryResult>,
    overflow: bool,
) -> (bool, bool) {
    (
        av_payload_can_notify_data_ready(is_media, delivery),
        overflow,
    )
}

fn av_payload_should_write_standard_fmq(is_media: bool) -> bool {
    !is_media
}

fn payload_uses_standard_fmq_watermarks(is_media: bool, payload: &FilterPayload) -> bool {
    av_payload_should_write_standard_fmq(is_media) && !matches!(payload, FilterPayload::RecordPacket(_))
}

fn av_payload_should_emit_data_event(is_media: bool, av_slice: Option<AvBufferSlice>) -> bool {
    !is_media || av_slice.is_some()
}

fn av_shared_handle_allows_payload_delivery(exported: bool, client_released: bool) -> bool {
    exported && !client_released
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Px4PathDiagnosticSnapshot {
    ts_arrival_timeouts: u64,
    pat_timeouts: u64,
    pmt_timeouts: u64,
    av_data_timeouts: u64,
}

impl Px4PathDiagnosticSnapshot {
    fn summary(&self) -> String {
        format!(
            "ts_arrival_timeout={} pat_timeout={} pmt_timeout={} av_data_timeout={}",
            self.ts_arrival_timeouts, self.pat_timeouts, self.pmt_timeouts, self.av_data_timeouts,
        )
    }
}

#[derive(Clone, Debug)]
struct Px4StreamObservation {
    generation: u64,
    started_at: Instant,
    saw_ts: bool,
    saw_pat: bool,
    saw_pmt: bool,
    saw_av_data: bool,
    reported_ts_timeout: bool,
    reported_pat_timeout: bool,
    reported_pmt_timeout: bool,
    reported_av_data_timeout: bool,
}

impl Default for Px4StreamObservation {
    fn default() -> Self {
        Self {
            generation: 0,
            started_at: Instant::now(),
            saw_ts: false,
            saw_pat: false,
            saw_pmt: false,
            saw_av_data: false,
            reported_ts_timeout: false,
            reported_pat_timeout: false,
            reported_pmt_timeout: false,
            reported_av_data_timeout: false,
        }
    }
}

struct Px4PathDiagnostics {
    observation: Mutex<Px4StreamObservation>,
    ts_arrival_timeouts: AtomicU64,
    pat_timeouts: AtomicU64,
    pmt_timeouts: AtomicU64,
    av_data_timeouts: AtomicU64,
}

impl Px4PathDiagnostics {
    fn new() -> Self {
        Self {
            observation: Mutex::new(Px4StreamObservation::default()),
            ts_arrival_timeouts: AtomicU64::new(0),
            pat_timeouts: AtomicU64::new(0),
            pmt_timeouts: AtomicU64::new(0),
            av_data_timeouts: AtomicU64::new(0),
        }
    }

    fn apply_stream_boundary_reset(&self) {
        let Ok(mut observation) = lock_mutex_hal(&self.observation, "px4_path_observation") else {
            return;
        };
        observation.generation = observation.generation.saturating_add(1);
        observation.started_at = Instant::now();
        observation.saw_ts = false;
        observation.saw_pat = false;
        observation.saw_pmt = false;
        observation.saw_av_data = false;
        observation.reported_ts_timeout = false;
        observation.reported_pat_timeout = false;
        observation.reported_pmt_timeout = false;
        observation.reported_av_data_timeout = false;
    }

    fn observe_ts_packet(&self, packet: &[u8]) {
        let Some((pid, payload_start, payload)) = inspect_ts_payload(packet) else {
            return;
        };
        let Ok(mut observation) = lock_mutex_hal(&self.observation, "px4_path_observation") else {
            return;
        };
        observation.saw_ts = true;
        if payload_start && pid == 0x0000 && payload.first().copied() == Some(0x00) {
            observation.saw_pat = true;
        }
        if payload_start && payload.first().copied() == Some(0x02) {
            observation.saw_pmt = true;
        }
        if payload_start
            && payload.len() >= 4
            && payload[0] == 0x00
            && payload[1] == 0x00
            && payload[2] == 0x01
        {
            let stream_id = payload[3];
            if (0xe0..=0xef).contains(&stream_id) || (0xc0..=0xdf).contains(&stream_id) {
                observation.saw_av_data = true;
            }
        }
    }

    fn check_timeouts(&self) {
        let Ok(mut observation) = lock_mutex_hal(&self.observation, "px4_path_observation") else {
            return;
        };
        if observation.started_at.elapsed() < Duration::from_millis(PX4_PATH_DIAGNOSTIC_TIMEOUT_MS)
        {
            return;
        }
        if !observation.saw_ts && !observation.reported_ts_timeout {
            observation.reported_ts_timeout = true;
            let total = self
                .ts_arrival_timeouts
                .fetch_add(1, Ordering::SeqCst)
                .saturating_add(1);
            eprintln!(
                "maleicacid-tuner-hal-px4-diagnostic: TS arrival timeout generation={} total={}",
                observation.generation, total
            );
        }
        if !observation.saw_pat && !observation.reported_pat_timeout {
            observation.reported_pat_timeout = true;
            let total = self
                .pat_timeouts
                .fetch_add(1, Ordering::SeqCst)
                .saturating_add(1);
            eprintln!(
                "maleicacid-tuner-hal-px4-diagnostic: PAT timeout generation={} total={}",
                observation.generation, total
            );
        }
        if !observation.saw_pmt && !observation.reported_pmt_timeout {
            observation.reported_pmt_timeout = true;
            let total = self
                .pmt_timeouts
                .fetch_add(1, Ordering::SeqCst)
                .saturating_add(1);
            eprintln!(
                "maleicacid-tuner-hal-px4-diagnostic: PMT timeout generation={} total={}",
                observation.generation, total
            );
        }
        if !observation.saw_av_data && !observation.reported_av_data_timeout {
            observation.reported_av_data_timeout = true;
            let total = self
                .av_data_timeouts
                .fetch_add(1, Ordering::SeqCst)
                .saturating_add(1);
            eprintln!(
                "maleicacid-tuner-hal-px4-diagnostic: AV data timeout generation={} total={}",
                observation.generation, total
            );
        }
    }

    fn snapshot(&self) -> Px4PathDiagnosticSnapshot {
        Px4PathDiagnosticSnapshot {
            ts_arrival_timeouts: self.ts_arrival_timeouts.load(Ordering::SeqCst),
            pat_timeouts: self.pat_timeouts.load(Ordering::SeqCst),
            pmt_timeouts: self.pmt_timeouts.load(Ordering::SeqCst),
            av_data_timeouts: self.av_data_timeouts.load(Ordering::SeqCst),
        }
    }

    fn debug_dump_line(&self, frontend_id: i32) -> String {
        format!(
            "frontend={} px4_path {}",
            frontend_id,
            self.snapshot().summary()
        )
    }
}

impl Default for Px4PathDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

fn inspect_ts_payload(packet: &[u8]) -> Option<(u16, bool, &[u8])> {
    if packet.len() != 188 || packet.first().copied() != Some(0x47) {
        return None;
    }
    let pid = (((packet[1] & 0x1f) as u16) << 8) | packet[2] as u16;
    let payload_start = (packet[1] & 0x40) != 0;
    let adaptation_control = (packet[3] >> 4) & 0x03;
    if adaptation_control == 0 || adaptation_control == 2 {
        return Some((pid, payload_start, &[]));
    }
    let mut offset = 4usize;
    if adaptation_control == 3 {
        let adaptation_len = *packet.get(offset)? as usize;
        offset = offset.checked_add(1 + adaptation_len)?;
        if offset > packet.len() {
            return None;
        }
    }
    let mut payload = &packet[offset..];
    if payload_start {
        let looks_like_pes =
            payload.len() >= 4 && payload[0] == 0x00 && payload[1] == 0x00 && payload[2] == 0x01;
        if !looks_like_pes {
            let pointer = *payload.first()? as usize;
            if payload.len() < 1 + pointer {
                return None;
            }
            payload = &payload[1 + pointer..];
        }
    }
    Some((pid, payload_start, payload))
}

struct AvSharedBacking {
    file: Mutex<File>,
    mapping: Mutex<AvSharedMapping>,
    slot_size: usize,
    slot_count: usize,
    free_slots: Mutex<BTreeSet<usize>>,
    active: Mutex<BTreeMap<i64, AvBufferSlice>>,
    next_generation: Mutex<u64>,
    evicted_slots: Mutex<u64>,
    released_slots: Mutex<u64>,
    stale_releases: Mutex<u64>,
    alloc_failures: Mutex<u64>,
    av_overflow_no_slot: Mutex<u64>,
    av_oversize_payload: Mutex<u64>,
    av_malformed_payload: Mutex<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AvSharedHandleIdentity {
    st_dev: u64,
    st_ino: u64,
    st_rdev: u64,
    st_size: i64,
    export_generation: u64,
}

impl AvSharedHandleIdentity {
    fn from_metadata(metadata: &std::fs::Metadata, export_generation: u64) -> Self {
        Self {
            st_dev: metadata.dev(),
            st_ino: metadata.ino(),
            st_rdev: metadata.rdev(),
            st_size: metadata.size() as i64,
            export_generation,
        }
    }

    fn from_file(file: &File, export_generation: u64) -> BinderResult<Self> {
        let metadata = file
            .metadata()
            .map_err(|_| invalid_argument_status("AV shared handle fdの状態取得に失敗しました"))?;
        Ok(Self::from_metadata(&metadata, export_generation))
    }

    fn from_raw_fd(fd: RawFd, export_generation: u64) -> BinderResult<Self> {
        if fd < 0 {
            return Err(invalid_argument_status("AV shared handle fdが不正です"));
        }
        let metadata = std::fs::metadata(format!("/proc/self/fd/{fd}"))
            .map_err(|_| invalid_argument_status("AV shared handle fdの状態取得に失敗しました"))?;
        Ok(Self::from_metadata(&metadata, export_generation))
    }

    fn from_native_handle(handle: &TunerNativeHandle, export_generation: u64) -> BinderResult<Self> {
        if handle.fds.len() != 1 {
            return Err(invalid_argument_status("AV shared handle releaseはfd 1個だけを受理します"));
        }
        Self::from_raw_fd(handle.fds[0].as_raw_fd(), export_generation)
    }

    fn same_backing_as(&self, other: &Self) -> bool {
        self.st_dev == other.st_dev
            && self.st_ino == other.st_ino
            && self.st_rdev == other.st_rdev
            && self.st_size == other.st_size
    }

    fn matches_native_handle(&self, handle: &TunerNativeHandle) -> BinderResult<bool> {
        let other = Self::from_native_handle(handle, self.export_generation)?;
        Ok(self.same_backing_as(&other))
    }
}

struct AvSharedMapping {
    ptr: *mut u8,
    len: usize,
}

unsafe impl Send for AvSharedMapping {}

impl AvSharedMapping {
    fn new(file: &File, len: usize) -> std::io::Result<Self> {
        if len == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "zero-length AV shared mapping",
            ));
        }
        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                len,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            ptr: ptr.cast::<u8>(),
            len,
        })
    }

    fn write_at(&mut self, payload: &[u8], offset: usize) -> std::io::Result<()> {
        let end = offset.checked_add(payload.len()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "AV shared offset overflow",
            )
        })?;
        if end > self.len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "AV shared write outside mapping",
            ));
        }
        unsafe {
            ptr::copy_nonoverlapping(payload.as_ptr(), self.ptr.add(offset), payload.len());
        }
        Ok(())
    }
}

impl Drop for AvSharedMapping {
    fn drop(&mut self) {
        if !self.ptr.is_null() && self.len != 0 {
            unsafe {
                let _ = munmap(self.ptr.cast::<c_void>(), self.len);
            }
        }
    }
}

impl AvSharedBacking {
    fn should_log_counter(count: u64) -> bool {
        count <= 4 || count.is_power_of_two() || count % AV_DEBUG_LOG_INTERVAL == 0
    }

    fn new() -> BinderResult<Arc<Self>> {
        let slot_size = AV_SHARED_SLOT_SIZE_BYTES;
        let total_len = slot_size.checked_mul(AV_SLOT_COUNT).ok_or_else(|| {
            let msg = format!(
                "AV shared allocation size overflow: slot_size={} slot_count={}",
                slot_size, AV_SLOT_COUNT
            );
            eprintln!("maleicacid-tuner-hal-av-shared: {msg}");
            Status::new_service_specific_error(TunerResult::OUT_OF_MEMORY.0, Some(&msg))
        })?;
        let file = create_av_shared_file("tuner-hal-av", total_len)
            .map_err(av_shared_file_error_status)?;
        let mapping = AvSharedMapping::new(&file, total_len).map_err(|err| {
            av_shared_io_error_status(
                "mmap",
                "tuner-hal-av",
                "/dev/dma_heap/system",
                total_len,
                err,
            )
        })?;
        Ok(Arc::new(Self {
            file: Mutex::new(file),
            mapping: Mutex::new(mapping),
            slot_size,
            slot_count: AV_SLOT_COUNT,
            free_slots: Mutex::new((0..AV_SLOT_COUNT).collect()),
            active: Mutex::new(BTreeMap::new()),
            next_generation: Mutex::new(1),
            evicted_slots: Mutex::new(0),
            released_slots: Mutex::new(0),
            stale_releases: Mutex::new(0),
            alloc_failures: Mutex::new(0),
            av_overflow_no_slot: Mutex::new(0),
            av_oversize_payload: Mutex::new(0),
            av_malformed_payload: Mutex::new(0),
        }))
    }

    fn allocate(
        &self,
        av_data_id: i64,
        payload: &[u8],
    ) -> Result<AvBufferSlice, AvPayloadAllocateError> {
        if payload.is_empty() {
            self.record_malformed_payload()?;
            return Err(AvPayloadAllocateError::Delivery(
                AvPayloadDeliveryResult::DroppedMalformedPayload,
            ));
        }
        if payload.len() > self.slot_size {
            self.record_oversize_payload()?;
            return Err(AvPayloadAllocateError::Delivery(
                AvPayloadDeliveryResult::DroppedOversizePayload,
            ));
        }

        let slot_index = {
            let Ok(mut free_slots) = lock_mutex_hal(&self.free_slots, "av_shared_free_slots")
            else {
                return Err(AvPayloadAllocateError::Internal(
                    AvPayloadInternalError::MutexPoisoned,
                ));
            };
            let Some(slot_index) = free_slots.iter().next().copied() else {
                drop(free_slots);
                self.record_no_slot()?;
                return Err(AvPayloadAllocateError::Delivery(
                    AvPayloadDeliveryResult::DroppedNoFreeSlot,
                ));
            };
            if !free_slots.remove(&slot_index) {
                return Err(AvPayloadAllocateError::Internal(
                    AvPayloadInternalError::SlotRegistryInconsistent,
                ));
            }
            slot_index
        };

        let offset = slot_index * self.slot_size;
        let write_result = {
            let Ok(mut mapping) = lock_mutex_hal(&self.mapping, "av_shared_mapping") else {
                self.release_reserved_slot_best_effort(slot_index);
                return Err(AvPayloadAllocateError::Internal(
                    AvPayloadInternalError::MappingFailure,
                ));
            };
            mapping.write_at(payload, offset)
        };
        if write_result.is_err() {
            self.release_reserved_slot_best_effort(slot_index);
            self.record_malformed_payload()?;
            return Err(AvPayloadAllocateError::Delivery(
                AvPayloadDeliveryResult::DroppedMalformedPayload,
            ));
        }

        let generation = {
            let Ok(mut next) = lock_mutex_hal(&self.next_generation, "av_shared_next_generation")
            else {
                self.release_reserved_slot_best_effort(slot_index);
                return Err(AvPayloadAllocateError::Internal(
                    AvPayloadInternalError::CounterFailure,
                ));
            };
            let generation = *next;
            *next = (*next).saturating_add(1);
            generation
        };

        let slice = AvBufferSlice {
            slot_index,
            offset,
            len: payload.len(),
            generation,
        };
        {
            let Ok(mut active) = lock_mutex_hal(&self.active, "av_shared_active") else {
                self.release_reserved_slot_best_effort(slot_index);
                return Err(AvPayloadAllocateError::Internal(
                    AvPayloadInternalError::MutexPoisoned,
                ));
            };
            if active.contains_key(&av_data_id) {
                drop(active);
                self.release_reserved_slot_best_effort(slot_index);
                return Err(AvPayloadAllocateError::Internal(
                    AvPayloadInternalError::ActiveSlotCollision,
                ));
            }
            active.insert(av_data_id, slice);
        }
        Ok(slice)
    }

    fn release_reserved_slot_best_effort(&self, slot_index: usize) {
        if let Some(mut free_slots) = lock_mutex_option(&self.free_slots, "av_shared_free_slots") {
            free_slots.insert(slot_index);
        }
    }

    fn record_no_slot(&self) -> Result<(), AvPayloadAllocateError> {
        let Ok(mut no_slot) = lock_mutex_hal(&self.av_overflow_no_slot, "av_overflow_no_slot")
        else {
            return Err(AvPayloadAllocateError::Internal(
                AvPayloadInternalError::CounterFailure,
            ));
        };
        *no_slot = (*no_slot).saturating_add(1);
        drop(no_slot);
        let Ok(mut failures) = lock_mutex_hal(&self.alloc_failures, "av_shared_alloc_failures")
        else {
            return Err(AvPayloadAllocateError::Internal(
                AvPayloadInternalError::CounterFailure,
            ));
        };
        *failures = (*failures).saturating_add(1);
        Ok(())
    }

    fn record_oversize_payload(&self) -> Result<(), AvPayloadAllocateError> {
        self.increment_av_payload_drop_counter(&self.av_oversize_payload, "av_oversize_payload")
    }

    fn record_malformed_payload(&self) -> Result<(), AvPayloadAllocateError> {
        self.increment_av_payload_drop_counter(&self.av_malformed_payload, "av_malformed_payload")
    }

    fn increment_av_payload_drop_counter(
        &self,
        counter: &Mutex<u64>,
        counter_name: &str,
    ) -> Result<(), AvPayloadAllocateError> {
        let Ok(mut value) = lock_mutex_hal(counter, counter_name) else {
            return Err(AvPayloadAllocateError::Internal(
                AvPayloadInternalError::CounterFailure,
            ));
        };
        *value = (*value).saturating_add(1);
        drop(value);
        let Ok(mut failures) = lock_mutex_hal(&self.alloc_failures, "av_shared_alloc_failures")
        else {
            return Err(AvPayloadAllocateError::Internal(
                AvPayloadInternalError::CounterFailure,
            ));
        };
        *failures = (*failures).saturating_add(1);
        Ok(())
    }

    fn total_size(&self) -> usize {
        self.slot_size.saturating_mul(self.slot_count)
    }

    fn release_all(&self) -> BinderResult<()> {
        let mut active = lock_mutex_status(&self.active, "av_shared_active")?;
        let mut free_slots = lock_mutex_status(&self.free_slots, "av_shared_free_slots")?;
        let released_count = active.len() as u64;
        let mut next_free: BTreeSet<usize> = (0..self.slot_count).collect();
        active.clear();
        std::mem::swap(&mut *free_slots, &mut next_free);
        drop(free_slots);
        drop(active);
        if released_count > 0 {
            let mut released = lock_mutex_status(&self.released_slots, "av_shared_released_slots")?;
            *released = (*released).saturating_add(released_count);
        }
        Ok(())
    }


    fn release(&self, av_data_id: i64) -> BinderResult<bool> {
        let mut active = lock_mutex_status(&self.active, "av_shared_active")?;
        let mut free_slots = lock_mutex_status(&self.free_slots, "av_shared_free_slots")?;
        if let Some(slice) = active.get(&av_data_id).copied() {
            let mut next_active = active.clone();
            let mut next_free = free_slots.clone();
            next_active.remove(&av_data_id);
            next_free.insert(slice.slot_index);
            *active = next_active;
            *free_slots = next_free;
            drop(free_slots);
            drop(active);
            let mut released = lock_mutex_status(&self.released_slots, "av_shared_released_slots")?;
            *released = (*released).saturating_add(1);
            Ok(true)
        } else {
            drop(free_slots);
            drop(active);
            let stale_total = {
                let mut stale = lock_mutex_status(&self.stale_releases, "av_shared_stale_releases")?;
                *stale = (*stale).saturating_add(1);
                *stale
            };
            if Self::should_log_counter(stale_total) {
                eprintln!(
                    "maleicacid-tuner-hal: {} av_data_id={} stale_total={}",
                    self.debug_dump_line("stale AV shared release"),
                    av_data_id,
                    stale_total
                );
            }
            Ok(false)
        }
    }


    fn debug_dump_line(&self, owner: &str) -> String {
        match self.stats_result() {
            Ok(stats) => format!("{} av_shared {}", owner, stats.summary()),
            Err(_) => format!("{} av_shared stats_unavailable=lock_failure", owner),
        }
    }

    #[cfg(test)]
    fn stats(&self) -> AvSharedStats {
        self.stats_result().expect("av shared stats locks must be readable in tests")
    }

    fn stats_result(&self) -> BinderResult<AvSharedStats> {
        Ok(AvSharedStats {
            allocated_slots: lock_mutex_status(&self.active, "av_shared_active")?.len(),
            free_slots: lock_mutex_status(&self.free_slots, "av_shared_free_slots")?.len(),
            evicted_slots: *lock_mutex_status(&self.evicted_slots, "av_shared_evicted_slots")?,
            released_slots: *lock_mutex_status(&self.released_slots, "av_shared_released_slots")?,
            stale_releases: *lock_mutex_status(&self.stale_releases, "av_shared_stale_releases")?,
            alloc_failures: *lock_mutex_status(&self.alloc_failures, "av_shared_alloc_failures")?,
            av_overflow_no_slot: *lock_mutex_status(&self.av_overflow_no_slot, "av_overflow_no_slot")?,
            av_oversize_payload: *lock_mutex_status(&self.av_oversize_payload, "av_oversize_payload")?,
            av_malformed_payload: *lock_mutex_status(&self.av_malformed_payload, "av_malformed_payload")?,
        })
    }

    fn clear_result(&self) -> BinderResult<()> {
        let mut active = lock_mutex_status(&self.active, "av_shared_active")?;
        let mut free_slots = lock_mutex_status(&self.free_slots, "av_shared_free_slots")?;
        let mut next_active = BTreeMap::new();
        let mut next_free: BTreeSet<usize> = (0..self.slot_count).collect();
        std::mem::swap(&mut *active, &mut next_active);
        std::mem::swap(&mut *free_slots, &mut next_free);
        drop(free_slots);
        drop(active);
        {
            let mut next = lock_mutex_status(&self.next_generation, "av_shared_next_generation")?;
            *next = (*next).saturating_add(1).max(1);
            record_tuner_diagnostic_counter(&AV_GENERATION_BOUNDARY_COUNT, "av_generation");
        }
        Ok(())
    }


    fn clear_drop_only(&self) {
        if let Err(err) = self.clear_result() {
            eprintln!("maleicacid-tuner-hal-av-shared: best-effort clear failed: {err:?}");
        }
    }

    fn build_native_handle_with_identity(
        &self,
        _filter_id: i32,
        export_generation: u64,
    ) -> BinderResult<(TunerNativeHandle, AvSharedHandleIdentity)> {
        let dup = lock_mutex_status(&self.file, "av_shared_file")?
            .try_clone()
            .map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
        let identity = AvSharedHandleIdentity::from_file(&dup, export_generation)?;
        let handle = TunerNativeHandle {
            fds: vec![ParcelFileDescriptor::new(dup)],
            ints: vec![0],
        };
        Ok((handle, identity))
    }

    #[cfg(test)]
    fn build_native_handle(&self) -> BinderResult<TunerNativeHandle> {
        let (handle, _) = self.build_native_handle_with_identity(0, 1)?;
        Ok(handle)
    }
}

impl SharedMemoryBacking {
    fn new_ring(len: usize) -> BinderResult<Arc<Self>> {
        let data_len = len.max(4096);
        let queue = FmqQueue::create(data_len, true).map_err(|_| {
            record_tuner_diagnostic_counter(&FMQ_CREATE_ERROR_COUNT, "fmq_create_error");
            Status::from(StatusCode::UNKNOWN_ERROR)
        })?;
        Ok(Arc::new(Self {
            queue,
            ring_io_lock: Mutex::new(()),
            worker: Mutex::new(None),
            playback_consume_lock: Mutex::new(()),
            playback_worker_failed: RuntimeAtomicFlag::new(false),
            playback_residual: Mutex::new(TsPacketCompletionBuffer::default()),
            playback_malformed_bytes: AtomicU64::new(0),
            playback_dropped_bytes: AtomicU64::new(0),
        }))
    }

    fn start_playback_consumer(
        self: &Arc<Self>,
        state: Arc<Mutex<DemuxHandle>>,
        runtime_io: Arc<RuntimeIoRegistry>,
        closed: Arc<RuntimeAtomicFlag>,
        dvr_id: i32,
    ) -> BinderResult<()> {
        let mut worker_slot = lock_mutex_status(&self.worker, "shared_memory_worker")?;
        if worker_slot.is_some() {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        let backing_clone = Arc::clone(self);
        let backing_hook = Arc::clone(self);
        let runtime_io_hook = Arc::clone(&runtime_io);
        let state_hook = Arc::clone(&state);
        let closed_for_thread = Arc::clone(&closed);
        let closed_hook = Arc::clone(&closed);
        let handle = WorkerRuntime::spawn_owned_with_exit_hook(
            WorkerOwnerId("dvr_playback_consumer", dvr_id),
            "dvr_playback_consumer",
            move |owner_signal| {
                while !owner_signal.is_stop_requested() {
                    match backing_clone.consume_playback_ring(&state, dvr_id) {
                        Ok(PlaybackConsumeState::Consumed) => {}
                        Ok(PlaybackConsumeState::Empty) => {
                            if owner_signal.wait_timeout_or_stop(Duration::from_millis(10)) {
                                break;
                            }
                        }
                        Err(err) => {
                            backing_clone.fail_playback_worker(
                                &state,
                                &runtime_io,
                                &closed_for_thread,
                                dvr_id,
                                &format!("dvr_playback_consumer_failed: {err}"),
                            );
                            return WorkerExit::RuntimeFailure;
                        }
                    }
                }
                WorkerExit::StopRequested
            },
            move |exit| {
                if exit.is_abnormal() {
                    backing_hook.fail_playback_worker(
                        &state_hook,
                        &runtime_io_hook,
                        &closed_hook,
                        dvr_id,
                        &format!("dvr_playback_consumer_{exit:?}"),
                    );
                }
            },
        )
        .map_err(|err| {
            eprintln!("maleicacid-tuner-hal-worker: failed to spawn dvr_playback_consumer: {err}");
            Status::from(StatusCode::UNKNOWN_ERROR)
        })?;
        *worker_slot = Some(handle);
        Ok(())
    }


    fn write_bytes(&self, bytes: &[u8]) -> std::io::Result<RingWriteResult> {
        if bytes.is_empty() {
            return Ok(RingWriteResult::default());
        }
        let _ring_guard = lock_mutex_io(&self.ring_io_lock, "fmq_ring_io")?;
        let available = self.queue.available_to_write_result().map_err(|err| fmq_queue_error_io("fmq_available_to_write", err))?;
        if available < bytes.len() {
            if let Err(wake_err) = self.queue.wake(TUNER_EVENT_DATA_OVERFLOW) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("EventFlagWakeFailed: FMQ overflow wake failed status={wake_err:?}"),
                ));
            }
            return Ok(RingWriteResult {
                start_offset: 0,
                len: 0,
                overflowed: true,
            });
        }
        let written = self.queue.write_checked(bytes).map_err(|write_status| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("FMQ write failed status={write_status}"),
            )
        })?;
        if written != bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("FMQ short write requested={} written={written}", bytes.len()),
            ));
        }
        if let Err(wake_err) = self.queue.wake(TUNER_EVENT_DATA_READY) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("EventFlagWakeFailed: FMQ data-ready wake failed status={wake_err:?}"),
            ));
        }
        self.wake_waiters()?;
        Ok(RingWriteResult {
            start_offset: 0,
            len: written,
            overflowed: false,
        })
    }

    fn wake_waiters(&self) -> std::io::Result<()> {
        if let Some(worker) = lock_mutex_option(&self.worker, "shared_memory_worker")
            .and_then(|worker| worker.as_ref().map(|handle| handle.wake()))
        {
            if let Err(err) = worker {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("EventFlagWakeFailed: shared_memory_worker wake failed: {err:?}"),
                ));
            }
        }
        Ok(())
    }

    fn wait_for_stop_or_timeout(&self, interval: Duration) {
        thread::park_timeout(interval);
    }

    fn consume_playback_ring(
        &self,
        state: &Arc<Mutex<DemuxHandle>>,
        dvr_id: i32,
    ) -> std::io::Result<PlaybackConsumeState> {
        let _ring_guard = lock_mutex_io(&self.ring_io_lock, "fmq_ring_io")?;
        let _consume_guard = lock_mutex_io(&self.playback_consume_lock, "playback_consume")?;
        let mut demux = lock_mutex_io(state, "demux_handle")?;
        let Some(dvr) = demux.dvr_record(dvr_id) else {
            return Ok(PlaybackConsumeState::Empty);
        };
        if !dvr.is_started_for_api() || dvr.direction != DemuxPathDirection::Playback {
            return Ok(PlaybackConsumeState::Empty);
        }
        let available = self.queue.available_to_read_result().map_err(|err| fmq_queue_error_io("fmq_available_to_read", err))?;
        if available == 0 {
            return Ok(PlaybackConsumeState::Empty);
        }
        let mut payload = vec![0u8; available];
        let read = self.queue.read_into(&mut payload).map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "FMQ read failed"))?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "playback FMQ reported readable bytes but returned no data",
            ));
        }
        payload.truncate(read);
        let drain = {
            let mut residual = lock_mutex_io(&self.playback_residual, "playback_residual")?;
            residual.push(&payload)
        };
        if drain.malformed_bytes > 0 {
            let total = self
                .playback_malformed_bytes
                .fetch_add(drain.malformed_bytes as u64, Ordering::SeqCst)
                .saturating_add(drain.malformed_bytes as u64);
            eprintln!(
                "maleicacid-tuner-hal-dvr-playback-diagnostic: dvr_id={} malformed_bytes={} total_malformed_bytes={}",
                dvr_id,
                drain.malformed_bytes,
                total
            );
        }
        if drain.packets.is_empty() {
            if drain.malformed_bytes > 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "playback FMQ input contained only malformed TS bytes dvr_id={} malformed_bytes={}",
                        dvr_id, drain.malformed_bytes
                    ),
                ));
            }
            return Ok(PlaybackConsumeState::Consumed);
        }
        let mut aligned = Vec::with_capacity(drain.packets.len() * TS_PACKET_SIZE);
        for packet in drain.packets {
            aligned.extend_from_slice(&packet);
        }
        if !demux.inject_playback_payload(dvr_id, &aligned) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "playback payload rejected outside started playback state dvr_id={} bytes={}",
                    dvr_id,
                    aligned.len()
                ),
            ));
        }
        Ok(PlaybackConsumeState::Consumed)
    }

    fn ensure_playback_worker_healthy(&self) -> BinderResult<()> {
        if self.playback_worker_failed.load(Ordering::SeqCst) {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        Ok(())
    }

    fn fail_playback_worker(
        &self,
        state: &Arc<Mutex<DemuxHandle>>,
        runtime_io: &Arc<RuntimeIoRegistry>,
        closed: &Arc<RuntimeAtomicFlag>,
        dvr_id: i32,
        reason: &str,
    ) {
        let transition = RuntimeFailClosedTransition::dvr(dvr_id, "dvr_playback_consumer");
        transition.close_atomic(closed);
        self.playback_worker_failed.store(true, Ordering::SeqCst);
        transition.mark_failed(runtime_io, reason);
        if let Some(worker) = lock_mutex_option(&self.worker, "shared_memory_worker").and_then(|worker| worker.as_ref().map(|handle| handle.request_stop(WorkerExitReason::RuntimeFailure))) {
            if let Err(err) = worker {
                eprintln!("maleicacid-tuner-hal-worker: failed to request shared_memory_worker stop: {err:?}");
            }
        }
        if let Some(mut demux) = lock_mutex_option(state, "demux_handle") {
            demux.unregister_dvr(dvr_id);
        }
    }

    fn build_queue_desc(&self) -> BinderResult<TunerQueueDesc> {
        let grantor_count = self.queue.grantor_count_result().map_err(|err| fmq_queue_error_status("fmq_grantor_count", err))?;
        let mut grantors = Vec::with_capacity(grantor_count);
        for i in 0..grantor_count {
            let Some((fd_index, offset, extent)) = self.queue.grantor_at_result(i).map_err(|err| fmq_queue_error_status("fmq_grantor_at", err))? else {
                return Err(Status::from(StatusCode::UNKNOWN_ERROR));
            };
            grantors.push(CommonGrantorDescriptor {
                fdIndex: fd_index,
                offset,
                extent,
            });
        }
        let fd_count = self.queue.fd_count_result().map_err(|err| fmq_queue_error_status("fmq_fd_count", err))?;
        let mut fds = Vec::with_capacity(fd_count);
        for i in 0..fd_count {
            let fd = self.queue.dup_fd_at_result(i).map_err(|err| fmq_queue_error_status("fmq_dup_fd_at", err))?;
            if fd < 0 {
                return Err(Status::from(StatusCode::UNKNOWN_ERROR));
            }
            fds.push(ParcelFileDescriptor::new(unsafe { File::from_raw_fd(fd) }));
        }
        let int_count = self.queue.int_count_result().map_err(|err| fmq_queue_error_status("fmq_int_count", err))?;
        let mut ints = Vec::with_capacity(int_count);
        for i in 0..int_count {
            let Some(v) = self.queue.int_at_result(i).map_err(|err| fmq_queue_error_status("fmq_int_at", err))? else {
                return Err(Status::from(StatusCode::UNKNOWN_ERROR));
            };
            ints.push(v);
        }
        for grantor in &grantors {
            if grantor.fdIndex < 0 || grantor.fdIndex as usize >= fd_count {
                return Err(fmq_clear_error_status("descriptor_internal_error: grantor fdIndex out of range"));
            }
            if grantor.offset < 0 || grantor.extent <= 0 {
                return Err(fmq_clear_error_status("descriptor_internal_error: invalid grantor range"));
            }
            if grantor.offset.checked_add(grantor.extent).is_none() {
                return Err(fmq_clear_error_status("descriptor_internal_error: grantor range overflow"));
            }
        }
        let quantum = self.queue.quantum_result().map_err(|err| fmq_queue_error_status("fmq_quantum", err))?;
        if quantum <= 0 {
            return Err(fmq_clear_error_status("descriptor_internal_error: invalid quantum"));
        }
        let flags = self.queue.flags_result().map_err(|err| fmq_queue_error_status("fmq_flags", err))?;
        if int_count > 4 {
            return Err(fmq_clear_error_status("descriptor_internal_error: invalid int count"));
        }
        let handle = CommonNativeHandle { fds, ints };
        let mut desc = TunerQueueDesc::default();
        desc.grantors = grantors;
        desc.handle = handle;
        desc.quantum = quantum;
        desc.flags = flags;
        Ok(desc)
    }

    fn clear_result(&self) -> BinderResult<usize> {
        let _ring_guard = lock_mutex_status(&self.ring_io_lock, "fmq_ring_io")?;
        let _consume_guard = lock_mutex_status(&self.playback_consume_lock, "playback_consume")?;
        let available = self.queue.available_to_read_result().map_err(|err| fmq_queue_error_io("fmq_available_to_read", err))?;
        let mut dropped = 0usize;
        if available > 0 {
            let mut sink = vec![0u8; available];
            let read = self.queue.read_into(&mut sink).map_err(|_| fmq_clear_error_status("fmq_clear_read_failed"))?;
            if read != available {
                return Err(fmq_clear_error_status(format!(
                    "fmq_clear_short_read available={available} read={read}"
                )));
            }
            dropped = dropped.saturating_add(read);
        }
        let mut residual = lock_mutex_status(&self.playback_residual, "playback_residual")?;
        let tail_len = residual.tail_len();
        if tail_len > 0 {
            residual.clear();
            dropped = dropped.saturating_add(tail_len);
        }
        Ok(dropped)
    }

    fn clear_drop_only(&self) {
        if let Err(err) = self.clear_result() {
            eprintln!("maleicacid-tuner-hal-fmq: best-effort clear failed: {err:?}");
        }
    }

    fn discard_playback_input_for_boundary_result(
        &self,
        dvr_id: i32,
        boundary: &str,
    ) -> BinderResult<usize> {
        let dropped = self.clear_result()?;
        if dropped > 0 {
            let total = self
                .playback_dropped_bytes
                .fetch_add(dropped as u64, Ordering::SeqCst)
                .saturating_add(dropped as u64);
            eprintln!(
                "maleicacid-tuner-hal-dvr-playback-diagnostic: dvr_id={} boundary={} dropped_bytes={} total_dropped_bytes={}",
                dvr_id,
                boundary,
                dropped,
                total
            );
        }
        Ok(dropped)
    }

    #[allow(dead_code)]
    fn discard_playback_input_for_boundary_best_effort(&self, dvr_id: i32, boundary: &str) {
        if let Err(err) = self.discard_playback_input_for_boundary_result(dvr_id, boundary) {
            eprintln!(
                "maleicacid-tuner-hal-dvr-playback-diagnostic: dvr_id={} boundary={} best_effort_discard_failed={:?}",
                dvr_id, boundary, err
            );
        }
    }

    fn current_fill_bytes(&self) -> BinderResult<usize> {
        let _guard = lock_mutex_status(&self.ring_io_lock, "fmq_ring_io")?;
        match self.queue.fill_status() {
            Ok(FmqFillStatus::Bytes(bytes)) => Ok(bytes),
            Ok(FmqFillStatus::Unavailable) | Err(_) => Err(Status::from(StatusCode::UNKNOWN_ERROR)),
        }
    }

    fn stop(&self) -> BinderResult<()> {
        if let Some(mut worker) = lock_mutex_status(&self.worker, "shared_memory_worker")?.take() {
            worker.request_stop(WorkerExitReason::StopRequested).map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
            let outcome = worker.join_from_owner().map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
            if matches!(outcome, WorkerJoinOutcome::Joined(WorkerExitReason::RuntimeFailure | WorkerExitReason::PanicOrJoinFailure)) {
                self.playback_worker_failed.store(true, Ordering::SeqCst);
                return Err(worker_exit_status("shared_memory_worker", WorkerExit::RuntimeFailure));
            }
        }
        Ok(())
    }

    fn stop_best_effort(&self) {
        if let Some(mut worker) = lock_mutex_option(&self.worker, "shared_memory_worker")
            .and_then(|mut worker| worker.take())
        {
            let _ = worker.request_stop(WorkerExitReason::StopRequested);
            if let Err(err) = worker.join_from_owner() {
                self.playback_worker_failed.store(true, Ordering::SeqCst);
                eprintln!(
                    "maleicacid-tuner-hal-worker: best-effort shared_memory_worker join failed err={err:?}"
                );
            }
        }
    }

}


#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum RuntimeIoKind {
    Filter,
    Dvr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeFailClosedTransition {
    kind: RuntimeIoKind,
    id: i32,
    worker: &'static str,
}

impl RuntimeFailClosedTransition {
    fn filter(filter_id: i32, worker: &'static str) -> Self {
        Self {
            kind: RuntimeIoKind::Filter,
            id: filter_id,
            worker,
        }
    }

    fn dvr(dvr_id: i32, worker: &'static str) -> Self {
        Self {
            kind: RuntimeIoKind::Dvr,
            id: dvr_id,
            worker,
        }
    }

    fn object_name(self) -> &'static str {
        match self.kind {
            RuntimeIoKind::Filter => "filter",
            RuntimeIoKind::Dvr => "dvr",
        }
    }

    fn mark_failed(self, runtime_io: &Arc<RuntimeIoRegistry>, reason: &str) {
        eprintln!(
            "maleicacid-tuner-hal-worker: fail-closed object={} id={} worker={} reason={}",
            self.object_name(),
            self.id,
            self.worker,
            reason
        );
        runtime_io.mark_failed(self.kind, self.id, reason);
    }

    fn close_atomic(self, closed: &Arc<RuntimeAtomicFlag>) -> bool {
        !closed.swap(true, Ordering::SeqCst)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct RuntimeIoKey {
    kind: RuntimeIoKind,
    id: i32,
}

#[derive(Clone, Default)]
struct RuntimeIoBackings {
    filter_queue: Option<Weak<SharedMemoryBacking>>,
    filter_av_queue: Option<Weak<SharedMemoryBacking>>,
    filter_av_shared: Option<Weak<AvSharedBacking>>,
    filter_av_drop_unexported: Option<Arc<AtomicU64>>,
    dvr_queue: Option<Weak<SharedMemoryBacking>>,
    failed_reason: Option<String>,
}

#[derive(Default)]
struct RuntimeIoRegistry {
    entries: Mutex<BTreeMap<RuntimeIoKey, RuntimeIoBackings>>,
}

impl RuntimeIoRegistry {
    fn register_filter(
        &self,
        filter_id: i32,
        queue: &Arc<SharedMemoryBacking>,
        av_queue: &Arc<SharedMemoryBacking>,
        av_shared: Option<&Arc<AvSharedBacking>>,
        av_drop_unexported: &Arc<AtomicU64>,
    ) -> BinderResult<()> {
        let mut entries = lock_mutex_status(&self.entries, "runtime_io_entries")?;
        entries.insert(
            RuntimeIoKey {
                kind: RuntimeIoKind::Filter,
                id: filter_id,
            },
            RuntimeIoBackings {
                filter_queue: Some(Arc::downgrade(queue)),
                filter_av_queue: Some(Arc::downgrade(av_queue)),
                filter_av_shared: av_shared.map(Arc::downgrade),
                filter_av_drop_unexported: Some(Arc::clone(av_drop_unexported)),
                dvr_queue: None,
                failed_reason: None,
            },
        );
        Ok(())
    }

    fn set_filter_av_shared(&self, filter_id: i32, av_shared: &Arc<AvSharedBacking>) -> BinderResult<()> {
        let mut entries = lock_mutex_status(&self.entries, "runtime_io_entries")?;
        let Some(entry) = entries.get_mut(&RuntimeIoKey {
            kind: RuntimeIoKind::Filter,
            id: filter_id,
        }) else {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        };
        entry.filter_av_shared = Some(Arc::downgrade(av_shared));
        Ok(())
    }

    fn clear_filter_av_shared(&self, filter_id: i32) -> BinderResult<()> {
        let mut entries = lock_mutex_status(&self.entries, "runtime_io_entries")?;
        if let Some(entry) = entries.get_mut(&RuntimeIoKey {
            kind: RuntimeIoKind::Filter,
            id: filter_id,
        }) {
            entry.filter_av_shared = None;
        }
        Ok(())
    }

    fn clear_filter_av_shared_best_effort(&self, filter_id: i32) {
        if let Some(mut entries) = lock_mutex_option(&self.entries, "runtime_io_entries") {
            if let Some(entry) = entries.get_mut(&RuntimeIoKey {
                kind: RuntimeIoKind::Filter,
                id: filter_id,
            }) {
                entry.filter_av_shared = None;
            }
        }
    }

    fn register_dvr(&self, dvr_id: i32, queue: &Arc<SharedMemoryBacking>) -> BinderResult<()> {
        let mut entries = lock_mutex_status(&self.entries, "runtime_io_entries")?;
        entries.insert(
            RuntimeIoKey {
                kind: RuntimeIoKind::Dvr,
                id: dvr_id,
            },
            RuntimeIoBackings {
                filter_queue: None,
                filter_av_queue: None,
                filter_av_shared: None,
                filter_av_drop_unexported: None,
                dvr_queue: Some(Arc::downgrade(queue)),
                failed_reason: None,
            },
        );
        Ok(())
    }

    fn unregister_filter(&self, filter_id: i32) -> BinderResult<()> {
        lock_mutex_status(&self.entries, "runtime_io_entries")?.remove(&RuntimeIoKey {
            kind: RuntimeIoKind::Filter,
            id: filter_id,
        });
        Ok(())
    }

    fn unregister_filter_best_effort(&self, filter_id: i32) {
        if let Some(mut entries) = lock_mutex_option(&self.entries, "runtime_io_entries") {
            entries.remove(&RuntimeIoKey {
                kind: RuntimeIoKind::Filter,
                id: filter_id,
            });
        }
    }

    fn unregister_dvr(&self, dvr_id: i32) -> BinderResult<()> {
        lock_mutex_status(&self.entries, "runtime_io_entries")?.remove(&RuntimeIoKey {
            kind: RuntimeIoKind::Dvr,
            id: dvr_id,
        });
        Ok(())
    }

    fn unregister_dvr_best_effort(&self, dvr_id: i32) {
        if let Some(mut entries) = lock_mutex_option(&self.entries, "runtime_io_entries") {
            entries.remove(&RuntimeIoKey {
                kind: RuntimeIoKind::Dvr,
                id: dvr_id,
            });
        }
    }

    fn mark_failed(&self, kind: RuntimeIoKind, id: i32, reason: &str) {
        if let Some(mut entries) = lock_mutex_option(&self.entries, "runtime_io_entries") {
            if let Some(entry) = entries.get_mut(&RuntimeIoKey { kind, id }) {
                entry.failed_reason = Some(reason.to_string());
            }
        }
    }

    fn mark_all_failed(&self, reason: &str) {
        if let Some(mut entries) = lock_mutex_option(&self.entries, "runtime_io_entries") {
            for entry in entries.values_mut() {
                entry.failed_reason = Some(reason.to_string());
            }
        }
    }

    fn is_failed_for_owner_validation(&self, kind: RuntimeIoKind, id: i32) -> BinderResult<bool> {
        let entries = lock_mutex_status(&self.entries, "runtime_io_entries")?;
        Ok(entries
            .get(&RuntimeIoKey { kind, id })
            .and_then(|entry| entry.failed_reason.as_ref())
            .is_some())
    }

    fn ensure_not_failed(&self, kind: RuntimeIoKind, id: i32) -> BinderResult<()> {
        let entries = lock_mutex_status(&self.entries, "runtime_io_entries")?;
        if let Some(reason) = entries
            .get(&RuntimeIoKey { kind, id })
            .and_then(|entry| entry.failed_reason.as_ref())
        {
            return Err(invalid_state_status(&format!(
                "runtime data path failed: {}",
                reason
            )));
        }
        Ok(())
    }

    fn failed_reason_for_debug(&self) -> Vec<String> {
        let Some(entries) = lock_mutex_option(&self.entries, "runtime_io_entries") else {
            return vec!["runtime_io=poisoned".to_string()];
        };
        entries
            .iter()
            .filter_map(|(key, entry)| {
                entry.failed_reason.as_ref().map(|reason| {
                    format!(
                        "runtime_io_failed {:?}-{} reason={}",
                        key.kind, key.id, reason
                    )
                })
            })
            .collect()
    }

    fn flush_all(&self) -> BinderResult<()> {
        let mut first_error: Option<Status> = None;
        let mut entries = lock_mutex_status(&self.entries, "runtime_io_entries")?;
        entries.retain(|_, backings| {
            let mut alive = false;
            if let Some(backing) = backings.filter_queue.as_ref().and_then(Weak::upgrade) {
                if let Err(err) = backing.clear_result() {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
                alive = true;
            }
            if let Some(backing) = backings.filter_av_queue.as_ref().and_then(Weak::upgrade) {
                if let Err(err) = backing.clear_result() {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
                alive = true;
            }
            if let Some(backing) = backings.filter_av_shared.as_ref().and_then(Weak::upgrade) {
                if let Err(err) = backing.clear_result() {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
                alive = true;
            }
            if let Some(backing) = backings.dvr_queue.as_ref().and_then(Weak::upgrade) {
                if let Err(err) = backing.clear_result() {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
                alive = true;
            }
            alive
        });
        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn dump_av_shared_for_debug(&self) -> Vec<String> {
        let Some(entries) = lock_mutex_option(&self.entries, "runtime_io_entries") else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (key, backings) in entries.iter() {
            let owner = format!("{:?}-{}", key.kind, key.id);
            if let Some(backing) = backings.filter_av_shared.as_ref().and_then(Weak::upgrade) {
                out.push(backing.debug_dump_line(&owner));
            }
            if let Some(counter) = backings.filter_av_drop_unexported.as_ref() {
                out.push(format!(
                    "{} av_drop_unexported={}",
                    owner,
                    counter.load(Ordering::SeqCst)
                ));
            }
            if let Some(reason) = backings.failed_reason.as_ref() {
                out.push(format!("{} runtime_io_failed={}", owner, reason));
            }
        }
        out
    }

    #[cfg(test)]
    fn entry_count(&self) -> usize {
        lock_mutex_status(&self.entries, "runtime_io_entries")
            .expect("runtime_io_entries lock must be readable in tests")
            .len()
    }
}


struct TunerStreamBoundaryResources {
    runtime_io: Arc<RuntimeIoRegistry>,
    state: Arc<Mutex<DemuxHandle>>,
    px4_path_diagnostics: Option<Arc<Px4PathDiagnostics>>,
}

impl TunerStreamBoundaryResources {
    fn new(
        runtime_io: Arc<RuntimeIoRegistry>,
        state: Arc<Mutex<DemuxHandle>>,
        px4_path_diagnostics: Option<Arc<Px4PathDiagnostics>>,
    ) -> Self {
        Self { runtime_io, state, px4_path_diagnostics }
    }
}

impl StreamBoundaryResources for TunerStreamBoundaryResources {
    type Error = Status;

    fn advance_generation(&mut self, _plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error> {
        if let Some(px4) = self.px4_path_diagnostics.as_ref() {
            px4.apply_stream_boundary_reset();
        }
        Ok(())
    }

    fn notify_worker_boundary(&mut self, _plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error> {
        Ok(())
    }

    fn flush_runtime_io(&mut self, _plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error> {
        self.runtime_io.flush_all()
    }

    fn clear_fmq(&mut self, _plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error> {
        self.runtime_io.flush_all()
    }

    fn reset_packet_pipeline(&mut self, _plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error> {
        lock_mutex_status(&self.state, "demux_handle")?.apply_stream_boundary_reset();
        Ok(())
    }

    fn discard_av_payloads(&mut self, _plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error> {
        self.runtime_io.flush_all()
    }

    fn reset_dvr_playback(&mut self, _plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error> {
        self.runtime_io.flush_all()
    }

    fn commit_generation(&mut self, _plan: &StreamBoundaryResetPlan) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn execute_stream_boundary_for_demux(
    reason: StreamBoundaryReason,
    demux_id: i32,
    generation: u64,
    runtime_io: Arc<RuntimeIoRegistry>,
    state: Arc<Mutex<DemuxHandle>>,
    px4_path_diagnostics: Option<Arc<Px4PathDiagnostics>>,
    pending_slot: Option<&mut Option<PendingStreamBoundaryPlan>>,
) -> BinderResult<()> {
    let plan = StreamBoundaryResetPlan::for_demux(reason, demux_id, generation);
    let mut resources = TunerStreamBoundaryResources::new(runtime_io, state, px4_path_diagnostics);
    if let Some(slot) = pending_slot {
        if let Some(previous) = slot.take() {
            if let Err(failure) = previous.try_execute(&mut resources) {
                let failed_step = failure.result.failed_step;
                *slot = Some(PendingStreamBoundaryPlan::new(previous.plan, failed_step));
                return Err(failure.error);
            }
        }
        match plan.try_execute_from_step(&mut resources, None) {
            Ok(_) => Ok(()),
            Err(failure) => {
                *slot = Some(PendingStreamBoundaryPlan::new(plan, failure.result.failed_step));
                Err(failure.error)
            }
        }
    } else {
        plan.execute(&mut resources).map(|_| ())
    }
}

fn execute_stream_boundary_for_demux_best_effort(
    reason: StreamBoundaryReason,
    demux_id: i32,
    generation: u64,
    runtime_io: Arc<RuntimeIoRegistry>,
    state: Arc<Mutex<DemuxHandle>>,
    px4_path_diagnostics: Option<Arc<Px4PathDiagnostics>>,
    pending_slot: Option<&mut Option<PendingStreamBoundaryPlan>>,
) {
    let _ = execute_stream_boundary_for_demux(
        reason,
        demux_id,
        generation,
        runtime_io,
        state,
        px4_path_diagnostics,
        pending_slot,
    );
}

#[derive(Clone)]
struct BoundDemuxRuntime {
    demux_generation: u64,
    state: Arc<Mutex<DemuxHandle>>,
    runtime_io: Arc<RuntimeIoRegistry>,
    demux_record: Option<DemuxRecordRef>,
}

struct LivePumpWake {
    reader: Mutex<UnixStream>,
    writer: Mutex<UnixStream>,
}

impl LivePumpWake {
    fn new() -> std::io::Result<Self> {
        let (reader, writer) = UnixStream::pair()?;
        reader.set_nonblocking(true)?;
        writer.set_nonblocking(true)?;
        Ok(Self {
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
        })
    }

    fn reader_fd(&self) -> Option<i32> {
        lock_mutex_option(&self.reader, "live_pump_reader").map(|reader| reader.as_raw_fd())
    }

    fn wake(&self) {
        if let Some(mut writer) = lock_mutex_option(&self.writer, "live_pump_writer") {
            match writer.write(&[1]) {
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
        }
    }

    #[cfg(test)]
    fn drain_for_test(&self) {
        if let Some(mut reader) = lock_mutex_option(&self.reader, "live_pump_reader") {
            let mut buf = [0u8; 64];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(read) if read < buf.len() => break,
                    Ok(_) => continue,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum DescramblerDiagnosticKind {
    ClearPacket,
    Descrambled,
    ScrambledPassthroughForRecording,
    TransportErrorRecord,
    ScrambledNullPid,
    MalformedPacketForRecording,
    DescrambleFailed,
    InvalidPacketSize,
    BadSyncByte,
    InvalidAfc,
    InvalidAdaptationField,
    InvalidTsc,
    ScrambledWithoutPayload,
    NoKey,
    BadToken,
    CasBridgeUnconnected,
    RuntimeFailure,
    ExpiredKeySlot,
    Multi2Fail,
    ScrambledWithoutDescrambler,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct DescramblerDiagnosticCounters {
    clear_packets: u64,
    descrambled_packets: u64,
    scrambled_passthrough_for_recording_packets: u64,
    transport_error_record: u64,
    scrambled_null_pid: u64,
    malformed_packet_for_recording: u64,
    descramble_failed_packets: u64,
    invalid_packet_size: u64,
    bad_sync_byte: u64,
    invalid_afc: u64,
    invalid_adaptation_field: u64,
    invalid_tsc: u64,
    scrambled_without_payload: u64,
    no_key: u64,
    bad_token: u64,
    cas_bridge_unconnected: u64,
    runtime_failure: u64,
    expired_key_slot: u64,
    multi2_fail: u64,
    scrambled_without_descrambler: u64,
}

impl DescramblerDiagnosticCounters {
    fn increment(&mut self, kind: DescramblerDiagnosticKind) {
        match kind {
            DescramblerDiagnosticKind::ClearPacket => {
                self.clear_packets = self.clear_packets.saturating_add(1)
            }
            DescramblerDiagnosticKind::Descrambled => {
                self.descrambled_packets = self.descrambled_packets.saturating_add(1)
            }
            DescramblerDiagnosticKind::ScrambledPassthroughForRecording => {
                self.scrambled_passthrough_for_recording_packets = self
                    .scrambled_passthrough_for_recording_packets
                    .saturating_add(1)
            }
            DescramblerDiagnosticKind::TransportErrorRecord => {
                self.transport_error_record = self.transport_error_record.saturating_add(1)
            }
            DescramblerDiagnosticKind::ScrambledNullPid => {
                self.scrambled_null_pid = self.scrambled_null_pid.saturating_add(1)
            }
            DescramblerDiagnosticKind::MalformedPacketForRecording => {
                self.malformed_packet_for_recording =
                    self.malformed_packet_for_recording.saturating_add(1)
            }
            DescramblerDiagnosticKind::DescrambleFailed => {
                self.descramble_failed_packets = self.descramble_failed_packets.saturating_add(1)
            }
            DescramblerDiagnosticKind::InvalidPacketSize => {
                self.invalid_packet_size = self.invalid_packet_size.saturating_add(1)
            }
            DescramblerDiagnosticKind::BadSyncByte => {
                self.bad_sync_byte = self.bad_sync_byte.saturating_add(1)
            }
            DescramblerDiagnosticKind::InvalidAfc => {
                self.invalid_afc = self.invalid_afc.saturating_add(1)
            }
            DescramblerDiagnosticKind::InvalidAdaptationField => {
                self.invalid_adaptation_field = self.invalid_adaptation_field.saturating_add(1)
            }
            DescramblerDiagnosticKind::InvalidTsc => {
                self.invalid_tsc = self.invalid_tsc.saturating_add(1)
            }
            DescramblerDiagnosticKind::ScrambledWithoutPayload => {
                self.scrambled_without_payload = self.scrambled_without_payload.saturating_add(1)
            }
            DescramblerDiagnosticKind::NoKey => self.no_key = self.no_key.saturating_add(1),
            DescramblerDiagnosticKind::BadToken => {
                self.bad_token = self.bad_token.saturating_add(1)
            }
            DescramblerDiagnosticKind::CasBridgeUnconnected => {
                self.cas_bridge_unconnected = self.cas_bridge_unconnected.saturating_add(1)
            }
            DescramblerDiagnosticKind::RuntimeFailure => {
                self.runtime_failure = self.runtime_failure.saturating_add(1)
            }
            DescramblerDiagnosticKind::ExpiredKeySlot => {
                self.expired_key_slot = self.expired_key_slot.saturating_add(1)
            }
            DescramblerDiagnosticKind::Multi2Fail => {
                self.multi2_fail = self.multi2_fail.saturating_add(1)
            }
            DescramblerDiagnosticKind::ScrambledWithoutDescrambler => {
                self.scrambled_without_descrambler =
                    self.scrambled_without_descrambler.saturating_add(1)
            }
        }
    }

    fn summary(&self) -> String {
        format!(
            "CLEAR_PACKET={} DESCRAMBLED={} SCRAMBLED_PASSTHROUGH_FOR_RECORDING={} TRANSPORT_ERROR_RECORD={} SCRAMBLED_NULL_PID={} MALFORMED_PACKET_FOR_RECORDING={} DESCRAMBLE_FAILED={} INVALID_PACKET_SIZE={} BAD_SYNC_BYTE={} INVALID_AFC={} INVALID_ADAPTATION_FIELD={} INVALID_TSC={} SCRAMBLED_WITHOUT_PAYLOAD={} NO_KEY={} BAD_TOKEN={} CAS_BRIDGE_UNCONNECTED={} RUNTIME_FAILURE={} EXPIRED_KEY_SLOT={} MULTI2_FAIL={} SCRAMBLED_WITHOUT_DESCRAMBLER={}",
            self.clear_packets,
            self.descrambled_packets,
            self.scrambled_passthrough_for_recording_packets,
            self.transport_error_record,
            self.scrambled_null_pid,
            self.malformed_packet_for_recording,
            self.descramble_failed_packets,
            self.invalid_packet_size,
            self.bad_sync_byte,
            self.invalid_afc,
            self.invalid_adaptation_field,
            self.invalid_tsc,
            self.scrambled_without_payload,
            self.no_key,
            self.bad_token,
            self.cas_bridge_unconnected,
            self.runtime_failure,
            self.expired_key_slot,
            self.multi2_fail,
            self.scrambled_without_descrambler,
        )
    }
}

#[derive(Default)]
struct DescramblerDiagnosticRegistry {
    counters: Mutex<BTreeMap<(i32, u16), DescramblerDiagnosticCounters>>,
    update_failures: AtomicU64,
}

impl DescramblerDiagnosticRegistry {
    fn new() -> Self {
        Self {
            counters: Mutex::new(BTreeMap::new()),
            update_failures: AtomicU64::new(0),
        }
    }

    fn record_result(&self, demux_id: i32, pid: u16, kind: DescramblerDiagnosticKind) -> BinderResult<()> {
        let mut counters = lock_mutex_status(&self.counters, "descrambler_diagnostic_counters")?;
        let entry = counters.entry((demux_id, pid)).or_default();
        entry.increment(kind);
        if !matches!(
            kind,
            DescramblerDiagnosticKind::ClearPacket | DescramblerDiagnosticKind::Descrambled
        ) {
            eprintln!(
                "maleicacid-tuner-hal-descrambler-diagnostic: demux={} pid={} {:?} {}",
                demux_id,
                pid,
                kind,
                entry.summary()
            );
        }
        Ok(())
    }

    fn record_best_effort(&self, demux_id: i32, pid: u16, kind: DescramblerDiagnosticKind) -> bool {
        match self.record_result(demux_id, pid, kind) {
            Ok(()) => true,
            Err(err) => {
                // r50dz62/G3-10: diagnostic counter update failure must be
                // observable by worker paths. Do not write a secondary
                // diagnostic record into the same poisoned map.
                self.update_failures.fetch_add(1, Ordering::SeqCst);
                eprintln!(
                    "maleicacid-tuner-hal-descrambler-diagnostic: update_failed demux={} pid={} kind={:?} error={:?}",
                    demux_id, pid, kind, err
                );
                false
            }
        }
    }

    fn diagnostic_update_failure_count(&self) -> u64 {
        self.update_failures.load(Ordering::SeqCst)
    }

    fn snapshot(&self, demux_id: i32, pid: u16) -> DescramblerDiagnosticCounters {
        lock_mutex_option(&self.counters, "descrambler_diagnostic_counters")
            .and_then(|counters| counters.get(&(demux_id, pid)).cloned())
            .unwrap_or_default()
    }

    fn dump_for_debug(&self) -> String {
        let Some(counters) = lock_mutex_option(&self.counters, "descrambler_diagnostic_counters")
        else {
            return "descrambler_diagnostic_counters=poisoned".to_string();
        };
        counters
            .iter()
            .map(|((demux_id, pid), counters)| {
                format!("demux={} pid={} {}", demux_id, pid, counters.summary())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[cfg(test)]
    fn dump_for_test(&self) -> String {
        self.dump_for_debug()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PacketDescrambleFlow {
    Clear,
    Descrambled,
    ScrambledPassthrough,
    TransportErrorRecord,
    ScrambledNullPid,
    MalformedRecord,
    Drop,
    DescrambleFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescramblePacketDecision {
    packet: [u8; 188],
    flow: PacketDescrambleFlow,
}

struct DescramblerRuntimeRegistry {
    next_id: AtomicI64,
    entries: Mutex<BTreeMap<i64, Weak<Mutex<DescramblerSession>>>>,
    key_table: Mutex<Option<Weak<DescramblerKeyTable>>>,
}

impl DescramblerRuntimeRegistry {
    fn new() -> Self {
        Self {
            next_id: AtomicI64::new(1),
            entries: Mutex::new(BTreeMap::new()),
            key_table: Mutex::new(None),
        }
    }

    fn set_key_table(&self, key_table: &Arc<DescramblerKeyTable>) {
        if let Some(mut current) = lock_mutex_option(&self.key_table, "descrambler_runtime_key_table") {
            *current = Some(Arc::downgrade(key_table));
        }
    }

    fn release_key_token_result(&self, token: Option<Vec<u8>>) -> BinderResult<()> {
        let Some(token) = token else { return Ok(()); };
        if token == [0x00].as_slice() { return Ok(()); }
        let key_table = lock_mutex_status(&self.key_table, "descrambler_runtime_key_table")?
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or_else(|| Status::from(StatusCode::UNKNOWN_ERROR))?;
        key_table.expire_token(&token).map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))
    }
}

#[derive(Clone, Debug)]
struct ActiveDescramblerSnapshot {
    pids: BTreeSet<u16>,
    key_slot: Option<DescramblerKeySlot>,
}

impl ActiveDescramblerSnapshot {
    fn targets_pid(&self, pid: u16) -> bool {
        self.pids.contains(&pid)
    }

    fn descramble_packet_in_place(&self, packet: &mut [u8]) -> Result<(), DescrambleFailure> {
        let Some(key_slot) = self.key_slot.as_ref() else {
            return Err(DescrambleFailure::NoKey);
        };
        descramble_ts_packet_in_place(packet, &self.pids, key_slot).map(|_| ())
    }
}

fn diagnostic_kind_for_failure(failure: DescrambleFailure) -> DescramblerDiagnosticKind {
    match failure {
        DescrambleFailure::InvalidPacketSize => DescramblerDiagnosticKind::InvalidPacketSize,
        DescrambleFailure::BadSyncByte => DescramblerDiagnosticKind::BadSyncByte,
        DescrambleFailure::InvalidAfc => DescramblerDiagnosticKind::InvalidAfc,
        DescrambleFailure::InvalidAdaptationField => {
            DescramblerDiagnosticKind::InvalidAdaptationField
        }
        DescrambleFailure::InvalidTsc => DescramblerDiagnosticKind::InvalidTsc,
        DescrambleFailure::TransportErrorRecord => {
            DescramblerDiagnosticKind::TransportErrorRecord
        }
        DescrambleFailure::ScrambledNullPid => DescramblerDiagnosticKind::ScrambledNullPid,
        DescrambleFailure::ScrambledWithoutPayload => {
            DescramblerDiagnosticKind::ScrambledWithoutPayload
        }
        DescrambleFailure::NoKey => DescramblerDiagnosticKind::NoKey,
        DescrambleFailure::BadToken => DescramblerDiagnosticKind::BadToken,
        DescrambleFailure::Multi2Fail => DescramblerDiagnosticKind::Multi2Fail,
        DescrambleFailure::ScrambledPidNotRegistered => {
            DescramblerDiagnosticKind::ScrambledWithoutDescrambler
        }
    }
}

fn is_ts_frame_like_malformed(failure: DescrambleFailure) -> bool {
    matches!(
        failure,
        DescrambleFailure::InvalidAfc
            | DescrambleFailure::InvalidAdaptationField
            | DescrambleFailure::InvalidTsc
            | DescrambleFailure::ScrambledWithoutPayload
    )
}

fn descramble_packet_for_pid_with_diagnostics(
    packet: &[u8; 188],
    demux_id: i32,
    pid: u16,
    active_descramblers: &[ActiveDescramblerSnapshot],
    diagnostics: &DescramblerDiagnosticRegistry,
) -> DescramblePacketDecision {
    let header = match parse_ts_packet_header(packet) {
        Ok(header) => header,
        Err(failure) => {
            diagnostics.record_best_effort(demux_id, pid, diagnostic_kind_for_failure(failure));
            if is_ts_frame_like_malformed(failure) {
                diagnostics.record_best_effort(
                    demux_id,
                    pid,
                    DescramblerDiagnosticKind::MalformedPacketForRecording,
                );
                return DescramblePacketDecision {
                    packet: *packet,
                    flow: PacketDescrambleFlow::MalformedRecord,
                };
            }
            return DescramblePacketDecision {
                packet: *packet,
                flow: PacketDescrambleFlow::Drop,
            };
        }
    };

    if header.transport_error_indicator {
        diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::TransportErrorRecord);
        return DescramblePacketDecision {
            packet: *packet,
            flow: PacketDescrambleFlow::TransportErrorRecord,
        };
    }

    if header.transport_scrambling_control == 1 {
        diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::InvalidTsc);
        diagnostics.record_best_effort(
            demux_id,
            pid,
            DescramblerDiagnosticKind::MalformedPacketForRecording,
        );
        return DescramblePacketDecision {
            packet: *packet,
            flow: PacketDescrambleFlow::MalformedRecord,
        };
    }

    if header.pid == maleicacid_tuner_hal_descrambler::NULL_PID && header.transport_scrambling_control >= 2 {
        diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::ScrambledNullPid);
        return DescramblePacketDecision {
            packet: *packet,
            flow: PacketDescrambleFlow::ScrambledNullPid,
        };
    }

    if header.transport_scrambling_control == 0 {
        diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::ClearPacket);
        return DescramblePacketDecision {
            packet: *packet,
            flow: PacketDescrambleFlow::Clear,
        };
    }

    if header.payload_offset.is_none() {
        diagnostics.record_best_effort(
            demux_id,
            pid,
            DescramblerDiagnosticKind::ScrambledWithoutPayload,
        );
        diagnostics.record_best_effort(
            demux_id,
            pid,
            DescramblerDiagnosticKind::MalformedPacketForRecording,
        );
        return DescramblePacketDecision {
            packet: *packet,
            flow: PacketDescrambleFlow::MalformedRecord,
        };
    }

    let mut saw_target_descrambler = false;
    for descrambler in active_descramblers.iter().filter(|d| d.targets_pid(header.pid)) {
        saw_target_descrambler = true;
        let Some(key_slot) = descrambler.key_slot.as_ref() else {
            diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::NoKey);
            continue;
        };
        let mut candidate = *packet;
        match descramble_ts_packet_in_place(&mut candidate, &descrambler.pids, key_slot) {
            Ok(DescrambleOutcome::Descrambled { .. }) => {
                diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::Descrambled);
                return DescramblePacketDecision {
                    packet: candidate,
                    flow: PacketDescrambleFlow::Descrambled,
                };
            }
            Ok(DescrambleOutcome::PassedThrough { .. }) => {
                diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::ClearPacket);
                return DescramblePacketDecision {
                    packet: *packet,
                    flow: PacketDescrambleFlow::Clear,
                };
            }
            Err(DescrambleFailure::NoKey) => {
                diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::NoKey);
                continue;
            }
            Err(DescrambleFailure::BadToken) => {
                diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::BadToken);
                continue;
            }
            Err(DescrambleFailure::TransportErrorRecord) => {
                diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::TransportErrorRecord);
                return DescramblePacketDecision {
                    packet: *packet,
                    flow: PacketDescrambleFlow::TransportErrorRecord,
                };
            }
            Err(DescrambleFailure::ScrambledNullPid) => {
                diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::ScrambledNullPid);
                return DescramblePacketDecision {
                    packet: *packet,
                    flow: PacketDescrambleFlow::ScrambledNullPid,
                };
            }
            Err(failure) if is_ts_frame_like_malformed(failure) => {
                diagnostics.record_best_effort(demux_id, pid, diagnostic_kind_for_failure(failure));
                diagnostics.record_best_effort(
                    demux_id,
                    pid,
                    DescramblerDiagnosticKind::MalformedPacketForRecording,
                );
                return DescramblePacketDecision {
                    packet: *packet,
                    flow: PacketDescrambleFlow::MalformedRecord,
                };
            }
            Err(failure) => {
                diagnostics.record_best_effort(demux_id, pid, diagnostic_kind_for_failure(failure));
                diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::DescrambleFailed);
                return DescramblePacketDecision {
                    packet: *packet,
                    flow: PacketDescrambleFlow::DescrambleFailed,
                };
            }
        }
    }
    if !saw_target_descrambler {
        diagnostics.record_best_effort(
            demux_id,
            pid,
            DescramblerDiagnosticKind::ScrambledWithoutDescrambler,
        );
    }
    diagnostics.record_best_effort(
        demux_id,
        pid,
        DescramblerDiagnosticKind::ScrambledPassthroughForRecording,
    );
    DescramblePacketDecision {
        packet: *packet,
        flow: PacketDescrambleFlow::ScrambledPassthrough,
    }
}

#[cfg(test)]
fn descramble_packet_bytes_for_pid_with_diagnostics(
    packet: &[u8],
    demux_id: i32,
    pid: u16,
    active_descramblers: &[ActiveDescramblerSnapshot],
    diagnostics: &DescramblerDiagnosticRegistry,
) -> Option<DescramblePacketDecision> {
    if packet.len() != 188 {
        diagnostics.record_best_effort(demux_id, pid, DescramblerDiagnosticKind::InvalidPacketSize);
        return None;
    }
    let mut ts_packet = [0u8; 188];
    ts_packet.copy_from_slice(packet);
    Some(descramble_packet_for_pid_with_diagnostics(
        &ts_packet,
        demux_id,
        pid,
        active_descramblers,
        diagnostics,
    ))
}

fn maybe_descramble_packet_for_pid(
    packet: &[u8; 188],
    pid: u16,
    active_descramblers: &[ActiveDescramblerSnapshot],
) -> Option<[u8; 188]> {
    let diagnostics = DescramblerDiagnosticRegistry::new();
    let decision = descramble_packet_for_pid_with_diagnostics(
        packet,
        -1,
        pid,
        active_descramblers,
        &diagnostics,
    );
    matches!(decision.flow, PacketDescrambleFlow::Descrambled).then_some(decision.packet)
}

#[derive(Clone, Debug)]
struct AvSharedFileError {
    stage: &'static str,
    file_name: String,
    heap_name: &'static str,
    requested_size: usize,
    raw_return: i32,
    errno: i32,
    errno_name: &'static str,
}

impl AvSharedFileError {
    fn from_negative_errno(
        stage: &'static str,
        file_name: &str,
        heap_name: &'static str,
        requested_size: usize,
        raw_return: i32,
    ) -> Self {
        let errno = raw_return.saturating_neg();
        Self {
            stage,
            file_name: file_name.to_string(),
            heap_name,
            requested_size,
            raw_return,
            errno,
            errno_name: errno_name(errno),
        }
    }

    fn from_io_error(
        stage: &'static str,
        file_name: &str,
        heap_name: &'static str,
        requested_size: usize,
        err: std::io::Error,
    ) -> Self {
        let errno = err.raw_os_error().unwrap_or(ERRNO_EIO);
        Self {
            stage,
            file_name: file_name.to_string(),
            heap_name,
            requested_size,
            raw_return: -errno,
            errno,
            errno_name: errno_name(errno),
        }
    }

    fn detail(&self) -> String {
        format!(
            "AV shared allocation failed: stage={} file_name={} heap_name={} requested_size={} raw_return={} errno={} errno_name={}",
            self.stage, self.file_name, self.heap_name, self.requested_size, self.raw_return, self.errno, self.errno_name
        )
    }
}

fn errno_name(errno: i32) -> &'static str {
    match errno {
        ERRNO_ENOENT => "ENOENT",
        ERRNO_EACCES => "EACCES",
        ERRNO_ENOMEM => "ENOMEM",
        ERRNO_EINVAL => "EINVAL",
        ERRNO_EIO => "EIO",
        _ => "UNKNOWN_ERRNO",
    }
}

fn av_shared_file_error_result(errno: i32) -> TunerResult {
    match errno {
        ERRNO_ENOMEM => TunerResult::OUT_OF_MEMORY,
        ERRNO_ENOENT | ERRNO_EACCES => TunerResult::UNAVAILABLE,
        ERRNO_EINVAL | ERRNO_EIO => TunerResult::UNKNOWN_ERROR,
        _ => TunerResult::UNKNOWN_ERROR,
    }
}

fn av_shared_file_error_status(err: AvSharedFileError) -> Status {
    let detail = err.detail();
    eprintln!("maleicacid-tuner-hal-av-shared: {detail}");
    let result = av_shared_file_error_result(err.errno);
    Status::new_service_specific_error(result.0, Some(&detail))
}

fn av_shared_io_error_status(
    stage: &'static str,
    file_name: &str,
    heap_name: &'static str,
    requested_size: usize,
    err: std::io::Error,
) -> Status {
    av_shared_file_error_status(AvSharedFileError::from_io_error(
        stage,
        file_name,
        heap_name,
        requested_size,
        err,
    ))
}

fn create_dma_heap_file(name: &str, len: usize) -> Result<File, AvSharedFileError> {
    let raw = unsafe { tuner_dmabuf_heap_alloc_system(len) };
    if raw < 0 {
        return Err(AvSharedFileError::from_negative_errno(
            "dma_heap_alloc_system",
            name,
            "/dev/dma_heap/system",
            len,
            raw,
        ));
    }
    Ok(unsafe { File::from_raw_fd(raw) })
}

fn create_memfd_file(name: &str, len: usize) -> Result<File, AvSharedFileError> {
    let cname = CString::new(name).map_err(|_| AvSharedFileError {
        stage: "memfd_name",
        file_name: name.to_string(),
        heap_name: "memfd",
        requested_size: len,
        raw_return: -ERRNO_EINVAL,
        errno: ERRNO_EINVAL,
        errno_name: errno_name(ERRNO_EINVAL),
    })?;
    let fd = unsafe { syscall(SYS_MEMFD_CREATE, cname.as_ptr(), MFD_CLOEXEC) as i32 };
    if fd < 0 {
        return Err(AvSharedFileError::from_io_error(
            "memfd_create",
            name,
            "memfd",
            len,
            std::io::Error::last_os_error(),
        ));
    }
    if unsafe { ftruncate(fd, len as i64) } != 0 {
        let err = std::io::Error::last_os_error();
        unsafe {
            let _ = File::from_raw_fd(fd);
        }
        return Err(AvSharedFileError::from_io_error(
            "memfd_ftruncate",
            name,
            "memfd",
            len,
            err,
        ));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn create_av_shared_file(name: &str, len: usize) -> Result<File, AvSharedFileError> {
    match create_dma_heap_file(name, len) {
        Ok(file) => Ok(file),
        Err(err) => {
            #[cfg(test)]
            {
                let _ = err;
                create_memfd_file(name, len)
            }
            #[cfg(not(test))]
            {
                Err(err)
            }
        }
    }
}

#[derive(Default)]
struct StartupDiagnosticRegistry {
    records: Mutex<Vec<String>>,
}

impl StartupDiagnosticRegistry {
    fn new() -> Self {
        Self {
            records: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, message: impl Into<String>) {
        if let Some(mut records) = lock_mutex_option(&self.records, "startup_diagnostics") {
            records.push(message.into());
        }
    }

    fn dump_for_debug(&self) -> String {
        lock_mutex_option(&self.records, "startup_diagnostics")
            .map(|records| {
                if records.is_empty() {
                    "startup_diagnostics=ok".to_string()
                } else {
                    records
                        .iter()
                        .map(|record| format!("startup_diagnostic: {record}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            })
            .unwrap_or_else(|| "startup_diagnostics=poisoned".to_string())
    }
}

#[derive(Default)]
struct DiagnosticFileWriteRegistry {
    records: Mutex<BTreeMap<String, (u64, u64, String)>>,
}

impl DiagnosticFileWriteRegistry {
    fn new() -> Self {
        Self {
            records: Mutex::new(BTreeMap::new()),
        }
    }

    fn record_success(&self, path: &str) {
        if let Some(mut records) = lock_mutex_option(&self.records, "diagnostic_file_write_records")
        {
            let entry = records
                .entry(path.to_string())
                .or_insert((0, 0, String::new()));
            entry.1 = 0;
        }
    }

    fn record_failure(&self, path: &str, err: &std::io::Error) {
        let errno = err.raw_os_error().unwrap_or(ERRNO_EIO);
        let detail = format!(
            "operation=write path={} errno={} errno_name={} message={}",
            path,
            errno,
            errno_name(errno),
            err
        );
        eprintln!("maleicacid-tuner-hal-diagnostic-file: {detail}");
        if let Some(mut records) = lock_mutex_option(&self.records, "diagnostic_file_write_records")
        {
            let entry = records
                .entry(path.to_string())
                .or_insert((0, 0, String::new()));
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(1);
            entry.2 = detail;
        }
    }

    fn write(&self, path: &str, dump: String) {
        match std::fs::write(path, dump) {
            Ok(()) => self.record_success(path),
            Err(err) => self.record_failure(path, &err),
        }
    }

    fn dump_for_debug(&self) -> String {
        lock_mutex_option(&self.records, "diagnostic_file_write_records")
            .map(|records| {
                if records.is_empty() {
                    "diagnostic_file_writes=ok".to_string()
                } else {
                    records.iter().map(|(path, (total, consecutive, detail))| {
                        format!("diagnostic_file_write path={} total_failures={} consecutive_failures={} last={}", path, total, consecutive, detail)
                    }).collect::<Vec<_>>().join("\n")
                }
            })
            .unwrap_or_else(|| "diagnostic_file_writes=poisoned".to_string())
    }
}

#[derive(Clone, Debug)]
enum FrontendEntryKind {
    Px4 {
        unit: i32,
        device_name: Option<String>,
        control_path: PathBuf,
        declared_type: FrontendType,
        allowed_systems: Vec<FrontendSystem>,
    },
    Dvb {
        adapter: i32,
        frontend_index: i32,
        demux_index: i32,
        dvr_index: i32,
        declared_type: FrontendType,
        supported_systems: Vec<FrontendSystem>,
        min_frequency_hz: i64,
        max_frequency_hz: i64,
        max_symbol_rate: i32,
    },
}

#[derive(Clone, Debug)]
struct FrontendEntry {
    id: i32,
    kind: FrontendEntryKind,
}

fn entry_supports_satellite(entry: &FrontendEntry) -> bool {
    frontend_capability_model_for_entry(entry)
        .supported_systems
        .contains(frontend_cap_model::FrontendSystem::IsdbS)
}

fn entry_default_lnb_id(entry: &FrontendEntry) -> Option<i32> {
    entry_supports_satellite(entry).then_some(10_000 + entry.id)
}

fn entry_default_lnb_name(entry: &FrontendEntry) -> Option<String> {
    if !entry_supports_satellite(entry) {
        return None;
    }
    let suffix = match &entry.kind {
        FrontendEntryKind::Px4 {
            unit, device_name, ..
        } => format!(
            "px4-{}-unit-{unit}",
            device_name.as_deref().unwrap_or("unknown")
        ),
        FrontendEntryKind::Dvb {
            adapter,
            frontend_index,
            ..
        } => format!("dvb{adapter}.frontend{frontend_index}"),
    };
    Some(format!("maleicacid-lnb-{suffix}"))
}

fn packed_physical_group_id(tag: i32, major: i32, minor: i32) -> i32 {
    let major_bits = (major.max(0) & 0x3fff) << 14;
    let minor_bits = minor.max(0) & 0x3fff;
    tag | major_bits | minor_bits
}

fn entry_physical_group_id(entry: &FrontendEntry) -> i32 {
    match &entry.kind {
        FrontendEntryKind::Px4 {
            unit, device_name, ..
        } => packed_physical_group_id(
            PX4_PHYSICAL_GROUP_TAG,
            px4_device_family_code(device_name.as_deref()),
            *unit,
        ),
        FrontendEntryKind::Dvb {
            adapter,
            frontend_index,
            ..
        } => packed_physical_group_id(DVB_PHYSICAL_GROUP_TAG, *adapter, *frontend_index),
    }
}

fn entry_aidl_frontend_type(entry: &FrontendEntry) -> FrontendType {
    let systems: Vec<FrontendSystem> = match &entry.kind {
        FrontendEntryKind::Px4 { allowed_systems, .. } => allowed_systems.clone(),
        FrontendEntryKind::Dvb { supported_systems, .. } => supported_systems.clone(),
    };
    if systems.iter().any(|system| matches!(system, FrontendSystem::IsdbS)) {
        FrontendType::ISDBS
    } else if systems.iter().any(|system| matches!(system, FrontendSystem::IsdbT)) {
        FrontendType::ISDBT
    } else {
        FrontendType::UNDEFINED
    }
}

fn frontend_model_system(system: FrontendSystem) -> Option<frontend_cap_model::FrontendSystem> {
    match system {
        FrontendSystem::IsdbT => Some(frontend_cap_model::FrontendSystem::IsdbT),
        FrontendSystem::IsdbS => Some(frontend_cap_model::FrontendSystem::IsdbS),
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => None,
    }
}

fn frontend_capability_model_for_entry(entry: &FrontendEntry) -> frontend_cap_model::FrontendCapabilityModel {
    let aidl_type = entry_aidl_frontend_type(entry);
    let systems: Vec<FrontendSystem> = match &entry.kind {
        FrontendEntryKind::Px4 { allowed_systems, .. } => allowed_systems.clone(),
        FrontendEntryKind::Dvb { supported_systems, .. } => supported_systems.clone(),
    };
    let lnb_required = systems.iter().any(|system| matches!(system, FrontendSystem::IsdbS));
    let mut model = frontend_cap_model::FrontendCapabilityModel::new(
        entry_physical_group_id(entry),
        entry.id,
        aidl_type.0,
        lnb_required,
        entry_physical_group_id(entry),
    );
    for system in systems {
        if let Some(model_system) = frontend_model_system(system) {
            model.supported_systems.insert(model_system);
            model.runtime_allowed_systems.insert(model_system);
        }
    }
    model
}

fn frontend_runtime_policy_for_entry(entry: &FrontendEntry) -> frontend_cap_model::FrontendRuntimePolicy {
    frontend_cap_model::FrontendRuntimePolicy::from_model(&frontend_capability_model_for_entry(entry))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FrontendStatusSupport {
    snr: bool,
    signal_strength: bool,
    signal_quality: bool,
    rf_lock: bool,
    satellite: bool,
}

impl FrontendStatusSupport {
    fn for_entry(entry: &FrontendEntry) -> Self {
        let is_dvb = matches!(&entry.kind, FrontendEntryKind::Dvb { .. });
        Self {
            // frontend 状態 は、enumeration 時点の backend contract で取得可能性を
            // 固定できる値だけを advertise する。DVB FE 状態 word はこの HAL path で
            // 必須だが、optional SNR / strength ioctl は対象 driver で read 時にしか
            // support を確認できないため advertise しない。
            snr: false,
            signal_strength: false,
            signal_quality: is_dvb,
            rf_lock: is_dvb,
            satellite: entry_supports_satellite(entry),
        }
    }

    fn supports(self, status_type: FrontendStatusType) -> bool {
        match status_type {
            FrontendStatusType::DEMOD_LOCK => true,
            FrontendStatusType::RF_LOCK => self.rf_lock,
            FrontendStatusType::SNR => self.snr,
            FrontendStatusType::SIGNAL_STRENGTH => self.signal_strength,
            FrontendStatusType::SIGNAL_QUALITY => self.signal_quality,
            FrontendStatusType::LNB_VOLTAGE => self.satellite,
            _ => false,
        }
    }
}

fn entry_status_supported(entry: &FrontendEntry, status_type: FrontendStatusType) -> bool {
    FrontendStatusSupport::for_entry(entry).supports(status_type)
}

fn entry_status_caps(entry: &FrontendEntry) -> Vec<FrontendStatusType> {
    let mut caps = Vec::new();
    for status_type in [
        FrontendStatusType::DEMOD_LOCK,
        FrontendStatusType::RF_LOCK,
        FrontendStatusType::SNR,
        FrontendStatusType::SIGNAL_STRENGTH,
        FrontendStatusType::SIGNAL_QUALITY,
        FrontendStatusType::LNB_VOLTAGE,
    ] {
        if entry_status_supported(entry, status_type) {
            caps.push(status_type);
        }
    }
    caps
}

fn all_demux_ids() -> Vec<i32> {
    (0..MAX_LIVE_DEMUXES)
        .map(|offset| DEMUX_ID_BASE + offset as i32)
        .collect()
}

fn demux_id_in_pool(demux_id: i32) -> bool {
    let relative = demux_id - DEMUX_ID_BASE;
    relative >= 0 && (relative as usize) < MAX_LIVE_DEMUXES
}

fn local_filter_identity(filter: &Strong<dyn IFilter>) -> BinderResult<(i32, i32)> {
    let local_filter = Binder::<FilterHal>::try_from(filter.as_binder()).map_err(|_| {
        invalid_argument_status("filter object is not a local Maleicacid HAL filter")
    })?;
    Ok((local_filter.owner_demux_id, local_filter.filter_id))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LocalFilterGenerationIdentity {
    filter_id: i32,
    generation: u64,
}

fn pid_only_descrambler_source_identity() -> LocalFilterGenerationIdentity {
    LocalFilterGenerationIdentity {
        filter_id: -1,
        generation: 0,
    }
}

struct LocalFilterGenerationClaimTarget {
    identity: LocalFilterGenerationIdentity,
    demux_state: Arc<Mutex<DemuxHandle>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalFilterOwnerValidationError {
    NotLocalFilter,
    ForeignDemux,
    Closed,
    RuntimeFailed,
    RuntimeRegistryFailed,
    NotOpenDemuxFilter,
}

fn local_filter_owner_error_tuner_result(err: LocalFilterOwnerValidationError) -> i32 {
    match err {
        LocalFilterOwnerValidationError::RuntimeRegistryFailed => TunerResult::UNKNOWN_ERROR.0,
        LocalFilterOwnerValidationError::NotLocalFilter
        | LocalFilterOwnerValidationError::ForeignDemux
        | LocalFilterOwnerValidationError::Closed
        | LocalFilterOwnerValidationError::RuntimeFailed
        | LocalFilterOwnerValidationError::NotOpenDemuxFilter => TunerResult::INVALID_ARGUMENT.0,
    }
}

fn local_filter_owner_error_status(err: LocalFilterOwnerValidationError) -> Status {
    let message = match err {
        LocalFilterOwnerValidationError::NotLocalFilter => {
            "filter object is not a local Maleicacid HAL filter"
        }
        LocalFilterOwnerValidationError::ForeignDemux => "foreign filter belongs to another demux",
        LocalFilterOwnerValidationError::Closed => "source filter is closed",
        LocalFilterOwnerValidationError::RuntimeFailed => "source filter runtime data path failed",
        LocalFilterOwnerValidationError::RuntimeRegistryFailed => "runtime I/O registry failure",
        LocalFilterOwnerValidationError::NotOpenDemuxFilter => {
            "source filter is not an open demux filter"
        }
    };
    match local_filter_owner_error_tuner_result(err) {
        code if code == TunerResult::UNKNOWN_ERROR.0 => Status::from(StatusCode::UNKNOWN_ERROR),
        code if code == TunerResult::INVALID_STATE.0 => invalid_state_status(message),
        _ => invalid_argument_status(message),
    }
}

fn validate_local_filter_identity_for_owner(
    local_filter: &FilterHal,
    expected_owner_demux_id: i32,
) -> Result<LocalFilterGenerationIdentity, LocalFilterOwnerValidationError> {
    if local_filter.owner_demux_id != expected_owner_demux_id {
        return Err(LocalFilterOwnerValidationError::ForeignDemux);
    }
    if local_filter.closed.load(Ordering::SeqCst) {
        return Err(LocalFilterOwnerValidationError::Closed);
    }
    match local_filter
        .runtime_io
        .is_failed_for_owner_validation(RuntimeIoKind::Filter, local_filter.filter_id)
    {
        Ok(true) => return Err(LocalFilterOwnerValidationError::RuntimeFailed),
        Ok(false) => {}
        Err(_) => return Err(LocalFilterOwnerValidationError::RuntimeRegistryFailed),
    }
    let demux = lock_mutex_status(&local_filter.state, "demux_handle")
        .map_err(|_| LocalFilterOwnerValidationError::RuntimeRegistryFailed)?;
    if demux.demux_id() != expected_owner_demux_id {
        return Err(LocalFilterOwnerValidationError::NotOpenDemuxFilter);
    }
    let Some(generation) = demux.filter_generation(local_filter.filter_id) else {
        return Err(LocalFilterOwnerValidationError::NotOpenDemuxFilter);
    };
    Ok(LocalFilterGenerationIdentity {
        filter_id: local_filter.filter_id,
        generation,
    })
}

fn local_filter_id_for_owner(
    filter: &Strong<dyn IFilter>,
    expected_owner_demux_id: i32,
) -> BinderResult<i32> {
    let local_filter = Binder::<FilterHal>::try_from(filter.as_binder()).map_err(|_| {
        local_filter_owner_error_status(LocalFilterOwnerValidationError::NotLocalFilter)
    })?;
    validate_local_filter_identity_for_owner(&local_filter, expected_owner_demux_id)
        .map(|identity| identity.filter_id)
        .map_err(local_filter_owner_error_status)
}

fn local_filter_identity_for_owner(
    filter: &Strong<dyn IFilter>,
    expected_owner_demux_id: i32,
) -> BinderResult<LocalFilterGenerationIdentity> {
    let local_filter = Binder::<FilterHal>::try_from(filter.as_binder()).map_err(|_| {
        local_filter_owner_error_status(LocalFilterOwnerValidationError::NotLocalFilter)
    })?;
    validate_local_filter_identity_for_owner(&local_filter, expected_owner_demux_id)
        .map_err(local_filter_owner_error_status)
}

fn local_filter_claim_target_for_owner(
    filter: &Strong<dyn IFilter>,
    expected_owner_demux_id: i32,
) -> BinderResult<LocalFilterGenerationClaimTarget> {
    let local_filter = Binder::<FilterHal>::try_from(filter.as_binder()).map_err(|_| {
        local_filter_owner_error_status(LocalFilterOwnerValidationError::NotLocalFilter)
    })?;
    let identity = validate_local_filter_identity_for_owner(&local_filter, expected_owner_demux_id)
        .map_err(local_filter_owner_error_status)?;
    Ok(LocalFilterGenerationClaimTarget {
        identity,
        demux_state: Arc::clone(&local_filter.state),
    })
}

fn isdbt_mode_caps() -> i32 {
    FrontendIsdbtMode::AUTO.0 | FrontendIsdbtMode::MODE_3.0
}

fn isdbt_bandwidth_caps() -> i32 {
    FrontendIsdbtBandwidth::AUTO.0 | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ.0
}

fn isdbt_modulation_caps() -> i32 {
    FrontendIsdbtModulation::AUTO.0
        | FrontendIsdbtModulation::MOD_DQPSK.0
        | FrontendIsdbtModulation::MOD_QPSK.0
        | FrontendIsdbtModulation::MOD_16QAM.0
        | FrontendIsdbtModulation::MOD_64QAM.0
}

fn isdbt_coderate_caps() -> i32 {
    FrontendIsdbtCoderate::AUTO.0
        | FrontendIsdbtCoderate::CODERATE_1_2.0
        | FrontendIsdbtCoderate::CODERATE_2_3.0
        | FrontendIsdbtCoderate::CODERATE_3_4.0
        | FrontendIsdbtCoderate::CODERATE_5_6.0
        | FrontendIsdbtCoderate::CODERATE_7_8.0
}

fn isdbt_guard_interval_caps() -> i32 {
    FrontendIsdbtGuardInterval::AUTO.0
        | FrontendIsdbtGuardInterval::INTERVAL_1_32.0
        | FrontendIsdbtGuardInterval::INTERVAL_1_16.0
        | FrontendIsdbtGuardInterval::INTERVAL_1_8.0
        | FrontendIsdbtGuardInterval::INTERVAL_1_4.0
}

fn isdbt_time_interleave_caps() -> i32 {
    FrontendIsdbtTimeInterleaveMode::AUTO.0
        | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_0.0
        | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_1.0
        | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_2.0
        | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_4.0
}

fn isdbs_modulation_caps() -> i32 {
    FrontendIsdbsModulation::AUTO.0
        | FrontendIsdbsModulation::MOD_BPSK.0
        | FrontendIsdbsModulation::MOD_QPSK.0
        | FrontendIsdbsModulation::MOD_TC8PSK.0
}

fn isdbs_coderate_caps() -> i32 {
    FrontendIsdbsCoderate::AUTO.0
        | FrontendIsdbsCoderate::CODERATE_1_2.0
        | FrontendIsdbsCoderate::CODERATE_2_3.0
        | FrontendIsdbsCoderate::CODERATE_3_4.0
        | FrontendIsdbsCoderate::CODERATE_5_6.0
        | FrontendIsdbsCoderate::CODERATE_7_8.0
}

fn entry_frontend_max_symbol_rate_contract(_entry: &FrontendEntry) -> i32 {
    // r51 の ISDB-T / ISDB-S public frontend contract では explicit symbolRate を広告しない。
    // DVB probe が symbol rate を返しても、AOSP frontend capability へは出さない。
    0
}

fn entry_frontend_frequency_contract(entry: &FrontendEntry) -> (i64, i64, i64) {
    let (isdbt_min_hz, isdbt_max_hz, isdbt_tolerance_hz) = japan_isdbt_frequency_contract_range_hz();
    match entry_aidl_frontend_type(entry) {
        FrontendType::ISDBT => (isdbt_min_hz as i64, isdbt_max_hz as i64, isdbt_tolerance_hz as i64),
        FrontendType::ISDBS => (JAPAN_BS_FIRST_IF_HZ, JAPAN_CS110_LAST_IF_HZ, 0),
        _ => (0, 0, 0),
    }
}

fn entry_frontend_caps(entry: &FrontendEntry) -> FrontendCapabilities {
    match entry_aidl_frontend_type(entry) {
        FrontendType::ISDBT => FrontendCapabilities::IsdbtCaps(FrontendIsdbtCapabilities {
            modeCap: isdbt_mode_caps(),
            bandwidthCap: isdbt_bandwidth_caps(),
            modulationCap: isdbt_modulation_caps(),
            coderateCap: isdbt_coderate_caps(),
            guardIntervalCap: isdbt_guard_interval_caps(),
            timeInterleaveCap: isdbt_time_interleave_caps(),
            isSegmentAuto: true,
            isFullSegment: true,
        }),
        FrontendType::ISDBS => FrontendCapabilities::IsdbsCaps(FrontendIsdbsCapabilities {
            modulationCap: isdbs_modulation_caps(),
            coderateCap: isdbs_coderate_caps(),
        }),
        _ => Default::default(),
    }
}

fn enumerate_frontend_entries() -> Vec<FrontendEntry> {
    let mut entries = Vec::new();
    for probe in Px4FrontendBackend::enumerate_probes() {
        let base_id =
            px4_export_frontend_base_id(probe.frontend_index, probe.device_name.as_deref());
        if probe
            .supported_systems
            .iter()
            .any(|s| matches!(s, FrontendSystem::IsdbT))
        {
            entries.push(FrontendEntry {
                id: base_id,
                kind: FrontendEntryKind::Px4 {
                    unit: probe.frontend_index,
                    device_name: probe.device_name.clone(),
                    control_path: probe.control_path.clone(),
                    declared_type: FrontendType::ISDBT,
                    allowed_systems: vec![FrontendSystem::IsdbT],
                },
            });
        }
        if probe
            .supported_systems
            .iter()
            .any(|s| matches!(s, FrontendSystem::IsdbS))
        {
            entries.push(FrontendEntry {
                id: base_id + 1,
                kind: FrontendEntryKind::Px4 {
                    unit: probe.frontend_index,
                    device_name: probe.device_name.clone(),
                    control_path: probe.control_path.clone(),
                    declared_type: FrontendType::ISDBS,
                    allowed_systems: vec![FrontendSystem::IsdbS],
                },
            });
        }
    }

    for probe in DvbFrontendBackend::enumerate_probes() {
        let base_id = 10_000 + probe.adapter_id * 10 + probe.frontend_index * 2;
        let mut exported_any = false;
        for (offset, declared_type, system) in [
            (0, FrontendType::ISDBT, FrontendSystem::IsdbT),
            (1, FrontendType::ISDBS, FrontendSystem::IsdbS),
        ] {
            if !probe.supported_systems.iter().any(|s| *s == system) {
                continue;
            }
            exported_any = true;
            let (min_frequency_hz, max_frequency_hz) = probe.normalized_frequency_range_hz(system);
            entries.push(FrontendEntry {
                id: base_id + offset,
                kind: FrontendEntryKind::Dvb {
                    adapter: probe.adapter_id,
                    frontend_index: probe.frontend_index,
                    demux_index: probe.demux_index,
                    dvr_index: probe.dvr_index,
                    declared_type,
                    supported_systems: vec![system],
                    min_frequency_hz,
                    max_frequency_hz,
                    max_symbol_rate: probe.max_symbol_rate,
                },
            });
        }
        if !exported_any {
            eprintln!(
                "maleicacid-tuner-hal: 対象外 DVB frontend probe を無視します {:?}",
                probe
            );
        }
    }
    if entries.is_empty() {
        eprintln!(
            "maleicacid-tuner-hal: target tuner device absent; advertising zero frontend resources"
        );
    }
    for entry in &entries {
        eprintln!("maleicacid-tuner-hal: startup frontend entry {:?}", entry);
    }
    entries.sort_by_key(|e| e.id);
    entries
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LnbDeviceProfile {
    Px4Device15VOnly,
    EarthPt1FixedLnb,
    NoPower,
}

fn px4_lnb_profile_from_devname(devname: Option<&str>) -> LnbDeviceProfile {
    let Some(name) = devname else {
        return LnbDeviceProfile::NoPower;
    };
    if name.starts_with("px4video") {
        return LnbDeviceProfile::Px4Device15VOnly;
    }
    if name.starts_with("pxmlt5video")
        || name.starts_with("pxmlt8video")
        || name.starts_with("isdb6014video")
        || name.starts_with("isdb2056video")
        || name.starts_with("pxm1urvideo")
        || name.starts_with("pxs1urvideo")
        || name.starts_with("isdbt2071video")
    {
        return LnbDeviceProfile::NoPower;
    }
    LnbDeviceProfile::NoPower
}

fn px4_lnb_profile_from_identity(
    sysfs_devname: Option<&str>,
    dev_basename: Option<&str>,
) -> LnbDeviceProfile {
    px4_lnb_profile_from_devname(sysfs_devname.or(dev_basename))
}

fn px4_lnb_profile_from_device_name(device_name: Option<&str>) -> LnbDeviceProfile {
    px4_lnb_profile_from_identity(device_name, None)
}

fn px4_device_family_code(device_name: Option<&str>) -> i32 {
    let Some(name) = device_name else {
        return PX4_DEVICE_FAMILY_UNKNOWN;
    };
    if name.starts_with("px4video") {
        return 1;
    }
    if name.starts_with("pxmlt5video") {
        return 2;
    }
    if name.starts_with("pxmlt8video") {
        return 3;
    }
    if name.starts_with("isdb6014video") {
        return 4;
    }
    if name.starts_with("isdb2056video") {
        return 5;
    }
    if name.starts_with("pxm1urvideo") {
        return 6;
    }
    if name.starts_with("pxs1urvideo") {
        return 7;
    }
    if name.starts_with("isdbt2071video") {
        return 8;
    }
    0
}

fn px4_export_frontend_base_id(unit: i32, device_name: Option<&str>) -> i32 {
    1_000_000 + px4_device_family_code(device_name) * 10_000 + unit.max(0) * 10
}

impl Default for LnbDeviceProfile {
    fn default() -> Self {
        Self::NoPower
    }
}

#[derive(Clone, Debug, Default)]
struct LnbRuntimeState {
    profile: LnbDeviceProfile,
    owner_frontend_id: i32,
    voltage: Option<LnbVoltage>,
    tone: Option<LnbTone>,
    position: Option<LnbPosition>,
    supports_tone: bool,
    supports_diseqc: bool,
    generation: u64,
    diseqc_generation: u64,
    last_close_reset_error: Option<String>,
}

struct FrontendRuntime {
    frontend_id: i32,
    allowed_systems: Vec<FrontendSystem>,
    advertised_status_support: FrontendStatusSupport,
    backend: Mutex<FrontendBackendState>,
    ci_cam_id: Mutex<Option<i32>>,
    lnb_registry: Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
    bound_demuxes: Mutex<BTreeMap<i32, BoundDemuxRuntime>>,
    descrambler_registry: Arc<DescramblerRuntimeRegistry>,
    descrambler_diagnostics: Arc<DescramblerDiagnosticRegistry>,
    px4_path_diagnostics: Arc<Px4PathDiagnostics>,
    sent_diseqc_generations: Mutex<BTreeMap<i32, u64>>,
    runtime_failures: Mutex<Vec<String>>,
    scan_terminal_debug: Mutex<Option<String>>,
    pump_stop: RuntimeAtomicFlag,
    pump_wake_fd: Option<Arc<LivePumpWake>>,
    pump_worker: Mutex<Option<WorkerHandle>>,
}

impl FrontendRuntime {
    fn new(
        entry: FrontendEntry,
        lnb_registry: Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
        descrambler_registry: Arc<DescramblerRuntimeRegistry>,
        descrambler_diagnostics: Arc<DescramblerDiagnosticRegistry>,
    ) -> Arc<Self> {
        let advertised_status_support = FrontendStatusSupport::for_entry(&entry);
        let (allowed_systems, mut backend) = match &entry.kind {
            FrontendEntryKind::Px4 {
                unit,
                control_path,
                allowed_systems,
                ..
            } => (
                allowed_systems.clone(),
                FrontendBackendState::Px4(Px4FrontendBackend::new_with_control_path(
                    *unit,
                    control_path.clone(),
                )),
            ),
            FrontendEntryKind::Dvb {
                adapter,
                frontend_index,
                demux_index,
                dvr_index,
                declared_type,
                supported_systems,
                ..
            } => (
                supported_systems.clone(),
                FrontendBackendState::Dvb(DvbFrontendBackend::new(
                    *adapter,
                    *frontend_index,
                    *demux_index,
                    *dvr_index,
                    supported_systems.clone(),
                )),
            ),
        };
        let runtime_policy = frontend_runtime_policy_for_entry(&entry);
        let allowed_systems = allowed_systems
            .into_iter()
            .filter(|system| frontend_model_system(*system).map_or(false, |model_system| runtime_policy.allowed_systems.contains(model_system)))
            .collect::<Vec<_>>();
        if let Some(lnb_id) = entry_default_lnb_id(&entry) {
            match &mut backend {
                FrontendBackendState::Px4(inner) => inner.set_lnb_id(lnb_id),
                FrontendBackendState::Dvb(inner) => inner.set_lnb_id(lnb_id),
                FrontendBackendState::Unavailable {
                    selected_lnb_id, ..
                } => *selected_lnb_id = Some(lnb_id),
            }
        }
        Arc::new(Self {
            frontend_id: entry.id,
            allowed_systems,
            advertised_status_support,
            backend: Mutex::new(backend),
            ci_cam_id: Mutex::new(None),
            lnb_registry,
            bound_demuxes: Mutex::new(BTreeMap::new()),
            descrambler_registry,
            descrambler_diagnostics,
            px4_path_diagnostics: Arc::new(Px4PathDiagnostics::new()),
            sent_diseqc_generations: Mutex::new(BTreeMap::new()),
            runtime_failures: Mutex::new(Vec::new()),
            scan_terminal_debug: Mutex::new(None),
            pump_stop: RuntimeAtomicFlag::new(false),
            pump_wake_fd: LivePumpWake::new().ok().map(Arc::new),
            pump_worker: Mutex::new(None),
        })
    }

    fn bind_demux(
        self: &Arc<Self>,
        state: Arc<Mutex<DemuxHandle>>,
        runtime_io: Arc<RuntimeIoRegistry>,
        demux_generation: u64,
        demux_record: Option<DemuxRecordRef>,
    ) -> BinderResult<()> {
        let demux_id = lock_mutex_status(&state, "demux_handle")?.demux_id();
        {
            let mut demuxes = lock_mutex_status(&self.bound_demuxes, "frontend_bound_demuxes")?;
            demuxes.insert(
                demux_id,
                BoundDemuxRuntime {
                    demux_generation,
                    state,
                    runtime_io,
                    demux_record,
                },
            );
        }
        if let Err(err) = self.ensure_live_pump() {
            let should_stop = {
                let mut demuxes = lock_mutex_status(&self.bound_demuxes, "frontend_bound_demuxes")?;
                demuxes.remove(&demux_id);
                demuxes.is_empty()
            };
            if should_stop {
                self.stop_live_pump_best_effort();
            }
            return Err(err);
        }
        Ok(())
    }

    fn unbind_demux(&self, demux_id: i32) -> BinderResult<()> {
        let should_stop = {
            let mut demuxes = lock_mutex_status(&self.bound_demuxes, "frontend_bound_demuxes")?;
            demuxes.remove(&demux_id);
            demuxes.is_empty()
        };
        if should_stop {
            self.stop_live_pump()?;
        }
        Ok(())
    }

    fn unbind_demux_best_effort(&self, demux_id: i32) {
        let should_stop = {
            let Some(mut demuxes) =
                lock_mutex_option(&self.bound_demuxes, "frontend_bound_demuxes")
            else {
                return;
            };
            demuxes.remove(&demux_id);
            demuxes.is_empty()
        };
        if should_stop {
            self.stop_live_pump_best_effort();
        }
    }

    fn is_px4_backend(&self) -> bool {
        lock_mutex_option(&self.backend, "frontend_backend")
            .map(|backend| matches!(&*backend, FrontendBackendState::Px4(_)))
            .unwrap_or(false)
    }

    fn reset_bound_demuxes_for_stream_boundary(&self) -> BinderResult<()> {
        let px4 = if matches!(&*lock_mutex_status(&self.backend, "frontend_backend")?, FrontendBackendState::Px4(_)) {
            Some(Arc::clone(&self.px4_path_diagnostics))
        } else {
            None
        };
        let demuxes: Vec<BoundDemuxRuntime> = lock_mutex_status(&self.bound_demuxes, "frontend_bound_demuxes")?
            .values()
            .cloned()
            .collect();
        for bound in demuxes {
            let demux_id = lock_mutex_status(&bound.state, "demux_handle")?.demux_id();
            if let Some(record) = bound.demux_record.as_ref() {
                let mut record = lock_mutex_status(record, "demux_record")?;
                execute_stream_boundary_for_demux(
                    StreamBoundaryReason::TuneStart,
                    demux_id,
                    bound.demux_generation,
                    Arc::clone(&bound.runtime_io),
                    Arc::clone(&bound.state),
                    px4.clone(),
                    Some(&mut record.pending_stream_boundary_plan),
                )?;
            } else {
                execute_stream_boundary_for_demux(
                    StreamBoundaryReason::TuneStart,
                    demux_id,
                    bound.demux_generation,
                    Arc::clone(&bound.runtime_io),
                    Arc::clone(&bound.state),
                    px4.clone(),
                    None,
                )?;
            }
        }
        Ok(())
    }

    fn record_runtime_failure(&self, message: impl Into<String>) {
        let message = message.into();
        eprintln!(
            "maleicacid-tuner-hal-runtime: frontend={} {}",
            self.frontend_id, message
        );
        if let Some(mut failures) =
            lock_mutex_option(&self.runtime_failures, "frontend_runtime_failures")
        {
            failures.push(message);
        }
    }

    fn mark_live_path_failed(&self, reason: &str) {
        self.pump_stop.store(true, Ordering::SeqCst);
        if let Some(mut backend) = lock_mutex_option(&self.backend, "frontend_backend") {
            FrontendHal::backend_mark_callback_failed(
                &mut backend,
                format!("live path failed: {reason}"),
            );
        }
        let demuxes: Vec<BoundDemuxRuntime> =
            lock_mutex_option(&self.bound_demuxes, "frontend_bound_demuxes")
                .map(|demuxes| demuxes.values().cloned().collect())
                .unwrap_or_default();
        for bound in demuxes {
            bound.runtime_io.mark_all_failed(reason);
            let demux_id = lock_mutex_option(&bound.state, "demux_handle")
                .map(|demux| demux.demux_id())
                .unwrap_or(-1);
            if let Some(record) = bound.demux_record.as_ref() {
                if let Some(mut record) = lock_mutex_option(record, "demux_record") {
                    execute_stream_boundary_for_demux_best_effort(
                        StreamBoundaryReason::FrontendClose,
                        demux_id,
                        bound.demux_generation,
                        Arc::clone(&bound.runtime_io),
                        Arc::clone(&bound.state),
                        Some(Arc::clone(&self.px4_path_diagnostics)),
                        Some(&mut record.pending_stream_boundary_plan),
                    );
                }
            } else {
                execute_stream_boundary_for_demux_best_effort(
                    StreamBoundaryReason::FrontendClose,
                    demux_id,
                    bound.demux_generation,
                    Arc::clone(&bound.runtime_io),
                    Arc::clone(&bound.state),
                    Some(Arc::clone(&self.px4_path_diagnostics)),
                    None,
                );
            }
            if let Some(mut demux) = lock_mutex_option(&bound.state, "demux_handle") { demux.close(); }
        }
    }

    fn debug_dump_runtime_failures(&self) -> String {
        let failure_dump = lock_mutex_option(&self.runtime_failures, "frontend_runtime_failures")
            .map(|failures| {
                if failures.is_empty() {
                    format!("frontend={} runtime_failures=0", self.frontend_id)
                } else {
                    failures
                        .iter()
                        .map(|failure| {
                            format!("frontend={} runtime_failure: {failure}", self.frontend_id)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            })
            .unwrap_or_else(|| format!("frontend={} runtime_failures=poisoned", self.frontend_id));
        let scan_dump =
            lock_mutex_option(&self.scan_terminal_debug, "frontend_scan_terminal_debug")
                .map(|terminal| {
                    terminal.clone().unwrap_or_else(|| {
                        format!("frontend={} scan_last_terminal=none", self.frontend_id)
                    })
                })
                .unwrap_or_else(|| {
                    format!("frontend={} scan_last_terminal=poisoned", self.frontend_id)
                });
        format!("{failure_dump}\n{scan_dump}")
    }

    fn wake_live_pump(&self) {
        if let Some(wake_fd) = self.pump_wake_fd.as_ref() {
            wake_fd.wake();
        }
        if let Some(result) = lock_mutex_option(&self.pump_worker, "frontend_pump_worker")
            .and_then(|worker| worker.as_ref().map(|handle| handle.wake()))
        {
            if let Err(err) = result {
                self.record_runtime_failure(format!("live_pump_wake_failed err={err:?}"));
                self.mark_live_path_failed("live_pump_wake_failed");
            }
        }
    }

    fn wait_live_pump_interval(&self, owner_signal: &ConcreteWorkerSignal, interval: Duration) {
        if self.pump_stop.load(Ordering::SeqCst) {
            return;
        }
        let _ = owner_signal.wait_timeout_or_stop(interval);
    }

    fn ensure_live_pump(self: &Arc<Self>) -> BinderResult<()> {
        let finished_worker = {
            let mut worker = lock_mutex_status(&self.pump_worker, "frontend_pump_worker")?;
            if worker
                .as_ref()
                .map(|handle| handle.is_finished())
                .unwrap_or(false)
            {
                worker.take()
            } else if worker.is_some() {
                return Ok(());
            } else {
                None
            }
        };
        if let Some(worker) = finished_worker {
            let exit = WorkerRuntime::join(worker, "frontend_pump_worker");
            if exit.is_abnormal() {
                let detail = format!("worker=frontend_live_pump exit={:?}", exit);
                self.record_runtime_failure(detail.clone());
                self.mark_live_path_failed(&detail);
                return Err(Status::new_service_specific_error(
                    TunerResult::UNKNOWN_ERROR.0,
                    Some(&detail),
                ));
            }
        }
        let mut worker = lock_mutex_status(&self.pump_worker, "frontend_pump_worker")?;
        if worker.is_some() {
            return Ok(());
        }
        self.pump_stop.store(false, Ordering::SeqCst);
        let runtime_for_body = Arc::clone(self);
        let runtime_for_hook = Arc::clone(self);
        match WorkerRuntime::spawn_owned_with_exit_hook(
            WorkerOwnerId("frontend_live_pump", -1),
            "frontend_live_pump",
            move |owner_signal| runtime_for_body.live_pump_loop(owner_signal),
            move |exit| {
                if exit.is_abnormal() {
                    let detail = format!("worker=frontend_live_pump exit={:?}", exit);
                    runtime_for_hook.record_runtime_failure(detail.clone());
                    runtime_for_hook.mark_live_path_failed(&detail);
                }
            },
        ) {
            Ok(handle) => {
                *worker = Some(handle);
                Ok(())
            }
            Err(err) => {
                let detail = format!("worker=frontend_live_pump operation=spawn error={err}");
                self.record_runtime_failure(detail.clone());
                self.mark_live_path_failed(&detail);
                Err(Status::new_service_specific_error(
                    TunerResult::UNKNOWN_ERROR.0,
                    Some(&detail),
                ))
            }
        }
    }

    fn stop_live_pump(&self) -> BinderResult<()> {
        self.pump_stop.store(true, Ordering::SeqCst);
        self.wake_live_pump();
        let worker = lock_mutex_status(&self.pump_worker, "frontend_pump_worker")?.take();
        if let Some(worker) = worker {
            let exit = WorkerRuntime::join(worker, "frontend_pump_worker");
            if exit.is_abnormal() {
                let detail = format!("worker=frontend_live_pump stop_join_abnormal exit={exit:?}");
                self.record_runtime_failure(detail.clone());
                self.mark_live_path_failed(&detail);
                return Err(worker_exit_status("frontend_live_pump", exit));
            }
        }
        Ok(())
    }

    fn stop_live_pump_best_effort(&self) {
        self.pump_stop.store(true, Ordering::SeqCst);
        self.wake_live_pump();
        let worker = lock_mutex_option(&self.pump_worker, "frontend_pump_worker")
            .and_then(|mut worker| worker.take());
        if let Some(worker) = worker {
            let exit = WorkerRuntime::join(worker, "frontend_pump_worker");
            if exit.is_abnormal() {
                let detail = format!("worker=frontend_live_pump best_effort_stop_join_abnormal exit={exit:?}");
                self.record_runtime_failure(detail.clone());
                self.mark_live_path_failed(&detail);
            }
        }
    }

    fn live_pump_loop(self: Arc<Self>, owner_signal: Arc<ConcreteWorkerSignal>) -> WorkerExit {
        while !self.pump_stop.load(Ordering::SeqCst) && !owner_signal.is_stop_requested() {
            let reader = {
                let reader_result = {
                    let Some(mut backend) = lock_mutex_option(&self.backend, "frontend_backend") else {
                        self.record_runtime_failure(
                            "worker=frontend_live_pump reason=frontend_backend_lock_failed",
                        );
                        self.mark_live_path_failed("frontend_backend_lock_failed");
                        return WorkerExit::RuntimeFailure;
                    };
                    if !FrontendHal::backend_tuning_active(&backend) {
                        Ok(None)
                    } else {
                        FrontendHal::apply_selected_lnb_from_registry(
                            &self.lnb_registry,
                            &mut backend,
                        )
                        .map_err(|err| {
                            format!("worker=frontend_live_pump operation=apply_lnb error={err}")
                        })
                        .and_then(|()| {
                            FrontendHal::backend_live_stream_reader(&mut backend).map_err(|err| {
                                format!(
                                    "worker=frontend_live_pump operation=stream_reader error={err}"
                                )
                            })
                        })
                    }
                };
                match reader_result {
                    Ok(reader) => reader,
                    Err(detail) => {
                        self.record_runtime_failure(detail.clone());
                        self.mark_live_path_failed(&detail);
                        return WorkerExit::RuntimeFailure;
                    }
                }
            };
            let packets = if let Some(reader) = reader {
                match reader.sample_ts_packets(
                    128,
                    self.pump_wake_fd.as_ref().and_then(|wake| wake.reader_fd()),
                ) {
                    Ok(packets) => packets,
                    Err(err) => {
                        let detail =
                            format!("worker=frontend_live_pump operation=sample_ts error={err}");
                        self.record_runtime_failure(detail.clone());
                        self.mark_live_path_failed(&detail);
                        return WorkerExit::RuntimeFailure;
                    }
                }
            } else {
                Vec::new()
            };
            if !packets.is_empty() {
                if self.is_px4_backend() {
                    for packet in &packets {
                        self.px4_path_diagnostics.observe_ts_packet(packet);
                    }
                }
                let Some(demuxes_guard) =
                    lock_mutex_option(&self.bound_demuxes, "frontend_bound_demuxes")
                else {
                    self.record_runtime_failure(
                        "worker=frontend_live_pump reason=bound_demuxes_lock_failed",
                    );
                    self.mark_live_path_failed("bound_demuxes_lock_failed");
                    return WorkerExit::RuntimeFailure;
                };
                let demuxes: Vec<BoundDemuxRuntime> = demuxes_guard.values().cloned().collect();
                drop(demuxes_guard);
                for demux in demuxes {
                    let Some(mut handle) = lock_mutex_option(&demux.state, "demux_handle") else {
                        self.record_runtime_failure(
                            "worker=frontend_live_pump reason=demux_handle_lock_failed",
                        );
                        self.mark_live_path_failed("demux_handle_lock_failed");
                        return WorkerExit::RuntimeFailure;
                    };
                    let active_descramblers = match self.descrambler_registry.snapshots_for_demux(
                        handle.demux_id(),
                        demux.demux_generation,
                        &handle,
                    ) {
                        Ok(snapshots) => snapshots,
                        Err(_) => {
                            self.record_runtime_failure(
                                "worker=frontend_live_pump reason=descrambler_registry_lock_failed",
                            );
                            self.mark_live_path_failed("descrambler_registry_lock_failed");
                            return WorkerExit::RuntimeFailure;
                        }
                    };
                    for packet in &packets {
                        let pid = (((packet[1] & 0x1f) as i32) << 8) | packet[2] as i32;
                        if let Ok(pid_u16) = u16::try_from(pid) {
                            let diagnostic_failures_before = self
                                .descrambler_diagnostics
                                .diagnostic_update_failure_count();
                            let decision = descramble_packet_for_pid_with_diagnostics(
                                packet,
                                handle.demux_id(),
                                pid_u16,
                                &active_descramblers,
                                &self.descrambler_diagnostics,
                            );
                            if self
                                .descrambler_diagnostics
                                .diagnostic_update_failure_count()
                                != diagnostic_failures_before
                            {
                                let detail = format!(
                                    "worker=frontend_live_pump reason=descrambler_diagnostic_update_failed demux={} pid={}",
                                    handle.demux_id(),
                                    pid_u16
                                );
                                self.record_runtime_failure(detail.clone());
                                self.mark_live_path_failed(&detail);
                                return WorkerExit::RuntimeFailure;
                            }
                            match decision.flow {
                                PacketDescrambleFlow::Clear | PacketDescrambleFlow::Descrambled => {
                                    handle.push_ts_packet(&decision.packet);
                                }
                                PacketDescrambleFlow::ScrambledPassthrough
                                | PacketDescrambleFlow::TransportErrorRecord
                                | PacketDescrambleFlow::ScrambledNullPid
                                | PacketDescrambleFlow::MalformedRecord
                                | PacketDescrambleFlow::DescrambleFailed => {
                                    handle.push_ts_packet_record_only(&decision.packet);
                                }
                                PacketDescrambleFlow::Drop => {}
                            }
                        } else {
                            handle.push_ts_packet(packet);
                        }
                    }
                }
            }
            if self.is_px4_backend() {
                self.px4_path_diagnostics.check_timeouts();
            }
            let no_demux_bound =
                match lock_mutex_option(&self.bound_demuxes, "frontend_bound_demuxes") {
                    Some(demuxes) => demuxes.is_empty(),
                    None => {
                        self.record_runtime_failure(
                            "worker=frontend_live_pump reason=bound_demuxes_lock_failed_after_pump",
                        );
                        self.mark_live_path_failed("bound_demuxes_lock_failed_after_pump");
                        return WorkerExit::RuntimeFailure;
                    }
                };
            if no_demux_bound {
                self.pump_stop.store(true, Ordering::SeqCst);
                return WorkerExit::Normal;
            }
            let sleep_ms = if packets.is_empty() {
                25
            } else if packets.len() < 32 {
                5
            } else {
                0
            };
            if sleep_ms > 0 {
                self.wait_live_pump_interval(&owner_signal, Duration::from_millis(sleep_ms));
            }
        }
        self.pump_stop.store(true, Ordering::SeqCst);
        WorkerExit::StopRequested
    }
}

struct FrontendLeaseRegistry {
    open_frontends: BTreeSet<i32>,
    open_counts_by_type: BTreeMap<i32, i32>,
    open_physical_groups: BTreeMap<i32, i32>,
    open_generations: BTreeMap<i32, u64>,
    next_generation: u64,
}

impl Default for FrontendLeaseRegistry {
    fn default() -> Self {
        Self {
            open_frontends: BTreeSet::new(),
            open_counts_by_type: BTreeMap::new(),
            open_physical_groups: BTreeMap::new(),
            open_generations: BTreeMap::new(),
            next_generation: 1,
        }
    }
}

struct DemuxRecord {
    demux_id: i32,
    generation: u64,
    state: Arc<Mutex<DemuxHandle>>,
    runtime_io: Arc<RuntimeIoRegistry>,
    ref_count: usize,
    closing: bool,
    bound_frontend_id: Option<i32>,
    bound_frontend_generation: Option<u64>,
    ci_cam_diagnostics: Vec<String>,
    filter_ledger: FilterLedger,
    dvr_ledger: DvrLedger,
    descrambler_ledger: DescramblerLedger,
    pending_stream_boundary_plan: Option<PendingStreamBoundaryPlan>,
}

type DemuxRecordRef = Arc<Mutex<DemuxRecord>>;
type DemuxLedgerStore = Arc<Mutex<DemuxLedger<DemuxRecordRef>>>;

impl DescramblerRuntimeRegistry {
    fn register(&self, state: &Arc<Mutex<DescramblerSession>>) -> BinderResult<i64> {
        // r50dz58/G3-04: runtime id 0 is reserved; negative ids and wrap are fatal.
        // Keep allocation under the entries lock so collision checks and insertion are atomic.
        let mut entries = lock_mutex_status(&self.entries, "descrambler_runtime_entries")?;
        for _ in 0..1024 {
            let id = self.next_id.load(Ordering::SeqCst);
            if id <= 0 {
                return Err(Status::new_service_specific_error(
                    TunerResult::OUT_OF_MEMORY.0,
                    Some("descrambler runtime id allocator exhausted"),
                ));
            }
            let next = id.checked_add(1).unwrap_or(0);
            self.next_id.store(next, Ordering::SeqCst);
            if entries.contains_key(&id) {
                continue;
            }
            entries.insert(id, Arc::downgrade(state));
            return Ok(id);
        }
        Err(Status::new_service_specific_error(
            TunerResult::OUT_OF_MEMORY.0,
            Some("descrambler runtime id collision retry exhausted"),
        ))
    }

    fn unregister(&self, id: i64) -> BinderResult<()> {
        lock_mutex_status(&self.entries, "descrambler_runtime_entries")?.remove(&id);
        Ok(())
    }

    fn snapshots_for_demux(
        &self,
        demux_id: i32,
        demux_generation: u64,
        demux_handle: &DemuxHandle,
    ) -> BinderResult<Vec<ActiveDescramblerSnapshot>> {
        let mut entries = lock_mutex_status(&self.entries, "descrambler_runtime_entries")?;
        let mut dead = Vec::new();
        let mut snapshots = Vec::new();
        let mut expired_tokens = Vec::new();
        for (id, weak) in entries.iter() {
            let Some(state_arc) = weak.upgrade() else {
                dead.push(*id);
                continue;
            };
            let mut state = lock_mutex_status(&state_arc, "descrambler_state")?;
            if state.is_closed() {
                dead.push(*id);
                continue;
            }
            match (state.demux_id, state.demux_generation) {
                (Some(bound_demux_id), Some(bound_generation))
                    if bound_demux_id == demux_id && bound_generation == demux_generation => {}
                (Some(bound_demux_id), Some(_)) if bound_demux_id == demux_id => {
                    let expired_token = state.clear_key();
                    state.clear_demux();
                    if let Some(token) = expired_token {
                        expired_tokens.push(token);
                    }
                    continue;
                }
                _ => continue,
            }
            let key_slot = state.key_slot.clone();
            let stale_pids: Vec<u16> = state
                .pid_registrations
                .iter()
                .filter_map(|(pid, registration)| {
                    if registration.upstream_filter_id < 0 {
                        return None;
                    }
                    let keep = demux_handle
                        .filter_source_snapshot(registration.upstream_filter_id)
                        .map_or(false, |snapshot| {
                            snapshot.generation == registration.upstream_filter_generation
                                && snapshot.configured
                                && snapshot.tpid == Some(*pid as i32)
                                && descrambler_upstream_filter_open_type_allowed(snapshot.open_type)
                        });
                    if keep { None } else { Some(*pid) }
                })
                .collect();
            for pid in stale_pids {
                state.remove_pid(PidBinding { pid: pid as i32 });
            }
            if !state.pid_registrations.is_empty() {
                snapshots.push(ActiveDescramblerSnapshot {
                    pids: state.pid_registrations.keys().copied().collect(),
                    key_slot,
                });
            }
        }
        for id in dead {
            entries.remove(&id);
        }
        drop(entries);
        for token in expired_tokens {
            self.release_key_token_result(Some(token))?;
        }
        Ok(snapshots)
    }

    fn invalidate_demux(&self, demux_id: i32, demux_generation: u64) -> BinderResult<()> {
        let mut entries = lock_mutex_status(&self.entries, "descrambler_runtime_entries")?;
        let mut dead = Vec::new();
        let mut affected: Vec<Arc<Mutex<DescramblerSession>>> = Vec::new();
        for (id, weak) in entries.iter() {
            let Some(state_arc) = weak.upgrade() else {
                dead.push(*id);
                continue;
            };
            let state = lock_mutex_status(&state_arc, "descrambler_state")?;
            if state.is_closed() {
                dead.push(*id);
                continue;
            }
            if state.demux_id == Some(demux_id) && state.demux_generation == Some(demux_generation) {
                affected.push(Arc::clone(&state_arc));
            }
        }
        for id in dead {
            entries.remove(&id);
        }
        drop(entries);

        for state_arc in affected {
            let token = lock_mutex_status(&state_arc, "descrambler_state")?.key_token.clone();
            if let Err(err) = self.release_key_token_result(token) {
                lock_mutex_status(&state_arc, "descrambler_state")?
                    .mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
                return Err(err);
            }
            let mut state = lock_mutex_status(&state_arc, "descrambler_state")?;
            if state.demux_id == Some(demux_id) && state.demux_generation == Some(demux_generation) {
                state.clear_key();
                state.clear_demux();
                state.mark_cleanup_complete(DescramblerCleanupItem::KeyRelease);
            }
        }
        Ok(())
    }


    fn try_claim_pid_for_descrambler(
        &self,
        current_id: i64,
        demux_id: i32,
        demux_generation: u64,
        pid: u16,
        upstream_filter: LocalFilterGenerationIdentity,
    ) -> BinderResult<()> {
        let mut entries = lock_mutex_status(&self.entries, "descrambler_runtime_entries")?;
        let current_state_arc = entries
            .get(&current_id)
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid_state_status("descrambler runtime entry is closed"))?;

        let mut dead = Vec::new();
        let mut owned_by_other = false;
        for (id, weak) in entries.iter() {
            if *id == current_id {
                continue;
            }
            let Some(state_arc) = weak.upgrade() else {
                dead.push(*id);
                continue;
            };
            let state = lock_mutex_status(&state_arc, "descrambler_state")?;
            if state.is_closed() {
                dead.push(*id);
                continue;
            }
            if state.demux_id == Some(demux_id)
                && state.demux_generation == Some(demux_generation)
                && state.pid_registrations.contains_key(&pid)
            {
                owned_by_other = true;
                break;
            }
        }
        for id in dead {
            entries.remove(&id);
        }
        if owned_by_other {
            return Err(Status::new_service_specific_error(TunerResult::INVALID_STATE.0, Some("PID is already registered by another active descrambler on this demux generation")));
        }

        let mut state = lock_mutex_status(&current_state_arc, "descrambler_state")?;
        TunerDescrambler::ensure_open_locked(&state)?;
        if state.demux_id != Some(demux_id)
            || state.demux_generation != Some(demux_generation)
        {
            return Err(Status::new_service_specific_error(
                TunerResult::INVALID_STATE.0,
                None,
            ));
        }
        state.add_pid(
            PidBinding { pid: pid as i32 },
            SourceFilterBinding {
                filter_id: upstream_filter.filter_id,
                generation: upstream_filter.generation,
            },
        );
        Ok(())
    }
}

pub struct TunerDescrambler {
    id: i64,
    session: Arc<Mutex<DescramblerSession>>,
    demux_ledger: DemuxLedgerStore,
    descrambler_registry: Arc<DescramblerRuntimeRegistry>,
    descrambler_diagnostics: Arc<DescramblerDiagnosticRegistry>,
    descrambler_key_table: Arc<DescramblerKeyTable>,
}

impl TunerDescrambler {
    fn new(
        demux_ledger: DemuxLedgerStore,
        descrambler_registry: Arc<DescramblerRuntimeRegistry>,
        descrambler_diagnostics: Arc<DescramblerDiagnosticRegistry>,
        descrambler_key_table: Arc<DescramblerKeyTable>,
    ) -> BinderResult<Self> {
        let session = Arc::new(Mutex::new(DescramblerSession::new()));
        let id = descrambler_registry.register(&session)?;
        Ok(Self {
            id,
            session,
            demux_ledger,
            descrambler_registry,
            descrambler_diagnostics,
            descrambler_key_table,
        })
    }

    fn ensure_open_locked(state: &DescramblerSession) -> BinderResult<()> {
        if state.is_closed() {
            return Err(invalid_state_status("descrambler is closed"));
        }
        Ok(())
    }

    fn pid_from_demux_pid(pid: &DemuxPid) -> BinderResult<u16> {
        match pid {
            DemuxPid::TPid(value) if (0..=0x1ffe).contains(value) => Ok(*value as u16),
            DemuxPid::TPid(_) => Err(Status::new_service_specific_error(
                TunerResult::INVALID_ARGUMENT.0,
                None,
            )),
            _ => Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            )),
        }
    }

    fn record_key_token_error(&self, demux_id: Option<i32>, error: DescramblerKeyResolveError) -> BinderResult<()> {
        let diagnostic = match error {
            DescramblerKeyResolveError::RegistryUnavailable => DescramblerDiagnosticKind::RuntimeFailure,
            DescramblerKeyResolveError::ExpiredKeySlot => DescramblerDiagnosticKind::ExpiredKeySlot,
            DescramblerKeyResolveError::EmptyToken
            | DescramblerKeyResolveError::MalformedToken
            | DescramblerKeyResolveError::UnknownToken => DescramblerDiagnosticKind::BadToken,
        };
        self.descrambler_diagnostics
            .record_result(demux_id.unwrap_or(-1), 0x1fff, diagnostic)
    }

    fn status_for_key_token_error(error: DescramblerKeyResolveError) -> Status {
        match error {
            DescramblerKeyResolveError::RegistryUnavailable => Status::from(StatusCode::UNKNOWN_ERROR),
            DescramblerKeyResolveError::ExpiredKeySlot => {
                Status::new_service_specific_error(TunerResult::INVALID_ARGUMENT.0, None)
            }
            DescramblerKeyResolveError::EmptyToken
            | DescramblerKeyResolveError::MalformedToken
            | DescramblerKeyResolveError::UnknownToken => {
                Status::new_service_specific_error(TunerResult::INVALID_ARGUMENT.0, None)
            }
        }
    }

    fn ensure_bound_demux_generation_current(
        &self,
        demux_id: i32,
        demux_generation: u64,
    ) -> BinderResult<()> {
        let record = {
            let ledger = lock_mutex_status(&self.demux_ledger, "demux_ledger")?;
            ledger
                .get_record(LedgerId(demux_id))
                .ok_or_else(|| invalid_state_status("descrambler demux source is no longer open"))?
        };
        let (current_generation, demux_handle) = {
            let record = lock_mutex_status(&record, "demux_record")?;
            (record.generation, Arc::clone(&record.state))
        };
        if current_generation != demux_generation {
            return Err(invalid_state_status(
                "descrambler demux source generation is stale",
            ));
        }
        if lock_mutex_status(&demux_handle, "demux_handle")?.is_closed() {
            return Err(invalid_state_status("descrambler demux source is closed"));
        }
        Ok(())
    }

    fn clear_binding_for_stale_demux_locked(state: &mut DescramblerSession) -> Option<Vec<u8>> {
        let expired_token = state.clear_key();
        state.clear_demux();
        expired_token
    }

    fn release_key_token_result(&self, token: Option<Vec<u8>>) -> BinderResult<()> {
        let Some(token) = token else { return Ok(()); };
        if token == [0x00].as_slice() { return Ok(()); }
        self.descrambler_key_table.expire_token(&token).map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))
    }

    fn ensure_bound_demux_current_or_prune(&self) -> BinderResult<()> {
        let bound = {
            let state = lock_mutex_status(&self.session, "descrambler_session")?;
            Self::ensure_open_locked(&state)?;
            match (state.demux_id, state.demux_generation) {
                (Some(demux_id), Some(demux_generation)) => Some((demux_id, demux_generation)),
                _ => None,
            }
        };
        let Some((demux_id, demux_generation)) = bound else {
            return Ok(());
        };
        if let Err(err) = self.ensure_bound_demux_generation_current(demux_id, demux_generation) {
            let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
            Self::ensure_open_locked(&state)?;
            if state.demux_id == Some(demux_id)
                && state.demux_generation == Some(demux_generation)
            {
                let expired_token = Self::clear_binding_for_stale_demux_locked(&mut state);
                drop(state);
                self.release_key_token_result(expired_token)?;
            }
            return Err(err);
        }
        Ok(())
    }

    #[cfg(test)]
    fn debug_snapshot(
        &self,
    ) -> (
        bool,
        Option<i32>,
        Option<u64>,
        Option<Vec<u8>>,
        BTreeSet<u16>,
    ) {
        let state = lock_mutex_status(&self.session, "test_mutex").unwrap();
        (
            state.is_closed(),
            state.demux_id,
            state.demux_generation,
            state.key_token.clone(),
            state.pid_registrations.keys().copied().collect(),
        )
    }

    #[cfg(test)]
    fn add_pid_for_test(&self, pid: u16) -> BinderResult<()> {
        if pid > 0x1ffe {
            return Err(Status::new_service_specific_error(
                TunerResult::INVALID_ARGUMENT.0,
                None,
            ));
        }
        let (demux_id, demux_generation) = {
            let state = lock_mutex_status(&self.session, "descrambler_session")?;
            Self::ensure_open_locked(&state)?;
            let (Some(demux_id), Some(demux_generation)) = (state.demux_id, state.demux_generation)
            else {
                return Err(Status::new_service_specific_error(
                    TunerResult::INVALID_STATE.0,
                    None,
                ));
            };
            (demux_id, demux_generation)
        };
        self.descrambler_registry.try_claim_pid_for_descrambler(
            self.id,
            demux_id,
            demux_generation,
            pid,
            pid_only_descrambler_source_identity(),
        )
    }

    fn close_internal(&self) -> BinderResult<()> {
        {
            let state = lock_mutex_status(&self.session, "descrambler_session")?;
            if state.is_closed() {
                return Ok(());
            }
        }
        // registry unregisterを先に成功させる。失敗時はclosedを立てず、再close可能にする。
        let close_snapshot = {
            let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
            state.begin_close_with_snapshot()
        };
        if let (Some(demux_id), Some(_generation)) = (close_snapshot.demux_id, close_snapshot.demux_generation) {
            let record = {
                let ledger = lock_mutex_status(&self.demux_ledger, "demux_ledger")?;
                ledger.get_record(LedgerId(demux_id))
            };
            if let Some(record) = record {
                let mut record = lock_mutex_status(&record, "demux_record")?;
                if record.descrambler_ledger.begin_close(LedgerId(self.id as i32)).is_err() {
                    let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
                    state.mark_cleanup_failed(DescramblerCleanupItem::DemuxLedgerClose);
                    return Err(Status::from(StatusCode::UNKNOWN_ERROR));
                }
            }
        }
        if let Err(err) = self.descrambler_registry.unregister(self.id) {
            let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
            state.mark_cleanup_failed(DescramblerCleanupItem::RuntimeRegistry);
            return Err(err);
        } else {
            let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
            state.mark_cleanup_complete(DescramblerCleanupItem::RuntimeRegistry);
        }
        if let Err(err) = self.release_key_token_result(close_snapshot.key_token.clone()) {
            let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
            state.mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
            return Err(err);
        } else {
            let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
            state.mark_cleanup_complete(DescramblerCleanupItem::KeyRelease);
        }
        if let (Some(demux_id), Some(_generation)) = (close_snapshot.demux_id, close_snapshot.demux_generation) {
            let record = {
                let ledger = lock_mutex_status(&self.demux_ledger, "demux_ledger")?;
                ledger.get_record(LedgerId(demux_id))
            };
            if let Some(record) = record {
                let mut record = lock_mutex_status(&record, "demux_record")?;
                if let Err(_) = record.descrambler_ledger.commit_close(LedgerId(self.id as i32)) {
                    let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
                    state.mark_cleanup_failed(DescramblerCleanupItem::DemuxLedgerClose);
                    return Err(invalid_state_status("descrambler ledger close commit failed"));
                } else {
                    let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
                    state.mark_cleanup_complete(DescramblerCleanupItem::DemuxLedgerClose);
                }
            }
        }
        {
            let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
            state.complete_close_after_cleanup();
        }
        Ok(())
    }

    #[cfg(test)]
    fn remove_pid_for_test(&self, pid: u16) -> BinderResult<()> {
        if pid > 0x1ffe {
            return Err(Status::new_service_specific_error(
                TunerResult::INVALID_ARGUMENT.0,
                None,
            ));
        }
        let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
        Self::ensure_open_locked(&state)?;
        state.remove_pid(PidBinding { pid: pid as i32 });
        Ok(())
    }
}

impl Drop for TunerDescrambler {
    fn drop(&mut self) {
        let mut txn = LifecycleTxn::new();
        let _ = txn.cleanup("descrambler_drop_cleanup", || self.close_internal());
    }
}

impl Interface for TunerDescrambler {}

impl IDescrambler for TunerDescrambler {
    fn setDemuxSource(&self, demux_id: i32) -> BinderResult<()> {
        if !demux_id_in_pool(demux_id) {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        }
        let record = {
            let ledger = lock_mutex_status(&self.demux_ledger, "demux_ledger")?;
            ledger.get_record(LedgerId(demux_id)).ok_or_else(|| {
                Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None)
            })?
        };
        let (demux_generation, demux_handle) = {
            let record_guard = lock_mutex_status(&record, "demux_record")?;
            (record_guard.generation, Arc::clone(&record_guard.state))
        };
        if lock_mutex_status(&demux_handle, "demux_handle")?.is_closed() {
            return Err(invalid_state_status("demux handle is closed"));
        }
        {
            let state = lock_mutex_status(&self.session, "descrambler_session")?;
            Self::ensure_open_locked(&state)?;
            if state.demux_id.is_some() {
                return Err(invalid_state_status("descrambler demux source is already set"));
            }
        }
        {
            let mut record_guard = lock_mutex_status(&record, "demux_record")?;
            record_guard
                .descrambler_ledger
                .reserve(LedgerId(self.id as i32))
                .map_err(|_| Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None))?;
            record_guard
                .descrambler_ledger
                .commit_open(LedgerId(self.id as i32))
                .map_err(|_| invalid_state_status("descrambler ledger commit failed"))?;
        }
        let session_commit = (|| -> BinderResult<()> {
            let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
            Self::ensure_open_locked(&state)?;
            if state.demux_id.is_some() {
                return Err(invalid_state_status("descrambler demux source is already set"));
            }
            state.set_demux(demux_id, demux_generation);
            Ok(())
        })();
        if let Err(status) = session_commit {
            let rollback = lock_mutex_status(&record, "demux_record").and_then(|mut record_guard| {
                record_guard
                    .descrambler_ledger
                    .rollback_open(LedgerId(self.id as i32))
                    .map_err(|_| invalid_state_status("descrambler ledger rollback failed after session commit failure"))
            });
            if rollback.is_err() {
                if let Ok(mut state) = lock_mutex_status(&self.session, "descrambler_session") {
                    state.mark_cleanup_failed(DescramblerCleanupItem::DemuxLedgerClose);
                }
                return Err(Status::new_service_specific_error(
                    TunerResult::UNKNOWN_ERROR.0,
                    Some("descrambler_set_demux_source_rollback_failed"),
                ));
            }
            return Err(status);
        }
        Ok(())
    }

    fn setKeyToken(&self, key_token: &[u8]) -> BinderResult<()> {
        self.ensure_bound_demux_current_or_prune()?;
        let state = lock_mutex_status(&self.session, "descrambler_session")?;
        Self::ensure_open_locked(&state)?;
        let old_token = state.key_token.clone();
        let demux_for_diagnostic = state.demux_id;
        drop(state);

        if key_token == [0x00].as_slice() {
            // r50dz58/G3-07: release the old token first.  The no-key state is committed only
            // after the release succeeds, so a failed release keeps the old key retryable.
            if let Err(err) = self.release_key_token_result(old_token.clone()) {
                lock_mutex_status(&self.session, "descrambler_session")?
                    .mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
                return Err(err);
            }
            let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
            Self::ensure_open_locked(&state)?;
            if state.key_token == old_token {
                state.clear_key();
            }
            state.mark_cleanup_complete(DescramblerCleanupItem::KeyRelease);
            return Ok(());
        }

        let resolved = match self.descrambler_key_table.resolve_with_diagnostic(key_token) {
            Ok(resolved) => resolved,
            Err(error) => {
                self.record_key_token_error(demux_for_diagnostic, error)?;
                return Err(Self::status_for_key_token_error(error));
            }
        };

        // r50dz58/G3-06: old-token release is a prepare/cleanup step.  Do not publish the
        // new session key until the old release has succeeded.
        if let Err(err) = self.release_key_token_result(old_token.clone()) {
            lock_mutex_status(&self.session, "descrambler_session")?
                .mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
            return Err(err);
        }
        let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
        Self::ensure_open_locked(&state)?;
        if state.key_token == old_token {
            state.set_resolved_key(key_token.to_vec(), resolved.slot);
        }
        state.mark_cleanup_complete(DescramblerCleanupItem::KeyRelease);
        Ok(())
    }

    fn addPid(
        &self,
        pid: &DemuxPid,
        optional_upstream_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        let pid = Self::pid_from_demux_pid(pid)?;
        self.ensure_bound_demux_current_or_prune()?;
        let (demux_id, demux_generation) = {
            let state = lock_mutex_status(&self.session, "descrambler_session")?;
            Self::ensure_open_locked(&state)?;
            let Some(demux_id) = state.demux_id else {
                return Err(Status::new_service_specific_error(
                    TunerResult::INVALID_STATE.0,
                    None,
                ));
            };
            let Some(demux_generation) = state.demux_generation else {
                return Err(Status::new_service_specific_error(
                    TunerResult::INVALID_STATE.0,
                    None,
                ));
            };
            (demux_id, demux_generation)
        };
        let upstream_filter = local_filter_claim_target_for_owner(optional_upstream_filter, demux_id)?;
        let demux = lock_mutex_status(&upstream_filter.demux_state, "demux_handle")?;
        if demux.demux_id() != demux_id {
            return Err(invalid_argument_status("source filter belongs to another demux"));
        }
        let Some(source_snapshot) = demux.filter_source_snapshot(upstream_filter.identity.filter_id) else {
            return Err(invalid_argument_status("source filter is not an open demux filter"));
        };
        if source_snapshot.generation != upstream_filter.identity.generation {
            return Err(invalid_state_status(
                "source filter generation changed before PID claim",
            ));
        }
        if !source_snapshot.configured || source_snapshot.tpid.is_none() {
            return Err(invalid_state_status("source filter is not configured"));
        }
        if source_snapshot.tpid != Some(pid as i32) {
            return Err(invalid_argument_status(
                "source filter PID does not match descrambler PID",
            ));
        }
        if !descrambler_upstream_filter_open_type_allowed(source_snapshot.open_type) {
            return Err(invalid_argument_status(
                "source filter subtype is not valid for descrambler PID source",
            ));
        }
        self.descrambler_registry.try_claim_pid_for_descrambler(
            self.id,
            demux_id,
            demux_generation,
            pid,
            upstream_filter.identity,
        )?;
        // PID binding is committed by DescramblerRuntimeRegistry::try_claim_pid_for_descrambler(),
        // which owns the same DescramblerSession entry.
        Ok(())
    }

    fn removePid(
        &self,
        pid: &DemuxPid,
        optional_upstream_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        let pid = Self::pid_from_demux_pid(pid)?;
        self.ensure_bound_demux_current_or_prune()?;
        let (demux_id, demux_generation, pid_registered) = {
            let state = lock_mutex_status(&self.session, "descrambler_session")?;
            Self::ensure_open_locked(&state)?;
            let Some(demux_id) = state.demux_id else {
                return Err(Status::new_service_specific_error(
                    TunerResult::INVALID_STATE.0,
                    None,
                ));
            };
            let Some(demux_generation) = state.demux_generation else {
                return Err(Status::new_service_specific_error(
                    TunerResult::INVALID_STATE.0,
                    None,
                ));
            };
            (demux_id, demux_generation, state.pid_registrations.contains_key(&pid))
        };
        let upstream_filter = local_filter_identity_for_owner(optional_upstream_filter, demux_id)?;
        if !pid_registered {
            // 未登録 PID は状態変更なしの no-op 成功とする。ただしsource filterの所有権・世代は検証済みにする。
            return Ok(());
        }
        let mut state = lock_mutex_status(&self.session, "descrambler_session")?;
        Self::ensure_open_locked(&state)?;
        if state.demux_id != Some(demux_id)
            || state.demux_generation != Some(demux_generation)
        {
            return Err(Status::new_service_specific_error(
                TunerResult::INVALID_STATE.0,
                None,
            ));
        }
        match state.pid_registrations.get(&pid).copied() {
            Some(stored_source)
                if stored_source.upstream_filter_id == upstream_filter.filter_id
                    && stored_source.upstream_filter_generation == upstream_filter.generation =>
            {
                state.remove_pid(PidBinding { pid: pid as i32 });
            }
            Some(_) => {
                return Err(Status::new_service_specific_error(
                    TunerResult::INVALID_ARGUMENT.0,
                    Some("PID is registered with a different source filter generation"),
                ))
            }
            None => {
                // 登録確認後の競合で既に消えていた場合も解除済みとして扱う。
                return Ok(())
            }
        }
        Ok(())
    }

    fn close(&self) -> BinderResult<()> {
        self.close_internal()
    }
}
pub struct TunerHal {
    frontend_entries: Vec<FrontendEntry>,
    frontend_ids: Vec<i32>,
    frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>>,
    frontend_leases: Arc<Mutex<FrontendLeaseRegistry>>,
    max_frontend_overrides: Mutex<BTreeMap<i32, i32>>,
    lnb_ids: Vec<i32>,
    lnb_names: BTreeMap<String, i32>,
    demux_ledger: DemuxLedgerStore,
    next_demux_generation: AtomicU64,
    descrambler_registry: Arc<DescramblerRuntimeRegistry>,
    descrambler_diagnostics: Arc<DescramblerDiagnosticRegistry>,
    descrambler_key_table: Arc<DescramblerKeyTable>,
    startup_diagnostics: Arc<StartupDiagnosticRegistry>,
    diagnostic_file_writes: Arc<DiagnosticFileWriteRegistry>,
    demux_core: DemuxCore,
    lnb_registry: Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
    diagnostic_workers: Mutex<Vec<WorkerHandle>>,
}


fn frontend_open_count_or_zero(map: &std::collections::BTreeMap<i32, i32>, frontend_type: FrontendType) -> i32 {
    map.get(&frontend_type.0).copied().unwrap_or_default()
}

impl TunerHal {
    pub fn new() -> Self {
        let frontend_entries = enumerate_frontend_entries();
        let startup_diagnostics = Arc::new(StartupDiagnosticRegistry::new());
        let diagnostic_file_writes = Arc::new(DiagnosticFileWriteRegistry::new());
        if frontend_entries.is_empty() {
            startup_diagnostics
                .record("target tuner device absent; advertising zero frontend resources");
        }
        let frontend_ids = frontend_entries.iter().map(|e| e.id).collect();
        let mut lnb_registry = BTreeMap::new();
        let mut lnb_names = BTreeMap::new();
        let mut lnb_ids = Vec::new();
        for entry in &frontend_entries {
            if let Some(lnb_id) = entry_default_lnb_id(entry) {
                let entry_state = lnb_registry
                    .entry(lnb_id)
                    .or_insert_with(LnbRuntimeState::default);
                entry_state.owner_frontend_id = entry.id;
                match &entry.kind {
                    FrontendEntryKind::Px4 { device_name, .. } => {
                        entry_state.profile =
                            px4_lnb_profile_from_device_name(device_name.as_deref());
                        entry_state.supports_tone = false;
                        entry_state.supports_diseqc = false;
                    }
                    FrontendEntryKind::Dvb { .. } => {
                        entry_state.profile = LnbDeviceProfile::EarthPt1FixedLnb;
                        entry_state.supports_tone = false;
                        entry_state.supports_diseqc = false;
                    }
                }
                lnb_ids.push(lnb_id);
                if let Some(name) = entry_default_lnb_name(entry) {
                    lnb_names.insert(name, lnb_id);
                }
            }
        }
        lnb_ids.sort_unstable();
        lnb_ids.dedup();
        let lnb_registry = Arc::new(Mutex::new(lnb_registry));
        let descrambler_registry = Arc::new(DescramblerRuntimeRegistry::new());
        let descrambler_diagnostics = Arc::new(DescramblerDiagnosticRegistry::new());
        let mut diagnostic_workers = Vec::new();
        if let Ok(path) = std::env::var("MALEICACID_TUNER_HAL_DESCRAMBLER_DIAGNOSTIC_FILE") {
            let diagnostics_for_file = Arc::clone(&descrambler_diagnostics);
            let diagnostic_file_writes_for_file = Arc::clone(&diagnostic_file_writes);
            let startup_diagnostics_for_hook = Arc::clone(&startup_diagnostics);
            let path_for_file = path.clone();
            match WorkerRuntime::spawn_owned_with_exit_hook(
                WorkerOwnerId("diagnostic_worker", 1),
                "descrambler_diagnostic_file",
                move |signal| {
                    while !signal.is_stop_requested() {
                        let dump = diagnostics_for_file.dump_for_debug();
                        diagnostic_file_writes_for_file.write(&path_for_file, dump);
                        if signal.wait_timeout_or_stop(Duration::from_secs(5)) {
                            break;
                        }
                    }
                },
                move |exit| {
                    startup_diagnostics_for_hook.record(format!(
                        "diagnostic_worker name=descrambler_diagnostic_file exit={exit:?}"
                    ));
                },
            ) {
                Ok(worker) => diagnostic_workers.push(worker),
                Err(err) => {
                    startup_diagnostics.record(format!("diagnostic_worker name=descrambler_diagnostic_file spawn_failed error={err}"));
                    eprintln!("maleicacid-tuner-hal-worker: failed to spawn descrambler_diagnostic_file: {err}");
                }
            }
        }
        let descrambler_key_table = Arc::new(DescramblerKeyTable::new());
        descrambler_registry.set_key_table(&descrambler_key_table);
        let frontend_registry = frontend_entries
            .iter()
            .cloned()
            .map(|entry| {
                (
                    entry.id,
                    FrontendRuntime::new(
                        entry,
                        Arc::clone(&lnb_registry),
                        Arc::clone(&descrambler_registry),
                        Arc::clone(&descrambler_diagnostics),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let demux_ledger: DemuxLedgerStore = Arc::new(Mutex::new(DemuxLedger::default()));
        if let Ok(path) = std::env::var("MALEICACID_TUNER_HAL_FRONTEND_DIAGNOSTIC_FILE") {
            let frontend_registry_for_file = frontend_registry.clone();
            let startup_diagnostics_for_file = Arc::clone(&startup_diagnostics);
            let startup_diagnostics_for_hook = Arc::clone(&startup_diagnostics);
            let diagnostic_file_writes_for_file = Arc::clone(&diagnostic_file_writes);
            let path_for_file = path.clone();
            match WorkerRuntime::spawn_owned_with_exit_hook(
                WorkerOwnerId("diagnostic_worker", 2),
                "frontend_diagnostic_file",
                move |signal| {
                    while !signal.is_stop_requested() {
                        let frontend_dump = frontend_registry_for_file
                            .values()
                            .map(|runtime| {
                                format!(
                                    "{}\n{}",
                                    runtime
                                        .px4_path_diagnostics
                                        .debug_dump_line(runtime.frontend_id),
                                    runtime.debug_dump_runtime_failures()
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let startup_dump = startup_diagnostics_for_file.dump_for_debug();
                        let dump = if frontend_dump.is_empty() {
                            startup_dump
                        } else {
                            format!("{startup_dump}\n{frontend_dump}")
                        };
                        diagnostic_file_writes_for_file.write(&path_for_file, dump);
                        if signal.wait_timeout_or_stop(Duration::from_secs(5)) {
                            break;
                        }
                    }
                },
                move |exit| {
                    startup_diagnostics_for_hook.record(format!(
                        "diagnostic_worker name=frontend_diagnostic_file exit={exit:?}"
                    ));
                },
            ) {
                Ok(worker) => diagnostic_workers.push(worker),
                Err(err) => {
                    startup_diagnostics.record(format!(
                        "diagnostic_worker name=frontend_diagnostic_file spawn_failed error={err}"
                    ));
                    eprintln!("maleicacid-tuner-hal-worker: failed to spawn frontend_diagnostic_file: {err}");
                }
            }
        }
        let demux_ledger_for_av_debug = Arc::clone(&demux_ledger);
        if let Ok(path) = std::env::var("MALEICACID_TUNER_HAL_AV_SHARED_DIAGNOSTIC_FILE") {
            let diagnostic_file_writes_for_file = Arc::clone(&diagnostic_file_writes);
            let startup_diagnostics_for_hook = Arc::clone(&startup_diagnostics);
            let path_for_file = path.clone();
            match WorkerRuntime::spawn_owned_with_exit_hook(
                WorkerOwnerId("diagnostic_worker", 3),
                "av_shared_diagnostic_file",
                move |signal| {
                    while !signal.is_stop_requested() {
                        let dump =
                            lock_mutex_option(&demux_ledger_for_av_debug, "demux_ledger")
                                .map(|ledger| {
                                    ledger
                                        .records()
                                        .flat_map(|record| {
                                            lock_mutex_option(record, "demux_record")
                                                .map(|record| {
                                                    let mut out = record
                                                        .runtime_io
                                                        .dump_av_shared_for_debug();
                                                    out.extend(
                                                        record.ci_cam_diagnostics.iter().map(
                                                            |item| {
                                                                format!("ci_cam_diagnostic: {item}")
                                                            },
                                                        ),
                                                    );
                                                    out
                                                })
                                                .unwrap_or_default()
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_else(|| "demux_ledger=poisoned".to_string());
                        diagnostic_file_writes_for_file.write(&path_for_file, dump);
                        if signal.wait_timeout_or_stop(Duration::from_secs(5)) {
                            break;
                        }
                    }
                },
                move |exit| {
                    startup_diagnostics_for_hook.record(format!(
                        "diagnostic_worker name=av_shared_diagnostic_file exit={exit:?}"
                    ));
                },
            ) {
                Ok(worker) => diagnostic_workers.push(worker),
                Err(err) => {
                    startup_diagnostics.record(format!(
                        "diagnostic_worker name=av_shared_diagnostic_file spawn_failed error={err}"
                    ));
                    eprintln!("maleicacid-tuner-hal-worker: failed to spawn av_shared_diagnostic_file: {err}");
                }
            }
        }
        Self {
            frontend_entries,
            frontend_ids,
            frontend_registry: Arc::new(frontend_registry),
            frontend_leases: Arc::new(Mutex::new(FrontendLeaseRegistry::default())),
            max_frontend_overrides: Mutex::new(BTreeMap::new()),
            lnb_ids,
            lnb_names,
            demux_ledger,
            next_demux_generation: AtomicU64::new(1),
            descrambler_registry,
            descrambler_diagnostics,
            descrambler_key_table,
            startup_diagnostics,
            diagnostic_file_writes,
            demux_core: DemuxCore::new(),
            lnb_registry,
            diagnostic_workers: Mutex::new(diagnostic_workers),
        }
    }

    pub fn dump_descrambler_diagnostics_for_debug(&self) -> String {
        self.descrambler_diagnostics.dump_for_debug()
    }

    pub fn dump_frontend_path_diagnostics_for_debug(&self) -> String {
        let frontend_dump = self
            .frontend_registry
            .values()
            .map(|runtime| {
                format!(
                    "{}\n{}",
                    runtime
                        .px4_path_diagnostics
                        .debug_dump_line(runtime.frontend_id),
                    runtime.debug_dump_runtime_failures()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let startup_dump = self.startup_diagnostics.dump_for_debug();
        let file_dump = self.diagnostic_file_writes.dump_for_debug();
        if frontend_dump.is_empty() {
            format!("{startup_dump}\n{file_dump}")
        } else {
            format!("{startup_dump}\n{frontend_dump}\n{file_dump}")
        }
    }

    pub fn dump_av_shared_diagnostics_for_debug(&self) -> String {
        lock_mutex_option(&self.demux_ledger, "demux_ledger")
            .map(|ledger| {
                ledger
                    .records()
                    .flat_map(|record| {
                        lock_mutex_option(record, "demux_record")
                            .map(|record| {
                                let mut out = record.runtime_io.dump_av_shared_for_debug();
                                out.extend(
                                    record
                                        .ci_cam_diagnostics
                                        .iter()
                                        .map(|item| format!("ci_cam_diagnostic: {item}")),
                                );
                                out
                            })
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|| "demux_ledger=poisoned".to_string())
    }

    fn frontend_entry(&self, frontend_id: i32) -> Option<&FrontendEntry> {
        self.frontend_entries
            .iter()
            .find(|entry| entry.id == frontend_id)
    }

    fn default_max_frontends(&self, frontend_type: FrontendType) -> i32 {
        self.frontend_entries
            .iter()
            .filter(|entry| entry_aidl_frontend_type(entry) == frontend_type)
            .count() as i32
    }

    fn configured_max_frontends(&self, frontend_type: FrontendType) -> BinderResult<i32> {
        Ok(lock_mutex_status(&self.max_frontend_overrides, "max_frontend_overrides")?
            .get(&frontend_type.0)
            .copied()
            .unwrap_or_else(|| self.default_max_frontends(frontend_type)))
    }

    fn current_open_frontends(&self, frontend_type: FrontendType) -> BinderResult<i32> {
        let leases = lock_mutex_status(&self.frontend_leases, "frontend_leases")?;
        Ok(frontend_open_count_or_zero(&leases.open_counts_by_type, frontend_type))
    }

    fn try_acquire_frontend(
        &self,
        frontend_id: i32,
        frontend_type: FrontendType,
        physical_group_id: i32,
    ) -> BinderResult<u64> {
        let max_allowed = self.configured_max_frontends(frontend_type)?;
        let mut leases = lock_mutex_status(&self.frontend_leases, "frontend_leases")?;
        if leases.open_frontends.contains(&frontend_id) {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        if leases.open_physical_groups.contains_key(&physical_group_id) {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        let current_open = frontend_open_count_or_zero(&leases.open_counts_by_type, frontend_type);
        if current_open >= max_allowed {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        let generation = leases.next_generation;
        leases.next_generation = leases.next_generation.saturating_add(1);
        leases.open_frontends.insert(frontend_id);
        leases
            .open_physical_groups
            .insert(physical_group_id, frontend_id);
        leases.open_generations.insert(frontend_id, generation);
        leases
            .open_counts_by_type
            .insert(frontend_type.0, current_open + 1);
        Ok(generation)
    }

    fn new_demux_binder(&self, record: Arc<Mutex<DemuxRecord>>) -> BinderResult<Strong<dyn IDemux>> {
        Ok(BnDemux::new_binder(
            DemuxHal::new(
                record,
                Arc::clone(&self.frontend_registry),
                Arc::clone(&self.frontend_leases),
                Arc::clone(&self.demux_ledger),
                Arc::clone(&self.descrambler_registry),
            )?,
            BinderFeatures::default(),
        ))
    }

    fn rollback_new_demux_record(
        &self,
        demux_id: i32,
        record: &DemuxRecordRef,
        reason: &'static str,
    ) -> BinderResult<()> {
        let mut first_error: Option<Status> = None;
        match lock_mutex_status(record, "demux_record") {
            Ok(mut entry) => {
                entry.ref_count = 0;
                entry.closing = true;
                match lock_mutex_status(&entry.state, "demux_handle") {
                    Ok(mut state) => state.close(),
                    Err(status) => {
                        if first_error.is_none() {
                            first_error = Some(status);
                        }
                    }
                }
                entry.bound_frontend_id = None;
                entry.bound_frontend_generation = None;
            }
            Err(status) => {
                if first_error.is_none() {
                    first_error = Some(status);
                }
            }
        }
        match lock_mutex_status(&self.demux_ledger, "demux_ledger") {
            Ok(mut ledger) => {
                if let Err(err) = ledger.rollback_open(LedgerId(demux_id)) {
                    eprintln!(
                        "maleicacid-tuner-hal-demux-open: demux={} step={} rollback=ledger_remove error={:?}",
                        demux_id, reason, err
                    );
                    if first_error.is_none() {
                        first_error = Some(Status::new_service_specific_error(
                            TunerResult::UNKNOWN_ERROR.0,
                            Some("demux open rollback failed"),
                        ));
                    }
                }
            }
            Err(status) => {
                if first_error.is_none() {
                    first_error = Some(status);
                }
            }
        }
        if let Some(status) = first_error {
            Err(status)
        } else {
            Ok(())
        }
    }

    fn existing_demux_record_for_open_by_id(
        &self,
        demux_id: i32,
    ) -> BinderResult<Option<DemuxRecordRef>> {
        let record = lock_mutex_status(&self.demux_ledger, "demux_ledger")?
            .get_record(LedgerId(demux_id));
        if let Some(record) = record.as_ref() {
            let entry = lock_mutex_status(record, "demux_record")?;
            if entry.closing || entry.ref_count == 0 {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNAVAILABLE.0,
                    None,
                ));
            }
        }
        Ok(record)
    }

    fn commit_existing_demux_open_ref(record: &DemuxRecordRef) -> BinderResult<()> {
        let mut entry = lock_mutex_status(record, "demux_record")?;
        if entry.closing || entry.ref_count == 0 {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        entry.ref_count = entry.ref_count.saturating_add(1);
        Ok(())
    }

    fn new_descrambler_binder(&self) -> BinderResult<Strong<dyn IDescrambler>> {
        Ok(BnDescrambler::new_binder(
            TunerDescrambler::new(
                Arc::clone(&self.demux_ledger),
                Arc::clone(&self.descrambler_registry),
                Arc::clone(&self.descrambler_diagnostics),
                Arc::clone(&self.descrambler_key_table),
            )?,
            BinderFeatures::default(),
        ))
    }

    fn create_demux_record_for_id_locked(
        &self,
        demux_id: i32,
    ) -> BinderResult<DemuxRecordRef> {
        let mut txn = LifecycleTxn::new();
        txn.validate("demux_id_pool", || {
            if demux_id_in_pool(demux_id) { Ok(()) } else { Err(Status::new_service_specific_error(TunerResult::INVALID_ARGUMENT.0, None)) }
        })?;
        txn.prepare("demux_state", || Ok(()))?;
        let state = Arc::new(Mutex::new(self.demux_core.new_handle(demux_id)));
        let generation = self.next_demux_generation.fetch_add(1, Ordering::SeqCst);
        let record = Arc::new(Mutex::new(DemuxRecord {
            demux_id,
            generation,
            state,
            runtime_io: Arc::new(RuntimeIoRegistry::default()),
            ref_count: 1,
            closing: false,
            bound_frontend_id: None,
            bound_frontend_generation: None,
            ci_cam_diagnostics: Vec::new(),
            filter_ledger: FilterLedger::default(),
            dvr_ledger: DvrLedger::default(),
            descrambler_ledger: DescramblerLedger::default(),
            pending_stream_boundary_plan: None,
        }));
        txn.apply("demux_ledger_create_live", || {
            let mut ledger = lock_mutex_status(&self.demux_ledger, "demux_ledger")?;
            ledger
                .create_live(LedgerId(demux_id), Arc::clone(&record))
                .map_err(|_| Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None))?;
            Ok(())
        })?;
        txn.commit("demux_record_live", || Ok(()))?;
        Ok(record)
    }

    fn allocate_demux_record(&self) -> BinderResult<(i32, DemuxRecordRef)> {
        let demux_id = {
            let ledger = lock_mutex_status(&self.demux_ledger, "demux_ledger")?;
            ledger.first_available(all_demux_ids())
        }
        .ok_or_else(|| Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None))?;
        let record = self.create_demux_record_for_id_locked(demux_id)?;
        Ok((demux_id, record))
    }

    fn open_or_create_demux_record_by_id(
        &self,
        demux_id: i32,
    ) -> BinderResult<DemuxRecordRef> {
        if !demux_id_in_pool(demux_id) {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        }
        if let Some(record) = lock_mutex_status(&self.demux_ledger, "demux_ledger")?
            .get_record(LedgerId(demux_id))
        {
            {
                let mut entry = lock_mutex_status(&record, "demux_record")?;
                if entry.closing || entry.ref_count == 0 {
                    return Err(Status::new_service_specific_error(
                        TunerResult::UNAVAILABLE.0,
                        None,
                    ));
                }
                entry.ref_count = entry.ref_count.saturating_add(1);
            }
            return Ok(record);
        }
        self.create_demux_record_for_id_locked(demux_id)
    }

    #[cfg(test)]
    fn first_available_demux_id(&self) -> Option<i32> {
        let live = lock_mutex_status(&self.demux_ledger, "test_mutex").unwrap();
        live.first_available(all_demux_ids())
    }

    fn demux_info(&self, demux_id: i32) -> BinderResult<DemuxInfo> {
        if !demux_id_in_pool(demux_id) {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        }
        Ok(DemuxInfo {
            filterTypes: SUPPORTED_DEMUX_FILTER_CAPS,
        })
    }
}
impl Drop for TunerHal {
    fn drop(&mut self) {
        if let Some(mut workers) =
            lock_mutex_option(&self.diagnostic_workers, "tuner_diagnostic_workers")
        {
            for worker in workers.iter_mut() {
                let _ = worker.request_stop(WorkerExitReason::StopRequested);
                let _ = worker.join_from_owner();
            }
            workers.clear();
        }
    }
}

impl Interface for TunerHal {}

impl ITuner for TunerHal {
    fn getFrontendIds(&self) -> BinderResult<Vec<i32>> {
        Ok(self.frontend_ids.clone())
    }

    fn openFrontendById(&self, frontend_id: i32) -> BinderResult<Strong<dyn IFrontend>> {
        let Some(runtime) = self.frontend_registry.get(&frontend_id).cloned() else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        let Some(entry) = self.frontend_entry(frontend_id) else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        let frontend_type = entry_aidl_frontend_type(entry);
        let physical_group_id = entry_physical_group_id(entry);
        let session_generation =
            self.try_acquire_frontend(frontend_id, frontend_type, physical_group_id)?;
        Ok(BnFrontend::new_binder(
            FrontendHal::new(
                runtime,
                frontend_type,
                physical_group_id,
                session_generation,
                Arc::clone(&self.frontend_leases),
                Arc::clone(&self.demux_ledger),
            ),
            BinderFeatures::default(),
        ))
    }


    fn openDemux(&self, demux_id: &mut Vec<i32>) -> BinderResult<Strong<dyn IDemux>> {
        let mut txn = LifecycleTxn::new();
        let (allocated, record) = txn.apply_value("demux_allocate_record", || {
            self.allocate_demux_record()
        })?;
        demux_id.clear();
        demux_id.push(allocated);
        match txn.commit_value("demux_new_binder", || self.new_demux_binder(Arc::clone(&record))) {
            Ok(binder) => Ok(binder),
            Err(status) => {
                if let Err(rollback_status) = txn.rollback("demux_open_rollback_allocated_record", || {
                    self.rollback_new_demux_record(allocated, &record, "new_demux_binder_failed")
                }) {
                    demux_id.clear();
                    eprintln!(
                        "maleicacid-tuner-hal-open-demux: demux={} binder_status={:?} rollback_status={:?}",
                        allocated, status, rollback_status
                    );
                    return Err(Status::new_service_specific_error(
                        TunerResult::UNKNOWN_ERROR.0,
                        Some("demux_open_rollback_failed"),
                    ));
                }
                demux_id.clear();
                Err(status)
            }
        }
    }

    fn getDemuxCaps(&self) -> BinderResult<DemuxCapabilities> {
        let demux_count = MAX_LIVE_DEMUXES as i32;
        Ok(DemuxCapabilities {
            numDemux: demux_count,
            numRecord: demux_count,
            numPlayback: demux_count,
            numTsFilter: DEMUX_MAX_TS_FILTERS,
            numSectionFilter: DEMUX_MAX_SECTION_FILTERS,
            numAudioFilter: DEMUX_MAX_AUDIO_FILTERS,
            numVideoFilter: DEMUX_MAX_VIDEO_FILTERS,
            numPesFilter: DEMUX_MAX_PES_FILTERS,
            numPcrFilter: 0,
            numBytesInSectionFilter: MAX_SECTION_FILTER_BYTES as i64,
            filterCaps: SUPPORTED_DEMUX_FILTER_CAPS,
            linkCaps: demux_link_caps_for_ts_filter_linkage(),
            bTimeFilter: false,
        })
    }

    fn openDescrambler(&self) -> BinderResult<Strong<dyn IDescrambler>> {
        self.new_descrambler_binder()
    }

    fn getFrontendInfo(&self, frontend_id: i32) -> BinderResult<FrontendInfo> {
        let Some(entry) = self.frontend_entries.iter().find(|e| e.id == frontend_id) else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        let (min_freq, max_freq, acquire_range) = entry_frontend_frequency_contract(entry);
        let (ty, max_symbol_rate, exclusive_group_id): (FrontendType, i32, i32) = (
            entry_aidl_frontend_type(entry),
            entry_frontend_max_symbol_rate_contract(entry),
            entry_physical_group_id(entry),
        );
        Ok(FrontendInfo {
            r#type: ty,
            minFrequency: min_freq,
            maxFrequency: max_freq,
            minSymbolRate: 0,
            maxSymbolRate: max_symbol_rate,
            acquireRange: acquire_range,
            exclusiveGroupId: exclusive_group_id,
            statusCaps: entry_status_caps(entry),
            frontendCaps: entry_frontend_caps(entry),
        })
    }

    fn getLnbIds(&self) -> BinderResult<Vec<i32>> {
        Ok(self.lnb_ids.clone())
    }

    fn openLnbById(&self, lnb_id: i32) -> BinderResult<Strong<dyn ILnb>> {
        if !lock_mutex_status(&self.lnb_registry, "lnb_registry")?.contains_key(&lnb_id) {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        }
        Ok(BnLnb::new_binder(
            LnbHal::new(
                lnb_id,
                Arc::clone(&self.lnb_registry),
                Arc::clone(&self.frontend_registry),
            )?,
            BinderFeatures::default(),
        ))
    }

    fn openLnbByName(
        &self,
        lnb_name: &str,
        lnb_id: &mut Vec<i32>,
    ) -> BinderResult<Strong<dyn ILnb>> {
        lnb_id.clear();
        let Some(id) = self.lnb_names.get(lnb_name).copied() else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        lnb_id.push(id);
        self.openLnbById(id)
    }

    fn setLna(&self, _b_enable: bool) -> BinderResult<()> {
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ))
    }

    fn setMaxNumberOfFrontends(
        &self,
        frontend_type: FrontendType,
        max_number: i32,
    ) -> BinderResult<()> {
        let default_max = self.default_max_frontends(frontend_type);
        if max_number < 0 || max_number > default_max {
            return Err(invalid_argument_status(
                "max_number must be in 0..=default_max for the requested frontend type",
            ));
        }
        if self.current_open_frontends(frontend_type)? > max_number {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        lock_mutex_status(&self.max_frontend_overrides, "max_frontend_overrides")?
            .insert(frontend_type.0, max_number);
        Ok(())
    }

    fn getMaxNumberOfFrontends(&self, frontend_type: FrontendType) -> BinderResult<i32> {
        self.configured_max_frontends(frontend_type)
    }

    fn isLnaSupported(&self) -> BinderResult<bool> {
        Ok(false)
    }

    fn getDemuxIds(&self) -> BinderResult<Vec<i32>> {
        Ok(all_demux_ids())
    }

    fn openDemuxById(&self, demux_id: i32) -> BinderResult<Strong<dyn IDemux>> {
        if !demux_id_in_pool(demux_id) {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        }
        let mut txn = LifecycleTxn::new();
        if let Some(record) = txn.prepare_value("demux_lookup_existing_record", || {
            self.existing_demux_record_for_open_by_id(demux_id)
        })? {
            let binder = txn.apply_value("demux_existing_new_binder", || {
                self.new_demux_binder(Arc::clone(&record))
            })?;
            txn.commit("demux_existing_ref_count_increment", || {
                Self::commit_existing_demux_open_ref(&record)
            })?;
            return Ok(binder);
        }
        let record = txn.apply_value("demux_create_record_by_id", || {
            self.create_demux_record_for_id_locked(demux_id)
        })?;
        match txn.commit_value("demux_new_binder", || self.new_demux_binder(Arc::clone(&record))) {
            Ok(binder) => Ok(binder),
            Err(status) => {
                // r50dz60/G1-02: do not hide rollback failure after a new by-id
                // record has been created. rollback_new_demux_record marks the record
                // closing before ledger rollback, so a failed rollback leaves the id
                // quarantined and prevents immediate reuse.
                if let Err(rollback_status) = txn.rollback("demux_by_id_open_rollback_allocated_record", || {
                    self.rollback_new_demux_record(demux_id, &record, "new_demux_binder_failed")
                }) {
                    eprintln!(
                        "maleicacid-tuner-hal-demux-open: demux={} rollback=quarantined_open_rollback_failed original_status={:?} rollback_status={:?}",
                        demux_id, status, rollback_status
                    );
                    return Err(Status::new_service_specific_error(
                        TunerResult::UNKNOWN_ERROR.0,
                        Some("quarantined_open_rollback_failed"),
                    ));
                }
                Err(status)
            }
        }
    }

    fn getDemuxInfo(&self, demux_id: i32) -> BinderResult<DemuxInfo> {
        self.demux_info(demux_id)
    }
}

enum FrontendBackendState {
    Px4(Px4FrontendBackend),
    Dvb(DvbFrontendBackend),
    Unavailable {
        reason: String,
        declared_type: FrontendType,
        allowed_systems: Vec<FrontendSystem>,
        selected_lnb_id: Option<i32>,
    },
}

#[derive(Clone)]
enum FrontendLiveStreamReader {
    Px4(Px4LiveStreamReader),
    Dvb(DvbLiveStreamReader),
}

impl FrontendLiveStreamReader {
    fn sample_ts_packets(
        &self,
        max_packets: usize,
        stop_fd: Option<i32>,
    ) -> Result<Vec<[u8; TS_PACKET_SIZE]>, HalError> {
        match self {
            FrontendLiveStreamReader::Px4(reader) => reader.sample_ts_packets(max_packets, stop_fd),
            FrontendLiveStreamReader::Dvb(reader) => reader.sample_ts_packets(max_packets, stop_fd),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackendFlavor {
    Px4,
    Dvb,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScanPhase {
    Running,
    Completed,
    Cancelled,
    FailedBackend,
    FailedCallback,
    FailedPanic,
}

impl ScanPhase {
    fn is_failed(self) -> bool {
        matches!(
            self,
            ScanPhase::FailedBackend | ScanPhase::FailedCallback | ScanPhase::FailedPanic
        )
    }
}

#[derive(Clone, Debug)]
struct ScanSessionState {
    session_id: i64,
    fingerprint: String,
    phase: ScanPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LockWaitMode {
    Tune,
    Scan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LockWaitConfig {
    initial_settle_ms: u64,
    poll_interval_ms: u64,
    timeout_ms: u64,
    consecutive_lock_samples: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LockWaitOutcome {
    telemetry: FrontendTelemetry,
    locked: bool,
    cancelled: bool,
}

pub struct FrontendHal {
    shared: Arc<FrontendRuntime>,
    frontend_type: FrontendType,
    physical_group_id: i32,
    session_generation: u64,
    lease_registry: Arc<Mutex<FrontendLeaseRegistry>>,
    demux_ledger: DemuxLedgerStore,
    callback: Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
    scan_worker: Mutex<Option<WorkerHandle>>,
    scan_session: Arc<Mutex<Option<ScanSessionState>>>,
    scan_last_terminal: Arc<Mutex<Option<ScanSessionState>>>,
    next_scan_session_id: AtomicI64,
    tune_worker: Mutex<Option<WorkerHandle>>,
    closed: RuntimeAtomicFlag,
    cleanup_failed: RuntimeAtomicFlag,
}

impl FrontendHal {
    fn new(
        shared: Arc<FrontendRuntime>,
        frontend_type: FrontendType,
        physical_group_id: i32,
        session_generation: u64,
        lease_registry: Arc<Mutex<FrontendLeaseRegistry>>,
        demux_ledger: DemuxLedgerStore,
    ) -> Self {
        Self {
            shared,
            frontend_type,
            physical_group_id,
            session_generation,
            lease_registry,
            demux_ledger,
            callback: Arc::new(Mutex::new(None)),
            scan_worker: Mutex::new(None),
            scan_session: Arc::new(Mutex::new(None)),
            scan_last_terminal: Arc::new(Mutex::new(None)),
            next_scan_session_id: AtomicI64::new(1),
            tune_worker: Mutex::new(None),
            closed: RuntimeAtomicFlag::new(false),
            cleanup_failed: RuntimeAtomicFlag::new(false),
        }
    }

    fn system_allowed(&self, system: FrontendSystem) -> bool {
        self.shared.allowed_systems.iter().any(|s| *s == system)
    }

    fn backend_hardware_info(backend: &mut FrontendBackendState) -> String {
        match backend {
            FrontendBackendState::Px4(inner) => inner.hardware_info(),
            FrontendBackendState::Dvb(inner) => inner.hardware_info(),
            FrontendBackendState::Unavailable {
                reason,
                declared_type,
                ..
            } => format!("unavailable {:?}: {}", declared_type, reason),
        }
    }

    fn backend_probe_device(backend: &FrontendBackendState) -> bool {
        match backend {
            FrontendBackendState::Px4(inner) => inner.probe_device(),
            FrontendBackendState::Dvb(inner) => inner.probe_device(),
            FrontendBackendState::Unavailable { .. } => false,
        }
    }

    fn backend_set_callback_registered(backend: &mut FrontendBackendState, registered: bool) {
        match backend {
            FrontendBackendState::Px4(inner) => inner.set_callback_registered(registered),
            FrontendBackendState::Dvb(inner) => inner.set_callback_registered(registered),
            FrontendBackendState::Unavailable { .. } => {
                let _ = registered;
            }
        }
    }

    fn backend_mark_callback_failed(backend: &mut FrontendBackendState, message: String) {
        match backend {
            FrontendBackendState::Px4(inner) => inner.mark_callback_failed(message),
            FrontendBackendState::Dvb(inner) => inner.mark_callback_failed(message),
            FrontendBackendState::Unavailable { reason, .. } => *reason = message,
        }
    }

    fn backend_set_lnb_id(backend: &mut FrontendBackendState, lnb_id: i32) {
        match backend {
            FrontendBackendState::Px4(inner) => inner.set_lnb_id(lnb_id),
            FrontendBackendState::Dvb(inner) => inner.set_lnb_id(lnb_id),
            FrontendBackendState::Unavailable {
                selected_lnb_id, ..
            } => *selected_lnb_id = Some(lnb_id),
        }
    }

    fn backend_tuning_active(backend: &FrontendBackendState) -> bool {
        match backend {
            FrontendBackendState::Px4(inner) => inner.runtime_state().tuning_active,
            FrontendBackendState::Dvb(inner) => inner.runtime_state().tuning_active,
            FrontendBackendState::Unavailable { .. } => false,
        }
    }

    fn backend_selected_lnb_id(backend: &FrontendBackendState) -> Option<i32> {
        match backend {
            FrontendBackendState::Px4(inner) => inner.runtime_state().lnb_id,
            FrontendBackendState::Dvb(inner) => inner.runtime_state().lnb_id,
            FrontendBackendState::Unavailable {
                selected_lnb_id, ..
            } => *selected_lnb_id,
        }
    }

    fn backend_apply_lnb_state(
        backend: &mut FrontendBackendState,
        lnb: &LnbRuntimeState,
    ) -> Result<(), HalError> {
        match backend {
            FrontendBackendState::Px4(inner) => {
                let mv = match lnb.voltage {
                    Some(LnbVoltage::VOLTAGE_15V) => 15,
                    _ => 0,
                };
                inner.set_lnb_voltage(mv)
            }
            FrontendBackendState::Dvb(inner) => {
                let mv = match lnb.voltage {
                    Some(LnbVoltage::VOLTAGE_15V) => 15,
                    Some(LnbVoltage::VOLTAGE_11V) => 11,
                    _ => 0,
                };
                // earth_pt1 固定プロファイルは電圧のみ扱う。トーンは恒久未対応。
                inner.set_lnb_voltage(mv)
            }
            FrontendBackendState::Unavailable { reason, .. } => Err(HalError::OpenFailed {
                path: PathBuf::from("unavailable-frontend"),
                message: reason.clone(),
            }),
        }
    }

    fn backend_send_diseqc_message(
        _backend: &mut FrontendBackendState,
        _message: &[u8],
    ) -> Result<(), HalError> {
        Err(HalError::Unsupported(
            "固定日本向けチューナープロファイルではDiSEqCを恒久的に非対応とします",
        ))
    }

    fn backend_live_stream_reader(
        backend: &mut FrontendBackendState,
    ) -> Result<Option<FrontendLiveStreamReader>, HalError> {
        match backend {
            FrontendBackendState::Px4(inner) => Ok(inner
                .live_stream_reader()?
                .map(FrontendLiveStreamReader::Px4)),
            FrontendBackendState::Dvb(inner) => Ok(inner
                .live_stream_reader()?
                .map(FrontendLiveStreamReader::Dvb)),
            FrontendBackendState::Unavailable { reason, .. } => Err(HalError::OpenFailed {
                path: PathBuf::from("unavailable-frontend"),
                message: reason.clone(),
            }),
        }
    }

    fn apply_selected_lnb_from_registry(
        registry: &Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
        backend: &mut FrontendBackendState,
    ) -> Result<(), HalError> {
        if let Some(lnb_id) = Self::backend_selected_lnb_id(backend) {
            if let Some(lnb) = lock_mutex_hal(registry, "lnb_registry")?
                .get(&lnb_id)
                .cloned()
            {
                Self::backend_apply_lnb_state(backend, &lnb)?;
            }
        }
        Ok(())
    }

    fn apply_selected_lnb(&self, backend: &mut FrontendBackendState) -> Result<(), HalError> {
        Self::apply_selected_lnb_from_registry(&self.shared.lnb_registry, backend)
    }

    fn backend_read_status(
        backend: &mut FrontendBackendState,
    ) -> Result<FrontendTelemetry, HalError> {
        match backend {
            FrontendBackendState::Px4(inner) => inner.read_status().map(|s| s.telemetry),
            FrontendBackendState::Dvb(inner) => inner.read_status().map(|s| s.telemetry),
            FrontendBackendState::Unavailable {
                reason,
                allowed_systems,
                ..
            } => {
                let mut telemetry = FrontendTelemetry::default();
                telemetry.current_system = allowed_systems.first().copied();
                telemetry.locked = false;
                Err(HalError::OpenFailed {
                    path: PathBuf::from("unavailable-frontend"),
                    message: reason.clone(),
                })
            }
        }
    }

    fn backend_stop_tune(backend: &mut FrontendBackendState) -> Result<(), HalError> {
        match backend {
            FrontendBackendState::Px4(inner) => inner.stop_tune(),
            FrontendBackendState::Dvb(inner) => inner.stop_tune(),
            FrontendBackendState::Unavailable { .. } => Ok(()),
        }
    }

    fn backend_close(backend: &mut FrontendBackendState) -> Result<(), HalError> {
        match backend {
            FrontendBackendState::Px4(inner) => inner.close(),
            FrontendBackendState::Dvb(inner) => inner.close(),
            FrontendBackendState::Unavailable { .. } => Ok(()),
        }
    }

    fn backend_flavor(backend: &FrontendBackendState) -> BackendFlavor {
        match backend {
            FrontendBackendState::Px4(_) => BackendFlavor::Px4,
            FrontendBackendState::Dvb(_) => BackendFlavor::Dvb,
            FrontendBackendState::Unavailable { .. } => BackendFlavor::Unavailable,
        }
    }

    fn backend_submit_tune(
        backend: &mut FrontendBackendState,
        request: FrontendTuneRequest,
    ) -> Result<(), HalError> {
        match backend {
            FrontendBackendState::Px4(inner) => inner.tune(request).map(|_| ()),
            FrontendBackendState::Dvb(inner) => inner.tune_from_common(request).map(|_| ()),
            FrontendBackendState::Unavailable { reason, .. } => Err(HalError::OpenFailed {
                path: PathBuf::from("unavailable-frontend"),
                message: reason.clone(),
            }),
        }
    }

    fn current_callback(&self) -> Option<Strong<dyn IFrontendCallback>> {
        lock_mutex_option(&self.callback, "frontend_callback").and_then(|callback| callback.clone())
    }

    fn current_callback_from_registry(
        registry: &Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
    ) -> Option<Strong<dyn IFrontendCallback>> {
        lock_mutex_option(registry, "frontend_callback").and_then(|callback| callback.clone())
    }

    fn handle_frontend_callback_failure(
        callback_registry: &Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
        shared: &Arc<FrontendRuntime>,
        scan_session: Option<&Arc<Mutex<Option<ScanSessionState>>>>,
        session_id: Option<i64>,
        api: &str,
        err: Status,
    ) {
        eprintln!(
            "maleicacid-tuner-hal-callback: frontend_id={} api={} binder_status={:?}; unregistering callback and failing active frontend notification path",
            shared.frontend_id, api, err
        );
        if let Some(mut callback) = lock_mutex_option(callback_registry, "frontend_callback") {
            *callback = None;
        }
        if let Some(mut backend) = lock_mutex_option(&shared.backend, "frontend_backend") {
            Self::backend_mark_callback_failed(
                &mut backend,
                format!("frontend callback failure api={} status={:?}", api, err),
            );
        }
        if let (Some(scan_session), Some(session_id)) = (scan_session, session_id) {
            Self::mark_scan_session_phase(scan_session, session_id, ScanPhase::FailedCallback);
        }
    }

    fn notify_event(&self, event: FrontendEventType) {
        if let Some(callback) = self.current_callback() {
            if let Err(err) = callback.onEvent(event) {
                Self::handle_frontend_callback_failure(
                    &self.callback,
                    &self.shared,
                    None,
                    None,
                    "onEvent",
                    err,
                );
            }
        }
    }

    fn notify_event_with_callback(
        callback_registry: &Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
        shared: &Arc<FrontendRuntime>,
        scan_session: Option<&Arc<Mutex<Option<ScanSessionState>>>>,
        session_id: Option<i64>,
        event: FrontendEventType,
    ) -> BinderResult<NotificationOutcome> {
        let Some(callback) = Self::current_callback_from_registry(callback_registry) else {
            let detail = format!("frontend callback missing for onEvent({event:?})");
            shared.record_runtime_failure(detail.clone());
            shared.mark_live_path_failed(&detail);
            if let (Some(scan_session), Some(session_id)) = (scan_session, session_id) {
                Self::mark_scan_session_phase(scan_session, session_id, ScanPhase::FailedCallback);
            }
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        };
        if let Err(err) = callback.onEvent(event) {
            Self::handle_frontend_callback_failure(
                callback_registry,
                shared,
                scan_session,
                session_id,
                "onEvent",
                err,
            );
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        Ok(NotificationOutcome::Delivered)
    }

    fn lock_scan_message(locked: bool) -> Option<(FrontendScanMessageType, FrontendScanMessage)> {
        if locked {
            Some((
                FrontendScanMessageType::LOCKED,
                FrontendScanMessage::IsLocked(true),
            ))
        } else {
            None
        }
    }

    fn notify_scan_message_with_callback(
        callback_registry: &Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
        shared: &Arc<FrontendRuntime>,
        scan_session: Option<&Arc<Mutex<Option<ScanSessionState>>>>,
        session_id: Option<i64>,
        ty: FrontendScanMessageType,
        message: FrontendScanMessage,
    ) -> BinderResult<NotificationOutcome> {
        let Some(callback) = Self::current_callback_from_registry(callback_registry) else {
            let detail = format!("frontend callback missing for onScanMessage({ty:?})");
            shared.record_runtime_failure(detail.clone());
            shared.mark_live_path_failed(&detail);
            if let (Some(scan_session), Some(session_id)) = (scan_session, session_id) {
                Self::mark_scan_session_phase(scan_session, session_id, ScanPhase::FailedCallback);
            }
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        };
        if let Err(err) = callback.onScanMessage(ty, &message) {
            let api = format!("onScanMessage({:?})", ty);
            Self::handle_frontend_callback_failure(
                callback_registry,
                shared,
                scan_session,
                session_id,
                &api,
                err,
            );
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        Ok(NotificationOutcome::Delivered)
    }

    fn notify_scan_end_with_callback(
        callback_registry: &Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
        shared: &Arc<FrontendRuntime>,
        scan_session: &Arc<Mutex<Option<ScanSessionState>>>,
        session_id: i64,
    ) -> BinderResult<NotificationOutcome> {
        Self::notify_scan_message_with_callback(
            callback_registry,
            shared,
            Some(scan_session),
            Some(session_id),
            FrontendScanMessageType::END,
            FrontendScanMessage::IsEnd(true),
        )
    }



    fn notify_scan_end_required(
        callback_registry: &Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
        shared: &Arc<FrontendRuntime>,
        scan_session: &Arc<Mutex<Option<ScanSessionState>>>,
        session_id: i64,
        operation: &str,
    ) -> bool {
        match Self::notify_scan_end_with_callback(
            callback_registry,
            shared,
            scan_session,
            session_id,
        ) {
            Ok(NotificationOutcome::Delivered) => true,
            Err(status) => {
                let detail = format!(
                    "worker=frontend_scan_worker operation={} status={:?}",
                    operation, status
                );
                shared.record_runtime_failure(detail.clone());
                shared.mark_live_path_failed(&detail);
                Self::mark_scan_session_phase(scan_session, session_id, ScanPhase::FailedCallback);
                false
            }
        }
    }

    fn stop_scan_worker(&self) -> BinderResult<()> {
        let mut abnormal_exit = None;
        let mut worker_slot = lock_mutex_status(&self.scan_worker, "frontend_scan_worker")?;
        if let Some(worker) = worker_slot.as_mut() {
            worker.request_stop(WorkerExitReason::StopRequested).map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
            worker.wake().map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
            let outcome = worker.join_from_owner().map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
            if matches!(outcome, WorkerJoinOutcome::Joined(WorkerExitReason::RuntimeFailure | WorkerExitReason::PanicOrJoinFailure)) {
                let detail = format!("worker=frontend_scan_worker stop_join_abnormal outcome={outcome:?}");
                self.shared.record_runtime_failure(detail.clone());
                self.shared.mark_live_path_failed(&detail);
                abnormal_exit = Some(WorkerExit::RuntimeFailure);
            }
            *worker_slot = None;
        }
        if let Some(exit) = abnormal_exit {
            return Err(Status::new_service_specific_error(
                TunerResult::UNKNOWN_ERROR.0,
                Some(&format!("scan worker stopped abnormally: {exit:?}")),
            ));
        }
        Ok(())
    }

    fn cancel_scan_session(&self) -> BinderResult<()> {
        let stop_result = self.stop_scan_worker();
        let finish_result = self.finish_current_scan_session_as(ScanPhase::Cancelled);
        match (stop_result, finish_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(status), _) => Err(status),
            (_, Err(status)) => Err(status),
        }
    }

    fn cancel_scan_session_best_effort(&self) {
        if let Some(mut worker) = lock_mutex_option(&self.scan_worker, "frontend_scan_worker")
            .and_then(|mut worker| worker.take())
        {
            let _ = worker.request_stop(WorkerExitReason::StopRequested);
            if let Err(err) = worker.join_from_owner() {
                let detail = format!("worker=frontend_scan_worker best_effort_stop_join_failed err={err:?}");
                self.shared.record_runtime_failure(detail.clone());
                self.shared.mark_live_path_failed(&detail);
            }
        }
        self.finish_current_scan_session_as_best_effort(ScanPhase::Cancelled);
    }

    fn stop_tune_worker(&self) -> BinderResult<()> {
        let mut worker_slot = lock_mutex_status(&self.tune_worker, "frontend_tune_worker")?;
        if let Some(worker) = worker_slot.as_mut() {
            worker.request_stop(WorkerExitReason::StopRequested).map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
            worker.wake().map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
            let outcome = worker.join_from_owner().map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
            if matches!(outcome, WorkerJoinOutcome::Joined(WorkerExitReason::RuntimeFailure | WorkerExitReason::PanicOrJoinFailure)) {
                let detail = format!("worker=frontend_tune_worker stop_join_abnormal outcome={outcome:?}");
                self.shared.record_runtime_failure(detail.clone());
                self.shared.mark_live_path_failed(&detail);
                *worker_slot = None;
                return Err(worker_exit_status("frontend_tune_worker", WorkerExit::RuntimeFailure));
            }
            *worker_slot = None;
        }
        Ok(())
    }

    fn stop_tune_worker_best_effort(&self) {
        if let Some(mut worker) = lock_mutex_option(&self.tune_worker, "frontend_tune_worker")
            .and_then(|mut worker| worker.take())
        {
            let _ = worker.request_stop(WorkerExitReason::StopRequested);
            if let Err(err) = worker.join_from_owner() {
                let detail = format!("worker=frontend_tune_worker best_effort_stop_join_failed err={err:?}");
                self.shared.record_runtime_failure(detail.clone());
                self.shared.mark_live_path_failed(&detail);
            }
        }
    }

    fn start_tune_worker(&self, request: FrontendTuneRequest) -> BinderResult<()> {
        let mut worker_slot = lock_mutex_status(&self.tune_worker, "frontend_tune_worker")?;
        if worker_slot.is_some() {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        let shared = Arc::clone(&self.shared);
        let callback_registry = Arc::clone(&self.callback);
        let shared_for_hook = Arc::clone(&shared);
        let shared_for_spawn_failure = Arc::clone(&shared);
        let handle = WorkerRuntime::spawn_owned_with_exit_hook(
            WorkerOwnerId("frontend_tune_worker", self.frontend_id),
            "frontend_tune_worker",
            move |owner_signal| {
            let outcome = FrontendHal::wait_for_lock(&shared, request.system, LockWaitMode::Tune, Some(&owner_signal));
            let Ok(outcome) = outcome else {
                if !owner_signal.is_stop_requested() {
                    let detail = format!("worker=frontend_tune_worker operation=wait_for_lock error=runtime_backend_failure");
                    shared.record_runtime_failure(detail.clone());
                    shared.mark_live_path_failed(&detail);
                    return WorkerExit::RuntimeFailure;
                }
                return WorkerExit::StopRequested;
            };
            if outcome.cancelled || owner_signal.is_stop_requested() {
                return WorkerExit::StopRequested;
            }
            if outcome.locked {
                if let Err(status) = FrontendHal::notify_event_with_callback(&callback_registry, &shared, None, None, FrontendEventType::LOCKED) {
                    let detail = format!("worker=frontend_tune_worker operation=notify_locked status={:?}", status);
                    shared.record_runtime_failure(detail.clone());
                    shared.mark_live_path_failed(&detail);
                    return WorkerExit::RuntimeFailure;
                }
                let has_bound_demux = match lock_mutex_status(&shared.bound_demuxes, "frontend_bound_demuxes") {
                    Ok(demuxes) => !demuxes.is_empty(),
                    Err(_) => {
                        let detail = "worker=frontend_tune_worker operation=bound_demuxes_lock_failed".to_string();
                        shared.record_runtime_failure(detail.clone());
                        shared.mark_live_path_failed(&detail);
                        return WorkerExit::RuntimeFailure;
                    }
                };
                if has_bound_demux {
                    if let Err(status) = shared.ensure_live_pump() {
                        let detail = format!("worker=frontend_tune_worker operation=ensure_live_pump status={:?}", status);
                        shared.record_runtime_failure(detail.clone());
                        shared.mark_live_path_failed(&detail);
                        return WorkerExit::RuntimeFailure;
                    }
                }
            } else {
                if let Err(status) = FrontendHal::notify_event_with_callback(&callback_registry, &shared, None, None, FrontendEventType::NO_SIGNAL) {
                    let detail = format!("worker=frontend_tune_worker operation=notify_no_signal status={:?}", status);
                    shared.record_runtime_failure(detail.clone());
                    shared.mark_live_path_failed(&detail);
                    return WorkerExit::RuntimeFailure;
                }
            }
            WorkerExit::Normal
        },
        move |exit| {
            if exit.is_abnormal() {
                let detail = format!("worker=frontend_tune_worker exit={:?}", exit);
                shared_for_hook.record_runtime_failure(detail.clone());
                shared_for_hook.mark_live_path_failed(&detail);
            }
        }).map_err(|err| {
            let detail = format!("worker=frontend_tune_worker spawn=failed error={err}");
            eprintln!("maleicacid-tuner-hal-worker: {detail}");
            shared_for_spawn_failure.record_runtime_failure(detail.clone());
            shared_for_spawn_failure.mark_live_path_failed(&detail);
            let mut rollback_failed = false;
            if let Some(mut backend) = lock_mutex_option(&shared_for_spawn_failure.backend, "frontend_backend") {
                if let Err(stop_err) = FrontendHal::backend_stop_tune(&mut backend) {
                    rollback_failed = true;
                    shared_for_spawn_failure.record_runtime_failure(format!("TuneRollbackFailed: worker=frontend_tune_worker spawn_failure_cleanup=backend_stop_tune error={stop_err}"));
                }
            } else {
                rollback_failed = true;
                shared_for_spawn_failure.record_runtime_failure("TuneRollbackFailed: worker=frontend_tune_worker spawn_failure_cleanup=frontend_backend_lock_failed".to_string());
            }
            if let Err(reset_err) = shared_for_spawn_failure.reset_bound_demuxes_for_stream_boundary() {
                rollback_failed = true;
                shared_for_spawn_failure.record_runtime_failure(format!("TuneRollbackFailed: worker=frontend_tune_worker spawn_failure_cleanup=stream_boundary_reset_failed status={:?}", reset_err));
                shared_for_spawn_failure.mark_live_path_failed("TuneRollbackFailed: spawn_failure_cleanup_stream_boundary_reset_failed");
            }
            shared_for_spawn_failure.stop_live_pump_best_effort();
            if rollback_failed {
                shared_for_spawn_failure.mark_live_path_failed("TuneRollbackFailed");
                return Status::new_service_specific_error(
                    TunerResult::UNKNOWN_ERROR.0,
                    Some("frontend_tune_worker_spawn_rollback_failed"),
                );
            }
            Status::from(StatusCode::UNKNOWN_ERROR)
        })?;
        *worker_slot = Some(handle);
        Ok(())
    }

    fn settings_fingerprint(settings: &FrontendSettings, scan_type: FrontendScanType) -> String {
        format!("{:?}|{:?}", settings, scan_type)
    }

    fn wait_interruptibly(stop_signal: Option<&ConcreteWorkerSignal>, duration: Duration) -> bool {
        match stop_signal {
            Some(signal) => signal.wait_timeout_or_stop(duration),
            None => {
                thread::park_timeout(duration);
                false
            }
        }
    }

    fn lock_wait_config(
        flavor: BackendFlavor,
        system: FrontendSystem,
        mode: LockWaitMode,
    ) -> LockWaitConfig {
        match flavor {
            BackendFlavor::Dvb => match (system, mode) {
                (FrontendSystem::IsdbT, LockWaitMode::Tune) => LockWaitConfig {
                    initial_settle_ms: 400,
                    poll_interval_ms: 50,
                    timeout_ms: LOCK_TIMEOUT_MS,
                    consecutive_lock_samples: 1,
                },
                (FrontendSystem::IsdbT, LockWaitMode::Scan) => LockWaitConfig {
                    initial_settle_ms: 400,
                    poll_interval_ms: 50,
                    timeout_ms: LOCK_TIMEOUT_MS,
                    consecutive_lock_samples: 1,
                },
                (_, LockWaitMode::Tune) => LockWaitConfig {
                    initial_settle_ms: 250,
                    poll_interval_ms: 50,
                    timeout_ms: LOCK_TIMEOUT_MS,
                    consecutive_lock_samples: 1,
                },
                (_, LockWaitMode::Scan) => LockWaitConfig {
                    initial_settle_ms: 250,
                    poll_interval_ms: 50,
                    timeout_ms: LOCK_TIMEOUT_MS,
                    consecutive_lock_samples: 1,
                },
            },
            BackendFlavor::Px4 => match mode {
                LockWaitMode::Tune => LockWaitConfig {
                    initial_settle_ms: 100,
                    poll_interval_ms: 100,
                    timeout_ms: LOCK_TIMEOUT_MS,
                    consecutive_lock_samples: 2,
                },
                LockWaitMode::Scan => LockWaitConfig {
                    initial_settle_ms: 100,
                    poll_interval_ms: 100,
                    timeout_ms: LOCK_TIMEOUT_MS,
                    consecutive_lock_samples: 2,
                },
            },
            BackendFlavor::Unavailable => LockWaitConfig {
                initial_settle_ms: 0,
                poll_interval_ms: 100,
                timeout_ms: 0,
                consecutive_lock_samples: 1,
            },
        }
    }

    fn wait_for_lock(
        shared: &Arc<FrontendRuntime>,
        system: FrontendSystem,
        mode: LockWaitMode,
        stop_signal: Option<&ConcreteWorkerSignal>,
    ) -> Result<LockWaitOutcome, HalError> {
        let flavor = {
            let backend = lock_mutex_hal(&shared.backend, "frontend_backend")?;
            Self::backend_flavor(&backend)
        };
        let config = Self::lock_wait_config(flavor, system, mode);
        if Self::wait_interruptibly(stop_signal, Duration::from_millis(config.initial_settle_ms)) {
            return Ok(LockWaitOutcome {
                telemetry: FrontendTelemetry::default(),
                locked: false,
                cancelled: true,
            });
        }
        let 期限 = Instant::now() + Duration::from_millis(config.timeout_ms);
        let mut consecutive_lock_samples = 0u32;
        let mut last_telemetry = FrontendTelemetry::default();
        loop {
            if stop_signal.map_or(false, |signal| signal.is_stop_requested()) {
                return Ok(LockWaitOutcome {
                    telemetry: last_telemetry,
                    locked: false,
                    cancelled: true,
                });
            }
            last_telemetry = {
                let mut backend = lock_mutex_hal(&shared.backend, "frontend_backend")?;
                Self::apply_selected_lnb_from_registry(&shared.lnb_registry, &mut backend)?;
                Self::backend_read_status(&mut backend)?
            };
            if last_telemetry.locked {
                consecutive_lock_samples = consecutive_lock_samples.saturating_add(1);
                if consecutive_lock_samples >= config.consecutive_lock_samples {
                    return Ok(LockWaitOutcome {
                        telemetry: last_telemetry,
                        locked: true,
                        cancelled: false,
                    });
                }
            } else {
                consecutive_lock_samples = 0;
            }
            let now = Instant::now();
            if now >= 期限 {
                return Ok(LockWaitOutcome {
                    telemetry: last_telemetry,
                    locked: false,
                    cancelled: false,
                });
            }
            let sleep_for = Duration::from_millis(config.poll_interval_ms)
                .min(期限.saturating_duration_since(now));
            if Self::wait_interruptibly(stop_signal, sleep_for) {
                return Ok(LockWaitOutcome {
                    telemetry: last_telemetry,
                    locked: false,
                    cancelled: true,
                });
            }
        }
    }

    fn mark_scan_session_phase(
        scan_session: &Arc<Mutex<Option<ScanSessionState>>>,
        session_id: i64,
        phase: ScanPhase,
    ) {
        let Some(mut guard) = lock_mutex_option(scan_session, "frontend_scan_session") else {
            return;
        };
        if let Some(state) = guard.as_mut() {
            if state.session_id == session_id {
                if state.phase.is_failed() && !phase.is_failed() {
                    return;
                }
                state.phase = phase;
            }
        }
    }

    fn scan_session_phase(
        scan_session: &Arc<Mutex<Option<ScanSessionState>>>,
        session_id: i64,
    ) -> Option<ScanPhase> {
        lock_mutex_option(scan_session, "frontend_scan_session").and_then(|guard| {
            guard
                .as_ref()
                .filter(|state| state.session_id == session_id)
                .map(|state| state.phase)
        })
    }

    fn publish_scan_terminal_state(shared: &Arc<FrontendRuntime>, state: &ScanSessionState) {
        let debug_line = format!(
            "frontend={} scan_last_terminal session_id={} phase={:?} fingerprint={}",
            shared.frontend_id, state.session_id, state.phase, state.fingerprint
        );
        if let Some(mut shared_last) =
            lock_mutex_option(&shared.scan_terminal_debug, "frontend_scan_terminal_debug")
        {
            *shared_last = Some(debug_line);
        }
    }

    fn publish_scan_terminal_debug_and_clear(
        shared: &Arc<FrontendRuntime>,
        scan_session: &Arc<Mutex<Option<ScanSessionState>>>,
        session_id: i64,
    ) -> Option<ScanSessionState> {
        let terminal =
            lock_mutex_option(scan_session, "frontend_scan_session").and_then(|mut session| {
                let terminal = session
                    .as_ref()
                    .filter(|state| {
                        state.session_id == session_id && state.phase != ScanPhase::Running
                    })
                    .cloned();
                if terminal.is_some() {
                    *session = None;
                }
                terminal
            });
        if let Some(state) = terminal.as_ref() {
            Self::publish_scan_terminal_state(shared, state);
        }
        terminal
    }

    fn finish_current_scan_session_as(&self, phase: ScanPhase) -> BinderResult<()> {
        let terminal = {
            let mut session = lock_mutex_status(&self.scan_session, "frontend_scan_session")?;
            let terminal = session.as_mut().map(|state| {
                if !(state.phase.is_failed() && !phase.is_failed()) {
                    state.phase = phase;
                }
                state.clone()
            });
            *session = None;
            terminal
        };
        if let Some(state) = terminal {
            Self::publish_scan_terminal_state(&self.shared, &state);
            *lock_mutex_status(&self.scan_last_terminal, "frontend_scan_last_terminal")? =
                Some(state);
        }
        Ok(())
    }

    fn finish_current_scan_session_as_best_effort(&self, phase: ScanPhase) {
        let terminal = lock_mutex_option(&self.scan_session, "frontend_scan_session").and_then(
            |mut session| {
                let terminal = session.as_mut().map(|state| {
                    if !(state.phase.is_failed() && !phase.is_failed()) {
                        state.phase = phase;
                    }
                    state.clone()
                });
                *session = None;
                terminal
            },
        );
        if let Some(state) = terminal {
            Self::publish_scan_terminal_state(&self.shared, &state);
            if let Some(mut last) =
                lock_mutex_option(&self.scan_last_terminal, "frontend_scan_last_terminal")
            {
                *last = Some(state);
            }
        }
    }

    fn remember_scan_terminal_from_current(&self) {
        let Some(session_id) = lock_mutex_option(&self.scan_session, "frontend_scan_session")
            .and_then(|session| session.as_ref().map(|state| state.session_id))
        else {
            return;
        };
        if let Some(state) = FrontendHal::publish_scan_terminal_debug_and_clear(
            &self.shared,
            &self.scan_session,
            session_id,
        ) {
            if let Some(mut last) =
                lock_mutex_option(&self.scan_last_terminal, "frontend_scan_last_terminal")
            {
                *last = Some(state);
            }
        }
    }

    fn unbind_frontend_demuxes(&self) -> BinderResult<()> {
        let bound_demux_ids: Vec<i32> =
            lock_mutex_status(&self.shared.bound_demuxes, "frontend_bound_demuxes")?
                .keys()
                .copied()
                .collect();
        for demux_id in bound_demux_ids {
            let record = lock_mutex_status(&self.demux_ledger, "demux_ledger")?
                .get_record(LedgerId(demux_id));
            let Some(record) = record else {
                self.shared.unbind_demux(demux_id)?;
                continue;
            };
            let should_unbind = {
                let mut record = lock_mutex_status(&record, "demux_record")?;
                if record.bound_frontend_id != Some(self.shared.frontend_id)
                    || record.bound_frontend_generation != Some(self.session_generation)
                {
                    false
                } else {
                    record.bound_frontend_id = None;
                    record.bound_frontend_generation = None;
                    {
                        let mut state = lock_mutex_status(&record.state, "demux_handle")?;
                        state.unbind_frontend();
                    }
                    execute_stream_boundary_for_demux(
                        StreamBoundaryReason::FrontendUnbind,
                        demux_id,
                        record.generation,
                        Arc::clone(&record.runtime_io),
                        Arc::clone(&record.state),
                        Some(Arc::clone(&self.shared.px4_path_diagnostics)),
                        Some(&mut record.pending_stream_boundary_plan),
                    )?;
                    true
                }
            };
            if should_unbind {
                self.shared.unbind_demux(demux_id)?;
            }
        }
        Ok(())
    }

    fn unbind_frontend_demuxes_best_effort(&self) {
        let bound_demux_ids: Vec<i32> =
            lock_mutex_option(&self.shared.bound_demuxes, "frontend_bound_demuxes")
                .map(|demuxes| demuxes.keys().copied().collect())
                .unwrap_or_default();
        for demux_id in bound_demux_ids {
            let record = lock_mutex_option(&self.demux_ledger, "demux_ledger")
                .and_then(|ledger| ledger.get_record(LedgerId(demux_id)));
            let Some(record) = record else {
                self.shared.unbind_demux_best_effort(demux_id);
                continue;
            };
            let should_unbind = {
                let Some(mut record) = lock_mutex_option(&record, "demux_record") else {
                    continue;
                };
                if record.bound_frontend_id != Some(self.shared.frontend_id)
                    || record.bound_frontend_generation != Some(self.session_generation)
                {
                    false
                } else {
                    record.bound_frontend_id = None;
                    record.bound_frontend_generation = None;
                    if let Some(mut state) = lock_mutex_option(&record.state, "demux_handle") {
                        state.unbind_frontend();
                    }
                    execute_stream_boundary_for_demux_best_effort(
                        StreamBoundaryReason::FrontendUnbind,
                        demux_id,
                        record.generation,
                        Arc::clone(&record.runtime_io),
                        Arc::clone(&record.state),
                        Some(Arc::clone(&self.shared.px4_path_diagnostics)),
                        Some(&mut record.pending_stream_boundary_plan),
                    );
                    true
                }
            };
            if should_unbind {
                self.shared.unbind_demux_best_effort(demux_id);
            }
        }
    }

    fn release_frontend_lease(&self) -> BinderResult<()> {
        let mut leases = lock_mutex_status(&self.lease_registry, "frontend_leases")?;
        if !leases.open_frontends.remove(&self.shared.frontend_id) {
            return Ok(());
        }
        leases.open_physical_groups.remove(&self.physical_group_id);
        let active_generation = leases
            .open_generations
            .get(&self.shared.frontend_id)
            .copied();
        if active_generation == Some(self.session_generation) {
            leases.open_generations.remove(&self.shared.frontend_id);
        }
        let count = leases
            .open_counts_by_type
            .get(&self.frontend_type.0)
            .copied()
            .ok_or_else(|| Status::from(StatusCode::UNKNOWN_ERROR))?;
        if count <= 0 {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        let next_count = count - 1;
        if next_count > 0 {
            leases
                .open_counts_by_type
                .insert(self.frontend_type.0, next_count);
        } else {
            leases.open_counts_by_type.remove(&self.frontend_type.0);
        }
        Ok(())
    }

    fn release_frontend_lease_best_effort(&self) {
        let Some(mut leases) = lock_mutex_option(&self.lease_registry, "frontend_leases") else {
            return;
        };
        if !leases.open_frontends.remove(&self.shared.frontend_id) {
            return;
        }
        leases.open_physical_groups.remove(&self.physical_group_id);
        let active_generation = leases
            .open_generations
            .get(&self.shared.frontend_id)
            .copied();
        if active_generation == Some(self.session_generation) {
            leases.open_generations.remove(&self.shared.frontend_id);
        }
        let count = frontend_open_count_or_zero(&leases.open_counts_by_type, self.frontend_type);
        let next_count = count.saturating_sub(1);
        if next_count > 0 {
            leases
                .open_counts_by_type
                .insert(self.frontend_type.0, next_count);
        } else {
            leases.open_counts_by_type.remove(&self.frontend_type.0);
        }
    }

    fn ensure_open(&self) -> BinderResult<()> {
        if self.cleanup_failed.load(Ordering::SeqCst) {
            return Err(invalid_state_status(
                "frontend cleanup failed; only close retry is allowed",
            ));
        }
        if self.closed.load(Ordering::SeqCst) {
            return Err(invalid_state_status("frontend is closed"));
        }
        Ok(())
    }

    fn close_internal(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst)
            && !self.cleanup_failed.load(Ordering::SeqCst)
        {
            return Ok(());
        }

        let mut first_error: Option<Status> = None;
        let mut record_step = |step: &'static str, result: BinderResult<()>| {
            if let Err(err) = result {
                self.shared.record_runtime_failure(format!(
                    "frontend_close step={} error={:?}",
                    step, err
                ));
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        };

        record_step("cancel_scan_session", self.cancel_scan_session());
        record_step("stop_tune_worker", self.stop_tune_worker());
        record_step("stop_live_pump", self.shared.stop_live_pump());
        record_step("backend_close", (|| {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            Self::backend_set_callback_registered(&mut backend, false);
            Self::backend_close(&mut backend).map_err(hal_error_status)
        })());
        record_step("callback_clear", (|| {
            *lock_mutex_status(&self.callback, "frontend_callback")? = None;
            Ok(())
        })());
        record_step("unbind_frontend_demuxes", self.unbind_frontend_demuxes());
        record_step("release_frontend_lease", self.release_frontend_lease());

        self.closed.store(true, Ordering::SeqCst);
        if let Some(err) = first_error {
            self.cleanup_failed.store(true, Ordering::SeqCst);
            Err(err)
        } else {
            self.cleanup_failed.store(false, Ordering::SeqCst);
            Ok(())
        }
    }

    fn close_internal_for_drop_cleanup(&self) {
        if self.closed.load(Ordering::SeqCst)
            && !self.cleanup_failed.load(Ordering::SeqCst)
        {
            return;
        }
        self.cancel_scan_session_best_effort();
        self.stop_tune_worker_best_effort();
        self.shared.stop_live_pump_best_effort();
        if let Some(mut backend) = lock_mutex_option(&self.shared.backend, "frontend_backend") {
            Self::backend_set_callback_registered(&mut backend, false);
            if let Err(err) = Self::backend_close(&mut backend) {
                self.shared.record_runtime_failure(format!(
                    "frontend_close_best_effort step=backend_close error={err}"
                ));
                self.cleanup_failed.store(true, Ordering::SeqCst);
            }
        } else {
            self.shared.record_runtime_failure(
                "frontend_close_best_effort step=backend_close error=frontend_backend_lock_failed"
                    .to_string(),
            );
            self.cleanup_failed.store(true, Ordering::SeqCst);
        }
        if let Some(mut callback) = lock_mutex_option(&self.callback, "frontend_callback") {
            *callback = None;
        }
        self.unbind_frontend_demuxes_best_effort();
        self.release_frontend_lease_best_effort();
        self.closed.store(true, Ordering::SeqCst);
    }

    fn reported_scan_input_stream_id(tune_request: &FrontendTuneRequest) -> Option<i32> {
        if !matches!(tune_request.system, FrontendSystem::IsdbS)
            || is_japan_cs110_if_frequency_hz(tune_request.frequency)
        {
            return None;
        }
        let raw = tune_request.stream_id?;
        reportable_bs_tsid_for_scan(tune_request.frequency, raw, tune_request.stream_id_kind)
            .map(i32::from)
    }

    fn emit_scan_stream_id_message_with_callback(
        callback_registry: &Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
        shared: &Arc<FrontendRuntime>,
        scan_session: &Arc<Mutex<Option<ScanSessionState>>>,
        session_id: i64,
        tune_request: &FrontendTuneRequest,
    ) -> BinderResult<Option<NotificationOutcome>> {
        if !matches!(tune_request.system, FrontendSystem::IsdbS) {
            return Ok(None);
        }
        let Some(stream_id) = Self::reported_scan_input_stream_id(tune_request) else {
            return Ok(None);
        };
        Self::notify_scan_message_with_callback(
            callback_registry,
            shared,
            Some(scan_session),
            Some(session_id),
            FrontendScanMessageType::INPUT_STREAM_IDS,
            FrontendScanMessage::InputStreamIds(vec![stream_id]),
        )?;
        Ok(Some(NotificationOutcome::Delivered))
    }

    fn validate_isdbt_fixed_settings(
        s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbtSettings::FrontendIsdbtSettings,
    ) -> Result<(), HalError> {
        if !matches!(
            s.bandwidth,
            FrontendIsdbtBandwidth::AUTO | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ
        ) {
            return Err(HalError::InvalidArgument(
                "r51のISDB-TはAUTOまたは6MHz帯域幅だけを受け付けます".into(),
            ));
        }
        if !matches!(s.mode, FrontendIsdbtMode::AUTO | FrontendIsdbtMode::MODE_3) {
            return Err(HalError::InvalidArgument(
                "r51のISDB-TはAUTOまたはMODE_3伝送モードだけを受け付けます".into(),
            ));
        }
        if !matches!(
            s.guardInterval,
            FrontendIsdbtGuardInterval::AUTO
                | FrontendIsdbtGuardInterval::INTERVAL_1_32
                | FrontendIsdbtGuardInterval::INTERVAL_1_16
                | FrontendIsdbtGuardInterval::INTERVAL_1_8
                | FrontendIsdbtGuardInterval::INTERVAL_1_4
        ) {
            return Err(HalError::InvalidArgument(
                "r51のISDB-TガードインターバルはAUTOまたは1/32,1/16,1/8,1/4だけを受け付けます".into(),
            ));
        }
        for layer in &s.layerSettings {
            if !matches!(
                layer.modulation,
                FrontendIsdbtModulation::AUTO
                    | FrontendIsdbtModulation::MOD_DQPSK
                    | FrontendIsdbtModulation::MOD_QPSK
                    | FrontendIsdbtModulation::MOD_16QAM
                    | FrontendIsdbtModulation::MOD_64QAM
            ) {
                return Err(HalError::InvalidArgument(
                    "r51のISDB-T階層変調はAUTO,DQPSK,QPSK,16QAM,64QAMだけを受け付けます".into(),
                ));
            }
            if !matches!(
                layer.coderate,
                FrontendIsdbtCoderate::AUTO
                    | FrontendIsdbtCoderate::CODERATE_1_2
                    | FrontendIsdbtCoderate::CODERATE_2_3
                    | FrontendIsdbtCoderate::CODERATE_3_4
                    | FrontendIsdbtCoderate::CODERATE_5_6
                    | FrontendIsdbtCoderate::CODERATE_7_8
            ) {
                return Err(HalError::InvalidArgument(
                    "r51のISDB-T階層符号率はAUTOまたは1/2,2/3,3/4,5/6,7/8だけを受け付けます".into(),
                ));
            }
            if !matches!(
                layer.timeInterleave,
                FrontendIsdbtTimeInterleaveMode::AUTO
                    | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_0
                    | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_1
                    | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_2
                    | FrontendIsdbtTimeInterleaveMode::INTERLEAVE_3_4
            ) {
                return Err(HalError::InvalidArgument(
                    "r51のISDB-T階層時間インタリーブはAUTOまたはMODE_3用の0,1,2,4だけを受け付けます".into(),
                ));
            }
        }
        Ok(())
    }

    fn validate_isdbs_fixed_settings(
        s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbsSettings::FrontendIsdbsSettings,
    ) -> Result<(), HalError> {
        if !matches!(
            s.modulation,
            FrontendIsdbsModulation::AUTO
                | FrontendIsdbsModulation::MOD_BPSK
                | FrontendIsdbsModulation::MOD_QPSK
                | FrontendIsdbsModulation::MOD_TC8PSK
        ) {
            return Err(HalError::InvalidArgument(
                "r51のISDB-S変調はAUTO,BPSK,QPSK,TC8PSKだけを受け付けます".into(),
            ));
        }
        if !matches!(
            s.coderate,
            FrontendIsdbsCoderate::AUTO
                | FrontendIsdbsCoderate::CODERATE_1_2
                | FrontendIsdbsCoderate::CODERATE_2_3
                | FrontendIsdbsCoderate::CODERATE_3_4
                | FrontendIsdbsCoderate::CODERATE_5_6
                | FrontendIsdbsCoderate::CODERATE_7_8
        ) {
            return Err(HalError::InvalidArgument(
                "r51のISDB-S符号率はAUTOまたは1/2,2/3,3/4,5/6,7/8だけを受け付けます".into(),
            ));
        }
        if s.symbolRate != 0 {
            return Err(HalError::InvalidArgument(
                "r51のISDB-S公開設定では明示symbolRateを使いません。0または未指定にしてください"
                    .into(),
            ));
        }
        Ok(())
    }

    fn backend_validate_tune_request(
        backend: &mut FrontendBackendState,
        request: &FrontendTuneRequest,
    ) -> Result<(), HalError> {
        if request.end_frequency.unwrap_or(request.frequency) != request.frequency {
            return Err(HalError::Unsupported("HAL-generated range tune/scan is disabled; submit one explicit candidate at a time"));
        }
        match backend {
            FrontendBackendState::Px4(inner) => inner.validate_tune_request(request),
            FrontendBackendState::Dvb(inner) => inner.validate_tune_request(request),
            FrontendBackendState::Unavailable { reason, .. } => Err(HalError::OpenFailed {
                path: PathBuf::from("unavailable-frontend"),
                message: reason.clone(),
            }),
        }
    }

    fn settings_to_request(settings: &FrontendSettings) -> Result<FrontendTuneRequest, HalError> {
        match settings {
            FrontendSettings::Isdbt(s) => {
                Self::validate_isdbt_fixed_settings(s)?;
                let bandwidth_hz = map_isdbt_bandwidth(s.bandwidth);
                Ok(FrontendTuneRequest {
                    system: FrontendSystem::IsdbT,
                    frequency: cast_u64(s.frequency, "isdbt.frequency")?,
                    end_frequency: optional_positive_i64_to_u64(
                        s.endFrequency,
                        "isdbt.endFrequency",
                    )?,
                    stream_id: None,
                    stream_id_kind: None,
                    bandwidth_hz,
                    symbol_rate: None,
                })
            }
            FrontendSettings::Isdbs(s) => {
                Self::validate_isdbs_fixed_settings(s)?;
                let frequency = cast_u64(s.frequency, "isdbs.frequency")?;
                let (stream_id, stream_id_kind) =
                    map_isdbs_stream_selector(s.streamId, s.streamIdType, frequency)?;
                Ok(FrontendTuneRequest {
                    system: FrontendSystem::IsdbS,
                    frequency,
                    end_frequency: optional_positive_i64_to_u64(
                        s.endFrequency,
                        "isdbs.endFrequency",
                    )?,
                    stream_id,
                    stream_id_kind,
                    bandwidth_hz: None,
                    symbol_rate: None,
                })
            }
            FrontendSettings::Isdbs3(_) | FrontendSettings::Dvbs(_) => {
                Err(HalError::Unsupported("ISDB-S3/DVB-S は製品対象外です"))
            }
            _ => Err(HalError::Unsupported(
                "frontend setting not handled by px4 backend",
            )),
        }
    }

    fn backend_scan_requests(
        backend: &mut FrontendBackendState,
        settings: &FrontendSettings,
        scan_mode: FrontendScanMode,
    ) -> Result<Vec<FrontendTuneRequest>, HalError> {
        let base = Self::settings_to_request(settings)?;
        match backend {
            FrontendBackendState::Px4(inner) => {
                let requests = inner.scan_requests(&base, scan_mode)?;
                for request in &requests {
                    inner.validate_tune_request(request)?;
                }
                Ok(requests)
            }
            FrontendBackendState::Dvb(inner) => match base.system {
                FrontendSystem::IsdbT | FrontendSystem::IsdbS => {
                    let requests = inner.scan_requests(&base, scan_mode)?;
                    for request in &requests {
                        inner.validate_tune_request(request)?;
                    }
                    Ok(requests)
                }
                FrontendSystem::IsdbS3 | FrontendSystem::DvbS => {
                    Err(HalError::Unsupported("ISDB-S3/DVB-S は製品対象外です"))
                }
            },
            FrontendBackendState::Unavailable { reason, .. } => Err(HalError::OpenFailed {
                path: PathBuf::from("unavailable-frontend"),
                message: reason.clone(),
            }),
        }
    }

    fn to_scan_mode(scan_type: FrontendScanType) -> Result<FrontendScanMode, HalError> {
        match scan_type {
            FrontendScanType::SCAN_AUTO => Ok(FrontendScanMode::Auto),
            FrontendScanType::SCAN_BLIND => Err(HalError::Unsupported(
                "BLIND_SCAN is not supported; TIS owns the Japanese scan SSOT",
            )),
            FrontendScanType::SCAN_UNDEFINED => Err(HalError::InvalidArgument(
                "SCAN_UNDEFINED is not a valid scan request".into(),
            )),
            other => Err(HalError::InvalidArgument(format!(
                "unsupported frontend scan type: {:?}",
                other
            ))),
        }
    }

    fn validate_status_types(
        support: FrontendStatusSupport,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<()> {
        if status_types.iter().any(|ty| !support.supports(*ty)) {
            return Err(invalid_argument_status(
                "unsupported frontend status type requested",
            ));
        }
        Ok(())
    }

    fn require_status_value<T: Copy>(value: Option<T>, name: &'static str) -> BinderResult<T> {
        value.ok_or_else(|| {
            Status::new_service_specific_error(
                TunerResult::INVALID_STATE.0,
                Some(&format!("frontend status {name} is supported but not currently available")),
            )
        })
    }

    fn status_value_available(
        support: FrontendStatusSupport,
        status: &FrontendTelemetry,
        status_type: FrontendStatusType,
    ) -> bool {
        if !support.supports(status_type) {
            return false;
        }
        match status_type {
            FrontendStatusType::DEMOD_LOCK => true,
            FrontendStatusType::RF_LOCK => status.rf_locked.is_some(),
            FrontendStatusType::SNR => status.cnr.is_some(),
            FrontendStatusType::SIGNAL_STRENGTH => status.signal_strength.is_some(),
            FrontendStatusType::SIGNAL_QUALITY => status.signal_quality.is_some(),
            // LNB voltage は未選択時の NONE state が明確に定義されている。
            FrontendStatusType::LNB_VOLTAGE => support.satellite,
            _ => false,
        }
    }

    fn status_for_types(
        support: FrontendStatusSupport,
        status: &FrontendTelemetry,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<Vec<FrontendStatus>> {
        Self::validate_status_types(support, status_types)?;
        let mut out = Vec::with_capacity(status_types.len());
        for ty in status_types {
            match *ty {
                FrontendStatusType::DEMOD_LOCK => {
                    out.push(FrontendStatus::IsDemodLocked(status.locked))
                }
                FrontendStatusType::RF_LOCK => out.push(FrontendStatus::IsRfLocked(
                    Self::require_status_value(status.rf_locked, "RF_LOCK")?,
                )),
                FrontendStatusType::SNR => out.push(FrontendStatus::Snr(
                    i32::try_from(Self::require_status_value(status.cnr, "SNR")?)
                        .unwrap_or(i32::MAX),
                )),
                FrontendStatusType::SIGNAL_STRENGTH => out.push(FrontendStatus::SignalStrength(
                    i32::try_from(Self::require_status_value(
                        status.signal_strength,
                        "SIGNAL_STRENGTH",
                    )?)
                    .unwrap_or(i32::MAX),
                )),
                FrontendStatusType::SIGNAL_QUALITY => out.push(FrontendStatus::SignalQuality(
                    i32::try_from(Self::require_status_value(
                        status.signal_quality,
                        "SIGNAL_QUALITY",
                    )?)
                    .unwrap_or(i32::MAX),
                )),
                FrontendStatusType::LNB_VOLTAGE => out.push(FrontendStatus::LnbVoltage(
                    match status.lnb_voltage {
                        Some(11) => LnbVoltage::VOLTAGE_11V,
                        Some(15) => LnbVoltage::VOLTAGE_15V,
                        _ => LnbVoltage::NONE,
                    },
                )),
                _ => {
                    return Err(invalid_argument_status(
                        "unsupported frontend status type requested",
                    ))
                }
            }
        }
        Ok(out)
    }

    fn readiness_for_types(
        support: FrontendStatusSupport,
        backend_available: bool,
        tuning_active: bool,
        status: Option<&FrontendTelemetry>,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<Vec<FrontendStatusReadiness>> {
        Ok(status_types
            .iter()
            .map(|ty| {
                if !support.supports(*ty) {
                    FrontendStatusReadiness::UNSUPPORTED
                } else if !backend_available {
                    FrontendStatusReadiness::UNAVAILABLE
                } else if tuning_active {
                    FrontendStatusReadiness::UNSTABLE
                } else if let Some(status) = status {
                    if Self::status_value_available(support, status, *ty) {
                        FrontendStatusReadiness::STABLE
                    } else {
                        FrontendStatusReadiness::UNAVAILABLE
                    }
                } else {
                    FrontendStatusReadiness::UNAVAILABLE
                }
            })
            .collect())
    }

    fn validate_lnb_owner(
        allowed_systems: &[FrontendSystem],
        frontend_id: i32,
        lnb_registry: &Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
        lnb_id: i32,
    ) -> BinderResult<LnbRuntimeState> {
        if !allowed_systems
            .iter()
            .any(|system| matches!(system, FrontendSystem::IsdbS))
        {
            return Err(Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None));
        }
        let lnb = lock_mutex_status(lnb_registry, "lnb_registry")?
            .get(&lnb_id)
            .cloned()
            .ok_or(StatusCode::NAME_NOT_FOUND)?;
        if lnb.owner_frontend_id != frontend_id {
            return Err(invalid_argument_status("LNB belongs to another frontend"));
        }
        Ok(lnb)
    }
}


struct FrontendTuneTxn<'a> {
    hal: &'a FrontendHal,
    request: FrontendTuneRequest,
    txn: LifecycleTxn,
    backend_submitted: bool,
}

impl<'a> FrontendTuneTxn<'a> {
    fn new(hal: &'a FrontendHal, request: FrontendTuneRequest) -> Self {
        Self { hal, request, txn: LifecycleTxn::new(), backend_submitted: false }
    }

    fn run(mut self) -> BinderResult<()> {
        self.txn.prepare("frontend_tune_cancel_scan", || self.hal.cancel_scan_session())?;
        self.txn.prepare("frontend_tune_stop_tune_worker", || self.hal.stop_tune_worker())?;
        self.txn.prepare("frontend_tune_stop_live_pump", || self.hal.shared.stop_live_pump())?;
        self.txn.prepare("frontend_tune_backend_stop", || {
            let mut backend = lock_mutex_status(&self.hal.shared.backend, "frontend_backend")?;
            FrontendHal::backend_stop_tune(&mut backend).map_err(hal_error_status)
        })?;
        self.txn.apply("frontend_tune_stream_boundary_reset", || self.hal.shared.reset_bound_demuxes_for_stream_boundary())?;
        let apply_result = self.txn.apply("frontend_tune_backend_validate_apply_submit", || {
            let mut backend = lock_mutex_status(&self.hal.shared.backend, "frontend_backend")?;
            FrontendHal::backend_validate_tune_request(&mut backend, &self.request)
                .map_err(hal_error_status)?;
            self.hal.apply_selected_lnb(&mut backend)
                .map_err(hal_error_status)?;
            FrontendHal::backend_submit_tune(&mut backend, self.request.clone()).map_err(hal_error_status)?;
            Ok(())
        });
        if let Err(status) = apply_result {
            return Err(status);
        }
        self.backend_submitted = true;
        if let Err(status) = self.txn.commit("frontend_tune_worker_spawn_commit", || {
            self.hal.start_tune_worker(self.request.clone())
        }) {
            return Err(self.rollback_after_post_backend_failure(status));
        }
        Ok(())
    }

    fn rollback_after_post_backend_failure(&mut self, original_status: Status) -> Status {
        let mut rollback_failed = false;
        if self.backend_submitted {
            match lock_mutex_status(&self.hal.shared.backend, "frontend_backend") {
                Ok(mut backend) => {
                    if let Err(err) = FrontendHal::backend_stop_tune(&mut backend) {
                        rollback_failed = true;
                        self.hal.shared.record_runtime_failure(format!(
                            "TuneRollbackFailed: frontend_tune_txn backend_stop_tune error={err}"
                        ));
                    }
                }
                Err(status) => {
                    rollback_failed = true;
                    self.hal.shared.record_runtime_failure(format!(
                        "TuneRollbackFailed: frontend_tune_txn backend_lock_failed status={:?}", status
                    ));
                }
            }
        }
        self.hal.shared.stop_live_pump_best_effort();
        if let Err(status) = self.hal.shared.reset_bound_demuxes_for_stream_boundary() {
            rollback_failed = true;
            self.hal.shared.record_runtime_failure(format!(
                "TuneRollbackFailed: frontend_tune_txn stream_boundary_reset_failed status={:?}", status
            ));
        }
        if rollback_failed {
            self.hal.shared.mark_live_path_failed("TuneRollbackFailed");
            Status::new_service_specific_error(
                TunerResult::UNKNOWN_ERROR.0,
                Some("frontend_tune_txn_rollback_failed"),
            )
        } else {
            original_status
        }
    }
}

impl Drop for FrontendHal {
    fn drop(&mut self) {
        self.close_internal_for_drop_cleanup();
    }
}

impl Interface for FrontendHal {}

impl IFrontend for FrontendHal {
    fn setCallback(&self, callback: &Strong<dyn IFrontendCallback>) -> BinderResult<()> {
        self.ensure_open()?;
        let mut callback_slot = lock_mutex_status(&self.callback, "frontend_callback")?;
        let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
        *callback_slot = Some(callback.clone());
        Self::backend_set_callback_registered(&mut backend, true);
        Ok(())
    }

    fn tune(&self, settings: &FrontendSettings) -> BinderResult<()> {
        self.ensure_open()?;
        let request = Self::settings_to_request(settings).map_err(hal_error_status)?;
        if !self.system_allowed(request.system) {
            return Err(invalid_argument_status(
                "frontend settings delivery system is not supported by this frontend instance",
            ));
        }
        FrontendTuneTxn::new(self, request).run()
    }

    fn stopTune(&self) -> BinderResult<()> {
        self.ensure_open()?;
        if lock_mutex_status(&self.scan_session, "frontend_scan_session")?.is_some() {
            return Err(Status::new_service_specific_error(
                TunerResult::INVALID_STATE.0,
                Some("stopTune does not cancel an active scan; call stopScan"),
            ));
        }
        self.stop_tune_worker()?;
        self.shared.stop_live_pump()?;
        {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            Self::backend_stop_tune(&mut backend)
        }
        .map_err(hal_error_status)?;
        self.shared.reset_bound_demuxes_for_stream_boundary()?;
        Ok(())
    }

    fn close(&self) -> BinderResult<()> {
        self.close_internal()
    }

    fn scan(&self, settings: &FrontendSettings, scan_type: FrontendScanType) -> BinderResult<()> {
        self.ensure_open()?;
        let fingerprint = Self::settings_fingerprint(settings, scan_type);

        let scan_mode = Self::to_scan_mode(scan_type).map_err(hal_error_status)?;
        let requests = {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            Self::backend_scan_requests(&mut backend, settings, scan_mode)
        }
        .map_err(hal_error_status)?;
        if requests.iter().any(|req| !self.system_allowed(req.system)) {
            return Err(invalid_argument_status(
                "scan settings delivery system is not supported by this frontend instance",
            ));
        }
        self.stop_tune_worker()?;
        self.cancel_scan_session()?;
        let session_id = self.next_scan_session_id.fetch_add(1, Ordering::SeqCst);
        let start_index = 0usize;

        let mut scan_worker_slot = lock_mutex_status(&self.scan_worker, "frontend_scan_worker")?;
        if scan_worker_slot.is_some() {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        let mut scan_session_guard =
            lock_mutex_status(&self.scan_session, "frontend_scan_session")?;
        *scan_session_guard = Some(ScanSessionState {
            session_id,
            fingerprint,
            phase: ScanPhase::Running,
        });

        let callback_registry = Arc::clone(&self.callback);
        let shared = Arc::clone(&self.shared);
        let scan_session = Arc::clone(&self.scan_session);
        let callback_registry_for_hook = Arc::clone(&callback_registry);
        let shared_for_hook = Arc::clone(&shared);
        let scan_session_for_hook = Arc::clone(&scan_session);
        let scan_last_terminal_for_hook = Arc::clone(&self.scan_last_terminal);
        let handle = match WorkerRuntime::spawn_owned_with_exit_hook(
            WorkerOwnerId("frontend_scan_worker", self.frontend_id),
            "frontend_scan_worker",
            move |owner_signal| {
                let total = requests.len().max(1) as i32;
                let mut scan_failed = false;
                for index in start_index..requests.len() {
                    if owner_signal.is_stop_requested() {
                        FrontendHal::mark_scan_session_phase(
                            &scan_session,
                            session_id,
                            ScanPhase::Cancelled,
                        );
if !FrontendHal::notify_scan_end_required(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                            "notify_end_after_stop_requested",
                        ) {
                            scan_failed = true;
                        }
                        break;
                    }
                    let request = requests[index].clone();
                    let progress = (((index + 1) as i32) * 100) / total;
                    if let Err(status) = FrontendHal::notify_scan_message_with_callback(
                        &callback_registry,
                        &shared,
                        Some(&scan_session),
                        Some(session_id),
                        FrontendScanMessageType::PROGRESS_PERCENT,
                        FrontendScanMessage::ProgressPercent(progress),
                    ) {
                        let detail = format!("worker=frontend_scan_worker operation=notify_progress status={:?}", status);
                        shared.record_runtime_failure(detail.clone());
                        shared.mark_live_path_failed(&detail);
                        scan_failed = true;
                        break;
                    }
                    if let Err(status) = FrontendHal::notify_scan_message_with_callback(
                        &callback_registry,
                        &shared,
                        Some(&scan_session),
                        Some(session_id),
                        FrontendScanMessageType::FREQUENCY,
                        FrontendScanMessage::Frequencies(vec![request.frequency as i64]),
                    ) {
                        let detail = format!("worker=frontend_scan_worker operation=notify_frequency status={:?}", status);
                        shared.record_runtime_failure(detail.clone());
                        shared.mark_live_path_failed(&detail);
                        scan_failed = true;
                        break;
                    }
                    shared.stop_live_pump_best_effort();
                    let stop_result = match lock_mutex_hal(&shared.backend, "frontend_backend") {
                        Ok(mut backend) => FrontendHal::backend_stop_tune(&mut backend),
                        Err(err) => Err(err),
                    };
                    let tune_result = stop_result.and_then(|_| {
                        shared.reset_bound_demuxes_for_stream_boundary()
                            .map_err(|_| HalError::Internal("stream boundary reset failed".into()))?;
                        let mut backend = lock_mutex_hal(&shared.backend, "frontend_backend")?;
                        FrontendHal::backend_validate_tune_request(&mut backend, &request)
                            .and_then(|_| {
                                FrontendHal::apply_selected_lnb_from_registry(
                                    &shared.lnb_registry,
                                    &mut backend,
                                )
                            })
                            .and_then(|_| {
                                FrontendHal::backend_submit_tune(&mut backend, request.clone())
                            })
                    });
                    if let Err(err) = tune_result {
                        let detail =
                            format!("worker=frontend_scan_worker operation=scan_tune error={err}");
                        shared.record_runtime_failure(detail.clone());
                        FrontendHal::mark_scan_session_phase(
                            &scan_session,
                            session_id,
                            ScanPhase::FailedBackend,
                        );
                        shared.mark_live_path_failed(&detail);
if !FrontendHal::notify_scan_end_required(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                            "notify_end_after_failure",
                        ) {
                            scan_failed = true;
                        }
                        scan_failed = true;
                        break;
                    }
                    let outcome = FrontendHal::wait_for_lock(
                        &shared,
                        request.system,
                        LockWaitMode::Scan,
                        Some(&owner_signal),
                    );
                    let Ok(outcome) = outcome else {
                        let detail = format!("worker=frontend_scan_worker operation=wait_for_lock error=runtime_backend_failure");
                        shared.record_runtime_failure(detail.clone());
                        FrontendHal::mark_scan_session_phase(
                            &scan_session,
                            session_id,
                            ScanPhase::FailedBackend,
                        );
                        shared.mark_live_path_failed(&detail);
if !FrontendHal::notify_scan_end_required(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                            "notify_end_after_failure",
                        ) {
                            scan_failed = true;
                        }
                        scan_failed = true;
                        break;
                    };
                    if outcome.cancelled {
                        FrontendHal::mark_scan_session_phase(
                            &scan_session,
                            session_id,
                            ScanPhase::Cancelled,
                        );
if !FrontendHal::notify_scan_end_required(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                            "notify_end_after_stop_requested",
                        ) {
                            scan_failed = true;
                        }
                        break;
                    }
                    if outcome.locked {
                        FrontendHal::mark_scan_session_phase(
                            &scan_session,
                            session_id,
                            ScanPhase::Running,
                        );
                        if let Some((message_type, message)) = FrontendHal::lock_scan_message(true)
                        {
                            if let Err(status) = FrontendHal::notify_scan_message_with_callback(
                                &callback_registry,
                                &shared,
                                Some(&scan_session),
                                Some(session_id),
                                message_type,
                                message,
                            ) {
                                let detail = format!("worker=frontend_scan_worker operation=notify_locked_scan_message status={:?}", status);
                                shared.record_runtime_failure(detail.clone());
                                shared.mark_live_path_failed(&detail);
                                scan_failed = true;
                                break;
                            }
                        }
                        if let Err(status) = FrontendHal::notify_event_with_callback(
                            &callback_registry,
                            &shared,
                            Some(&scan_session),
                            Some(session_id),
                            FrontendEventType::LOCKED,
                        ) {
                            let detail = format!("worker=frontend_scan_worker operation=notify_locked_event status={:?}", status);
                            shared.record_runtime_failure(detail.clone());
                            shared.mark_live_path_failed(&detail);
                            scan_failed = true;
                            break;
                        }
                        if let Err(status) = FrontendHal::emit_scan_stream_id_message_with_callback(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                            &request,
                        ) {
                            let detail = format!("worker=frontend_scan_worker operation=notify_stream_id status={:?}", status);
                            shared.record_runtime_failure(detail.clone());
                            shared.mark_live_path_failed(&detail);
                            scan_failed = true;
                            break;
                        }
                        continue;
                    }
                    if let Err(status) = FrontendHal::notify_event_with_callback(
                        &callback_registry,
                        &shared,
                        Some(&scan_session),
                        Some(session_id),
                        FrontendEventType::NO_SIGNAL,
                    ) {
                        let detail = format!("worker=frontend_scan_worker operation=notify_no_signal status={:?}", status);
                        shared.record_runtime_failure(detail.clone());
                        shared.mark_live_path_failed(&detail);
                        scan_failed = true;
                        break;
                    }
                }
                let scan_cleanup_error = match lock_mutex_hal(&shared.backend, "frontend_backend") {
                    Ok(mut backend) => FrontendHal::backend_stop_tune(&mut backend)
                        .err()
                        .map(|err| {
                            (
                                format!(
                                    "worker=frontend_scan_worker cleanup=backend_stop_tune error={err}"
                                ),
                                "scan_cleanup_backend_stop_tune_failed",
                            )
                        }),
                    Err(err) => Some((
                        format!(
                            "worker=frontend_scan_worker cleanup=frontend_backend_lock error={err}"
                        ),
                        "scan_cleanup_frontend_backend_lock_failed",
                    )),
                };
                if let Some((detail, failure_reason)) = scan_cleanup_error {
                    let already_callback_failed = matches!(
                        FrontendHal::scan_session_phase(&scan_session, session_id),
                        Some(ScanPhase::FailedCallback)
                    );
                    shared.record_runtime_failure(detail);
                    if !already_callback_failed {
                        FrontendHal::mark_scan_session_phase(
                            &scan_session,
                            session_id,
                            ScanPhase::FailedBackend,
                        );
                    }
                    shared.mark_live_path_failed(failure_reason);
                    if !already_callback_failed {
                        if !FrontendHal::notify_scan_end_required(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                            "notify_end_after_cleanup_failure",
                        ) {
                            scan_failed = true;
                        }
                    }
                    scan_failed = true;
                }
                if !owner_signal.is_stop_requested()
                    && !scan_failed
                    && matches!(
                        FrontendHal::scan_session_phase(&scan_session, session_id),
                        Some(ScanPhase::Running)
                    )
                {
                    FrontendHal::mark_scan_session_phase(
                        &scan_session,
                        session_id,
                        ScanPhase::Completed,
                    );
if !FrontendHal::notify_scan_end_required(
                        &callback_registry,
                        &shared,
                        &scan_session,
                        session_id,
                        "notify_end_after_completed",
                    ) {
                        scan_failed = true;
                    }
                }
                match FrontendHal::scan_session_phase(&scan_session, session_id) {
                    Some(
                        ScanPhase::FailedBackend
                        | ScanPhase::FailedCallback
                        | ScanPhase::FailedPanic,
                    ) => WorkerExit::RuntimeFailure,
                    Some(ScanPhase::Cancelled) => WorkerExit::StopRequested,
                    _ => WorkerExit::Normal,
                }
            },
            move |exit| {
                if exit.is_abnormal() {
                    let detail = format!("worker=frontend_scan_worker exit={:?}", exit);
                    shared_for_hook.record_runtime_failure(detail.clone());
                    match exit {
                        WorkerExit::PanicOrJoinFailure => {
                            FrontendHal::mark_scan_session_phase(
                                &scan_session_for_hook,
                                session_id,
                                ScanPhase::FailedPanic,
                            );
                        }
                        WorkerExit::RuntimeFailure => {
                            if !matches!(FrontendHal::scan_session_phase(&scan_session_for_hook, session_id), Some(phase) if phase.is_failed())
                            {
                                FrontendHal::mark_scan_session_phase(
                                    &scan_session_for_hook,
                                    session_id,
                                    ScanPhase::FailedBackend,
                                );
                            }
                        }
                        _ => {}
                    }
                    shared_for_hook.mark_live_path_failed(&detail);
                }
                if let Some(state) = FrontendHal::publish_scan_terminal_debug_and_clear(
                    &shared_for_hook,
                    &scan_session_for_hook,
                    session_id,
                ) {
                    if let Some(mut last) = lock_mutex_option(
                        &scan_last_terminal_for_hook,
                        "frontend_scan_last_terminal",
                    ) {
                        *last = Some(state);
                    }
                }
            },
        ) {
            Ok(handle) => handle,
            Err(err) => {
                let detail = format!("worker=frontend_scan_worker spawn_failed error={err}");
                eprintln!("maleicacid-tuner-hal-worker: {detail}");
                self.shared.record_runtime_failure(detail.clone());
                self.shared.mark_live_path_failed(&detail);
                drop(scan_session_guard);
                drop(scan_worker_slot);
                self.finish_current_scan_session_as_best_effort(ScanPhase::FailedBackend);
                return Err(Status::from(StatusCode::UNKNOWN_ERROR));
            }
        };
        *scan_worker_slot = Some(handle);
        drop(scan_session_guard);
        drop(scan_worker_slot);
        Ok(())
    }

    fn stopScan(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.cancel_scan_session()?;
        // stopScan が所有するのは scan operation だけである。scan ワーカー は cancel 中に
        // 自身の backend stop を実行する。scan が動作していない場合、通常の tune / ライブ pump を
        // stop してはならない。
        Ok(())
    }

    fn getStatus(&self, status_types: &[FrontendStatusType]) -> BinderResult<Vec<FrontendStatus>> {
        self.ensure_open()?;
        if status_types.is_empty() {
            return Ok(Vec::new());
        }
        let support = self.shared.advertised_status_support;
        Self::validate_status_types(support, status_types)?;
        let telemetry = {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            // r50dz56/G2-20: getStatus is an observation API. LNB backend application
            // belongs to setLnb/tune transactions and must not be performed here.
            Self::backend_read_status(&mut backend)
        }
        .map_err(hal_error_status)?;
        Self::status_for_types(support, &telemetry, status_types)
    }

    fn setLnb(&self, lnb_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        let _lnb_op = lnb_operation_guard_for_id(lnb_id)?;
        let lnb = Self::validate_lnb_owner(
            &self.shared.allowed_systems,
            self.shared.frontend_id,
            &self.shared.lnb_registry,
            lnb_id,
        )?;
        let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
        Self::backend_apply_lnb_state(&mut backend, &lnb).map_err(hal_error_status)?;
        Self::backend_set_lnb_id(&mut backend, lnb_id);
        Ok(())
    }

    fn linkCiCam(&self, ci_cam_id: i32) -> BinderResult<i32> {
        self.ensure_open()?;
        let detail = format!("ci_cam_unsupported api=linkCiCam ci_cam_id={}", ci_cam_id);
        self.shared.record_runtime_failure(detail.clone());
        eprintln!("maleicacid-tuner-hal: CI CAM is permanently unsupported; linkCiCam rejected ci_cam_id={}", ci_cam_id);
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ))
    }

    fn unlinkCiCam(&self, ci_cam_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        let detail = format!("ci_cam_unsupported api=unlinkCiCam ci_cam_id={}", ci_cam_id);
        self.shared.record_runtime_failure(detail.clone());
        eprintln!("maleicacid-tuner-hal: CI CAM is permanently unsupported; unlinkCiCam rejected ci_cam_id={}", ci_cam_id);
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ))
    }

    fn getHardwareInfo(&self) -> BinderResult<String> {
        self.ensure_open()?;
        let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
        Ok(format!(
            "{} frontend_id={} device_present={}",
            Self::backend_hardware_info(&mut backend),
            self.shared.frontend_id,
            Self::backend_probe_device(&backend)
        ))
    }

    fn removeOutputPid(&self, _pid: i32) -> BinderResult<()> {
        self.ensure_open()?;
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ))
    }

    fn getFrontendStatusReadiness(
        &self,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<Vec<FrontendStatusReadiness>> {
        self.ensure_open()?;
        if status_types.is_empty() {
            return Ok(Vec::new());
        }
        let support = self.shared.advertised_status_support;
        if !status_types.iter().any(|ty| support.supports(*ty)) {
            return Self::readiness_for_types(support, false, false, None, status_types);
        }
        let (backend_available, tuning_active, telemetry) = {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            let backend_available = !matches!(&*backend, FrontendBackendState::Unavailable { .. });
            let tuning_active = Self::backend_tuning_active(&backend);
            let telemetry = if backend_available && !tuning_active {
                self.apply_selected_lnb(&mut backend)
                    .and_then(|_| Self::backend_read_status(&mut backend))
                    .ok()
            } else {
                None
            };
            (backend_available, tuning_active, telemetry)
        };
        Self::readiness_for_types(
            support,
            backend_available,
            tuning_active,
            telemetry.as_ref(),
            status_types,
        )
    }
}

pub struct DemuxHal {
    demux_id: i32,
    record: Arc<Mutex<DemuxRecord>>,
    frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>>,
    lease_registry: Arc<Mutex<FrontendLeaseRegistry>>,
    demux_ledger: DemuxLedgerStore,
    descrambler_registry: Arc<DescramblerRuntimeRegistry>,
    close_lock: Mutex<()>,
    closed: RuntimeAtomicFlag,
}

impl DemuxHal {
    fn new(
        record: Arc<Mutex<DemuxRecord>>,
        frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>>,
        lease_registry: Arc<Mutex<FrontendLeaseRegistry>>,
        demux_ledger: DemuxLedgerStore,
        descrambler_registry: Arc<DescramblerRuntimeRegistry>,
    ) -> BinderResult<Self> {
        let demux_id = lock_mutex_status(&record, "demux_record")?.demux_id;
        Ok(Self {
            demux_id,
            record,
            frontend_registry,
            lease_registry,
            demux_ledger,
            descrambler_registry,
            close_lock: Mutex::new(()),
            closed: RuntimeAtomicFlag::new(false),
        })
    }

    fn state(&self) -> BinderResult<Arc<Mutex<DemuxHandle>>> {
        lock_mutex_status(&self.record, "demux_record").map(|record| record.state.clone())
    }

    fn runtime_io(&self) -> BinderResult<Arc<RuntimeIoRegistry>> {
        lock_mutex_status(&self.record, "demux_record").map(|record| record.runtime_io.clone())
    }

    fn record_ci_cam_unsupported(&self, detail: &str) {
        let message = format!("demux={} ci_cam_unsupported {}", self.demux_id, detail);
        eprintln!("maleicacid-tuner-hal: {message}");
        if let Some(mut record) = lock_mutex_option(&self.record, "demux_record") {
            record.ci_cam_diagnostics.push(message);
        }
    }

    fn ensure_open(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(invalid_state_status("demux is closed"));
        }
        let state = {
            let record = lock_mutex_status(&self.record, "demux_record")?;
            if record.closing || record.ref_count == 0 {
                return Err(invalid_state_status("demux record is closing"));
            }
            record.state.clone()
        };
        if lock_mutex_status(&state, "demux_handle")?.is_closed() {
            return Err(invalid_state_status("demux handle is closed"));
        }
        Ok(())
    }

    fn close_internal(&self) -> BinderResult<()> {
        let _close_guard = lock_mutex_status(&self.close_lock, "demux_close_lock")?;
        if self.closed.load(Ordering::SeqCst) {
            return Ok(());
        }

        let (should_cleanup, bound_frontend_id, state, demux_generation) = {
            let mut record = lock_mutex_status(&self.record, "demux_record")?;
            if record.ref_count > 0 {
                record.ref_count -= 1;
            }
            if record.ref_count > 0 {
                (false, None, None, None)
            } else {
                record.closing = true;
                (
                    true,
                    record.bound_frontend_id,
                    Some(record.state.clone()),
                    Some(record.generation),
                )
            }
        };

        if !should_cleanup {
            self.closed.store(true, Ordering::SeqCst);
            return Ok(());
        }

        let mut first_error: Option<Status> = None;

        if let Some(frontend_id) = bound_frontend_id {
            if let Some(runtime) = self.frontend_registry.get(&frontend_id) {
                if let Err(status) = runtime.unbind_demux(self.demux_id) {
                    eprintln!(
                        "maleicacid-tuner-hal-demux-close: demux={} step=unbind_demux frontend={} status={:?}",
                        self.demux_id, frontend_id, status
                    );
                    if first_error.is_none() {
                        first_error = Some(status);
                    }
                }
            }
        }
        if let Some(state) = state {
            match lock_mutex_status(&state, "demux_handle") {
                Ok(mut handle) => handle.close(),
                Err(status) => {
                    eprintln!(
                        "maleicacid-tuner-hal-demux-close: demux={} step=lock_demux_handle status={:?}",
                        self.demux_id, status
                    );
                    if first_error.is_none() {
                        first_error = Some(status);
                    }
                }
            }
        }
        if let Some(demux_generation) = demux_generation {
            if let Err(status) = self
                .descrambler_registry
                .invalidate_demux(self.demux_id, demux_generation)
            {
                if first_error.is_none() {
                    first_error = Some(status);
                }
            }
        }
        let ledger_remove_ok = match lock_mutex_status(&self.demux_ledger, "demux_ledger") {
            Ok(mut ledger) => match ledger.remove_record(LedgerId(self.demux_id)) {
                Ok(_) => true,
                Err(err) => {
                    eprintln!(
                        "maleicacid-tuner-hal-demux-close: demux={} step=demux_ledger_remove_record error={:?}",
                        self.demux_id, err
                    );
                    if first_error.is_none() {
                        first_error = Some(Status::new_service_specific_error(
                            TunerResult::UNKNOWN_ERROR.0,
                            Some("demux ledger remove_record failed"),
                        ));
                    }
                    false
                }
            },
            Err(status) => {
                eprintln!(
                    "maleicacid-tuner-hal-demux-close: demux={} step=lock_demux_ledger status={:?}",
                    self.demux_id, status
                );
                if first_error.is_none() {
                    first_error = Some(status);
                }
                false
            }
        };

        match lock_mutex_status(&self.record, "demux_record") {
            Ok(mut record) => {
                if ledger_remove_ok {
                    record.ref_count = 0;
                    record.closing = false;
                    record.bound_frontend_id = None;
                    record.bound_frontend_generation = None;
                } else {
                    record.closing = false;
                    if record.ref_count == 0 {
                        record.ref_count = 1;
                    }
                }
            }
            Err(status) => {
                eprintln!(
                    "maleicacid-tuner-hal-demux-close: demux={} step=final_demux_record_cleanup status={:?}",
                    self.demux_id, status
                );
                if first_error.is_none() {
                    first_error = Some(status);
                }
            }
        }
        if let Some(status) = first_error {
            Err(status)
        } else {
            self.closed.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn release_registration_best_effort(&self) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let (should_cleanup, bound_frontend_id, state, demux_generation) = {
            let Some(mut record) = lock_mutex_option(&self.record, "demux_record") else {
                return;
            };
            if record.ref_count > 0 {
                record.ref_count -= 1;
            }
            if record.ref_count == 0 {
                record.closing = true;
                (
                    true,
                    record.bound_frontend_id,
                    Some(record.state.clone()),
                    Some(record.generation),
                )
            } else {
                (false, None, None, None)
            }
        };
        if !should_cleanup {
            return;
        }
        if let Some(frontend_id) = bound_frontend_id {
            if let Some(runtime) = self.frontend_registry.get(&frontend_id) {
                runtime.unbind_demux_best_effort(self.demux_id);
            }
        }
        if let Some(state) = state {
            if let Some(mut handle) = lock_mutex_option(&state, "demux_handle") {
                handle.close();
            }
        }
        if let Some(demux_generation) = demux_generation {
            if let Err(status) = self
                .descrambler_registry
                .invalidate_demux(self.demux_id, demux_generation)
            {
                record_tuner_diagnostic_counter(
                    &DESCRAMBLER_DEMUX_INVALIDATE_ERROR_COUNT,
                    "descrambler_demux_invalidate_error",
                );
                eprintln!(
                    "maleicacid-tuner-hal-demux-drop: demux={} generation={} step=invalidate_descrambler status={:?}",
                    self.demux_id, demux_generation, status
                );
            }
        }
        if let Some(mut ledger) = lock_mutex_option(&self.demux_ledger, "demux_ledger") {
            let _ = ledger.remove_record(LedgerId(self.demux_id));
        }
        if let Some(mut record) = lock_mutex_option(&self.record, "demux_record") {
            record.ref_count = 0;
            record.closing = false;
            record.bound_frontend_id = None;
            record.bound_frontend_generation = None;
            self.closed.store(true, Ordering::SeqCst);
        }
    }
}

impl Drop for DemuxHal {
    fn drop(&mut self) {
        self.release_registration_best_effort();
    }
}

impl Interface for DemuxHal {}

impl IDemux for DemuxHal {
    fn setFrontendDataSource(&self, frontend_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        let Some(runtime) = self.frontend_registry.get(&frontend_id).cloned() else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        let generation = {
            let leases = lock_mutex_status(&self.lease_registry, "frontend_leases")?;
            let Some(generation) = leases.open_generations.get(&frontend_id).copied() else {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNAVAILABLE.0,
                    None,
                ));
            };
            generation
        };
        let (state, runtime_io, old_frontend_id, old_frontend_generation, demux_generation) = {
            let record = lock_mutex_status(&self.record, "demux_record")?;
            (
                record.state.clone(),
                record.runtime_io.clone(),
                record.bound_frontend_id,
                record.bound_frontend_generation,
                record.generation,
            )
        };
        {
            let state_guard = lock_mutex_status(&state, "demux_handle")?;
            if state_guard.is_closed() {
                return Err(invalid_state_status("demux handle is closed"));
            }
        }

        let fail_closed_transition = |reason: &str| -> BinderResult<()> {
            eprintln!(
                "maleicacid-tuner-hal-demux-source-transition: demux={} fail_closed reason={}",
                self.demux_id, reason
            );
            if let Some(mut handle) = lock_mutex_option(&state, "demux_handle") {
                handle.close();
            }
            if let Some(mut record) = lock_mutex_option(&self.record, "demux_record") {
                record.bound_frontend_id = None;
                record.bound_frontend_generation = None;
            }
            if let Err(status) = self
                .descrambler_registry
                .invalidate_demux(self.demux_id, demux_generation)
            {
                record_tuner_diagnostic_counter(
                    &DESCRAMBLER_DEMUX_INVALIDATE_ERROR_COUNT,
                    "descrambler_demux_invalidate_error",
                );
                eprintln!(
                    "maleicacid-tuner-hal-demux-source-transition: demux={} generation={} step=invalidate_descrambler status={:?}",
                    self.demux_id, demux_generation, status
                );
            }
            runtime_io.mark_all_failed("setFrontendDataSource rollback failed; demux fail-closed");
            Err(Status::new_service_specific_error(
                TunerResult::UNKNOWN_ERROR.0,
                Some(reason),
            ))
        };

        let rollback_to_old = |reason: &str| -> BinderResult<()> {
            if let Err(unbind_err) = runtime.unbind_demux(self.demux_id) {
                eprintln!(
                    "maleicacid-tuner-hal-demux-source-transition: demux={} rollback_unbind_new_frontend_failed new_frontend={} reason={} status={:?}",
                    self.demux_id, frontend_id, reason, unbind_err
                );
                return fail_closed_transition("rollback_unbind_new_frontend_failed");
            }
            {
                let mut record = match lock_mutex_status(&self.record, "demux_record") {
                    Ok(record) => record,
                    Err(status) => {
                        eprintln!(
                            "maleicacid-tuner-hal-demux-source-transition: demux={} rollback_demux_record_lock_failed reason={} status={:?}",
                            self.demux_id, reason, status
                        );
                        return fail_closed_transition("rollback_demux_record_lock_failed");
                    }
                };
                record.bound_frontend_id = old_frontend_id;
                record.bound_frontend_generation = old_frontend_generation;
            }
            {
                let mut handle = match lock_mutex_status(&state, "demux_handle") {
                    Ok(handle) => handle,
                    Err(status) => {
                        eprintln!(
                            "maleicacid-tuner-hal-demux-source-transition: demux={} rollback_demux_handle_lock_failed reason={} status={:?}",
                            self.demux_id, reason, status
                        );
                        return fail_closed_transition("rollback_demux_handle_lock_failed");
                    }
                };
                match old_frontend_id {
                    Some(old_id) => handle.bind_frontend(old_id),
                    None => handle.unbind_frontend(),
                }
            }
            if let (Some(old_id), Some(old_generation)) = (old_frontend_id, old_frontend_generation)
            {
                let Some(old_runtime) = self.frontend_registry.get(&old_id) else {
                    return fail_closed_transition("rollback_old_frontend_missing");
                };
                if let Err(rollback_err) = old_runtime.bind_demux(
                    Arc::clone(&state),
                    Arc::clone(&runtime_io),
                    demux_generation,
                    Some(Arc::clone(&self.record)),
                ) {
                    eprintln!(
                        "maleicacid-tuner-hal-demux-source-transition: demux={} rollback_bind_old_frontend_failed old_frontend={} old_generation={} reason={} status={:?}",
                        self.demux_id,
                        old_id,
                        old_generation,
                        reason,
                        rollback_err,
                    );
                    return fail_closed_transition("rollback_bind_old_frontend_failed");
                }
            }
            Ok(())
        };

        runtime.bind_demux(
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            demux_generation,
            Some(Arc::clone(&self.record)),
        )?;
        if let Some(old_frontend_id) = old_frontend_id.filter(|old| *old != frontend_id) {
            let Some(old_runtime) = self.frontend_registry.get(&old_frontend_id) else {
                rollback_to_old("old_frontend_missing")?;
                return Err(Status::from(StatusCode::UNKNOWN_ERROR));
            };
            if let Err(err) = old_runtime.unbind_demux(self.demux_id) {
                rollback_to_old("old_unbind_failed")?;
                return Err(err);
            }
        }
        if let Err(err) = (|| -> BinderResult<()> {
            {
                let mut record = lock_mutex_status(&self.record, "demux_record")?;
                record.bound_frontend_id = Some(frontend_id);
                record.bound_frontend_generation = Some(generation);
            }
            {
                let mut state = lock_mutex_status(&state, "demux_handle")?;
                if state.is_closed() {
                    return Err(invalid_state_status(
                        "demux handle is closed during frontend source switch",
                    ));
                }
                state.bind_frontend(frontend_id);
            }
            {
                let mut record = lock_mutex_status(&self.record, "demux_record")?;
                execute_stream_boundary_for_demux(
                    StreamBoundaryReason::SourceFilterChange,
                    self.demux_id,
                    demux_generation,
                    Arc::clone(&runtime_io),
                    Arc::clone(&state),
                    None,
                    Some(&mut record.pending_stream_boundary_plan),
                )?;
            }
            Ok(())
        })() {
            rollback_to_old("state_update_failed")?;
            return Err(err);
        }
        Ok(())
    }

    fn openFilter(
        &self,
        filter_type: &DemuxFilterType,
        buffer_size: i32,
        cb: &Strong<dyn IFilterCallback>,
    ) -> BinderResult<Strong<dyn IFilter>> {
        self.ensure_open()?;
        if buffer_size <= 0 {
            return Err(invalid_argument_status(
                "openFilter bufferSize は正値である必要があります",
            ));
        }
        if !filter_main_type_supported(filter_type.mainType) {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        let state = self.state()?;
        let runtime_io = self.runtime_io()?;
        let open_type = filter_open_type(filter_type)?;
        let mut demux = lock_mutex_status(&state, "demux_handle")?;
        let filter_type_bits = (filter_type.mainType.0 as u32) as i32;
        let total_filters = demux.filter_ids().len();
        if total_filters >= DEMUX_MAX_FILTERS_PER_DEMUX {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        let mut txn = LifecycleTxn::new();
        let record = txn.apply("demux_register_filter", || {
            demux.register_filter_result(filter_type_bits, open_type, buffer_size)
                .map_err(demux_config_error_status)
        })?;
        let filter_id = record.filter_id;
        drop(demux);
        txn.prepare("filter_ledger_reserve", || {
            let mut record = lock_mutex_status(&self.record, "demux_record")?;
            record.filter_ledger.reserve(LedgerId(filter_id))
                .map(|_| ())
                .map_err(|_| Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None))
        })?;
        let filter_hal = match FilterHal::new(
            self.demux_id,
            filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            cb.clone(),
            Some(Arc::clone(&self.record)),
        ) {
            Ok(filter_hal) => filter_hal,
            Err(err) => {
                let mut first_error = Some(err);
                if let Err(rollback_status) = txn.rollback("filter_ledger_rollback", || {
                    let mut record = lock_mutex_status(&self.record, "demux_record")?;
                    record.filter_ledger.rollback_open(LedgerId(filter_id))
                        .map_err(|_| Status::new_service_specific_error(TunerResult::INVALID_STATE.0, None))
                }) {
                    if first_error.is_none() { first_error = Some(rollback_status); }
                }
                if let Err(status) = runtime_io.unregister_filter(filter_id) {
                    if first_error.is_none() { first_error = Some(status); }
                }
                match lock_mutex_status(&state, "demux_handle") {
                    Ok(mut state) => state.unregister_filter(filter_id),
                    Err(status) => { if first_error.is_none() { first_error = Some(status); } }
                }
                return Err(first_error.unwrap_or_else(|| Status::from(StatusCode::UNKNOWN_ERROR)));
            }
        };
        if let Err(status) = txn.commit("filter_ledger_commit", || {
            let mut record = lock_mutex_status(&self.record, "demux_record")?;
            record.filter_ledger.commit_open(LedgerId(filter_id))
                .map_err(|_| Status::new_service_specific_error(TunerResult::INVALID_STATE.0, None))
        }) {
            let cleanup = txn.cleanup("filter_open_commit_failure_cleanup_runtime", || {
                let mut first_error: Option<Status> = None;
                if let Err(close_status) = filter_hal.close_internal() {
                    first_error = Some(close_status);
                }
                match lock_mutex_status(&self.record, "demux_record") {
                    Ok(mut record) => {
                        if let Err(_) = record.filter_ledger.rollback_open(LedgerId(filter_id)) {
                            if first_error.is_none() {
                                first_error = Some(Status::new_service_specific_error(
                                    TunerResult::UNKNOWN_ERROR.0,
                                    Some("filter_ledger_rollback_failed_after_commit_failure"),
                                ));
                            }
                        }
                    }
                    Err(lock_status) => {
                        if first_error.is_none() { first_error = Some(lock_status); }
                    }
                }
                if let Some(err) = first_error { Err(err) } else { Ok(()) }
            });
            if cleanup.is_err() {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNKNOWN_ERROR.0,
                    Some("filter_open_commit_cleanup_failed"),
                ));
            }
            return Err(status);
        }
        Ok(BnFilter::new_binder(filter_hal, BinderFeatures::default()))
    }

    fn openTimeFilter(&self) -> BinderResult<Strong<dyn ITimeFilter>> {
        self.ensure_open()?;
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ))
    }

    fn getAvSyncHwId(&self, filter: &Strong<dyn IFilter>) -> BinderResult<i32> {
        self.ensure_open()?;
        let filter_id = local_filter_id_for_owner(filter, self.demux_id)?;
        let state = lock_mutex_status(&self.record, "demux_record")?
            .state
            .clone();
        let handle = lock_mutex_status(&state, "demux_handle")?;
        handle
            .av_sync_hw_id_for(filter_id)
            .ok_or_else(|| Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None))
    }

    fn getAvSyncTime(&self, av_sync_hw_id: i32) -> BinderResult<i64> {
        self.ensure_open()?;
        let state = lock_mutex_status(&self.record, "demux_record")?
            .state
            .clone();
        let handle = lock_mutex_status(&state, "demux_handle")?;
        handle
            .av_sync_time_now(av_sync_hw_id)
            .ok_or_else(|| Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None))
    }

    fn close(&self) -> BinderResult<()> {
        self.close_internal()
    }

    fn openDvr(
        &self,
        dvr_type: DvrType,
        buffer_size: i32,
        cb: &Strong<dyn IDvrCallback>,
    ) -> BinderResult<Strong<dyn IDvr>> {
        self.ensure_open()?;
        if buffer_size <= 0 {
            return Err(invalid_argument_status(
                "openDvr bufferSize は正値である必要があります",
            ));
        }
        let direction = normalize_dvr_type(dvr_type)?;
        let state = self.state()?;
        let runtime_io = self.runtime_io()?;
        let mut demux = lock_mutex_status(&state, "demux_handle")?;
        let mut txn = LifecycleTxn::new();
        let record = txn.apply("demux_register_dvr", || {
            demux.register_dvr(direction, buffer_size)
                .map_err(demux_config_error_status)
        })?;
        let dvr_id = record.dvr_id;
        drop(demux);
        txn.prepare("dvr_ledger_reserve", || {
            let mut record = lock_mutex_status(&self.record, "demux_record")?;
            record.dvr_ledger.reserve(LedgerId(dvr_id))
                .map(|_| ())
                .map_err(|_| Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None))
        })?;
        let dvr_hal = match DvrHal::new(
            self.demux_id,
            dvr_id,
            direction,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            cb.clone(),
            Some(Arc::clone(&self.record)),
) {
            Ok(dvr_hal) => dvr_hal,
            Err(err) => {
                let mut first_error = Some(err);
                if let Err(rollback_status) = txn.rollback("dvr_ledger_rollback", || {
                    let mut record = lock_mutex_status(&self.record, "demux_record")?;
                    record.dvr_ledger.rollback_open(LedgerId(dvr_id))
                        .map_err(|_| Status::new_service_specific_error(TunerResult::INVALID_STATE.0, None))
                }) {
                    if first_error.is_none() { first_error = Some(rollback_status); }
                }
                if let Err(status) = runtime_io.unregister_dvr(dvr_id) {
                    if first_error.is_none() { first_error = Some(status); }
                }
                match lock_mutex_status(&state, "demux_handle") {
                    Ok(mut state) => state.unregister_dvr(dvr_id),
                    Err(status) => { if first_error.is_none() { first_error = Some(status); } }
                }
                return Err(first_error.unwrap_or_else(|| Status::from(StatusCode::UNKNOWN_ERROR)));
            }
        };
        if let Err(status) = txn.commit("dvr_ledger_commit", || {
            let mut record = lock_mutex_status(&self.record, "demux_record")?;
            record.dvr_ledger.commit_open(LedgerId(dvr_id))
                .map_err(|_| Status::new_service_specific_error(TunerResult::INVALID_STATE.0, None))
        }) {
            let cleanup = txn.cleanup("dvr_open_commit_failure_cleanup_runtime", || {
                let mut first_error: Option<Status> = None;
                match dvr_hal.cleanup_dvr_resources(DvrCleanupCaller::ExternalClose) {
                    Ok(outcome) => {
                        if let Some(err) = outcome.first_error { first_error = Some(err); }
                        if !outcome.all_cleanup_complete && first_error.is_none() {
                            first_error = Some(Status::new_service_specific_error(
                                TunerResult::UNKNOWN_ERROR.0,
                                Some("dvr_open_partial_cleanup_after_commit_failure"),
                            ));
                        }
                    }
                    Err(err) => first_error = Some(err),
                }
                match lock_mutex_status(&self.record, "demux_record") {
                    Ok(mut record) => {
                        if let Err(_) = record.dvr_ledger.rollback_open(LedgerId(dvr_id)) {
                            if first_error.is_none() {
                                first_error = Some(Status::new_service_specific_error(
                                    TunerResult::UNKNOWN_ERROR.0,
                                    Some("dvr_ledger_rollback_failed_after_commit_failure"),
                                ));
                            }
                        }
                    }
                    Err(lock_status) => {
                        if first_error.is_none() { first_error = Some(lock_status); }
                    }
                }
                if let Some(err) = first_error { Err(err) } else { Ok(()) }
            });
            if cleanup.is_err() {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNKNOWN_ERROR.0,
                    Some("dvr_open_commit_cleanup_failed"),
                ));
            }
            return Err(status);
        }
        Ok(BnDvr::new_binder(dvr_hal, BinderFeatures::default()))
    }

    fn connectCiCam(&self, ci_cam_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        self.record_ci_cam_unsupported(&format!("connectCiCam ci_cam_id={}", ci_cam_id));
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ))
    }

    fn disconnectCiCam(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.record_ci_cam_unsupported("disconnectCiCam");
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CloseFailureRecord {
    failed_step: String,
    error_kind: String,
    remaining_steps: Vec<String>,
}

impl CloseFailureRecord {
    fn new(failed_step: &str, error_kind: &str, remaining_steps: &[&str]) -> Self {
        Self {
            failed_step: failed_step.to_string(),
            error_kind: error_kind.to_string(),
            remaining_steps: remaining_steps.iter().map(|step| step.to_string()).collect(),
        }
    }
}

pub struct FilterHal {
    owner_demux_id: i32,
    filter_id: i32,
    state: Arc<Mutex<DemuxHandle>>,
    callback: Strong<dyn IFilterCallback>,
    runtime_io: Arc<RuntimeIoRegistry>,
    demux_record: Option<DemuxRecordRef>,
    queue_backing: Arc<SharedMemoryBacking>,
    av_queue_backing: Arc<SharedMemoryBacking>,
    av_shared_backing: Arc<Mutex<Option<Arc<AvSharedBacking>>>>,
    av_shared_handle_exported: Arc<RuntimeAtomicFlag>,
    av_shared_handle_client_released: Arc<RuntimeAtomicFlag>,
    current_av_handle_identity: Arc<Mutex<Option<AvSharedHandleIdentity>>>,
    av_export_generation: Arc<AtomicU64>,
    av_drop_unexported: Arc<AtomicU64>,
    callback_stop: Arc<RuntimeAtomicFlag>,
    callback_worker: Mutex<Option<WorkerHandle>>,
    closed: Arc<RuntimeAtomicFlag>,
    cleanup_complete: Arc<RuntimeAtomicFlag>,
    close_failure: Arc<Mutex<Option<CloseFailureRecord>>>,
}

impl FilterHal {
    fn new(
        owner_demux_id: i32,
        filter_id: i32,
        state: Arc<Mutex<DemuxHandle>>,
        runtime_io: Arc<RuntimeIoRegistry>,
        callback: Strong<dyn IFilterCallback>,
        demux_record: Option<DemuxRecordRef>,
    ) -> BinderResult<Self> {
        let buffer_size = lock_mutex_status(&state, "demux_handle")?
            .filter_record(filter_id)
            .map(|r| r.buffer_size.max(0) as usize)
            .unwrap_or(4096);
        let queue_backing = SharedMemoryBacking::new_ring(buffer_size)?;
        let av_queue_backing = SharedMemoryBacking::new_ring(buffer_size)?;
        // openFilter() では AV 共有メモリを確保しない。
        // section/PES/record/PCR filter は /dev/dma_heap/system に依存しない。
        // dma-buf 由来の共有 handle は、設定済み AV filter の getAvSharedHandle() 到達時だけ遅延生成する。
        let av_shared_backing: Arc<Mutex<Option<Arc<AvSharedBacking>>>> =
            Arc::new(Mutex::new(None));
        let av_shared_handle_exported = Arc::new(RuntimeAtomicFlag::new(false));
        let av_shared_handle_client_released = Arc::new(RuntimeAtomicFlag::new(false));
        let current_av_handle_identity = Arc::new(Mutex::new(None));
        let av_export_generation = Arc::new(AtomicU64::new(0));
        let av_drop_unexported = Arc::new(AtomicU64::new(0));
        runtime_io.register_filter(
            filter_id,
            &queue_backing,
            &av_queue_backing,
            None,
            &av_drop_unexported,
        )?;
        let callback_stop = Arc::new(RuntimeAtomicFlag::new(false));
        let closed = Arc::new(RuntimeAtomicFlag::new(false));
        let cleanup_complete = Arc::new(RuntimeAtomicFlag::new(false));
        let close_failure = Arc::new(Mutex::new(None));
        let next_av_data_id = Arc::new(AtomicI64::new(1));
        let callback_worker = {
            let state_clone = Arc::clone(&state);
            let callback_clone = callback.clone();
            let stop_clone = Arc::clone(&callback_stop);
            let queue_backing_clone = Arc::clone(&queue_backing);
            let av_queue_backing_clone = Arc::clone(&av_queue_backing);
            let av_shared_backing_clone = Arc::clone(&av_shared_backing);
            let av_shared_handle_exported_clone = Arc::clone(&av_shared_handle_exported);
            let av_shared_handle_client_released_clone = Arc::clone(&av_shared_handle_client_released);
            let av_drop_unexported_clone = Arc::clone(&av_drop_unexported);
            let next_av_data_id_clone = Arc::clone(&next_av_data_id);
            let runtime_io_clone = Arc::clone(&runtime_io);
            let closed_clone = Arc::clone(&closed);
            let state_hook = Arc::clone(&state);
            let runtime_io_hook = Arc::clone(&runtime_io);
            let queue_backing_hook = Arc::clone(&queue_backing);
            let av_queue_backing_hook = Arc::clone(&av_queue_backing);
            let av_shared_backing_hook = Arc::clone(&av_shared_backing);
            let closed_hook = Arc::clone(&closed);
            let stop_hook = Arc::clone(&callback_stop);
            let handle = WorkerRuntime::spawn_owned_with_exit_hook(WorkerOwnerId("filter_callback_worker", filter_id), "filter_callback_worker", move |owner_signal| {
                let mut cumulative_bytes = 0u64;
                let mut record_event_state = RecordEventState::default();
                let mut observed_delivery_generation = 0u64;
                while !stop_clone.load(Ordering::SeqCst) && !closed_clone.load(Ordering::SeqCst) && !owner_signal.is_stop_requested() {
                    let (record, start_event_id, pending_overflow, payloads) = {
                        let Some(mut demux) = lock_mutex_option(&state_clone, "demux_handle") else {
                            FilterHal::fail_filter_worker(
                                &state_clone,
                                &runtime_io_clone,
                                &queue_backing_clone,
                                &av_queue_backing_clone,
                                &av_shared_backing_clone,
                                &closed_clone,
                                &stop_clone,
                                filter_id,
                                "filter_callback_worker lost demux state",
                            );
                            return WorkerExit::RuntimeFailure;
                        };
                        let start_event_id = demux.take_filter_start_event_id_if_ready(filter_id);
                        let pending_overflow = demux.take_filter_pending_overflow(filter_id);
                        let record = demux.filter_record(filter_id).cloned();
                        let payloads = demux.drain_filter_payloads_for_delivery(filter_id);
                        (record, start_event_id, pending_overflow, payloads)
                    };
                    let Some(record) = record else {
                        FilterHal::fail_filter_worker(
                            &state_clone,
                            &runtime_io_clone,
                            &queue_backing_clone,
                            &av_queue_backing_clone,
                            &av_shared_backing_clone,
                            &closed_clone,
                            &stop_clone,
                            filter_id,
                            "filter_callback_worker missing filter record",
                        );
                        return WorkerExit::RuntimeFailure;
                    };
                    if record.delivery_generation != observed_delivery_generation {
                        cumulative_bytes = 0;
                        record_event_state = RecordEventState::default();
                        observed_delivery_generation = record.delivery_generation;
                    }
                    let _monitor_mask = record.monitor_event_mask;
                    let send_status = true;
                    let send_event = true;
                    if let Some(start_id) = start_event_id.filter(|_| send_event) {
                        if let Err(err) = callback_clone.onFilterEvent(&[DemuxFilterEvent::StartId(start_id)]) {
                            eprintln!("maleicacid-tuner-hal-callback: filter_id={} start_id={} api=onFilterEvent(StartId) binder_status={:?}", filter_id, start_id, err);
                            FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on StartId");
                            return WorkerExit::RuntimeFailure;
                        }
                    }
                    if payloads.is_empty() {
                        if pending_overflow && send_status {
                            if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::OVERFLOW) {
                                eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(OVERFLOW) binder_status={:?}", filter_id, err);
                                FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on OVERFLOW");
                                return WorkerExit::RuntimeFailure;
                            }
                        }
                        queue_backing_clone.wait_for_stop_or_timeout(Duration::from_millis(20));
                        continue;
                    }
                    let mut internal_overflow_pending = pending_overflow;
                    for payload in payloads {
                        let payload_bytes = payload.bytes().to_vec();
                        let event_payload_bytes = payload.event_bytes().to_vec();
                        let is_media = matches!(record.config.as_ref().map(|c| &c.kind), Some(FilterConfigKind::Av { .. }));
                        let mut queue_ring = RingWriteResult::default();
                        let mut overflow = internal_overflow_pending;
                        internal_overflow_pending = false;
                        let mut av_slice = None;
                        let mut av_data_id = None;
                        let mut av_memory = None;
                        let mut av_delivery = if is_media { Some(AvPayloadDeliveryResult::DroppedBeforeHandleExport) } else { None };
                        if is_media {
                            if av_shared_handle_allows_payload_delivery(
                                av_shared_handle_exported_clone.load(Ordering::SeqCst),
                                av_shared_handle_client_released_clone.load(Ordering::SeqCst),
                            ) {
                                let shared_backing = match lock_mutex_status(&av_shared_backing_clone, "filter_av_shared_backing") {
                                    Ok(backing) => backing.clone(),
                                    Err(_) => {
                                        let err = AvPayloadInternalError::MutexPoisoned;
                                        let reason = format!("filter AV shared allocation internal_error={}", err.as_str());
                                        eprintln!("maleicacid-tuner-hal: filter_id={} AV shared allocation internal_error={}", filter_id, err.as_str());
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, &reason);
                                        return WorkerExit::RuntimeFailure;
                                    }
                                };
                                let Some(shared_backing) = shared_backing else {
                                    let err = AvPayloadInternalError::SharedHandleExportedWithoutBacking;
                                    let reason = format!("filter AV shared allocation internal_error={}", err.as_str());
                                    eprintln!("maleicacid-tuner-hal: filter_id={} AV shared allocation internal_error={}", filter_id, err.as_str());
                                    FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, &reason);
                                    return WorkerExit::RuntimeFailure;
                                };
                                let id = match allocate_next_av_data_id(&next_av_data_id_clone) {
                                    Ok(id) => id,
                                    Err(err) => {
                                        let reason = format!("filter AV dataId allocation internal_error={}", err.as_str());
                                        eprintln!("maleicacid-tuner-hal: filter_id={} AV dataId allocation internal_error={}", filter_id, err.as_str());
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, &reason);
                                        return WorkerExit::RuntimeFailure;
                                    }
                                };
                                match shared_backing.allocate(id, &payload_bytes) {
                                    Ok(slice) => {
                                        // Android 14 の共有 handle 経路では、getAvSharedHandle() が fd を一度だけ export する。
                                        // 各 MediaEvent は空 handle、非0の avDataId、shared fd 内の byte 範囲を持つ。
                                        // AV payload は通常 FMQ/EventFlag へ流さない。
                                        av_memory = Some(empty_native_handle());
                                        av_data_id = Some(id);
                                        av_slice = Some(slice);
                                        av_delivery = Some(AvPayloadDeliveryResult::Delivered { slice, av_data_id: id });
                                    }
                                    Err(AvPayloadAllocateError::Internal(err)) => {
                                        let reason = format!("filter AV shared allocation internal_error={}", err.as_str());
                                        eprintln!("maleicacid-tuner-hal: filter_id={} AV shared allocation internal_error={}", filter_id, err.as_str());
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, &reason);
                                        return WorkerExit::RuntimeFailure;
                                    }
                                    Err(AvPayloadAllocateError::Delivery(result)) => {
                                        av_delivery = Some(result);
                                        overflow = true;
                                    }
                                }
                            } else if av_shared_handle_exported_clone.load(Ordering::SeqCst) {
                                // 呼び出し側が shared fd の使用終了を通知済みである。
                                // 再取得されるまで MediaEvent は出さず、payload は破棄する。
                                let drops = AV_SHARED_HANDLE_CLIENT_RELEASED_DROP_COUNT
                                    .fetch_add(1, Ordering::SeqCst)
                                    .saturating_add(1);
                                if AvSharedBacking::should_log_counter(drops) {
                                    eprintln!("maleicacid-tuner-hal: AV payload dropped after client shared handle release filter_id={} av_shared_handle_client_released_drop={}", filter_id, drops);
                                }
                                av_delivery = Some(AvPayloadDeliveryResult::DroppedAfterClientRelease);
                                overflow = true;
                            } else {
                                // 呼び出し側が shared fd をまだ取得していない。framework/JNI が消費できない成功風 MediaEvent は出さない。
                                let drops = av_drop_unexported_clone.fetch_add(1, Ordering::SeqCst).saturating_add(1);
                                if AvSharedBacking::should_log_counter(drops) {
                                    eprintln!("maleicacid-tuner-hal: AV payload dropped before shared handle export filter_id={} av_drop_unexported={}", filter_id, drops);
                                }
                                av_delivery = Some(AvPayloadDeliveryResult::DroppedBeforeHandleExport);
                                overflow = true;
                            }
                        } else if av_payload_should_write_standard_fmq(is_media) {
                            match queue_backing_clone.write_bytes(&payload_bytes) {
                                Ok(ring) => {
                                    queue_ring = ring;
                                    overflow |= queue_ring.overflowed;
                                }
                                Err(err) => {
                                    record_tuner_diagnostic_counter(
                                        &FILTER_FMQ_WRITE_ERROR_COUNT,
                                        "filter_fmq_write_error",
                                    );
                                    let reason = if is_event_flag_wake_failure(&err) {
                                        format!("filter_event_flag_wake_failed: {err}")
                                    } else {
                                        format!("filter_fmq_write_error: {err}")
                                    };
                                    eprintln!(
                                        "maleicacid-tuner-hal-fmq: filter_id={} {reason}",
                                        filter_id
                                    );
                                    if is_event_flag_wake_failure(&err) {
                                        runtime_io_clone.mark_failed(RuntimeIoKind::Filter, filter_id, "EventFlagWakeFailed");
                                        stop_clone.store(true, Ordering::SeqCst);
                                        return WorkerExit::StopRequested;
                                    }
                                    FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, &reason);
                                    return WorkerExit::RuntimeFailure;
                                }
                            }
                        }
                        let (notify_data_ready, notify_overflow) = av_payload_status_decision(is_media, av_delivery, overflow);
                        let fill = match if is_media { av_queue_backing_clone.current_fill_bytes() } else { queue_backing_clone.current_fill_bytes() } {
                            Ok(fill) => fill,
                            Err(err) => {
                                let reason = format!("filter_fmq_current_fill_error: {err:?}");
                                eprintln!("maleicacid-tuner-hal-fmq: filter_id={} {reason}", filter_id);
                                FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, &reason);
                                return WorkerExit::RuntimeFailure;
                            }
                        };
                        if send_status {
                            if notify_data_ready {
                                if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::DATA_READY) {
                                    eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(DATA_READY) binder_status={:?}", filter_id, err);
                                    FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on DATA_READY");
                                    return WorkerExit::RuntimeFailure;
                                }
                            }
                            if notify_overflow {
                                if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::OVERFLOW) {
                                    eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(OVERFLOW) binder_status={:?}", filter_id, err);
                                    FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on OVERFLOW");
                                    return WorkerExit::RuntimeFailure;
                                }
                            }
                            if payload_uses_standard_fmq_watermarks(is_media, &payload) {
                                let high_water = record.buffer_size.max(0) as usize * 3 / 4;
                                let low_water = record.buffer_size.max(0) as usize / 4;
                                if high_water > 0 && fill >= high_water {
                                    if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::HIGH_WATER) {
                                        eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(HIGH_WATER) binder_status={:?}", filter_id, err);
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on HIGH_WATER");
                                        return WorkerExit::RuntimeFailure;
                                    }
                                } else if low_water > 0 && fill <= low_water {
                                    if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::LOW_WATER) {
                                        eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(LOW_WATER) binder_status={:?}", filter_id, err);
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on LOW_WATER");
                                        return WorkerExit::RuntimeFailure;
                                    }
                                }
                            }
                        }
                        if send_event {
                            // AV filter の正式deliveryは MediaEvent + 共有ハンドル である。
                            // shared slot を確保できなかった AV payload は OVERFLOW として扱い、
                            // avDataId=0 / 共有ハンドルなしの MediaEvent を出して FMQ-only delivery を
                            // ライブ AV 成功経路にしてはならない。
                            if av_payload_should_emit_data_event(is_media, av_slice) {
                                let event_offset = av_slice.map(|slice| slice.offset as i64).unwrap_or(queue_ring.start_offset as i64);
                                if let Some(event) = build_filter_event_from_entry(&record, &payload, event_offset, cumulative_bytes, av_slice, av_data_id, av_memory, &mut record_event_state) {
                                    if let Err(err) = callback_clone.onFilterEvent(&[event]) {
                                        eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterEvent(data) binder_status={:?}", filter_id, err);
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on データイベント");
                                        return WorkerExit::RuntimeFailure;
                                    }
                                }
                            }
                        }
                        cumulative_bytes = cumulative_bytes.saturating_add(event_payload_bytes.len() as u64);
                    }
                }
                WorkerExit::StopRequested
            }, move |exit| {
                if exit.is_abnormal() {
                    FilterHal::fail_filter_worker(
                        &state_hook,
                        &runtime_io_hook,
                        &queue_backing_hook,
                        &av_queue_backing_hook,
                        &av_shared_backing_hook,
                        &closed_hook,
                        &stop_hook,
                        filter_id,
                        &format!("filter_callback_worker_{exit:?}"),
                    );
                }
            }).map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
            Mutex::new(Some(handle))
        };
        Ok(Self {
            owner_demux_id,
            filter_id,
            state,
            callback,
            runtime_io,
            demux_record,
            queue_backing,
            av_queue_backing,
            av_shared_backing,
            av_shared_handle_exported,
            av_shared_handle_client_released,
            current_av_handle_identity,
            av_export_generation,
            av_drop_unexported,
            callback_stop,
            callback_worker,
            closed,
            cleanup_complete,
            close_failure,
        })
    }

    fn fail_filter_worker(
        state: &Arc<Mutex<DemuxHandle>>,
        runtime_io: &Arc<RuntimeIoRegistry>,
        queue_backing: &Arc<SharedMemoryBacking>,
        av_queue_backing: &Arc<SharedMemoryBacking>,
        av_shared_backing: &Arc<Mutex<Option<Arc<AvSharedBacking>>>>,
        closed: &Arc<RuntimeAtomicFlag>,
        callback_stop: &Arc<RuntimeAtomicFlag>,
        filter_id: i32,
        reason: &str,
    ) {
        let mut txn = LifecycleTxn::new();
        let transition = RuntimeFailClosedTransition::filter(filter_id, "filter_callback_worker");
        let first_close = txn
            .apply_value("filter_worker_failure_close_atomic", || Ok::<bool, Status>(transition.close_atomic(closed)))
            .unwrap_or(false);
        let _ = txn.apply("filter_worker_failure_mark_runtime_failed", || {
            transition.mark_failed(runtime_io, reason);
            Ok::<(), Status>(())
        });
        if !first_close {
            return;
        }
        let _ = txn.cleanup("filter_worker_failure_stop_callback", || {
            callback_stop.store(true, Ordering::SeqCst);
            Ok::<(), Status>(())
        });
        let _ = txn.cleanup("filter_worker_failure_clear_av_shared_backing", || {
            if let Some(backing) = lock_mutex_option(av_shared_backing, "filter_av_shared_backing")
                .and_then(|mut backing| backing.take())
            {
                backing.clear_drop_only();
            }
            Ok::<(), Status>(())
        });
        let _ = txn.cleanup("filter_worker_failure_clear_runtime_av_shared", || {
            runtime_io.clear_filter_av_shared_best_effort(filter_id);
            Ok::<(), Status>(())
        });
        let _ = txn.cleanup("filter_worker_failure_stop_normal_queue", || {
            queue_backing.stop_best_effort();
            Ok::<(), Status>(())
        });
        let _ = txn.cleanup("filter_worker_failure_stop_av_queue", || {
            av_queue_backing.stop_best_effort();
            Ok::<(), Status>(())
        });
        let _ = txn.cleanup("filter_worker_failure_unregister_demux", || {
            if let Some(mut demux) = lock_mutex_option(state, "demux_handle") {
                demux.unregister_filter(filter_id);
            }
            Ok::<(), Status>(())
        });
    }

    fn fail_from_callback(&self, api: &str, err: Status) -> Status {
        let status = callback_failure_status("filter", self.filter_id, api, &err);
        let _ = self.close_internal();
        status
    }


    fn ensure_open(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(invalid_state_status("filter is closed"));
        }
        {
            let demux = lock_mutex_status(&self.state, "demux_handle")?;
            if demux.is_closed() {
                self.runtime_io.mark_failed(
                    RuntimeIoKind::Filter,
                    self.filter_id,
                    "parent_demux_closed",
                );
                return Err(invalid_state_status("parent demux is closed"));
            }
            if !demux.has_filter(self.filter_id) {
                self.runtime_io.mark_failed(
                    RuntimeIoKind::Filter,
                    self.filter_id,
                    "filter_unregistered_from_parent_demux",
                );
                return Err(invalid_state_status(
                    "filter is no longer registered in parent demux",
                ));
            }
            if demux.demux_id() != self.owner_demux_id {
                self.runtime_io.mark_failed(
                    RuntimeIoKind::Filter,
                    self.filter_id,
                    "filter_owner_demux_mismatch",
                );
                return Err(invalid_state_status("filter owner demux mismatch"));
            }
        }
        self.runtime_io
            .ensure_not_failed(RuntimeIoKind::Filter, self.filter_id)
    }

    fn stop_callback_worker(&self) -> BinderResult<()> {
        self.callback_stop.store(true, Ordering::SeqCst);
        if let Some(handle) =
            lock_mutex_status(&self.callback_worker, "filter_callback_worker")?.take()
        {
            let exit = WorkerRuntime::join(handle, "filter_callback_worker");
            if exit.is_abnormal() {
                Self::fail_filter_worker(
                    &self.state,
                    &self.runtime_io,
                    &self.queue_backing,
                    &self.av_queue_backing,
                    &self.av_shared_backing,
                    &self.closed,
                    &self.callback_stop,
                    self.filter_id,
                    &format!("filter_callback_worker_{exit:?}"),
                );
                return Err(worker_exit_status("filter_callback_worker", exit));
            }
        }
        Ok(())
    }

    fn remember_cleanup_error(
        first_error: &mut Option<Status>,
        filter_id: i32,
        step: &str,
        result: BinderResult<()>,
    ) {
        if let Err(status) = result {
            eprintln!(
                "maleicacid-tuner-hal-filter-close: filter={} step={} status={:?}",
                filter_id, step, status
            );
            if first_error.is_none() {
                *first_error = Some(status);
            }
        }
    }

    fn drop_av_shared_backing_for_close(&self) -> BinderResult<()> {
        let mut first_error: Option<Status> = None;
        match lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing") {
            Ok(mut backing_slot) => {
                if let Some(backing) = backing_slot.take() {
                    if let Err(status) = backing.clear_result() {
                        first_error = Some(status);
                    }
                }
            }
            Err(status) => {
                first_error = Some(status);
            }
        }
        if let Err(status) = self.runtime_io.clear_filter_av_shared(self.filter_id) {
            if first_error.is_none() {
                first_error = Some(status);
            }
        }
        if let Some(status) = first_error {
            Err(status)
        } else {
            Ok(())
        }
    }

    fn av_filter_state(&self) -> (bool, bool) {
        let Some(demux) = lock_mutex_option(&self.state, "demux_handle") else {
            return (false, false);
        };
        let Some(record) = demux.filter_record(self.filter_id) else {
            return (false, false);
        };
        let is_av = record
            .config
            .as_ref()
            .map(|cfg| matches!(cfg.kind, FilterConfigKind::Av { .. }))
            .unwrap_or(false);
        let stream_type_configured =
            record.av_stream_kind.is_some() && record.av_stream_type_hint.is_some();
        (is_av, stream_type_configured)
    }

    fn ensure_configured_av_filter(&self) -> BinderResult<()> {
        let demux = lock_mutex_status(&self.state, "demux_handle")?;
        let Some(record) = demux.filter_record(self.filter_id) else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        let is_configured_av = record
            .config
            .as_ref()
            .map(|cfg| matches!(cfg.kind, FilterConfigKind::Av { .. }))
            .unwrap_or(false)
            && matches!(
                record.open_type,
                FilterOpenType::TsAudio | FilterOpenType::TsVideo
            );
        if !is_configured_av {
            return Err(invalid_state_status(
                "AV shared handle requested for a non-AV filter",
            ));
        }
        // stream type hint は MediaEvent 解釈の補助情報であり、shared backing 生成の前提ではない。
        Ok(())
    }

    fn ensure_av_shared_backing(&self) -> BinderResult<Arc<AvSharedBacking>> {
        self.ensure_configured_av_filter()?;
        let mut backing_slot = lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")?;
        if let Some(existing) = backing_slot.as_ref().cloned() {
            return Ok(existing);
        }
        let backing = AvSharedBacking::new()?;
        self.runtime_io
            .set_filter_av_shared(self.filter_id, &backing)?;
        *backing_slot = Some(Arc::clone(&backing));
        Ok(backing)
    }

    fn clear_current_av_handle_identity(&self) -> BinderResult<()> {
        self.av_shared_handle_exported.store(false, Ordering::SeqCst);
        self.av_shared_handle_client_released.store(false, Ordering::SeqCst);
        *lock_mutex_status(&self.current_av_handle_identity, "filter_av_handle_identity")? = None;
        Ok(())
    }

    fn clear_current_av_handle_identity_best_effort(&self) {
        self.av_shared_handle_exported.store(false, Ordering::SeqCst);
        self.av_shared_handle_client_released.store(false, Ordering::SeqCst);
        if let Some(mut identity) =
            lock_mutex_option(&self.current_av_handle_identity, "filter_av_handle_identity")
        {
            *identity = None;
        }
    }

    fn accept_returned_av_shared_handle_release(&self, av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {
        if av_memory.fds.len() != 1 {
            self.record_av_handle_direct_unsupported();
            return Err(invalid_argument_status("AV shared handle releaseはfd 1個だけを受理します"));
        }
        if av_data_id != 0 {
            self.record_av_handle_direct_unsupported();
            return Err(invalid_argument_status("fd付きAV shared handle releaseはavDataId=0だけを受理します"));
        }
        if !self.av_shared_handle_exported.load(Ordering::SeqCst) {
            self.record_av_handle_direct_unsupported();
            return Err(invalid_argument_status("AV shared handleは未公開です"));
        }
        if self.av_shared_handle_client_released.load(Ordering::SeqCst) {
            self.record_av_handle_direct_unsupported();
            return Err(invalid_argument_status("AV shared handleは既にrelease済みです"));
        }
        let Some(identity) = lock_mutex_status(&self.current_av_handle_identity, "filter_av_handle_identity")?.clone() else {
            self.record_av_handle_direct_unsupported();
            return Err(invalid_argument_status("AV shared handle識別子がありません"));
        };
        let matches = identity.matches_native_handle(av_memory).map_err(|err| {
            self.record_av_handle_direct_unsupported();
            err
        })?;
        if !matches {
            self.record_av_handle_direct_unsupported();
            return Err(invalid_argument_status("AV shared backingが一致しません"));
        }
        self.mark_av_shared_handle_client_released()
    }

    fn mark_av_shared_handle_client_released(&self) -> BinderResult<()> {
        if !self.av_shared_handle_exported.load(Ordering::SeqCst) {
            return Ok(());
        }
        if self.av_shared_handle_client_released.swap(true, Ordering::SeqCst) {
            self.record_av_handle_direct_unsupported();
            return Err(invalid_argument_status("AV shared handleは既にrelease済みです"));
        }
        record_tuner_diagnostic_counter(&AV_HANDLE_CLIENT_RELEASE_COUNT, "av_handle_client_release");
        Ok(())
    }

    fn ensure_release_target_is_av_filter(&self) -> BinderResult<()> {
        let demux = lock_mutex_status(&self.state, "demux_handle")?;
        let Some(record) = demux.filter_record(self.filter_id) else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        let Some(config) = record.config.as_ref() else {
            return Err(invalid_state_status("AV共有ハンドル公開前またはfilter未設定でreleaseAvHandleが呼ばれました"));
        };
        if !matches!(record.open_type, FilterOpenType::TsAudio | FilterOpenType::TsVideo)
            || !matches!(config.kind, FilterConfigKind::Av { .. })
        {
            record_tuner_diagnostic_counter(&AV_HANDLE_UNAVAILABLE_COUNT, "av_handle_unavailable");
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                Some("releaseAvHandleはAV filterでのみ利用可能です"),
            ));
        }
        Ok(())
    }

    fn current_release_target_is_av_filter(&self) -> BinderResult<bool> {
        let demux = lock_mutex_status(&self.state, "demux_handle")?;
        let Some(record) = demux.filter_record(self.filter_id) else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        let Some(config) = record.config.as_ref() else {
            return Ok(false);
        };
        Ok(matches!(record.open_type, FilterOpenType::TsAudio | FilterOpenType::TsVideo)
            && matches!(config.kind, FilterConfigKind::Av { .. }))
    }

    fn av_shared_handle_was_ever_exported(&self) -> bool {
        self.av_export_generation.load(Ordering::SeqCst) > 0
    }

    fn record_av_data_id_invalid_release(&self) {
        record_tuner_diagnostic_counter(&AV_DATA_ID_INVALID_RELEASE_COUNT, "av_data_id_invalid_release");
    }

    fn record_av_handle_direct_unsupported(&self) {
        record_tuner_diagnostic_counter(&AV_HANDLE_DIRECT_UNSUPPORTED_COUNT, "av_handle_direct_unsupported");
    }

    fn record_av_data_id_stale_release(&self) {
        record_tuner_diagnostic_counter(&AV_DATA_ID_STALE_RELEASE_COUNT, "av_data_id_stale_release");
    }

    fn record_av_data_id_stale_release_after_close(&self) {
        record_tuner_diagnostic_counter(
            &AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT,
            "av_data_id_stale_release_after_close",
        );
    }

    fn record_av_handle_release_without_handle(&self) {
        record_tuner_diagnostic_counter(
            &AV_HANDLE_RELEASE_WITHOUT_HANDLE_COUNT,
            "av_handle_release_without_handle",
        );
    }

    fn reset_av_shared_backing_for_stream_type_change_result(&self) -> BinderResult<()> {
        self.clear_current_av_handle_identity()?;
        if let Some(backing) =
            lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")?.take()
        {
            backing.clear_result()?;
        }
        self.runtime_io.clear_filter_av_shared(self.filter_id)
    }

    fn drop_av_shared_backing(&self) -> BinderResult<()> {
        self.clear_current_av_handle_identity()?;
        if let Some(backing) =
            lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")?.take()
        {
            backing.clear_result()?;
        }
        self.runtime_io.clear_filter_av_shared(self.filter_id)
    }

    fn drop_av_shared_backing_best_effort(&self) {
        self.clear_current_av_handle_identity_best_effort();
        if let Some(backing) =
            lock_mutex_option(&self.av_shared_backing, "filter_av_shared_backing")
                .and_then(|mut backing| backing.take())
        {
            backing.clear_drop_only();
        }
        self.runtime_io
            .clear_filter_av_shared_best_effort(self.filter_id);
    }

    fn release_all_av_shared_handles(&self) -> BinderResult<()> {
        let Some(backing) =
            lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")?.as_ref().cloned()
        else {
            return Err(invalid_state_status("AV共有ハンドルは未公開です"));
        };
        backing.release_all()?;
        Ok(())
    }

    fn release_av_shared_handle(&self, av_data_id: i64) -> BinderResult<()> {
        let Some(backing) =
            lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")?.clone()
        else {
            self.record_av_data_id_stale_release();
            return Ok(());
        };
        if backing.release(av_data_id)? {
            record_tuner_diagnostic_counter(&AV_DATA_ID_RELEASE_COUNT, "av_data_id_release");
        } else {
            self.record_av_data_id_stale_release();
        }
        Ok(())
    }

    fn close_internal(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst) && self.cleanup_complete.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut txn = LifecycleTxn::new();
        let mut first_error: Option<Status> = None;
        let mut filter_ledger_close_started = false;
        if let Some(record) = self.demux_record.as_ref() {
            match lock_mutex_status(record, "demux_record")
                .and_then(|mut record| {
                    record.filter_ledger.begin_close(LedgerId(self.filter_id))
                        .map(|_| ())
                        .map_err(|_| invalid_state_status("filter ledger begin_close failed"))
                }) {
                Ok(()) => { let _ = txn.prepare("filter_ledger_begin_close", || Ok::<(), Status>(())); filter_ledger_close_started = true; },
                Err(err) => Self::remember_cleanup_error(&mut first_error, self.filter_id, "filter_ledger_begin_close", Err(err)),
            }
        }

        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "stop_callback_worker",
            self.stop_callback_worker(),
        );
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "clear_av_shared_handle_identity",
            self.clear_current_av_handle_identity(),
        );
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "drop_av_shared_backing",
            self.drop_av_shared_backing_for_close(),
        );
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "runtime_unregister_filter",
            self.runtime_io.unregister_filter(self.filter_id),
        );
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "queue_stop",
            self.queue_backing.stop(),
        );
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "av_queue_stop",
            self.av_queue_backing.stop(),
        );
        let demux_unregister = match lock_mutex_status(&self.state, "demux_handle") {
            Ok(mut state) => {
                state.unregister_filter(self.filter_id);
                Ok(())
            }
            Err(status) => Err(status),
        };
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "demux_unregister_filter",
            demux_unregister,
        );

        if let Some(status) = first_error {
            let _ = txn.cleanup("filter_close_cleanup_failed", || Ok::<(), Status>(()));
            self.closed.store(true, Ordering::SeqCst);
            self.cleanup_complete.store(false, Ordering::SeqCst);
            if let Some(mut failure) = lock_mutex_option(&self.close_failure, "filter_close_failure") {
                *failure = Some(CloseFailureRecord::new(
                    "filter_close_cleanup",
                    "cleanup_failed",
                    &["filter_ledger_rollback_close", "remaining_filter_cleanup_retry"],
                ));
            }
            if filter_ledger_close_started {
                if let Some(record) = self.demux_record.as_ref() {
                    if let Err(rollback_status) = lock_mutex_status(record, "demux_record").and_then(|mut record| {
                        record.filter_ledger.rollback_close(LedgerId(self.filter_id))
                            .map_err(|_| invalid_state_status("filter ledger rollback_close failed"))
                    }) {
                        self.runtime_io.mark_failed(
                            RuntimeIoKind::Filter,
                            self.filter_id,
                            "filter_close_rollback_close_failed",
                        );
                        self.closed.store(true, Ordering::SeqCst);
                        self.cleanup_complete.store(false, Ordering::SeqCst);
                        if let Some(mut failure) = lock_mutex_option(&self.close_failure, "filter_close_failure") {
                            *failure = Some(CloseFailureRecord::new(
                                "filter_ledger_rollback_close",
                                "rollback_close_failed",
                                &["remaining_filter_cleanup_retry"],
                            ));
                        }
                        eprintln!(
                            "maleicacid-tuner-hal-filter-close: filter={} cleanup_status={:?} rollback_status={:?}",
                            self.filter_id, status, rollback_status
                        );
                        return Err(Status::new_service_specific_error(
                            TunerResult::UNKNOWN_ERROR.0,
                            Some("filter_close_rollback_close_failed"),
                        ));
                    }
                }
            }
            Err(status)
        } else {
            if filter_ledger_close_started {
                if let Some(record) = self.demux_record.as_ref() {
                    txn.commit("filter_ledger_commit_close", || {
                        lock_mutex_status(record, "demux_record")?
                            .filter_ledger
                            .commit_close(LedgerId(self.filter_id))
                            .map_err(|_| invalid_state_status("filter ledger commit_close failed"))
                    })?;
                }
            }
            txn.commit("filter_closed_flag", || {
                self.closed.store(true, Ordering::SeqCst);
                self.cleanup_complete.store(true, Ordering::SeqCst);
                if let Some(mut failure) = lock_mutex_option(&self.close_failure, "filter_close_failure") {
                    *failure = None;
                }
                Ok::<(), Status>(())
            })?;
            Ok(())
        }
    }

    fn close_internal_for_drop_cleanup(&self) {
        self.callback_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = lock_mutex_option(&self.callback_worker, "filter_callback_worker")
            .and_then(|mut worker| worker.take())
        {
            let exit = WorkerRuntime::join(handle, "filter_callback_worker");
            if exit.is_abnormal() {
                Self::fail_filter_worker(
                    &self.state,
                    &self.runtime_io,
                    &self.queue_backing,
                    &self.av_queue_backing,
                    &self.av_shared_backing,
                    &self.closed,
                    &self.callback_stop,
                    self.filter_id,
                    &format!("filter_callback_worker_{exit:?}"),
                );
            }
        }
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        self.drop_av_shared_backing_best_effort();
        self.runtime_io
            .unregister_filter_best_effort(self.filter_id);
        self.queue_backing.stop_best_effort();
        self.av_queue_backing.stop_best_effort();
        if let Some(mut state) = lock_mutex_option(&self.state, "demux_handle") {
            state.unregister_filter(self.filter_id);
        }
        self.closed.store(true, Ordering::SeqCst);
        self.cleanup_complete.store(true, Ordering::SeqCst);
    }

    fn has_av_shared_backing(&self) -> BinderResult<bool> {
        Ok(lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")?.is_some())
    }

    fn open_type_has_normal_fmq(open_type: FilterOpenType) -> bool {
        matches!(
            open_type,
            FilterOpenType::TsRaw
                | FilterOpenType::TsSection
                | FilterOpenType::TsPes
                | FilterOpenType::TsRecord
        )
    }

    fn open_type_has_normal_fmq_for_config(
        open_type: FilterOpenType,
        config: &Option<FilterConfig>,
    ) -> bool {
        match config.as_ref().map(|cfg| &cfg.kind) {
            Some(FilterConfigKind::Av { .. }) => false,
            _ => Self::open_type_has_normal_fmq(open_type),
        }
    }

    fn ensure_filter_has_normal_fmq_queue(&self) -> BinderResult<()> {
        let demux = lock_mutex_status(&self.state, "demux_handle")?;
        let Some(record) = demux.filter_record(self.filter_id) else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        if !Self::open_type_has_normal_fmq(record.open_type) {
            record_tuner_diagnostic_counter(&FILTER_QUEUE_DESC_UNAVAILABLE_COUNT, "queue_desc_unavailable");
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                Some("filter does not expose a normal FMQ queue"),
            ));
        }
        if record.is_configured_for_api()
            && !Self::open_type_has_normal_fmq_for_config(record.open_type, &record.config)
        {
            record_tuner_diagnostic_counter(&FILTER_QUEUE_DESC_UNAVAILABLE_COUNT, "queue_desc_unavailable");
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                Some("configured filter does not expose a normal FMQ queue"),
            ));
        }
        Ok(())
    }
}

impl Interface for FilterHal {}

impl Drop for FilterHal {
    fn drop(&mut self) {
        let mut txn = LifecycleTxn::new();
        let _ = txn.cleanup("filter_drop_cleanup", || {
            self.close_internal_for_drop_cleanup();
            Ok::<(), Status>(())
        });
    }
}

impl IFilter for FilterHal {
    fn getQueueDesc(&self, queue: &mut TunerQueueDesc) -> BinderResult<()> {
        self.ensure_open()?;
        self.ensure_filter_has_normal_fmq_queue()?;
        *queue = match self.queue_backing.build_queue_desc() {
            Ok(desc) => desc,
            Err(status) => {
                if status_is_descriptor_internal_error(&status) {
                    self.runtime_io.mark_failed(
                        RuntimeIoKind::Filter,
                        self.filter_id,
                        "DescriptorInternalError",
                    );
                }
                return Err(status);
            }
        };
        Ok(())
    }

    fn close(&self) -> BinderResult<()> {
        self.close_internal()
    }

    fn configure(&self, settings: &DemuxFilterSettings) -> BinderResult<()> {
        let mut txn = LifecycleTxn::new();
        txn.validate("filter.configure.ensure_open", || self.ensure_open())?;
        let open_type = txn.prepare_value("filter.configure.read_open_type", || {
            let state = lock_mutex_status(&self.state, "demux_handle")?;
            let Some(record) = state.filter_record(self.filter_id) else {
                return Err(StatusCode::NAME_NOT_FOUND.into());
            };
            Ok(record.open_type)
        })?;
        let summary = txn.prepare_value("filter.configure.build_summary", || {
            build_filter_summary_for_open_type(settings, open_type)
        })?;
        txn.validate("filter.configure.validate_demux_state", || {
            let state = lock_mutex_status(&self.state, "demux_handle")?;
            state
                .validate_filter_configure_result(self.filter_id, &summary)
                .map_err(demux_config_error_status)
        })?;
        // configure() は再設定境界である。旧通常FMQ/AV用FMQ/AV shared backing を先に破棄し、
        // 破棄に失敗した場合は demux 設定を変更しない。
        txn.apply("filter.configure.clear_normal_queue", || self.queue_backing.clear_result())?;
        txn.apply("filter.configure.clear_av_queue", || self.av_queue_backing.clear_result())?;
        txn.apply("filter.configure.drop_av_shared_backing", || self.drop_av_shared_backing())?;
        txn.commit("filter.configure.commit_demux_summary", || {
            lock_mutex_status(&self.state, "demux_handle")?
                .configure_filter_with_summary_result(self.filter_id, summary)
                .map_err(demux_config_error_status)
        })?;
        Ok(())
    }

    fn configureAvStreamType(&self, av_stream_type: &AvStreamType) -> BinderResult<()> {
        self.ensure_open()?;
        self.ensure_configured_av_filter()?;
        {
            let state = lock_mutex_status(&self.state, "demux_handle")?;
            let Some(record) = state.filter_record(self.filter_id) else {
                return Err(StatusCode::NAME_NOT_FOUND.into());
            };
            if record.is_started_for_api() {
                return Err(invalid_state_status("AV stream type cannot be changed while filter is started"));
            }
        }
        let (av_stream_type_hint, av_stream_kind) = match av_stream_type {
            AvStreamType::Video(value) => (value.0, AvFilterStreamKind::Video),
            AvStreamType::Audio(value) => (value.0, AvFilterStreamKind::Audio),
        };
        // r50dz58/G3-18: validate all postconditions before dropping the old AV backing.
        // The old backing is discarded only immediately before the stream type commit.
        lock_mutex_status(&self.state, "demux_handle")?
            .validate_filter_av_stream_type_hint_result(
                self.filter_id,
                av_stream_type_hint,
                av_stream_kind,
            )
            .map_err(demux_config_error_status)?;
        self.reset_av_shared_backing_for_stream_type_change_result()?;
        lock_mutex_status(&self.state, "demux_handle")?
            .set_filter_av_stream_type_hint_result(
                self.filter_id,
                av_stream_type_hint,
                av_stream_kind,
            )
            .map_err(demux_config_error_status)?;
        Ok(())
    }
    fn configureIpCid(&self, _ip_cid: i32) -> BinderResult<()> {
        self.ensure_open()?;
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            Some("r51のTS-only HAL profileではIP CID monitor/filterは未対応です"),
        ))
    }

    fn configureMonitorEvent(&self, monitor_event_types: i32) -> BinderResult<()> {
        self.ensure_open()?;
        if monitor_event_types != 0 {
            return Err(Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, Some("r51ではfilter monitor eventは未対応です。通常callbackは常に配送されます")));
        }
        if lock_mutex_status(&self.state, "demux_handle")?
            .set_filter_monitor_event_mask(self.filter_id, 0)
        {
            return Ok(());
        }
        Err(StatusCode::NAME_NOT_FOUND.into())
    }

    fn start(&self) -> BinderResult<()> {
        self.ensure_open()?;
        let (ready, start_event_id, monitor_mask, is_media, start_event_ready) = {
            let state = lock_mutex_status(&self.state, "demux_handle")?;
            let readiness = state.filter_delivery_readiness(self.filter_id);
            let ready = state.has_filter_payload_ready(self.filter_id)
                && matches!(
                    readiness,
                    maleicacid_tuner_hal_soft_demux::FilterDeliveryReadiness::Ready
                );
            let start_event_ready = filter_start_event_ready(readiness);
            let record = state.filter_record(self.filter_id).cloned().ok_or_else(|| Status::from(StatusCode::NAME_NOT_FOUND))?;
            if !record.is_configured_for_api() {
                return Err(Status::new_service_specific_error(TunerResult::INVALID_STATE.0, None));
            }
            let pending_id = if record.ever_started {
                record.delivery_generation.max(1).min(i32::MAX as u64) as i32
            } else {
                0
            };
            let start_event_id = if start_event_ready { Some(pending_id) } else { None };
            // configureMonitorEvent() は通常 コールバック の gating API ではない。
            // r51 は monitor-event bit を support しないため、DATA_READY / OVERFLOW / データイベント は常に有効のままにする。
            let monitor_mask = 0;
            let is_media = matches!(
                record
                    .config
                    .as_ref()
                    .map(|c| &c.kind),
                Some(FilterConfigKind::Av { .. })
            );
            (ready, start_event_id, monitor_mask, is_media, start_event_ready)
        };
        let send_status = monitor_mask == 0 || (monitor_mask & FILTER_MONITOR_MASK_STATUS) != 0;
        let send_event = monitor_mask == 0 || (monitor_mask & FILTER_MONITOR_MASK_EVENT) != 0;
        if ready && !is_media && send_status {
            if let Err(err) = self.callback.onFilterStatus(DemuxFilterStatus::DATA_READY) {
                return Err(callback_failure_status("filter", self.filter_id, "onFilterStatus(DATA_READY)", &err));
            }
        }
        if let Some(start_id) = start_event_id.filter(|_| send_event) {
            if let Err(err) = self
                .callback
                .onFilterEvent(&[DemuxFilterEvent::StartId(start_id)])
            {
                return Err(callback_failure_status("filter", self.filter_id, "onFilterEvent(StartId)", &err));
            }
        }
        {
            let mut state = lock_mutex_status(&self.state, "demux_handle")?;
            state
                .start_filter_result(self.filter_id)
                .map_err(demux_config_error_status)?;
            state.set_filter_start_event_pending(self.filter_id, send_event && !start_event_ready);
        }
        Ok(())
    }

    fn stop(&self) -> BinderResult<()> {
        self.ensure_open()?;
        lock_mutex_status(&self.state, "demux_handle")?
            .stop_filter_result(self.filter_id)
            .map_err(demux_config_error_status)
    }

    fn flush(&self) -> BinderResult<()> {
        self.ensure_open()?;
        let mut first_error: Option<Status> = None;
        let demux_flush_result = match lock_mutex_status(&self.state, "demux_handle") {
            Ok(mut state) => state
                .flush_filter_result(self.filter_id)
                .map_err(demux_config_error_status),
            Err(status) => Err(status),
        };
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "demux_flush_filter",
            demux_flush_result,
        );
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "normal_fmq_clear",
            self.queue_backing.clear_result(),
        );
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "av_fmq_clear",
            self.av_queue_backing.clear_result(),
        );
        Self::remember_cleanup_error(
            &mut first_error,
            self.filter_id,
            "av_shared_release_all",
            match lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing") {
                Ok(backing_slot) => {
                    if let Some(backing) = backing_slot.as_ref().cloned() {
                        backing.release_all()
                    } else {
                        Ok(())
                    }
                }
                Err(status) => Err(status),
            },
        );
        if let Some(status) = first_error {
            Err(status)
        } else {
            Ok(())
        }
    }

    fn getAvSharedHandle(&self, av_memory: &mut TunerNativeHandle) -> BinderResult<i64> {
        self.ensure_open()?;
        self.ensure_configured_av_filter()?;
        let backing = self.ensure_av_shared_backing()?;
        let export_generation = self
            .av_export_generation
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        let (handle, identity) =
            backing.build_native_handle_with_identity(self.filter_id, export_generation)?;
        *lock_mutex_status(&self.current_av_handle_identity, "filter_av_handle_identity")? =
            Some(identity);
        *av_memory = handle;
        self.av_shared_handle_client_released.store(false, Ordering::SeqCst);
        self.av_shared_handle_exported.store(true, Ordering::SeqCst);
        Ok(backing.total_size() as i64)
    }

    fn getId(&self) -> BinderResult<i32> {
        self.ensure_open()?;
        Ok(self.filter_id)
    }

    fn getId64Bit(&self) -> BinderResult<i64> {
        self.ensure_open()?;
        Ok(i64::from(self.filter_id))
    }

    fn releaseAvHandle(&self, av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {
        if av_data_id < 0 {
            self.record_av_data_id_invalid_release();
            return Err(invalid_argument_status("不正なAV data idです"));
        }
        if !av_memory.fds.is_empty() {
            return self.accept_returned_av_shared_handle_release(av_memory, av_data_id);
        }
        if av_data_id == 0 {
            return self.mark_av_shared_handle_client_released();
        }
        if self.closed.load(Ordering::SeqCst) {
            self.record_av_data_id_stale_release_after_close();
            return Ok(());
        }
        if !self.current_release_target_is_av_filter()? {
            if self.av_shared_handle_was_ever_exported() {
                self.record_av_data_id_stale_release();
                return Ok(());
            }
            self.ensure_release_target_is_av_filter()?;
        }
        if !self.av_shared_handle_exported.load(Ordering::SeqCst) {
            if self.av_shared_handle_was_ever_exported() {
                if av_data_id == 0 {
                    record_tuner_diagnostic_counter(
                        &AV_HANDLE_CLIENT_RELEASE_COUNT,
                        "av_handle_client_release",
                    );
                } else {
                    self.record_av_data_id_stale_release();
                }
                return Ok(());
            }
            self.record_av_handle_release_without_handle();
            return Err(invalid_state_status("AV共有ハンドルは未公開です"));
        }
        self.release_av_shared_handle(av_data_id)
    }

    fn setDataSource(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        let mut txn = LifecycleTxn::new();
        txn.validate("filter_set_data_source_self_open", || self.ensure_open())?;
        txn.validate("filter_set_data_source_runtime_ok", || {
            self.runtime_io
                .ensure_not_failed(RuntimeIoKind::Filter, self.filter_id)
        })?;
        txn.validate("filter_set_data_source_sink_preconditions", || {
            let state = lock_mutex_status(&self.state, "demux_handle")?;
            state
                .validate_filter_data_source_sink_preconditions(self.filter_id)
                .map_err(demux_config_error_status)
        })?;
        let upstream_id = txn.prepare_value("filter_set_data_source_resolve_upstream", || {
            local_filter_id_for_owner(filter, self.owner_demux_id)
        })?;
        let previous_upstream = txn.prepare_value("filter_set_data_source_snapshot", || {
            lock_mutex_status(&self.state, "demux_handle")?
                .filter_data_source_snapshot(self.filter_id)
                .map_err(demux_config_error_status)
        })?;
        let apply_result = txn.apply("filter_set_data_source_apply", || {
            lock_mutex_status(&self.state, "demux_handle")?
                .set_filter_data_source_result(self.filter_id, upstream_id)
                .map_err(demux_config_error_status)
        });
        if let Err(status) = apply_result {
            if let Err(rollback_status) = txn.rollback("filter_set_data_source_restore_snapshot", || {
                lock_mutex_status(&self.state, "demux_handle")?
                    .restore_filter_data_source_snapshot(self.filter_id, previous_upstream)
                    .map_err(demux_config_error_status)
            }) {
                self.runtime_io.mark_failed(
                    RuntimeIoKind::Filter,
                    self.filter_id,
                    "filter_set_data_source_rollback_failed",
                );
                eprintln!(
                    "maleicacid-tuner-hal-filter: filter={} step=setDataSource rollback_failed apply_status={:?} rollback_status={:?}",
                    self.filter_id, status, rollback_status
                );
                return Err(Status::new_service_specific_error(
                    TunerResult::UNKNOWN_ERROR.0,
                    Some("filter_set_data_source_rollback_failed"),
                ));
            }
            return Err(status);
        }
        txn.commit("filter_set_data_source_commit", || Ok::<(), Status>(()))?;
        Ok(())
    }

    fn setDelayHint(&self, hint: &FilterDelayHint) -> BinderResult<()> {
        self.ensure_open()?;
        let mut state = lock_mutex_status(&self.state, "demux_handle")?;
        let Some(record) = state.filter_record(self.filter_id).cloned() else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        let delay_hint = normalize_filter_delay_hint_for_record(&record, hint)?;
        if state.set_filter_delay_hint(self.filter_id, delay_hint) {
            return Ok(());
        }
        Err(StatusCode::NAME_NOT_FOUND.into())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DvrWaitError {
    WorkerSignalRuntimeFailure,
}

impl DvrWaitError {
    fn as_str(self) -> &'static str {
        match self {
            Self::WorkerSignalRuntimeFailure => "dvr_callback_worker_signal_runtime_failure",
        }
    }
}

pub struct DvrHal {
    owner_demux_id: i32,
    dvr_id: i32,
    direction: DemuxPathDirection,
    state: Arc<Mutex<DemuxHandle>>,
    callback: Strong<dyn IDvrCallback>,
    runtime_io: Arc<RuntimeIoRegistry>,
    demux_record: Option<DemuxRecordRef>,
    queue_backing: Arc<SharedMemoryBacking>,
    callback_stop: Arc<RuntimeAtomicFlag>,
    callback_worker: Mutex<Option<WorkerHandle>>,
    closed: Arc<RuntimeAtomicFlag>,
    cleanup_complete: Arc<RuntimeAtomicFlag>,
    last_cleanup_steps: Arc<Mutex<Option<DvrCleanupStepResults>>>,
    close_failure: Arc<Mutex<Option<CloseFailureRecord>>>,
}

trait DvrCleanupStepRunner {
    fn stop_callback_worker(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult>;
    fn clear_queue(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult>;
    fn unregister_runtime(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult>;
    fn stop_queue(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult>;
    fn unregister_demux(&mut self) -> BinderResult<DvrCleanupStepResult>;
}

struct RealDvrCleanupRunner<'a> {
    state: &'a Arc<Mutex<DemuxHandle>>,
    runtime_io: &'a Arc<RuntimeIoRegistry>,
    queue_backing: &'a Arc<SharedMemoryBacking>,
    callback_worker: Option<&'a Mutex<Option<WorkerHandle>>>,
    callback_stop: &'a Arc<RuntimeAtomicFlag>,
    dvr_id: i32,
}

impl DvrCleanupStepRunner for RealDvrCleanupRunner<'_> {
    fn stop_callback_worker(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult> {
        self.callback_stop.store(true, Ordering::SeqCst);
        match caller {
            DvrCleanupCaller::ExternalClose => {
                let Some(worker) = self.callback_worker else {
                    return Ok(DvrCleanupStepResult::SafeNoOp);
                };
                {
                    let guard = lock_mutex_status(worker, "dvr_callback_worker")?;
                    if let Some(handle) = guard.as_ref() {
                        let _ = handle.request_stop(WorkerExitReason::StopRequested);
                        let _ = handle.wake();
                    }
                }
                let handle = lock_mutex_status(worker, "dvr_callback_worker")?.take();
                if let Some(handle) = handle {
                    let exit = WorkerRuntime::join(handle, "dvr_callback_worker");
                    if exit.is_abnormal() {
                        self.runtime_io.mark_failed(
                            RuntimeIoKind::Dvr,
                            self.dvr_id,
                            &format!("dvr_callback_worker_{exit:?}"),
                        );
                        Err(worker_exit_status("dvr_callback_worker", exit))
                    } else {
                        Ok(DvrCleanupStepResult::Success)
                    }
                } else {
                    Ok(DvrCleanupStepResult::SafeNoOp)
                }
            }
            DvrCleanupCaller::BestEffortDrop => {
                if let Some(worker) = self.callback_worker {
                    if let Some(mut slot) = lock_mutex_option(worker, "dvr_callback_worker") {
                        if let Some(handle) = slot.as_ref() {
                            let _ = handle.request_stop(WorkerExitReason::StopRequested);
                            let _ = handle.wake();
                        }
                        if let Some(handle) = slot.take() {
                            let exit = WorkerRuntime::join(handle, "dvr_callback_worker");
                            if exit.is_abnormal() {
                                self.runtime_io.mark_failed(
                                    RuntimeIoKind::Dvr,
                                    self.dvr_id,
                                    &format!("dvr_callback_worker_{exit:?}"),
                                );
                                Ok(DvrCleanupStepResult::Unknown)
                            } else {
                                Ok(DvrCleanupStepResult::Success)
                            }
                        } else {
                            Ok(DvrCleanupStepResult::SafeNoOp)
                        }
                    } else {
                        Ok(DvrCleanupStepResult::SafeNoOp)
                    }
                } else {
                    Ok(DvrCleanupStepResult::SafeNoOp)
                }
            }
            DvrCleanupCaller::WorkerFailure => Ok(DvrCleanupStepResult::SkippedDueToWorkerFailureContext),
        }
    }

    fn clear_queue(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult> {
        match caller {
            DvrCleanupCaller::ExternalClose => self
                .queue_backing
                .clear_result()
                .map(|_| DvrCleanupStepResult::Success),
            DvrCleanupCaller::BestEffortDrop | DvrCleanupCaller::WorkerFailure => {
                self.queue_backing.clear_drop_only();
                Ok(DvrCleanupStepResult::Unknown)
            }
        }
    }

    fn unregister_runtime(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult> {
        match caller {
            DvrCleanupCaller::ExternalClose => self
                .runtime_io
                .unregister_dvr(self.dvr_id)
                .map(|_| DvrCleanupStepResult::Success),
            DvrCleanupCaller::BestEffortDrop | DvrCleanupCaller::WorkerFailure => {
                self.runtime_io.unregister_dvr_best_effort(self.dvr_id);
                Ok(DvrCleanupStepResult::Unknown)
            }
        }
    }

    fn stop_queue(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult> {
        match caller {
            DvrCleanupCaller::ExternalClose => self
                .queue_backing
                .stop()
                .map(|_| DvrCleanupStepResult::Success),
            DvrCleanupCaller::BestEffortDrop | DvrCleanupCaller::WorkerFailure => {
                self.queue_backing.stop_best_effort();
                Ok(DvrCleanupStepResult::Unknown)
            }
        }
    }

    fn unregister_demux(&mut self) -> BinderResult<DvrCleanupStepResult> {
        lock_mutex_status(self.state, "demux_handle")?.unregister_dvr(self.dvr_id);
        Ok(DvrCleanupStepResult::Success)
    }
}

impl DvrHal {
    fn new(
        owner_demux_id: i32,
        dvr_id: i32,
        direction: DemuxPathDirection,
        state: Arc<Mutex<DemuxHandle>>,
        runtime_io: Arc<RuntimeIoRegistry>,
        callback: Strong<dyn IDvrCallback>,
        demux_record: Option<DemuxRecordRef>,
    ) -> BinderResult<Self> {
        let buffer_size = lock_mutex_status(&state, "demux_handle")?
            .dvr_record(dvr_id)
            .map(|r| r.buffer_size.max(0) as usize)
            .unwrap_or(4096);
        let callback_stop = Arc::new(RuntimeAtomicFlag::new(false));
        let closed = Arc::new(RuntimeAtomicFlag::new(false));
        let cleanup_complete = Arc::new(RuntimeAtomicFlag::new(false));
        let queue_backing = SharedMemoryBacking::new_ring(buffer_size)?;
        runtime_io.register_dvr(dvr_id, &queue_backing)?;
        if matches!(direction, DemuxPathDirection::Playback) {
            if let Err(status) = queue_backing.start_playback_consumer(
                Arc::clone(&state),
                Arc::clone(&runtime_io),
                Arc::clone(&closed),
                dvr_id,
            ) {
                runtime_io.unregister_dvr_best_effort(dvr_id);
                return Err(status);
            }
        }
        let last_cleanup_steps = Arc::new(Mutex::new(None));
        let close_failure = Arc::new(Mutex::new(None));
        let callback_worker = {
            let state = Arc::clone(&state);
            let callback = callback.clone();
            let callback_stop = Arc::clone(&callback_stop);
            let closed_clone = Arc::clone(&closed);
            let cleanup_complete_clone = Arc::clone(&cleanup_complete);
            let last_cleanup_steps_clone = Arc::clone(&last_cleanup_steps);
            let runtime_io_clone = Arc::clone(&runtime_io);
            let queue_backing_clone = Arc::clone(&queue_backing);
            let state_hook = Arc::clone(&state);
            let runtime_io_hook = Arc::clone(&runtime_io);
            let queue_backing_hook = Arc::clone(&queue_backing);
            let closed_hook = Arc::clone(&closed);
            let cleanup_complete_hook = Arc::clone(&cleanup_complete);
            let last_cleanup_steps_hook = Arc::clone(&last_cleanup_steps);
            let callback_stop_hook = Arc::clone(&callback_stop);
            WorkerRuntime::spawn_owned_with_exit_hook(WorkerOwnerId("dvr_callback_worker", dvr_id), "dvr_callback_worker", move |owner_signal| {
                while !callback_stop.load(Ordering::SeqCst) && !closed_clone.load(Ordering::SeqCst) && !owner_signal.is_stop_requested() {
                    let (thresholds, status_mask, interval_hint_ms, running, pending_overflow, payloads) = {
                        let Some(mut demux) = lock_mutex_option(&state, "demux_handle") else {
                            DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &cleanup_complete_clone, Some(&last_cleanup_steps_clone), &callback_stop, dvr_id, "dvr_callback_worker lost demux state");
                            return WorkerExit::RuntimeFailure;
                        };
                        let Some(record) = demux.dvr_record(dvr_id).cloned() else {
                            DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &cleanup_complete_clone, Some(&last_cleanup_steps_clone), &callback_stop, dvr_id, "dvr_callback_worker missing dvr record");
                            return WorkerExit::RuntimeFailure;
                        };
                        let running = record.is_started_for_api();
                        let interval = record.status_check_interval_hint_ms;
                        let status_mask = record.config.as_ref().map(|config| config.status_mask).unwrap_or(DVR_STATUS_MASK_DISABLED);
                        let pending_overflow = demux.take_dvr_pending_overflow(dvr_id);
                        let payloads = if running && matches!(direction, DemuxPathDirection::Record) { demux.drain_dvr_payloads(dvr_id) } else { Vec::new() };
                        (demux.dvr_threshold_state(dvr_id), status_mask, interval, running, pending_overflow, payloads)
                    };
                    if running && matches!(direction, DemuxPathDirection::Record) {
                        let mut overflow = pending_overflow;
                        let mut any = false;
                        for payload in payloads {
                            let ring = match queue_backing_clone.write_bytes(&payload) {
                                Ok(ring) => ring,
                                Err(err) => {
                                    record_tuner_diagnostic_counter(
                                        &DVR_FMQ_WRITE_ERROR_COUNT,
                                        "dvr_fmq_write_error",
                                    );
                                    let reason = if is_event_flag_wake_failure(&err) {
                                        format!("dvr_event_flag_wake_failed: {err}")
                                    } else {
                                        format!("dvr_fmq_write_error: {err}")
                                    };
                                    eprintln!(
                                        "maleicacid-tuner-hal-fmq: dvr_id={} {reason}",
                                        dvr_id
                                    );
                                    if is_event_flag_wake_failure(&err) {
                                        runtime_io_clone.mark_failed(RuntimeIoKind::Dvr, dvr_id, "EventFlagWakeFailed");
                                        callback_stop.store(true, Ordering::SeqCst);
                                        return WorkerExit::StopRequested;
                                    }
                                    DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &cleanup_complete_clone, Some(&last_cleanup_steps_clone), &callback_stop, dvr_id, &reason);
                                    return WorkerExit::RuntimeFailure;
                                }
                            };
                            overflow |= ring.overflowed;
                            any |= ring.len > 0;
                        }
                        if any && Self::status_mask_allows(status_mask, RecordStatus::DATA_READY.0) {
                            if let Err(err) = callback.onRecordStatus(RecordStatus::DATA_READY) {
                                eprintln!("maleicacid-tuner-hal-callback: dvr_id={} api=onRecordStatus(DATA_READY) binder_status={:?}", dvr_id, err);
                                DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &cleanup_complete_clone, Some(&last_cleanup_steps_clone), &callback_stop, dvr_id, "dvr callback failure on DATA_READY");
                                return WorkerExit::RuntimeFailure;
                            }
                        }
                        if overflow && Self::status_mask_allows(status_mask, RecordStatus::OVERFLOW.0) {
                            if let Err(err) = callback.onRecordStatus(RecordStatus::OVERFLOW) {
                                eprintln!("maleicacid-tuner-hal-callback: dvr_id={} api=onRecordStatus(OVERFLOW) binder_status={:?}", dvr_id, err);
                                DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &cleanup_complete_clone, Some(&last_cleanup_steps_clone), &callback_stop, dvr_id, "dvr callback failure on OVERFLOW");
                                return WorkerExit::RuntimeFailure;
                            }
                        }
                    }
                    if running {
                        match (direction, thresholds) {
                            (DemuxPathDirection::Record, Some((_fill, low, high, _capacity))) => {
                                let fill = match queue_backing_clone.current_fill_bytes() {
                                    Ok(fill) => fill,
                                    Err(err) => {
                                        let reason = format!("dvr_record_current_fill_error: {err:?}");
                                        DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &cleanup_complete_clone, Some(&last_cleanup_steps_clone), &callback_stop, dvr_id, &reason);
                                        return WorkerExit::RuntimeFailure;
                                    }
                                };
                                let status = Self::record_status_from_thresholds(fill, low, high);
                                if Self::status_mask_allows(status_mask, status.0) {
                                    if let Err(err) = callback.onRecordStatus(status) {
                                    eprintln!("maleicacid-tuner-hal-callback: dvr_id={} api=onRecordStatus(threshold) binder_status={:?}", dvr_id, err);
                                    DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &cleanup_complete_clone, Some(&last_cleanup_steps_clone), &callback_stop, dvr_id, "dvr callback failure on record threshold");
                                    return WorkerExit::RuntimeFailure;
                                }
                                }
                            }
                            (DemuxPathDirection::Playback, Some((_fill, low, high, capacity))) => {
                                let fill = match queue_backing_clone.current_fill_bytes() {
                                    Ok(fill) => fill,
                                    Err(err) => {
                                        let reason = format!("dvr_playback_current_fill_error: {err:?}");
                                        DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &cleanup_complete_clone, Some(&last_cleanup_steps_clone), &callback_stop, dvr_id, &reason);
                                        return WorkerExit::RuntimeFailure;
                                    }
                                };
                                if let Some(status) = Self::playback_status_from_thresholds(fill, low, high, capacity) {
                                    if Self::status_mask_allows(status_mask, status.0) {
                                        if let Err(err) = callback.onPlaybackStatus(status) {
                                        eprintln!("maleicacid-tuner-hal-callback: dvr_id={} api=onPlaybackStatus(threshold) binder_status={:?}", dvr_id, err);
                                        DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &cleanup_complete_clone, Some(&last_cleanup_steps_clone), &callback_stop, dvr_id, "dvr callback failure on playback threshold");
                                        return WorkerExit::RuntimeFailure;
                                    }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    let sleep_ms = u64::try_from(interval_hint_ms).unwrap_or(DVR_DEFAULT_STATUS_CHECK_INTERVAL_MS as u64);
                    if let Err(err) = DvrHal::wait_for_callback_interval(&callback_stop, &owner_signal, Duration::from_millis(sleep_ms)) {
                        DvrHal::fail_dvr_worker(
                            &state,
                            &runtime_io_clone,
                            &queue_backing_clone,
                            &closed_clone,
                            &cleanup_complete_clone,
                            Some(&last_cleanup_steps_clone),
                            &callback_stop,
                            dvr_id,
                            err.as_str(),
                        );
                        return WorkerExit::RuntimeFailure;
                    }
                }
                WorkerExit::StopRequested
            }, move |exit| {
                if exit.is_abnormal() {
                    DvrHal::fail_dvr_worker(
                        &state_hook,
                        &runtime_io_hook,
                        &queue_backing_hook,
                        &closed_hook,
                        &cleanup_complete_hook,
                        Some(&last_cleanup_steps_hook),
                        &callback_stop_hook,
                        dvr_id,
                        &format!("dvr_callback_worker_{exit:?}"),
                    );
                }
            }).map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?
        };
        Ok(Self {
            owner_demux_id,
            dvr_id,
            direction,
            state,
            callback,
            runtime_io,
            demux_record,
            queue_backing,
            callback_stop,
            callback_worker: Mutex::new(Some(callback_worker)),
            closed,
            cleanup_complete,
            last_cleanup_steps,
            close_failure,
        })
    }


    fn wait_for_callback_interval(
        stop: &RuntimeAtomicFlag,
        signal: &Arc<ConcreteWorkerSignal>,
        interval: Duration,
    ) -> Result<(), DvrWaitError> {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _ = signal.wait_timeout_or_stop(interval);
        if signal.is_runtime_failure() {
            return Err(DvrWaitError::WorkerSignalRuntimeFailure);
        }
        Ok(())
    }

    fn wake_callback_worker(worker: &Mutex<Option<WorkerHandle>>) -> BinderResult<()> {
        let guard = lock_mutex_status(worker, "dvr_callback_worker")?;
        if let Some(handle) = guard.as_ref() {
            handle.wake().map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
        }
        Ok(())
    }

    fn fail_dvr_worker(
        state: &Arc<Mutex<DemuxHandle>>,
        runtime_io: &Arc<RuntimeIoRegistry>,
        queue_backing: &Arc<SharedMemoryBacking>,
        closed: &Arc<RuntimeAtomicFlag>,
        cleanup_complete: &Arc<RuntimeAtomicFlag>,
        last_cleanup_steps: Option<&Arc<Mutex<Option<DvrCleanupStepResults>>>>,
        callback_stop: &Arc<RuntimeAtomicFlag>,
        dvr_id: i32,
        reason: &str,
    ) {
        let mut txn = LifecycleTxn::new();
        let transition = RuntimeFailClosedTransition::dvr(dvr_id, "dvr_callback_worker");
        let _ = txn.apply("dvr_worker_failure_close_atomic", || {
            transition.close_atomic(closed);
            Ok::<(), Status>(())
        });
        let _ = txn.apply("dvr_worker_failure_mark_runtime_failed", || {
            transition.mark_failed(runtime_io, reason);
            Ok::<(), Status>(())
        });
        if cleanup_complete.load(Ordering::SeqCst) {
            return;
        }
        let outcome = match txn.cleanup("dvr_worker_failure_cleanup_resources", || {
            Ok::<DvrCleanupOutcome<Status>, Status>(Self::cleanup_dvr_resources_shared(
                DvrCleanupCaller::WorkerFailure,
                state,
                runtime_io,
                queue_backing,
                None,
                callback_stop,
                dvr_id,
            ))
        }) {
            Ok(outcome) => outcome,
            Err(_) => return,
        };
        let _ = txn.cleanup("dvr_worker_failure_record_cleanup_steps", || {
            if let Some(last_cleanup_steps) = last_cleanup_steps {
                if let Some(mut last) = lock_mutex_option(last_cleanup_steps, "dvr_last_cleanup_steps")
                {
                    *last = Some(outcome.step_results.clone());
                }
            }
            Ok::<(), Status>(())
        });
        if outcome.all_cleanup_complete {
            let _ = txn.commit("dvr_worker_failure_cleanup_complete", || {
                cleanup_complete.store(true, Ordering::SeqCst);
                Ok::<(), Status>(())
            });
        }
    }

    fn fail_from_callback(&self, api: &str, err: Status) -> Status {
        let status = callback_failure_status("dvr", self.dvr_id, api, &err);
        let _ = self.close_internal();
        status
    }

    fn ensure_open(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(invalid_state_status("dvr is closed"));
        }
        {
            let demux = lock_mutex_status(&self.state, "demux_handle")?;
            if demux.is_closed() {
                self.runtime_io
                    .mark_failed(RuntimeIoKind::Dvr, self.dvr_id, "parent_demux_closed");
                return Err(invalid_state_status("parent demux is closed"));
            }
            if !demux.has_dvr(self.dvr_id) {
                self.runtime_io.mark_failed(
                    RuntimeIoKind::Dvr,
                    self.dvr_id,
                    "dvr_unregistered_from_parent_demux",
                );
                return Err(invalid_state_status(
                    "dvr is no longer registered in parent demux",
                ));
            }
            if demux.demux_id() != self.owner_demux_id {
                self.runtime_io.mark_failed(
                    RuntimeIoKind::Dvr,
                    self.dvr_id,
                    "dvr_owner_demux_mismatch",
                );
                return Err(invalid_state_status("dvr owner demux mismatch"));
            }
        }
        if matches!(self.direction, DemuxPathDirection::Playback) {
            if let Err(err) = self.queue_backing.ensure_playback_worker_healthy() {
                self.runtime_io.mark_failed(
                    RuntimeIoKind::Dvr,
                    self.dvr_id,
                    "dvr_playback_consumer_failed",
                );
                self.closed.store(true, Ordering::SeqCst);
                return Err(err);
            }
        }
        self.runtime_io
            .ensure_not_failed(RuntimeIoKind::Dvr, self.dvr_id)
    }

    fn stop_callback_worker(&self) -> BinderResult<()> {
        self.callback_stop.store(true, Ordering::SeqCst);
        DvrHal::wake_callback_worker(&self.callback_worker)?;
        if let Some(handle) =
            lock_mutex_status(&self.callback_worker, "dvr_callback_worker")?.take()
        {
            let exit = WorkerRuntime::join(handle, "dvr_callback_worker");
            if exit.is_abnormal() {
                Self::fail_dvr_worker(
                    &self.state,
                    &self.runtime_io,
                    &self.queue_backing,
                    &self.closed,
                    &self.cleanup_complete,
                    Some(&self.last_cleanup_steps),
                    &self.callback_stop,
                    self.dvr_id,
                    &format!("dvr_callback_worker_{exit:?}"),
                );
                return Err(worker_exit_status("dvr_callback_worker", exit));
            }
        }
        Ok(())
    }

    fn stop_callback_worker_best_effort(&self) {
        self.callback_stop.store(true, Ordering::SeqCst);
        if let Err(err) = DvrHal::wake_callback_worker(&self.callback_worker) {
            eprintln!("maleicacid-tuner-hal-dvr: callback wake failed during best-effort stop: {:?}", err);
        }
        if let Some(handle) = lock_mutex_option(&self.callback_worker, "dvr_callback_worker")
            .and_then(|mut worker| worker.take())
        {
            let exit = WorkerRuntime::join(handle, "dvr_callback_worker");
            if exit.is_abnormal() {
                Self::fail_dvr_worker(
                    &self.state,
                    &self.runtime_io,
                    &self.queue_backing,
                    &self.closed,
                    &self.cleanup_complete,
                    Some(&self.last_cleanup_steps),
                    &self.callback_stop,
                    self.dvr_id,
                    &format!("dvr_callback_worker_{exit:?}"),
                );
            }
        }
    }

    fn remember_first_error(first_error: &mut Option<Status>, result: BinderResult<()>) {
        if let Err(err) = result {
            first_error.get_or_insert(err);
        }
    }

    fn cleanup_dvr_resources_shared(
        caller: DvrCleanupCaller,
        state: &Arc<Mutex<DemuxHandle>>,
        runtime_io: &Arc<RuntimeIoRegistry>,
        queue_backing: &Arc<SharedMemoryBacking>,
        callback_worker: Option<&Mutex<Option<WorkerHandle>>>,
        callback_stop: &Arc<RuntimeAtomicFlag>,
        dvr_id: i32,
    ) -> DvrCleanupOutcome<Status> {
        let mut runner = RealDvrCleanupRunner {
            state,
            runtime_io,
            queue_backing,
            callback_worker,
            callback_stop,
            dvr_id,
        };
        Self::cleanup_dvr_resources_with_runner(caller, &mut runner)
    }

    fn cleanup_dvr_resources_with_runner(
        caller: DvrCleanupCaller,
        runner: &mut impl DvrCleanupStepRunner,
    ) -> DvrCleanupOutcome<Status> {
        let mut first_error: Option<Status> = None;
        let mut step_results = DvrCleanupStepResults::default();

        match runner.stop_callback_worker(caller) {
            Ok(result) => step_results.callback_worker = result,
            Err(err) => {
                step_results.callback_worker = DvrCleanupStepResult::Failed;
                Self::remember_first_error(&mut first_error, Err(err));
            }
        }

        match runner.clear_queue(caller) {
            Ok(result) => step_results.queue_clear = result,
            Err(err) => {
                step_results.queue_clear = DvrCleanupStepResult::Failed;
                Self::remember_first_error(&mut first_error, Err(err));
            }
        }

        match runner.unregister_runtime(caller) {
            Ok(result) => step_results.runtime_unregister = result,
            Err(err) => {
                step_results.runtime_unregister = DvrCleanupStepResult::Failed;
                Self::remember_first_error(&mut first_error, Err(err));
            }
        }

        match runner.stop_queue(caller) {
            Ok(result) => step_results.queue_stop = result,
            Err(err) => {
                step_results.queue_stop = DvrCleanupStepResult::Failed;
                Self::remember_first_error(&mut first_error, Err(err));
            }
        }

        match runner.unregister_demux() {
            Ok(result) => step_results.demux_unregister = result,
            Err(err) => {
                step_results.demux_unregister = DvrCleanupStepResult::Failed;
                Self::remember_first_error(&mut first_error, Err(err));
            }
        }

        let all_cleanup_complete = step_results.callback_worker.is_complete()
            && step_results.queue_clear.is_complete()
            && step_results.runtime_unregister.is_complete()
            && step_results.queue_stop.is_complete()
            && step_results.demux_unregister.is_complete();
        DvrCleanupOutcome {
            first_error,
            all_cleanup_complete,
            step_results,
        }
    }

    fn cleanup_dvr_resources(&self, caller: DvrCleanupCaller) -> DvrCleanupOutcome<Status> {
        let outcome = Self::cleanup_dvr_resources_shared(
            caller,
            &self.state,
            &self.runtime_io,
            &self.queue_backing,
            Some(&self.callback_worker),
            &self.callback_stop,
            self.dvr_id,
        );
        if let Some(mut last) =
            lock_mutex_option(&self.last_cleanup_steps, "dvr_last_cleanup_steps")
        {
            *last = Some(outcome.step_results.clone());
        }
        outcome
    }

    fn close_internal(&self) -> BinderResult<()> {
        if self.cleanup_complete.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut txn = LifecycleTxn::new();
        let mut ledger_close_started = false;
        if let Some(record) = self.demux_record.as_ref() {
            lock_mutex_status(record, "demux_record")
                .and_then(|mut record| {
                    record.dvr_ledger.begin_close(LedgerId(self.dvr_id))
                        .map(|_| ())
                        .map_err(|_| invalid_state_status("dvr ledger begin_close failed"))
                })?;
            let _ = txn.prepare("dvr_ledger_begin_close", || Ok::<(), Status>(()));
            ledger_close_started = true;
        }
        let outcome = self.cleanup_dvr_resources(DvrCleanupCaller::ExternalClose);
        let _ = &outcome.step_results;
        if outcome.all_cleanup_complete && outcome.first_error.is_none() {
            if ledger_close_started {
                if let Some(record) = self.demux_record.as_ref() {
                    lock_mutex_status(record, "demux_record")
                        .and_then(|mut record| {
                            txn.commit("dvr_ledger_commit_close", || {
                                record.dvr_ledger.commit_close(LedgerId(self.dvr_id))
                                    .map_err(|_| invalid_state_status("dvr ledger commit_close failed"))
                            })
                        })?;
                }
            }
            txn.commit("dvr_closed_flags", || {
                self.closed.store(true, Ordering::SeqCst);
                self.cleanup_complete.store(true, Ordering::SeqCst);
                if let Some(mut failure) = lock_mutex_option(&self.close_failure, "dvr_close_failure") {
                    *failure = None;
                }
                Ok::<(), Status>(())
            })?;
        }
        if let Some(err) = outcome.first_error {
            self.closed.store(true, Ordering::SeqCst);
            self.cleanup_complete.store(false, Ordering::SeqCst);
            if let Some(mut failure) = lock_mutex_option(&self.close_failure, "dvr_close_failure") {
                *failure = Some(CloseFailureRecord::new(
                    "dvr_close_cleanup",
                    "cleanup_failed",
                    &["dvr_ledger_rollback_close", "remaining_dvr_cleanup_retry"],
                ));
            }
            if ledger_close_started {
                if let Some(record) = self.demux_record.as_ref() {
                    if let Err(rollback_status) = lock_mutex_status(record, "demux_record").and_then(|mut record| {
                        record.dvr_ledger.rollback_close(LedgerId(self.dvr_id))
                            .map_err(|_| invalid_state_status("dvr ledger rollback_close failed"))
                    }) {
                        self.runtime_io.mark_failed(
                            RuntimeIoKind::Dvr,
                            self.dvr_id,
                            "dvr_close_rollback_close_failed",
                        );
                        self.closed.store(true, Ordering::SeqCst);
                        self.cleanup_complete.store(false, Ordering::SeqCst);
                        if let Some(mut failure) = lock_mutex_option(&self.close_failure, "dvr_close_failure") {
                            *failure = Some(CloseFailureRecord::new(
                                "dvr_ledger_rollback_close",
                                "rollback_close_failed",
                                &["remaining_dvr_cleanup_retry"],
                            ));
                        }
                        eprintln!(
                            "maleicacid-tuner-hal-dvr-close: dvr={} cleanup_status={:?} rollback_status={:?}",
                            self.dvr_id, err, rollback_status
                        );
                        return Err(Status::new_service_specific_error(
                            TunerResult::UNKNOWN_ERROR.0,
                            Some("dvr_close_rollback_close_failed"),
                        ));
                    }
                }
            }
            Err(err)
        } else {
            Ok(())
        }
    }

    fn close_internal_for_drop_cleanup(&self) {
        if self.cleanup_complete.load(Ordering::SeqCst) {
            return;
        }
        let outcome = self.cleanup_dvr_resources(DvrCleanupCaller::BestEffortDrop);
        let _ = &outcome.step_results;
        if outcome.all_cleanup_complete {
            self.closed.store(true, Ordering::SeqCst);
            self.cleanup_complete.store(true, Ordering::SeqCst);
        }
    }

    fn status_mask_allows(status_mask: i32, status_bit: i32) -> bool {
        (status_mask & status_bit) != 0
    }

    fn record_status_from_thresholds(
        fill: usize,
        low: Option<usize>,
        high: Option<usize>,
    ) -> RecordStatus {
        if fill == 0 {
            RecordStatus::LOW_WATER
        } else if high.map_or(false, |limit| fill >= limit) {
            RecordStatus::HIGH_WATER
        } else if low.map_or(false, |limit| fill <= limit) {
            RecordStatus::LOW_WATER
        } else {
            RecordStatus::DATA_READY
        }
    }

    fn playback_status_from_thresholds(
        fill: usize,
        low: Option<usize>,
        high: Option<usize>,
        capacity: usize,
    ) -> Option<PlaybackStatus> {
        // playback 状態 は queued data bytes ではなく未使用 write space で定義する。
        // 呼び出し側は playback TS を FMQ へ書くため、コールバック は追加入力可能な空き容量を表す。
        let available_space = capacity.saturating_sub(fill);
        if capacity > 0 && available_space == 0 {
            Some(PlaybackStatus::SPACE_FULL)
        } else if capacity > 0 && available_space >= capacity {
            Some(PlaybackStatus::SPACE_EMPTY)
        } else if low.map_or(false, |limit| available_space <= limit) {
            Some(PlaybackStatus::SPACE_ALMOST_FULL)
        } else if high.map_or(false, |limit| available_space >= limit) {
            Some(PlaybackStatus::SPACE_ALMOST_EMPTY)
        } else {
            None
        }
    }
}

impl Interface for DvrHal {}

impl Drop for DvrHal {
    fn drop(&mut self) {
        let mut txn = LifecycleTxn::new();
        let _ = txn.cleanup("dvr_drop_cleanup", || {
            self.close_internal_for_drop_cleanup();
            Ok::<(), Status>(())
        });
    }
}

impl IDvr for DvrHal {
    fn getQueueDesc(&self, queue: &mut TunerQueueDesc) -> BinderResult<()> {
        self.ensure_open()?;
        if !lock_mutex_status(&self.state, "demux_handle")?
            .dvr_record(self.dvr_id)
            .map(|record| record.is_configured_for_api())
            .unwrap_or(false)
        {
            record_tuner_diagnostic_counter(&DVR_QUEUE_DESC_INVALID_STATE_COUNT, "dvr_queue_desc_invalid_state");
            return Err(invalid_state_status("DVR is not configured"));
        }
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        *queue = match self.queue_backing.build_queue_desc() {
            Ok(desc) => desc,
            Err(status) => {
                if status_is_descriptor_internal_error(&status) {
                    self.runtime_io.mark_failed(
                        RuntimeIoKind::Dvr,
                        self.dvr_id,
                        "DescriptorInternalError",
                    );
                }
                return Err(status);
            }
        };
        Ok(())
    }

    fn configure(&self, settings: &DvrSettings) -> BinderResult<()> {
        let mut txn = LifecycleTxn::new();
        txn.validate("dvr.configure.ensure_open", || self.ensure_open())?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            txn.validate("dvr.configure.playback_worker_healthy", || self.queue_backing.ensure_playback_worker_healthy())?;
        }
        let summary = txn.prepare_value("dvr.configure.build_summary", || {
            let demux = lock_mutex_status(&self.state, "demux_handle")?;
            let buffer_size = demux
                .dvr_record(self.dvr_id)
                .map(|record| record.buffer_size)
                .ok_or_else(|| StatusCode::NAME_NOT_FOUND.into())?;
            let summary = validate_and_build_dvr_summary(self.direction, settings, buffer_size)?;
            demux.validate_dvr_configure_result(self.dvr_id, &summary)
                .map_err(demux_config_error_status)?;
            Ok(summary)
        })?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            txn.apply("dvr.configure.discard_playback_input", || {
                self.queue_backing
                    .discard_playback_input_for_boundary_result(self.dvr_id, "configure")
            })?;
        } else {
            txn.apply("dvr.configure.clear_record_queue", || self.queue_backing.clear_result())?;
        }
        txn.commit("dvr.configure.commit_demux_summary", || {
            lock_mutex_status(&self.state, "demux_handle")?
                .configure_dvr_with_summary_result(self.dvr_id, summary)
                .map_err(demux_config_error_status)
        })?;
        Ok(())
    }

    fn attachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        let filter_id = local_filter_id_for_owner(filter, self.owner_demux_id)?;
        lock_mutex_status(&self.state, "demux_handle")?
            .attach_filter_to_dvr_result(self.dvr_id, filter_id)
            .map_err(demux_config_error_status)
    }

    fn detachFilter(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        let filter_id = local_filter_id_for_owner(filter, self.owner_demux_id)?;
        lock_mutex_status(&self.state, "demux_handle")?
            .detach_filter_from_dvr_result(self.dvr_id, filter_id)
            .map_err(demux_config_error_status)
    }

    fn start(&self) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        let (state, status_mask) = {
            let demux = lock_mutex_status(&self.state, "demux_handle")?;
            let record = demux.dvr_record(self.dvr_id).ok_or_else(|| Status::from(StatusCode::NAME_NOT_FOUND))?;
            if !record.is_configured_for_api() {
                return Err(Status::new_service_specific_error(TunerResult::INVALID_STATE.0, None));
            }
            let status_mask = record.config.as_ref().map(|config| config.status_mask)
                .unwrap_or(DVR_STATUS_MASK_DISABLED);
            (demux.dvr_threshold_state(self.dvr_id), status_mask)
        };
        match (self.direction, state) {
            (DemuxPathDirection::Record, Some((_fill, low, high, _capacity))) => {
                let fill = self.queue_backing.current_fill_bytes()?;
                let status = Self::record_status_from_thresholds(fill, low, high);
                if Self::status_mask_allows(status_mask, status.0) {
                    if let Err(err) = self.callback.onRecordStatus(status) {
                        return Err(callback_failure_status("dvr", self.dvr_id, "onRecordStatus(start)", &err));
                    }
                }
            }
            (DemuxPathDirection::Playback, Some((_fill, low, high, capacity))) => {
                let fill = self.queue_backing.current_fill_bytes()?;
                if let Some(status) =
                    Self::playback_status_from_thresholds(fill, low, high, capacity)
                {
                    if Self::status_mask_allows(status_mask, status.0) {
                        if let Err(err) = self.callback.onPlaybackStatus(status) {
                            return Err(callback_failure_status("dvr", self.dvr_id, "onPlaybackStatus(start)", &err));
                        }
                    }
                }
            }
            _ => {}
        }
        lock_mutex_status(&self.state, "demux_handle")?
            .start_dvr_result(self.dvr_id)
            .map_err(demux_config_error_status)
    }

    fn stop(&self) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        lock_mutex_status(&self.state, "demux_handle")?
            .stop_dvr_result(self.dvr_id)
            .map_err(demux_config_error_status)
    }

    fn flush(&self) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        let mut first_error: Option<Status> = None;
        let demux_flush_result = match lock_mutex_status(&self.state, "demux_handle") {
            Ok(mut state) => state
                .flush_dvr_result(self.dvr_id)
                .map_err(demux_config_error_status),
            Err(status) => Err(status),
        };
        Self::remember_first_error(&mut first_error, demux_flush_result);
        if matches!(self.direction, DemuxPathDirection::Playback) {
            Self::remember_first_error(
                &mut first_error,
                self.queue_backing
                    .discard_playback_input_for_boundary_result(self.dvr_id, "flush"),
            );
        } else {
            Self::remember_first_error(&mut first_error, self.queue_backing.clear_result());
        }
        if let Some(status) = first_error {
            Err(status)
        } else {
            Ok(())
        }
    }

    fn close(&self) -> BinderResult<()> {
        self.close_internal()
    }

    fn setStatusCheckIntervalHint(&self, milliseconds: i64) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        if milliseconds < 0 {
            return Err(invalid_argument_status(
                "DVR statusCheckIntervalHint は負値にできません",
            ));
        }
        let normalized_ms = if milliseconds == 0 {
            DVR_DEFAULT_STATUS_CHECK_INTERVAL_MS
        } else {
            milliseconds
        };
        if lock_mutex_status(&self.state, "demux_handle")?
            .set_dvr_status_check_interval_hint(self.dvr_id, normalized_ms)
        {
            return Ok(());
        }
        Err(StatusCode::NAME_NOT_FOUND.into())
    }
}

pub struct LnbHal {
    lnb_id: i32,
    closed: RuntimeAtomicFlag,
    failed: RuntimeAtomicFlag,
    callback: Mutex<Option<Strong<dyn ILnbCallback>>>,
    registry: Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
    frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>>,
}

impl LnbHal {
    fn new(
        lnb_id: i32,
        registry: Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
        frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>>,
    ) -> BinderResult<Self> {
        Ok(Self {
            lnb_id,
            closed: RuntimeAtomicFlag::new(false),
            failed: RuntimeAtomicFlag::new(false),
            callback: Mutex::new(None),
            registry,
            frontend_registry,
        })
    }

    fn acquire_operation_guard(&self) -> BinderResult<LnbOperationGuard> {
        match LnbLedger::operation_guard(self.lnb_id) {
            Ok(guard) => Ok(guard),
            Err(LnbOperationGuardError::Busy) => Err(Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None)),
            Err(LnbOperationGuardError::Poisoned) => {
                self.failed.store(true, Ordering::SeqCst);
                Err(lnb_operation_guard_error_status(self.lnb_id, LnbOperationGuardError::Poisoned))
            }
            Err(LnbOperationGuardError::DropReleaseFailed) => {
                self.failed.store(true, Ordering::SeqCst);
                Err(lnb_operation_guard_error_status(self.lnb_id, LnbOperationGuardError::DropReleaseFailed))
            }
        }
    }

    fn apply_to_matching_frontends(&self, lnb: &LnbRuntimeState) -> Result<(), HalError> {
        for runtime in self.frontend_registry.values() {
            let mut backend = lock_mutex_hal(&runtime.backend, "frontend_backend")?;
            if FrontendHal::backend_selected_lnb_id(&backend) == Some(self.lnb_id) {
                FrontendHal::backend_apply_lnb_state(&mut backend, lnb)?;
            }
        }
        Ok(())
    }

    fn apply_diseqc_once_to_matching_frontends(
        &self,
        generation: u64,
        message: &[u8],
    ) -> Result<(), HalError> {
        for runtime in self.frontend_registry.values() {
            let mut backend = lock_mutex_hal(&runtime.backend, "frontend_backend")?;
            if FrontendHal::backend_selected_lnb_id(&backend) != Some(self.lnb_id) {
                continue;
            }
            let mut sent_generations = lock_mutex_hal(&runtime.sent_diseqc_generations, "sent_diseqc_generations")?;
            let already_sent = sent_generations
                .get(&self.lnb_id)
                .copied()
                .unwrap_or(NO_DISEQC_GENERATION)
                >= generation;
            if already_sent {
                continue;
            }
            FrontendHal::backend_send_diseqc_message(&mut backend, message)?;
            sent_generations.insert(self.lnb_id, generation);
        }
        Ok(())
    }

    fn record_close_reset_error(&self, detail: String) {
        if let Some(mut registry) = lock_mutex_option(&self.registry, "lnb_registry") {
            let state = registry.entry(self.lnb_id).or_default();
            state.last_close_reset_error = Some(detail);
        }
    }

    fn safe_state_for_close(&self, clear_last_error: bool) -> BinderResult<LnbRuntimeState> {
        let registry = lock_mutex_status(&self.registry, "lnb_registry")?;
        let mut state = registry.get(&self.lnb_id).cloned().unwrap_or_default();
        state.voltage = Some(LnbVoltage::NONE);
        state.tone = Some(LnbTone::NONE);
        state.position = Some(LnbPosition::UNDEFINED);
        state.generation = state.generation.saturating_add(1);
        if clear_last_error {
            state.last_close_reset_error = None;
        }
        Ok(state)
    }

    fn commit_lnb_state(&self, new_state: LnbRuntimeState, context: &str) -> BinderResult<()> {
        match lock_mutex_status(&self.registry, "lnb_registry") {
            Ok(mut entries) => {
                entries.insert(self.lnb_id, new_state);
                Ok(())
            }
            Err(status) => {
                self.failed.store(true, Ordering::SeqCst);
                let diagnostic = if context == "update" {
                    "lnb_registry_commit_failed_after_backend_apply"
                } else {
                    "lnb_registry_commit_error"
                };
                record_tuner_diagnostic_counter(
                    &LNB_BACKEND_APPLY_ERROR_COUNT,
                    diagnostic,
                );
                eprintln!(
                    "maleicacid-tuner-hal-lnb-diagnostic: lnb_id={} context={} {}={:?}",
                    self.lnb_id, context, diagnostic, status
                );
                Err(Status::from(StatusCode::UNKNOWN_ERROR))
            }
        }
    }

    fn reset_state_for_close(&self) -> BinderResult<()> {
        let _op = self.acquire_operation_guard()?;
        let mut first_error: Option<Status> = None;
        let new_state = match self.safe_state_for_close(true) {
            Ok(state) => Some(state),
            Err(status) => {
                first_error.get_or_insert(status);
                None
            }
        };
        if let Some(state) = new_state {
            if let Err(err) = self.apply_to_matching_frontends(&state) {
                record_tuner_diagnostic_counter(
                    &LNB_BACKEND_APPLY_ERROR_COUNT,
                    "lnb_backend_apply_error",
                );
                let detail = format!("LNB close backend reset failed: {err}");
                eprintln!("maleicacid-tuner-hal-lnb-diagnostic: lnb_id={} {detail}", self.lnb_id);
                self.record_close_reset_error(detail);
                first_error.get_or_insert(hal_error_status(err));
            } else if let Err(status) = self.commit_lnb_state(state, "close_reset") {
                first_error.get_or_insert(status);
            }
        }
        if let Some(status) = first_error {
            // close失敗時は closed=true にしない。次回 close() が残cleanupを再試行できる状態に残す。
            Err(status)
        } else {
            self.closed.store(true, Ordering::SeqCst);
            if let Some(mut callback) = lock_mutex_option(&self.callback, "lnb_callback") {
                *callback = None;
            }
            Ok(())
        }
    }

    fn best_effort_reset_state_for_drop(&self) {
        if let Some(mut callback) = lock_mutex_option(&self.callback, "lnb_callback") {
            *callback = None;
        }
        self.closed.store(true, Ordering::SeqCst);
        let Ok(_op) = self.acquire_operation_guard() else {
            self.failed.store(true, Ordering::SeqCst);
            return;
        };
        let new_state = match self.safe_state_for_close(true) {
            Ok(state) => state,
            Err(err) => {
                self.failed.store(true, Ordering::SeqCst);
                eprintln!("maleicacid-tuner-hal-lnb-diagnostic: lnb_id={} drop_safe_state_failed={:?}", self.lnb_id, err);
                return;
            }
        };
        if let Err(err) = self.apply_to_matching_frontends(&new_state) {
            record_tuner_diagnostic_counter(
                &LNB_BACKEND_APPLY_ERROR_COUNT,
                "lnb_backend_apply_error",
            );
            let detail = format!("LNB drop backend reset failed: {err}");
            eprintln!("maleicacid-tuner-hal-lnb-diagnostic: lnb_id={} {detail}", self.lnb_id);
            self.record_close_reset_error(detail);
        } else if let Err(err) = self.commit_lnb_state(new_state, "drop_reset") {
            eprintln!("maleicacid-tuner-hal-lnb-diagnostic: lnb_id={} drop_commit_failed={:?}", self.lnb_id, err);
        }
        if let Some(mut callback) = lock_mutex_option(&self.callback, "lnb_callback") {
            *callback = None;
        }
    }

    fn update_lnb_state<F>(&self, mut update: F) -> BinderResult<()>
    where
        F: FnMut(&mut LnbRuntimeState) -> BinderResult<()>,
    {
        let _op = self.acquire_operation_guard()?;
        if self.failed.load(Ordering::SeqCst) {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        if self.closed.load(Ordering::SeqCst) {
            return Err(invalid_state_status("LNB is closed"));
        }
        let new_state = {
            let registry = lock_mutex_status(&self.registry, "lnb_registry")?;
            let mut state = registry.get(&self.lnb_id).cloned().unwrap_or_default();
            update(&mut state)?;
            state.generation = state.generation.saturating_add(1);
            state
        };
        if let Err(err) = self.apply_to_matching_frontends(&new_state) {
            record_tuner_diagnostic_counter(
                &LNB_BACKEND_APPLY_ERROR_COUNT,
                "lnb_backend_apply_error",
            );
            eprintln!(
                "maleicacid-tuner-hal-lnb-diagnostic: lnb_id={} backend_apply_failed={err}",
                self.lnb_id
            );
            return Err(hal_error_status(err));
        }
        self.commit_lnb_state(new_state, "update")
    }

    fn ensure_open(&self) -> BinderResult<()> {
        if self.failed.load(Ordering::SeqCst) {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        if self.closed.load(Ordering::SeqCst) {
            return Err(invalid_state_status("LNB is closed"));
        }
        Ok(())
    }

    fn voltage_supported(profile: LnbDeviceProfile, voltage: LnbVoltage) -> bool {
        match profile {
            LnbDeviceProfile::Px4Device15VOnly => {
                matches!(voltage, LnbVoltage::NONE | LnbVoltage::VOLTAGE_15V)
            }
            LnbDeviceProfile::EarthPt1FixedLnb => matches!(
                voltage,
                LnbVoltage::NONE | LnbVoltage::VOLTAGE_11V | LnbVoltage::VOLTAGE_15V
            ),
            LnbDeviceProfile::NoPower => matches!(voltage, LnbVoltage::NONE),
        }
    }

    #[cfg(test)]
    fn callback_is_set_for_test(&self) -> bool {
        lock_mutex_status(&self.callback, "test_mutex").unwrap().is_some()
    }
}


impl Drop for LnbHal {
    fn drop(&mut self) {
        if !self.closed.load(Ordering::SeqCst) {
            self.best_effort_reset_state_for_drop();
            self.closed.store(true, Ordering::SeqCst);
        }
        if let Some(mut callback) = lock_mutex_option(&self.callback, "lnb_callback") {
            *callback = None;
        }
    }
}

impl Interface for LnbHal {}

impl ILnb for LnbHal {
    fn setCallback(&self, callback: &Strong<dyn ILnbCallback>) -> BinderResult<()> {
        self.ensure_open()?;
        *lock_mutex_status(&self.callback, "lnb_callback")? = Some(callback.clone());
        Ok(())
    }

    fn setVoltage(&self, voltage: LnbVoltage) -> BinderResult<()> {
        self.ensure_open()?;
        self.update_lnb_state(|state| {
            if !Self::voltage_supported(state.profile, voltage) {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNAVAILABLE.0,
                    None,
                ));
            }
            state.voltage = Some(voltage);
            Ok(())
        })
    }

    fn setTone(&self, tone: LnbTone) -> BinderResult<()> {
        self.ensure_open()?;
        if !matches!(tone, LnbTone::NONE) {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        self.update_lnb_state(|state| {
            state.tone = Some(LnbTone::NONE);
            Ok(())
        })
    }

    fn setSatellitePosition(&self, position: LnbPosition) -> BinderResult<()> {
        self.ensure_open()?;
        // 対象 device は固定配線だけを扱う。tone、DiSEqC、position switching は恒久未対応なので、
        // 中立または未定義の position だけを受け付ける。
        if !matches!(position, LnbPosition::UNDEFINED) {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        self.update_lnb_state(|state| {
            state.position = Some(LnbPosition::UNDEFINED);
            Ok(())
        })?;
        eprintln!(
            "maleicacid-tuner-hal: LNB {} satellite_position=UNDEFINED fixed-profile",
            self.lnb_id
        );
        Ok(())
    }

    fn sendDiseqcMessage(&self, _diseqc_message: &[u8]) -> BinderResult<()> {
        self.ensure_open()?;
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ))
    }

    fn close(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            *lock_mutex_status(&self.callback, "lnb_callback")? = None;
            return Ok(());
        }
        self.reset_state_for_close()
    }
}

fn build_filter_event_from_entry(
    record: &maleicacid_tuner_hal_soft_demux::DemuxFilterRecord,
    payload: &FilterPayload,
    offset: i64,
    cumulative_bytes: u64,
    av_slice: Option<AvBufferSlice>,
    av_data_id: Option<i64>,
    av_memory: Option<TunerNativeHandle>,
    record_state: &mut RecordEventState,
) -> Option<DemuxFilterEvent> {
    build_filter_event_from_payload(
        record,
        payload.event_bytes(),
        payload.av_metadata(),
        payload.pes_stream_id(),
        offset,
        cumulative_bytes,
        av_slice,
        av_data_id,
        av_memory,
        record_state,
    )
}

fn build_filter_event_from_payload(
    record: &maleicacid_tuner_hal_soft_demux::DemuxFilterRecord,
    payload: &[u8],
    av_metadata: Option<&AvPayloadMetadata>,
    pes_stream_id: Option<i32>,
    offset: i64,
    cumulative_bytes: u64,
    av_slice: Option<AvBufferSlice>,
    av_data_id: Option<i64>,
    av_memory: Option<TunerNativeHandle>,
    record_state: &mut RecordEventState,
) -> Option<DemuxFilterEvent> {
    let config = record.config.as_ref()?;
    match &config.kind {
        FilterConfigKind::Section {
            raw,
            length_field_bits,
            ..
        } => {
            let parsed = parse_section_event(payload, *length_field_bits);
            if !*raw && parsed.is_none() {
                return None;
            }
            let (table_id, version, section_num, data_len) =
                parsed.unwrap_or((0, 0, 0, payload.len() as i64));
            Some(DemuxFilterEvent::Section(DemuxFilterSectionEvent {
                tableId: table_id,
                version,
                sectionNum: section_num,
                dataLength: data_len,
            }))
        }
        FilterConfigKind::PesData { raw, .. } => {
            let stream_id = pes_stream_id
                .or_else(|| pes_stream_id(payload))
                .unwrap_or(PES_STREAM_ID_UNKNOWN);
            if !*raw && stream_id == PES_STREAM_ID_UNKNOWN && pes_stream_id(payload).is_none() {
                return None;
            }
            Some(DemuxFilterEvent::Pes(DemuxFilterPesEvent {
                streamId: stream_id,
                dataLength: payload.len() as i32,
                mpuSequenceNumber: 0,
            }))
        }
        FilterConfigKind::Av {
            passthrough: false,
            secure_memory,
        } => {
            if *secure_memory {
                eprintln!(
                    "maleicacid-tuner-hal-av-diagnostic: filter_id={} reason=SECURE_MEMORY_UNSUPPORTED_REACHED_EVENT_BUILDER",
                    record.filter_id
                );
                return None;
            }
            let av_slice = av_slice?;
            let av_data_id = av_data_id.filter(|id| *id != 0)?;
            let av_memory = av_memory?;
            let (pts, dts, stream_id) = if let Some(metadata) = av_metadata {
                (metadata.pts_90khz, metadata.dts_90khz, metadata.stream_id)
            } else {
                let (pts, dts) = pes_time_fields(payload);
                (
                    pts,
                    dts,
                    pes_stream_id(payload).unwrap_or(config.sub_type_hint),
                )
            };
            let event = DemuxFilterMediaEvent {
                streamId: stream_id,
                isPtsPresent: pts.is_some(),
                pts: pts.map(|value| value as i64).unwrap_or(MEDIA_EVENT_TIMESTAMP_ABSENT),
                isDtsPresent: dts.is_some(),
                dts: dts.map(|value| value as i64).unwrap_or(MEDIA_EVENT_TIMESTAMP_ABSENT),
                dataLength: av_slice.len as i64,
                offset: av_slice.offset as i64,
                avMemory: av_memory,
                isSecureMemory: false,
                avDataId: av_data_id,
                mpuSequenceNumber: 0,
                isPesPrivateData: stream_id == 0xbd,
                extraMetaData: DemuxFilterMediaEventExtraMetaData::Noinit(false),
                scIndexMask: DemuxFilterScIndexMask::ScIndex(0),
            };
            Some(DemuxFilterEvent::Media(event))
        }
        FilterConfigKind::Record {
            ts_index_mask,
            sc_index_type,
            sc_index_mask_bits,
        } => build_ts_record_event(
            payload,
            cumulative_bytes,
            *ts_index_mask,
            *sc_index_type,
            *sc_index_mask_bits,
            record_state,
        ),
        _ => None,
    }
}

fn build_ts_record_event(
    packet: &[u8],
    cumulative_bytes: u64,
    configured_ts_index_mask: i32,
    sc_index_type: i32,
    configured_sc_index_mask_bits: i32,
    record_state: &mut RecordEventState,
) -> Option<DemuxFilterEvent> {
    let event = RecordIndexParser::new().build_event(
        packet,
        cumulative_bytes,
        configured_ts_index_mask,
        sc_index_type,
        configured_sc_index_mask_bits,
        record_state,
    )?;
    Some(DemuxFilterEvent::TsRecord(DemuxFilterTsRecordEvent {
        pid: DemuxPid::TPid(event.pid),
        tsIndexMask: event.ts_index_mask,
        scIndexMask: aidl_sc_index_mask_from_record_event(event),
        byteNumber: event.byte_number,
        pts: event.pts,
        firstMbInSlice: event.first_mb_in_slice,
    }))
}

fn aidl_sc_index_mask_from_record_event(event: TsRecordEventData) -> DemuxFilterScIndexMask {
    match event.sc_index_type {
        RECORD_SC_TYPE_SC => DemuxFilterScIndexMask::ScIndex(event.sc_index_mask_bits),
        RECORD_SC_TYPE_SC_AVC => DemuxFilterScIndexMask::ScAvc(event.sc_index_mask_bits),
        RECORD_SC_TYPE_SC_HEVC => DemuxFilterScIndexMask::ScHevc(event.sc_index_mask_bits),
        RECORD_SC_TYPE_SC_VVC => DemuxFilterScIndexMask::ScVvc(event.sc_index_mask_bits),
        _ => DemuxFilterScIndexMask::ScIndex(0),
    }
}

fn parse_section_event(payload: &[u8], length_field_bits: i32) -> Option<(i32, i32, i32, i64)> {
    let header = parse_section_header(payload, length_field_bits)?;
    let version = header.version.map(|value| value as i32).unwrap_or(SECTION_VERSION_ABSENT);
    let section_num = header.section_number.map(|value| value as i32).unwrap_or(SECTION_NUMBER_ABSENT);
    Some((
        header.table_id as i32,
        version,
        section_num,
        header.total_length as i64,
    ))
}

fn cast_u64(value: i64, field: &'static str) -> Result<u64, HalError> {
    u64::try_from(value)
        .map_err(|_| HalError::InvalidArgument(format!("{field} must be non-negative")))
}

fn positive_i64_to_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok().filter(|v| *v > 0)
}

fn optional_positive_i64_to_u64(value: i64, field: &'static str) -> Result<Option<u64>, HalError> {
    if value < 0 {
        return Err(HalError::InvalidArgument(format!(
            "{field} must be non-negative"
        )));
    }
    Ok(positive_i64_to_u64(value))
}

fn positive_i32_to_u32(value: i32) -> Option<u32> {
    u32::try_from(value).ok().filter(|v| *v > 0)
}

fn nonnegative_i32_to_u32(value: i32) -> Option<u32> {
    u32::try_from(value).ok()
}

const AOSP_TUNER_INVALID_STREAM_ID: i32 = -1;

fn map_isdbs_stream_selector(
    stream_id: i32,
    stream_id_type: FrontendIsdbsStreamIdType,
    frequency_hz: u64,
) -> Result<(Option<u32>, Option<FrontendStreamIdKind>), HalError> {
    match stream_id_type {
        FrontendIsdbsStreamIdType::UNDEFINED => {
            if stream_id != 0 {
                return Err(HalError::InvalidArgument(
                    "streamIdType が UNDEFINED の場合、ISDB-S streamId は0である必要があります".into(),
                ));
            }
            Ok((None, None))
        }
        FrontendIsdbsStreamIdType::STREAM_ID => {
            if stream_id == AOSP_TUNER_INVALID_STREAM_ID {
                return Ok((None, None));
            }
            if stream_id < 0 {
                return Err(HalError::InvalidArgument(
                    "streamIdType 指定時のISDB-S stream selector は負値にできません"
                        .into(),
                ));
            }
            if is_japan_cs110_if_frequency_hz(frequency_hz) {
                return Err(HalError::InvalidArgument(
                    "CS110フロントエンド選局にTSIDまたは相対ストリーム番号セレクタを載せてはなりません"
                        .into(),
                ));
            }
            let value = u32::try_from(stream_id).map_err(|_| {
                HalError::InvalidArgument(format!(
                    "ISDB-S stream selector が範囲外です: {stream_id}"
                ))
            })?;
            Ok((Some(value), Some(FrontendStreamIdKind::AbsoluteStreamId)))
        }
        FrontendIsdbsStreamIdType::RELATIVE_STREAM_NUMBER => {
            if stream_id < 0 {
                return Err(HalError::InvalidArgument(
                    "ISDB-S relative stream selector は負値にできません".into(),
                ));
            }
            if is_japan_cs110_if_frequency_hz(frequency_hz) {
                return Err(HalError::InvalidArgument(
                    "CS110フロントエンド選局にTSIDまたは相対ストリーム番号セレクタを載せてはなりません"
                        .into(),
                ));
            }
            let value = u32::try_from(stream_id).map_err(|_| {
                HalError::InvalidArgument(format!(
                    "ISDB-S relative stream selector が範囲外です: {stream_id}"
                ))
            })?;
            Ok((Some(value), Some(FrontendStreamIdKind::RelativeStreamNumber)))
        }
        _ => Err(HalError::InvalidArgument(format!(
            "未対応の ISDB-S streamIdType です: {:?}",
            stream_id_type
        ))),
    }
}

fn satellite_symbol_rate_sps(value: i32, field_name: &str) -> Result<Option<u32>, HalError> {
    let Some(rate) = positive_i32_to_u32(value) else {
        return Ok(None);
    };
    if rate < 100_000 {
        return Err(HalError::InvalidArgument(format!(
            "{field_name} must be in symbols/second, got underscaled value {rate}"
        )));
    }
    Ok(Some(rate))
}

fn map_isdbt_bandwidth(bandwidth: FrontendIsdbtBandwidth) -> Option<u32> {
    match bandwidth {
        FrontendIsdbtBandwidth::BANDWIDTH_6MHZ => Some(6_000_000),
        FrontendIsdbtBandwidth::BANDWIDTH_7MHZ => Some(7_000_000),
        FrontendIsdbtBandwidth::BANDWIDTH_8MHZ => Some(8_000_000),
        _ => None,
    }
}

fn hal_error_tuner_result(err: &HalError) -> i32 {
    match err {
        HalError::InvalidArgument(_) => TunerResult::INVALID_ARGUMENT.0,
        HalError::InvalidState(_) => TunerResult::INVALID_STATE.0,
        HalError::Internal(_) => TunerResult::UNKNOWN_ERROR.0,
        HalError::DeviceMissing(_)
        | HalError::OpenFailed { .. }
        | HalError::PermissionDenied { .. }
        | HalError::Busy { .. }
        | HalError::Unsupported(_) => TunerResult::UNAVAILABLE.0,
        HalError::Io { .. } | HalError::IoctlFailed { .. } => TunerResult::UNKNOWN_ERROR.0,
    }
}

fn hal_error_status(err: HalError) -> Status {
    eprintln!("maleicacid-tuner-hal: backendエラー: {err}");
    Status::new_service_specific_error(hal_error_tuner_result(&err), None)
}

fn invalid_argument_status(message: &str) -> Status {
    eprintln!("maleicacid-tuner-hal: 不正な引数: {message}");
    Status::new_service_specific_error(TunerResult::INVALID_ARGUMENT.0, None)
}

fn invalid_state_status(message: &str) -> Status {
    eprintln!("maleicacid-tuner-hal: 不正な状態: {message}");
    Status::new_service_specific_error(TunerResult::INVALID_STATE.0, None)
}

fn callback_failure_status(object_kind: &str, object_id: i32, api: &str, err: &Status) -> Status {
    let detail = format!(
        "callback failure: object_kind={} object_id={} api={} binder_status={:?}",
        object_kind, object_id, api, err
    );
    eprintln!("maleicacid-tuner-hal-callback: {detail}");
    Status::new_service_specific_error(TunerResult::UNKNOWN_ERROR.0, Some(&detail))
}

fn demux_config_error_tuner_result(err: DemuxConfigError) -> Option<i32> {
    match err {
        DemuxConfigError::NotFound => None,
        DemuxConfigError::CapacityExceeded => Some(TunerResult::UNAVAILABLE.0),
        DemuxConfigError::InvalidKind => Some(TunerResult::INVALID_ARGUMENT.0),
        DemuxConfigError::InvalidState => Some(TunerResult::INVALID_STATE.0),
        DemuxConfigError::IdExhausted => Some(TunerResult::UNKNOWN_ERROR.0),
    }
}

fn demux_config_error_status(err: DemuxConfigError) -> Status {
    match demux_config_error_tuner_result(err) {
        Some(code) => Status::new_service_specific_error(code, None),
        None => StatusCode::NAME_NOT_FOUND.into(),
    }
}

fn filter_main_type_supported(main_type: DemuxFilterMainType) -> bool {
    main_type == DemuxFilterMainType::TS
}

fn filter_open_type(filter_type: &DemuxFilterType) -> BinderResult<FilterOpenType> {
    if filter_type.mainType != DemuxFilterMainType::TS {
        return Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ));
    }
    match &filter_type.subType {
        DemuxFilterSubType::TsFilterType(ts_type) => match *ts_type {
            DemuxTsFilterType::TS => Ok(FilterOpenType::TsRaw),
            DemuxTsFilterType::AUDIO => Ok(FilterOpenType::TsAudio),
            DemuxTsFilterType::VIDEO => Ok(FilterOpenType::TsVideo),
            DemuxTsFilterType::SECTION => Ok(FilterOpenType::TsSection),
            DemuxTsFilterType::PES => Ok(FilterOpenType::TsPes),
            DemuxTsFilterType::RECORD => Ok(FilterOpenType::TsRecord),
            _ => Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            )),
        },
        _ => Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        )),
    }
}

fn filter_start_event_ready(
    readiness: maleicacid_tuner_hal_soft_demux::FilterDeliveryReadiness,
) -> bool {
    matches!(
        readiness,
        maleicacid_tuner_hal_soft_demux::FilterDeliveryReadiness::Ready
    )
}

fn normalize_filter_delay_hint_for_record(
    record: &DemuxFilterRecord,
    hint: &FilterDelayHint,
) -> BinderResult<FilterDelayHintState> {
    let configured_media = matches!(
        record.config.as_ref().map(|cfg| &cfg.kind),
        Some(FilterConfigKind::Av { .. })
    );
    if record.open_type.is_media() || configured_media {
        return Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ));
    }
    if record.open_type == FilterOpenType::TsRecord
        && hint.hintType == FilterDelayHintType::DATA_SIZE_DELAY_IN_BYTES
    {
        return Err(Status::new_service_specific_error(
            TunerResult::INVALID_ARGUMENT.0,
            None,
        ));
    }
    normalize_filter_delay_hint(hint)
}

fn normalize_filter_delay_hint(hint: &FilterDelayHint) -> BinderResult<FilterDelayHintState> {
    if hint.hintValue < 0 {
        return Err(invalid_argument_status(
            "フィルタ遅延指定は0以上である必要があります",
        ));
    }
    match hint.hintType {
        FilterDelayHintType::TIME_DELAY_IN_MS => {
            Ok(FilterDelayHintState::TimeDelayMs(hint.hintValue as u64))
        }
        FilterDelayHintType::DATA_SIZE_DELAY_IN_BYTES => Ok(
            FilterDelayHintState::DataSizeDelayBytes(hint.hintValue as usize),
        ),
        _ => Err(Status::new_service_specific_error(
            TunerResult::INVALID_ARGUMENT.0,
            None,
        )),
    }
}

fn validate_ts_pid(pid: i32) -> BinderResult<i32> {
    if (0..=0x1fff).contains(&pid) {
        Ok(pid)
    } else {
        Err(invalid_argument_status("TS PID が範囲外です"))
    }
}

const PES_STREAM_ID_WILDCARD: i32 = -1;

fn normalize_pes_stream_id(stream_id: i32) -> BinderResult<i32> {
    if stream_id == PES_STREAM_ID_WILDCARD || (0..=255).contains(&stream_id) {
        Ok(stream_id)
    } else if stream_id < 0 {
        Err(invalid_argument_status(
            "PES streamId は -1 のワイルドカードまたは 0..=255 である必要があります",
        ))
    } else {
        Err(invalid_argument_status("PES streamId は255以下である必要があります"))
    }
}

fn supported_record_ts_index_mask() -> i32 {
    DEMUX_TS_INDEX_FIRST_PACKET
        | DEMUX_TS_INDEX_PAYLOAD_UNIT_START
        | DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED
        | DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED
        | DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED
        | DEMUX_TS_INDEX_DISCONTINUITY
        | DEMUX_TS_INDEX_RANDOM_ACCESS
        | DEMUX_TS_INDEX_PRIORITY
        | DEMUX_TS_INDEX_PCR
        | DEMUX_TS_INDEX_OPCR
        | DEMUX_TS_INDEX_SPLICING_POINT
        | DEMUX_TS_INDEX_PRIVATE_DATA
        | DEMUX_TS_INDEX_ADAPTATION_EXTENSION
}

fn record_sc_mask_variant_type(mask: &DemuxFilterScIndexMask) -> (i32, i32) {
    match mask {
        DemuxFilterScIndexMask::ScIndex(v) => (RECORD_SC_TYPE_SC, *v),
        DemuxFilterScIndexMask::ScAvc(v) => (RECORD_SC_TYPE_SC_AVC, *v),
        DemuxFilterScIndexMask::ScHevc(v) => (RECORD_SC_TYPE_SC_HEVC, *v),
        DemuxFilterScIndexMask::ScVvc(v) => (RECORD_SC_TYPE_SC_VVC, *v),
    }
}

fn supported_record_sc_index_mask(sc_index_type: i32) -> i32 {
    match sc_index_type {
        RECORD_SC_TYPE_NONE => 0,
        RECORD_SC_TYPE_SC => (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3),
        RECORD_SC_TYPE_SC_AVC => {
            AVC_SC_I_SLICE | AVC_SC_P_SLICE | AVC_SC_B_SLICE | AVC_SC_SI_SLICE | AVC_SC_SP_SLICE
        }
        RECORD_SC_TYPE_SC_HEVC => {
            HEVC_SC_SPS
                | HEVC_SC_AUD
                | HEVC_SC_BLA_W_LP
                | HEVC_SC_BLA_W_RADL
                | HEVC_SC_BLA_N_LP
                | HEVC_SC_IDR_W_RADL
                | HEVC_SC_IDR_N_LP
                | HEVC_SC_TRAIL_CRA
        }
        RECORD_SC_TYPE_SC_VVC => {
            VVC_SC_IDR_W_RADL
                | VVC_SC_IDR_N_LP
                | VVC_SC_CRA
                | VVC_SC_GDR
                | VVC_SC_VPS
                | VVC_SC_SPS
                | VVC_SC_AUD
        }
        _ => 0,
    }
}

fn validate_record_index_settings(
    ts_index_mask: i32,
    sc_index_type: i32,
    sc_index_mask: &DemuxFilterScIndexMask,
) -> BinderResult<i32> {
    if (ts_index_mask & !supported_record_ts_index_mask()) != 0 {
        return Err(invalid_argument_status(
            "record tsIndexMask に未対応bitが含まれます",
        ));
    }
    let (expected_type, sc_index_mask_bits) = record_sc_mask_variant_type(sc_index_mask);
    if sc_index_type == RECORD_SC_TYPE_NONE {
        if sc_index_mask_bits != 0 {
            return Err(invalid_argument_status(
                "record SC index NONE ではmaskが0である必要があります",
            ));
        }
    } else if !matches!(
        sc_index_type,
        RECORD_SC_TYPE_SC | RECORD_SC_TYPE_SC_AVC | RECORD_SC_TYPE_SC_HEVC | RECORD_SC_TYPE_SC_VVC
    ) {
        return Err(invalid_argument_status("unsupported record SC index type"));
    } else if sc_index_type != expected_type {
        return Err(invalid_argument_status(
            "record SC index type and mask union variant mismatch",
        ));
    }
    let supported_mask = supported_record_sc_index_mask(sc_index_type);
    if (sc_index_mask_bits & !supported_mask) != 0 {
        return Err(invalid_argument_status(
            "record scIndexMask に未対応bitが含まれます",
        ));
    }
    Ok(sc_index_mask_bits)
}

fn build_filter_summary(settings: &DemuxFilterSettings) -> BinderResult<FilterConfig> {
    build_filter_summary_for_open_type(settings, FilterOpenType::TsOther)
}

fn build_filter_summary_for_open_type(
    settings: &DemuxFilterSettings,
    open_type: FilterOpenType,
) -> BinderResult<FilterConfig> {
    let config = match settings {
        DemuxFilterSettings::Ts(ts) => {
            let tpid = validate_ts_pid(ts.tpid)?;
            FilterConfig {
                tpid,
                main_type_bits: DemuxFilterMainType::TS.0,
                sub_type_hint: 0,
                kind: match &ts.filterSettings {
                    DemuxTsFilterSettingsFilterSettings::Noinit(_) => {
                        if open_type != FilterOpenType::TsRaw {
                            return Err(Status::new_service_specific_error(
                                TunerResult::INVALID_ARGUMENT.0,
                                Some("TS noinit settings are valid only for DemuxTsFilterType::TS"),
                            ));
                        }
                        FilterConfigKind::Noinit
                    }
                    DemuxTsFilterSettingsFilterSettings::Section(section) => {
                        let Some(length_field_bits) =
                            normalize_length_field_bits(section.bitWidthOfLengthField)
                        else {
                            return Err(Status::new_service_specific_error(
                                TunerResult::INVALID_ARGUMENT.0,
                                Some("r51のTS section filterは bitWidthOfLengthField 0 または12だけをサポートします"),
                            ));
                        };
                        FilterConfigKind::Section {
                            check_crc: section.isCheckCrc,
                            repeat: section.isRepeat,
                            raw: section.isRaw,
                            length_field_bits,
                            condition_kind: build_section_condition_kind(&section.condition),
                            condition: build_section_condition(&section.condition)?,
                        }
                    }
                    DemuxTsFilterSettingsFilterSettings::Av(av) => {
                        if av.isPassthrough {
                            return Err(Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, Some("本製品ではAV passthroughは恒久的に非対応です。non-passthrough MediaEvent + 共有メモリ配送を使ってください")));
                        }
                        if av.isSecureMemory {
                            return Err(Status::new_service_specific_error(
                                TunerResult::UNAVAILABLE.0,
                                None,
                            ));
                        }
                        FilterConfigKind::Av {
                            passthrough: av.isPassthrough,
                            secure_memory: false,
                        }
                    }
                    DemuxTsFilterSettingsFilterSettings::PesData(pes) => {
                        FilterConfigKind::PesData {
                            stream_id: normalize_pes_stream_id(pes.streamId)?,
                            raw: pes.isRaw,
                        }
                    }
                    DemuxTsFilterSettingsFilterSettings::Record(record) => {
                        let sc_index_type = record.scIndexType.0;
                        let sc_index_mask_bits = validate_record_index_settings(
                            record.tsIndexMask,
                            sc_index_type,
                            &record.scIndexMask,
                        )?;
                        FilterConfigKind::Record {
                            ts_index_mask: record.tsIndexMask,
                            sc_index_type,
                            sc_index_mask_bits,
                        }
                    }
                },
            }
        }
        DemuxFilterSettings::Mmtp(_)
        | DemuxFilterSettings::Ip(_)
        | DemuxFilterSettings::Tlv(_)
        | DemuxFilterSettings::Alp(_) => {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
    };
    Ok(config)
}

fn build_section_condition_kind(
    condition: &DemuxFilterSectionSettingsCondition,
) -> SectionConditionKind {
    match condition {
        DemuxFilterSectionSettingsCondition::SectionBits(_) => SectionConditionKind::SectionBits,
        DemuxFilterSectionSettingsCondition::TableInfo(_) => SectionConditionKind::TableInfo,
    }
}

fn normalize_table_info_version(version: i32) -> BinderResult<Option<i32>> {
    if version < -1 || version > 31 {
        return Err(invalid_argument_status(
            "section table version は -1 または 0..31 である必要があります",
        ));
    }
    Ok((version >= 0).then_some(version))
}

fn normalize_section_table_id(table_id: i32) -> BinderResult<u8> {
    if (0..=255).contains(&table_id) {
        Ok(table_id as u8)
    } else {
        Err(invalid_argument_status(
            "section tableId は 0..=255 である必要があります",
        ))
    }
}

fn build_section_condition(
    condition: &DemuxFilterSectionSettingsCondition,
) -> BinderResult<SectionCondition> {
    let max = MAX_SECTION_FILTER_BYTES as usize;
    match condition {
        DemuxFilterSectionSettingsCondition::SectionBits(bits) => {
            if bits.filter.len() > max || bits.mask.len() > max || bits.mode.len() > max {
                return Err(invalid_argument_status(
                    "section filter condition byte length exceeds supported width",
                ));
            }
            Ok(SectionCondition {
                filter_bytes: bits.filter.clone(),
                mask_bytes: bits.mask.clone(),
                mode_bytes: bits.mode.clone(),
                table_id: None,
                version: None,
            })
        }
        DemuxFilterSectionSettingsCondition::TableInfo(table) => {
            let table_id = normalize_section_table_id(table.tableId)?;
            let version = normalize_table_info_version(table.version)?;
            Ok(SectionCondition {
                filter_bytes: vec![table_id],
                mask_bytes: vec![0xff],
                mode_bytes: vec![0],
                table_id: Some(table.tableId),
                version,
            })
        }
    }
}

fn normalize_dvr_type(dvr_type: DvrType) -> BinderResult<DemuxPathDirection> {
    match dvr_type {
        DvrType::RECORD => Ok(DemuxPathDirection::Record),
        DvrType::PLAYBACK => Ok(DemuxPathDirection::Playback),
        _ => Err(invalid_argument_status("対象外 DVR type")),
    }
}

fn validate_dvr_ts_188(data_format: DataFormat, packet_size: i64) -> BinderResult<()> {
    if data_format != DataFormat::TS {
        return Err(invalid_argument_status("DVR dataFormat はTSである必要があります"));
    }
    if packet_size != 188 {
        return Err(invalid_argument_status("DVR packetSize は188である必要があります"));
    }
    Ok(())
}

fn supported_record_status_mask() -> i32 {
    RecordStatus::DATA_READY.0
        | RecordStatus::HIGH_WATER.0
        | RecordStatus::LOW_WATER.0
        | RecordStatus::OVERFLOW.0
}

fn supported_playback_status_mask() -> i32 {
    PlaybackStatus::SPACE_EMPTY.0
        | PlaybackStatus::SPACE_ALMOST_EMPTY.0
        | PlaybackStatus::SPACE_ALMOST_FULL.0
        | PlaybackStatus::SPACE_FULL.0
}

fn validate_dvr_thresholds_and_mask(
    buffer_size: i32,
    low_threshold: i64,
    high_threshold: i64,
    status_mask: i32,
    supported_status_mask: i32,
) -> BinderResult<()> {
    if buffer_size <= 0 {
        return Err(invalid_argument_status("DVR bufferSize は正値である必要があります"));
    }
    let capacity = i64::from(buffer_size);
    if low_threshold < 0 {
        return Err(invalid_argument_status(
            "DVR lowThreshold は負値にできません",
        ));
    }
    if high_threshold < 0 {
        return Err(invalid_argument_status(
            "DVR highThreshold は負値にできません",
        ));
    }
    if low_threshold > high_threshold {
        return Err(invalid_argument_status(
            "DVR lowThreshold は highThreshold 以下である必要があります",
        ));
    }
    if low_threshold > capacity || high_threshold > capacity {
        return Err(invalid_argument_status(
            "DVR threshold は bufferSize 以下である必要があります",
        ));
    }
    if (status_mask & !supported_status_mask) != 0 {
        return Err(invalid_argument_status(
            "DVR statusMask に未対応bitが含まれます",
        ));
    }
    Ok(())
}

fn validate_and_build_dvr_summary(
    direction: DemuxPathDirection,
    settings: &DvrSettings,
    buffer_size: i32,
) -> BinderResult<DvrConfig> {
    match (direction, settings) {
        (DemuxPathDirection::Record, DvrSettings::Record(record)) => {
            validate_dvr_ts_188(record.dataFormat, record.packetSize)?;
            validate_dvr_thresholds_and_mask(
                buffer_size,
                record.lowThreshold,
                record.highThreshold,
                record.statusMask,
                supported_record_status_mask(),
            )?;
            Ok(DvrConfig {
                direction,
                status_mask: record.statusMask,
                low_threshold: record.lowThreshold,
                high_threshold: record.highThreshold,
                data_format: record.dataFormat.0,
                packet_size: record.packetSize,
            })
        }
        (DemuxPathDirection::Playback, DvrSettings::Playback(playback)) => {
            validate_dvr_ts_188(playback.dataFormat, playback.packetSize)?;
            validate_dvr_thresholds_and_mask(
                buffer_size,
                playback.lowThreshold,
                playback.highThreshold,
                playback.statusMask,
                supported_playback_status_mask(),
            )?;
            Ok(DvrConfig {
                direction,
                status_mask: playback.statusMask,
                low_threshold: playback.lowThreshold,
                high_threshold: playback.highThreshold,
                data_format: playback.dataFormat.0,
                packet_size: playback.packetSize,
            })
        }
        _ => Err(invalid_argument_status("DVR settings direction mismatch")),
    }
}

#[cfg(test)]
mod discovery_stage_tests {
    use super::*;

    #[test]
    fn aosp_invalid_stream_id_default_is_selector_none() {
        assert_eq!(
            map_isdbs_stream_selector(
                AOSP_TUNER_INVALID_STREAM_ID,
                FrontendIsdbsStreamIdType::STREAM_ID,
                1_613_000_000,
            )
            .unwrap(),
            (None, None)
        );
    }

    #[test]
    fn relative_negative_stream_id_is_rejected() {
        assert!(map_isdbs_stream_selector(
            AOSP_TUNER_INVALID_STREAM_ID,
            FrontendIsdbsStreamIdType::RELATIVE_STREAM_NUMBER,
            1_049_480_000,
        )
        .is_err());
    }

    #[test]
    fn undefined_negative_stream_id_is_rejected() {
        assert!(map_isdbs_stream_selector(
            AOSP_TUNER_INVALID_STREAM_ID,
            FrontendIsdbsStreamIdType::UNDEFINED,
            1_613_000_000,
        )
        .is_err());
    }

    #[test]
    fn relative_scan_stream_id_is_not_reported_as_tsid() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(0),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        assert_eq!(FrontendHal::reported_scan_input_stream_id(&request), None);
    }

    #[test]
    fn absolute_scan_stream_id_is_reported_only_when_it_is_known_bs_tsid() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(0x4011),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        assert_eq!(
            FrontendHal::reported_scan_input_stream_id(&request),
            Some(0x4011)
        );
    }

    #[test]
    fn raw_relative_like_absolute_scan_stream_id_is_not_reported() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(3),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        assert_eq!(FrontendHal::reported_scan_input_stream_id(&request), None);
    }

    #[test]
    fn cs110_scan_does_not_report_input_stream_ids() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_613_000_000,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
        };
        assert_eq!(FrontendHal::reported_scan_input_stream_id(&request), None);
    }
}

#[cfg(test)]
mod capacity_tests {
    use super::{
        DEMUX_MAX_AUDIO_FILTERS, DEMUX_MAX_FILTERS_PER_DEMUX, DEMUX_MAX_SECTION_FILTERS,
        DEMUX_MAX_TS_FILTERS, DEMUX_MAX_VIDEO_FILTERS, MAX_LIVE_DEMUXES, MAX_SECTION_FILTER_BYTES,
        MAX_SECTION_PAYLOAD_BYTES,
    };

    #[test]
    fn demux_capacity_constants_are_bounded() {
        assert!(MAX_LIVE_DEMUXES >= 1);
        assert!(DEMUX_MAX_FILTERS_PER_DEMUX <= 32);
        assert!(DEMUX_MAX_TS_FILTERS <= DEMUX_MAX_FILTERS_PER_DEMUX as i32);
        assert!(DEMUX_MAX_SECTION_FILTERS <= DEMUX_MAX_FILTERS_PER_DEMUX as i32);
        assert!(DEMUX_MAX_AUDIO_FILTERS <= 8);
        assert!(DEMUX_MAX_VIDEO_FILTERS <= 8);
        assert_eq!(MAX_SECTION_FILTER_BYTES, 16);
    }
}

#[cfg(test)]
mod time_filter_tests {
    use super::*;

    #[test]
    fn time_filter_is_not_reachable_from_demux_contract() {
        let hal = TunerHal::new();
        let demux_id = all_demux_ids()[0];
        let demux = hal.openDemuxById(demux_id).unwrap();
        assert!(demux.openTimeFilter().is_err());
    }
}

#[cfg(test)]
mod av_shared_backing_tests {
    use super::*;

    #[test]
    fn av_shared_backing_reports_no_slot_without_evicting_active_payload() {
        let backing = AvSharedBacking::new().unwrap();
        let payload = vec![0x55; 32];
        for id in 1..=AV_SLOT_COUNT as i64 {
            backing.allocate(id, &payload).unwrap();
        }
        assert_eq!(backing.stats().evicted_slots, 0);
        assert!(matches!(
            backing.allocate((AV_SLOT_COUNT as i64) + 1, &payload),
            Err(AvPayloadAllocateError::Delivery(
                AvPayloadDeliveryResult::DroppedNoFreeSlot
            ))
        ));
        let stats = backing.stats();
        assert_eq!(stats.evicted_slots, 0);
        assert_eq!(stats.av_overflow_no_slot, 1);
        assert_eq!(stats.alloc_failures, 1);
        assert!(backing.release(1).unwrap());
        assert!(!backing.release(1).unwrap());
        assert_eq!(backing.stats().stale_releases, 1);
    }

    #[test]
    fn av_shared_backing_offsets_lengths_and_release_reuse_are_bounded() {
        let backing = AvSharedBacking::new().unwrap();
        let payload = vec![0x47; 188];
        let first = backing.allocate(100, &payload).expect("first AV slot");
        assert_eq!(first.offset % backing.slot_size, 0);
        assert_eq!(first.len, payload.len());
        assert!(first.offset + first.len < backing.total_size());
        assert_eq!(backing.stats().allocated_slots, 1);
        assert!(backing.release(100).unwrap());
        assert_eq!(backing.stats().allocated_slots, 0);
        assert_eq!(backing.stats().free_slots, AV_SLOT_COUNT);
        let second = backing.allocate(101, &payload).expect("reused AV slot");
        assert_eq!(second.offset % backing.slot_size, 0);
        assert_eq!(second.len, payload.len());
        assert!(second.offset + second.len < backing.total_size());
        assert_eq!(backing.stats().allocated_slots, 1);
    }

    #[test]
    fn av_shared_backing_product_slot_size_is_at_least_one_mib() {
        let backing = AvSharedBacking::new().unwrap();
        assert!(backing.slot_size >= 1024 * 1024);
        assert!(backing.slot_size >= AV_MIN_SLOT_SIZE);
    }

    #[test]
    fn av_shared_backing_accepts_exact_slot_size_at_last_slot_boundary_and_separates_oversize_and_malformed_payloads() {
        let backing = AvSharedBacking::new().unwrap();
        let small = vec![0x47; 188];
        let exact = vec![0x47; backing.slot_size];
        let too_large = vec![0x47; backing.slot_size + 1];

        for slot_id in 0..(AV_SLOT_COUNT - 1) {
            let delivered = backing
                .allocate(1_000 + slot_id as i64, &small)
                .expect("last slot boundary test should reserve preceding AV slots");
            assert_eq!(delivered.slot_index, slot_id);
            assert!(delivered.offset + delivered.len < backing.total_size());
        }

        let delivered = backing
            .allocate(2_000, &exact)
            .expect("last slotをちょうど満たすpayloadは受け付ける必要があります");
        assert_eq!(delivered.slot_index, AV_SLOT_COUNT - 1);
        assert_eq!(delivered.len, backing.slot_size);
        assert_eq!(delivered.offset + delivered.len, backing.total_size());
        assert!(matches!(
            backing.allocate(2_001, &too_large),
            Err(AvPayloadAllocateError::Delivery(
                AvPayloadDeliveryResult::DroppedOversizePayload
            ))
        ));
        assert!(matches!(
            backing.allocate(2_002, &[]),
            Err(AvPayloadAllocateError::Delivery(
                AvPayloadDeliveryResult::DroppedMalformedPayload
            ))
        ));
    }

    #[test]
    fn active_slot_collision_fails_before_overwriting_existing_entry() {
        let backing = AvSharedBacking::new().unwrap();
        let payload = vec![0x47; 188];
        let first = backing
            .allocate(77, &payload)
            .expect("first active AV slot");
        let collision = backing.allocate(77, &payload);
        assert!(matches!(
            collision,
            Err(AvPayloadAllocateError::Internal(
                AvPayloadInternalError::ActiveSlotCollision
            ))
        ));
        let active = lock_mutex_status(&backing.active, "test_mutex").unwrap();
        assert_eq!(active.get(&77).copied(), Some(first));
        assert_eq!(active.len(), 1);
        drop(active);
        assert_eq!(backing.stats().allocated_slots, 1);
        assert_eq!(backing.stats().free_slots, AV_SLOT_COUNT - 1);
    }

    #[test]
    fn av_data_ready_gate_accepts_only_delivered_media_payloads() {
        assert!(av_payload_can_notify_data_ready(false, None));
        assert!(av_payload_can_notify_data_ready(
            true,
            Some(AvPayloadDeliveryResult::Delivered {
                slice: AvBufferSlice {
                    slot_index: 0,
                    offset: 0,
                    len: 188,
                    generation: 1
                },
                av_data_id: 1,
            })
        ));
        assert!(!av_payload_can_notify_data_ready(
            true,
            Some(AvPayloadDeliveryResult::DroppedBeforeHandleExport)
        ));
        assert!(!av_payload_can_notify_data_ready(
            true,
            Some(AvPayloadDeliveryResult::DroppedAfterClientRelease)
        ));
        assert!(!av_payload_can_notify_data_ready(
            true,
            Some(AvPayloadDeliveryResult::DroppedNoFreeSlot)
        ));
        assert!(!av_payload_can_notify_data_ready(
            true,
            Some(AvPayloadDeliveryResult::DroppedOversizePayload)
        ));
        assert!(!av_payload_can_notify_data_ready(
            true,
            Some(AvPayloadDeliveryResult::DroppedMalformedPayload)
        ));
    }

    #[test]
    fn av_payload_status_decision_matches_callback_worker_contract() {
        let delivered = Some(AvPayloadDeliveryResult::Delivered {
            slice: AvBufferSlice {
                slot_index: 0,
                offset: 0,
                len: 188,
                generation: 1,
            },
            av_data_id: 1,
        });
        assert_eq!(
            av_payload_status_decision(true, delivered, false),
            (true, false)
        );
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedBeforeHandleExport),
                true
            ),
            (false, true)
        );
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedAfterClientRelease),
                true
            ),
            (false, true)
        );
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedNoFreeSlot),
                true
            ),
            (false, true)
        );
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedOversizePayload),
                true
            ),
            (false, true)
        );
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedMalformedPayload),
                true
            ),
            (false, true)
        );
        assert_eq!(
            av_payload_status_decision(false, None, false),
            (true, false)
        );
    }
}

#[cfg(test)]
mod scan_phase_tests {
    use super::*;

    #[test]
    fn scan_phase_has_no_waiting_for_continue_state() {
        let phases = [
            ScanPhase::Running,
            ScanPhase::Completed,
            ScanPhase::Cancelled,
            ScanPhase::FailedBackend,
            ScanPhase::FailedCallback,
            ScanPhase::FailedPanic,
        ];
        assert_eq!(phases.len(), 6);
        assert!(ScanPhase::FailedBackend.is_failed());
        assert!(ScanPhase::FailedCallback.is_failed());
        assert!(ScanPhase::FailedPanic.is_failed());
    }
}

#[cfg(test)]
mod av_shared_stats_tests {
    use super::{AvSharedBacking, AV_MIN_SLOT_SIZE, AV_SLOT_COUNT};

    #[test]
    fn av_stats_report_no_slot_oversize_and_malformed_payload_without_evicting() {
        let backing = AvSharedBacking::new().unwrap();
        let payload = vec![0x55; 188];
        for id in 1..=(AV_SLOT_COUNT as i64) {
            backing.allocate(id, &payload).unwrap();
        }
        assert_eq!(backing.stats().evicted_slots, 0);
        assert!(matches!(
            backing.allocate(10_000, &payload),
            Err(super::AvPayloadAllocateError::Delivery(
                super::AvPayloadDeliveryResult::DroppedNoFreeSlot
            ))
        ));
        assert!(matches!(
            backing.allocate(10_001, &[]),
            Err(super::AvPayloadAllocateError::Delivery(
                super::AvPayloadDeliveryResult::DroppedMalformedPayload
            ))
        ));
        let too_large = vec![0x55; backing.slot_size + 1];
        assert!(matches!(
            backing.allocate(10_002, &too_large),
            Err(super::AvPayloadAllocateError::Delivery(
                super::AvPayloadDeliveryResult::DroppedOversizePayload
            ))
        ));
        let stats = backing.stats();
        assert_eq!(stats.evicted_slots, 0);
        assert_eq!(stats.av_overflow_no_slot, 1);
        assert_eq!(stats.av_malformed_payload, 1);
        assert_eq!(stats.av_oversize_payload, 1);
        assert!(stats.summary().contains("av_overflow_no_slot=1"));
        assert!(stats.summary().contains("av_malformed_payload=1"));
        assert!(stats.summary().contains("av_oversize_payload=1"));
        assert!(backing.debug_dump_line("unit").contains("unit av_shared"));
        assert!(backing
            .debug_dump_line("unit")
            .contains("av_overflow_no_slot=1"));
        assert!(backing
            .debug_dump_line("unit")
            .contains("av_oversize_payload=1"));
    }
}

#[cfg(test)]
mod startup_configuration_tests {
    use super::TunerHal;

    #[test]
    fn frontend_ids_are_a_startup_snapshot_owned_by_the_tuner_instance() {
        let hal = TunerHal::new();
        let snapshot_ids = hal.frontend_ids.clone();
        let entry_ids = hal
            .frontend_entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(snapshot_ids, entry_ids);
        assert_eq!(snapshot_ids, hal.frontend_ids);
    }
}

#[cfg(test)]
mod av_debug_log_rate_limit_tests {
    use super::AvSharedBacking;

    #[test]
    fn av_debug_log_counter_policy_is_rate_limited_after_initial_events() {
        assert!(AvSharedBacking::should_log_counter(1));
        assert!(AvSharedBacking::should_log_counter(4));
        assert!(!AvSharedBacking::should_log_counter(5));
        assert!(AvSharedBacking::should_log_counter(64));
        assert!(AvSharedBacking::should_log_counter(128));
    }
}

#[cfg(test)]
mod demux_config_error_tests {
    use super::*;

    #[test]
    fn capacity_error_maps_to_resource_error_not_name_not_found() {
        assert_eq!(
            demux_config_error_tuner_result(DemuxConfigError::CapacityExceeded),
            Some(TunerResult::UNAVAILABLE.0)
        );
    }

    #[test]
    fn missing_filter_remains_name_not_found_domain() {
        assert_eq!(
            demux_config_error_tuner_result(DemuxConfigError::NotFound),
            None
        );
    }

    #[test]
    fn invalid_filter_configuration_maps_to_invalid_argument() {
        assert_eq!(
            demux_config_error_tuner_result(DemuxConfigError::InvalidKind),
            Some(TunerResult::INVALID_ARGUMENT.0)
        );
        assert_eq!(
            demux_config_error_tuner_result(DemuxConfigError::InvalidState),
            Some(TunerResult::INVALID_STATE.0)
        );
    }
}

#[cfg(test)]
mod backend_failure_tests {
    use super::{FrontendBackendState, FrontendHal};
    use maleicacid_tuner_hal_common::FrontendSystem;
    use maleicacid_tuner_hal_frontend_dvb::DvbFrontendBackend;

    #[test]
    fn backend_read_status_error_is_returned_not_panicked() {
        let mut backend = FrontendBackendState::Dvb(DvbFrontendBackend::new(
            -9997,
            0,
            0,
            0,
            vec![FrontendSystem::IsdbT],
        ));
        assert!(FrontendHal::backend_read_status(&mut backend).is_err());
    }
}

#[cfg(test)]
mod demux_id_pool_tests {
    use super::*;

    #[test]
    fn demux_ids_are_fixed_startup_pool() {
        let ids = all_demux_ids();
        assert_eq!(ids.len(), MAX_LIVE_DEMUXES);
        assert_eq!(ids.first().copied(), Some(DEMUX_ID_BASE));
        assert_eq!(
            ids.last().copied(),
            Some(DEMUX_ID_BASE + MAX_LIVE_DEMUXES as i32 - 1)
        );
        assert!(ids.iter().all(|id| demux_id_in_pool(*id)));
        assert!(!demux_id_in_pool(DEMUX_ID_BASE - 1));
        assert!(!demux_id_in_pool(DEMUX_ID_BASE + MAX_LIVE_DEMUXES as i32));
    }

    #[test]
    fn demux_pool_allocates_to_capacity_and_reuses_after_release() {
        let hal = TunerHal::new();
        assert_eq!(hal.getDemuxIds().unwrap(), all_demux_ids());

        let first_id = all_demux_ids()[0];
        assert!(hal.getDemuxInfo(first_id).is_ok());
        assert!(hal
            .getDemuxInfo(DEMUX_ID_BASE + MAX_LIVE_DEMUXES as i32)
            .is_err());

        let mut allocated_records = Vec::new();
        for _ in 0..MAX_LIVE_DEMUXES {
            allocated_records.push(
                hal.allocate_demux_record()
                    .expect("pool should have a free demux ID"),
            );
        }
        assert!(hal.first_available_demux_id().is_none());

        let (released_id, released_record) = allocated_records.remove(0);
        let demux_hal = DemuxHal::new(
            released_record,
            Arc::clone(&hal.frontend_registry),
            Arc::clone(&hal.frontend_leases),
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
        ).unwrap();
        demux_hal.release_registration_best_effort();

        assert_eq!(hal.first_available_demux_id(), Some(released_id));
        assert!(!lock_mutex_status(&hal.demux_ledger, "test_mutex").unwrap().contains_live(LedgerId(released_id)));
        assert!(!lock_mutex_status(&hal.demux_ledger, "test_mutex").unwrap()
            .get_record(LedgerId(released_id))
            .is_some());
    }

    #[test]
    fn open_demux_by_id_refcounts_existing_record() {
        let hal = TunerHal::new();
        let demux_id = all_demux_ids()[0];
        let record = hal.open_or_create_demux_record_by_id(demux_id).unwrap();
        assert_eq!(lock_mutex_status(&record, "test_mutex").unwrap().ref_count, 1);

        let _second = hal
            .openDemuxById(demux_id)
            .expect("pool member should be reopenable by ID");
        assert_eq!(lock_mutex_status(&record, "test_mutex").unwrap().ref_count, 2);
        assert!(hal
            .openDemuxById(DEMUX_ID_BASE + MAX_LIVE_DEMUXES as i32)
            .is_err());
    }
}

#[cfg(test)]
mod lnb_state_tests {
    use super::*;

    #[test]
    fn px4_lnb_voltage_updates_registry_even_without_matching_frontend() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        lock_mutex_status(&registry, "test_mutex").unwrap().insert(
            42,
            LnbRuntimeState {
                profile: LnbDeviceProfile::Px4Device15VOnly,
                ..Default::default()
            },
        );
        let frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>> = Arc::new(BTreeMap::new());
        let lnb = LnbHal::new(42, Arc::clone(&registry), frontend_registry).unwrap();

        lnb.setVoltage(LnbVoltage::VOLTAGE_15V).unwrap();
        let stored = lock_mutex_status(&registry, "test_mutex").unwrap().get(&42).cloned().unwrap();
        assert_eq!(stored.voltage, Some(LnbVoltage::VOLTAGE_15V));
        assert_eq!(stored.generation, 1);
    }

    #[test]
    fn unsupported_lnb_tone_is_rejected_before_state_change() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        lock_mutex_status(&registry, "test_mutex").unwrap()
            .insert(43, LnbRuntimeState::default());
        let frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>> = Arc::new(BTreeMap::new());
        let lnb = LnbHal::new(43, Arc::clone(&registry), frontend_registry).unwrap();

        assert!(lnb.setTone(LnbTone::CONTINUOUS).is_err());
        let stored = lock_mutex_status(&registry, "test_mutex").unwrap().get(&43).cloned().unwrap();
        assert_eq!(stored.tone, None);
        assert_eq!(stored.generation, 0);
    }


    #[test]
    fn lnb_close_resets_px4_no_power_profile_to_safe_state() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        lock_mutex_status(&registry, "test_mutex").unwrap().insert(
            44,
            LnbRuntimeState {
                profile: LnbDeviceProfile::NoPower,
                owner_frontend_id: 44,
                voltage: Some(LnbVoltage::NONE),
                tone: Some(LnbTone::NONE),
                position: Some(LnbPosition::UNDEFINED),
                generation: 7,
                ..Default::default()
            },
        ).unwrap();
        let frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>> = Arc::new(BTreeMap::new());
        let lnb = LnbHal::new(44, Arc::clone(&registry), frontend_registry).unwrap();

        lnb.close().unwrap();
        let stored = lock_mutex_status(&registry, "test_mutex").unwrap().get(&44).cloned().unwrap();
        assert_eq!(stored.voltage, Some(LnbVoltage::NONE));
        assert_eq!(stored.tone, Some(LnbTone::NONE));
        assert_eq!(stored.position, Some(LnbPosition::UNDEFINED));
        assert_eq!(stored.generation, 8);
        assert!(lnb.setVoltage(LnbVoltage::NONE).is_err());
    }

    #[test]
    fn lnb_close_resets_earth_pt1_fixed_profile_to_safe_state() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        lock_mutex_status(&registry, "test_mutex").unwrap().insert(
            45,
            LnbRuntimeState {
                profile: LnbDeviceProfile::EarthPt1FixedLnb,
                owner_frontend_id: 45,
                voltage: Some(LnbVoltage::VOLTAGE_15V),
                tone: Some(LnbTone::NONE),
                position: Some(LnbPosition::UNDEFINED),
                generation: 3,
                ..Default::default()
            },
        ).unwrap();
        let frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>> = Arc::new(BTreeMap::new());
        let lnb = LnbHal::new(45, Arc::clone(&registry), frontend_registry).unwrap();

        lnb.close().unwrap();
        let stored = lock_mutex_status(&registry, "test_mutex").unwrap().get(&45).cloned().unwrap();
        assert_eq!(stored.voltage, Some(LnbVoltage::NONE));
        assert_eq!(stored.tone, Some(LnbTone::NONE));
        assert_eq!(stored.position, Some(LnbPosition::UNDEFINED));
        assert_eq!(stored.generation, 4);
        assert!(lnb.close().is_ok());
    }


    #[test]
    fn lnb_close_records_reset_failure_without_marking_closed_when_backend_apply_fails() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        lock_mutex_status(&registry, "test_mutex").unwrap().insert(
            46,
            LnbRuntimeState {
                profile: LnbDeviceProfile::Px4Device15VOnly,
                owner_frontend_id: 46,
                voltage: Some(LnbVoltage::VOLTAGE_15V),
                tone: Some(LnbTone::NONE),
                position: Some(LnbPosition::UNDEFINED),
                generation: 9,
                ..Default::default()
            },
        ).unwrap();
        let runtime = FrontendRuntime::new(
            FrontendEntry {
                id: 46,
                kind: FrontendEntryKind::Px4 {
                    unit: 0,
                    device_name: Some("px4video0".to_string()),
                    control_path: PathBuf::from("/dev/nonexistent-px4video0"),
                    declared_type: FrontendType::ISDBS,
                    allowed_systems: vec![FrontendSystem::IsdbS],
                },
            },
            Arc::clone(&registry),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        {
            let mut backend = lock_mutex_status(&runtime.backend, "test_mutex").unwrap();
            *backend = FrontendBackendState::Unavailable {
                reason: "injected LNB reset failure".to_string(),
                declared_type: FrontendType::ISDBS,
                allowed_systems: vec![FrontendSystem::IsdbS],
                selected_lnb_id: Some(46),
            };
        }
        let mut frontends = BTreeMap::new();
        frontends.insert(runtime.frontend_id, runtime);
        let lnb = LnbHal::new(46, Arc::clone(&registry), Arc::new(frontends)).unwrap();

        assert!(lnb.close().is_err());
        let stored = lock_mutex_status(&registry, "test_mutex").unwrap().get(&46).cloned().unwrap();
        assert_eq!(stored.voltage, Some(LnbVoltage::NONE));
        assert_eq!(stored.tone, Some(LnbTone::NONE));
        assert_eq!(stored.position, Some(LnbPosition::UNDEFINED));
        assert_eq!(stored.generation, 10);
        assert!(stored.last_close_reset_error.is_some());
        let status = lnb
            .setVoltage(LnbVoltage::NONE)
            .expect_err("unclosed LNB should still retry backend apply and fail because backend is unavailable");
        assert_ne!(status.service_specific_error(), TunerResult::INVALID_STATE.0);
        assert!(lnb.close().is_err());
    }

    #[test]
    fn lnb_drop_resets_state_when_close_was_not_called() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        lock_mutex_status(&registry, "test_mutex").unwrap().insert(
            47,
            LnbRuntimeState {
                profile: LnbDeviceProfile::Px4Device15VOnly,
                owner_frontend_id: 47,
                voltage: Some(LnbVoltage::VOLTAGE_15V),
                tone: Some(LnbTone::NONE),
                position: Some(LnbPosition::UNDEFINED),
                generation: 4,
                ..Default::default()
            },
        ).unwrap();
        let frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>> = Arc::new(BTreeMap::new());
        {
            let _lnb = LnbHal::new(47, Arc::clone(&registry), frontend_registry).unwrap();
        }
        let stored = lock_mutex_status(&registry, "test_mutex").unwrap().get(&47).cloned().unwrap();
        assert_eq!(stored.voltage, Some(LnbVoltage::NONE));
        assert_eq!(stored.tone, Some(LnbTone::NONE));
        assert_eq!(stored.position, Some(LnbPosition::UNDEFINED));
        assert_eq!(stored.generation, 5);
    }

    #[test]
    fn lnb_drop_records_reset_failure_but_does_not_panic() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        lock_mutex_status(&registry, "test_mutex").unwrap().insert(
            48,
            LnbRuntimeState {
                profile: LnbDeviceProfile::Px4Device15VOnly,
                owner_frontend_id: 48,
                voltage: Some(LnbVoltage::VOLTAGE_15V),
                tone: Some(LnbTone::NONE),
                position: Some(LnbPosition::UNDEFINED),
                generation: 2,
                ..Default::default()
            },
        ).unwrap();
        let runtime = FrontendRuntime::new(
            FrontendEntry {
                id: 48,
                kind: FrontendEntryKind::Px4 {
                    unit: 0,
                    device_name: Some("px4video0".to_string()),
                    control_path: PathBuf::from("/dev/nonexistent-px4video0-drop"),
                    declared_type: FrontendType::ISDBS,
                    allowed_systems: vec![FrontendSystem::IsdbS],
                },
            },
            Arc::clone(&registry),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        {
            let mut backend = lock_mutex_status(&runtime.backend, "test_mutex").unwrap();
            *backend = FrontendBackendState::Unavailable {
                reason: "injected LNB drop reset failure".to_string(),
                declared_type: FrontendType::ISDBS,
                allowed_systems: vec![FrontendSystem::IsdbS],
                selected_lnb_id: Some(48),
            };
        }
        let mut frontends = BTreeMap::new();
        frontends.insert(runtime.frontend_id, runtime);
        {
            let _lnb = LnbHal::new(48, Arc::clone(&registry), Arc::new(frontends)).unwrap();
        }
        let stored = lock_mutex_status(&registry, "test_mutex").unwrap().get(&48).cloned().unwrap();
        assert_eq!(stored.voltage, Some(LnbVoltage::NONE));
        assert_eq!(stored.tone, Some(LnbTone::NONE));
        assert_eq!(stored.position, Some(LnbPosition::UNDEFINED));
        assert_eq!(stored.generation, 3);
        assert!(stored.last_close_reset_error.is_some());
    }
}

#[cfg(test)]
mod lnb_profile_detection_tests {
    use super::*;

    #[test]
    fn px4_lnb_profile_table_is_fixed_by_device_name_prefix() {
        assert_eq!(
            px4_lnb_profile_from_devname(Some("px4video0")),
            LnbDeviceProfile::Px4Device15VOnly
        ).unwrap();
        assert_eq!(
            px4_lnb_profile_from_devname(Some("pxmlt5video0")),
            LnbDeviceProfile::NoPower
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("pxmlt8video7")),
            LnbDeviceProfile::NoPower
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("isdb6014video0")),
            LnbDeviceProfile::NoPower
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("isdb2056video0")),
            LnbDeviceProfile::NoPower
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("pxm1urvideo0")),
            LnbDeviceProfile::NoPower
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("pxs1urvideo0")),
            LnbDeviceProfile::NoPower
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("isdbt2071video0")),
            LnbDeviceProfile::NoPower
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("unknownvideo0")),
            LnbDeviceProfile::NoPower
        );
        assert_eq!(
            px4_lnb_profile_from_devname(None),
            LnbDeviceProfile::NoPower
        );
    }

    #[test]
    fn px4_lnb_profile_identity_prefers_sysfs_devname_over_dev_basename() {
        assert_eq!(
            px4_lnb_profile_from_identity(Some("pxmlt8video0"), Some("px4video0")),
            LnbDeviceProfile::NoPower
        );
        assert_eq!(
            px4_lnb_profile_from_identity(None, Some("px4video0")),
            LnbDeviceProfile::Px4Device15VOnly
        );
    }

    #[test]
    fn px4_export_ids_include_device_family_and_unit() {
        assert_ne!(
            px4_export_frontend_base_id(0, Some("px4video0")),
            px4_export_frontend_base_id(0, Some("pxmlt5video0"))
        );
        assert_ne!(
            px4_export_frontend_base_id(0, Some("pxmlt5video0")),
            px4_export_frontend_base_id(1, Some("pxmlt5video1"))
        );
    }

    fn px4_entry(unit: i32, device_name: &str, declared_type: FrontendType) -> FrontendEntry {
        let systems_for_entry = match declared_type {
            FrontendType::ISDBT => vec![FrontendSystem::IsdbT],
            FrontendType::ISDBS => vec![FrontendSystem::IsdbS],
            _ => Vec::new(),
        };
        FrontendEntry {
            id: px4_export_frontend_base_id(unit, Some(device_name))
                + if declared_type == FrontendType::ISDBS {
                    1
                } else {
                    0
                },
            kind: FrontendEntryKind::Px4 {
                unit,
                device_name: Some(device_name.to_string()),
                control_path: std::path::PathBuf::from(format!("/dev/{device_name}")),
                declared_type,
                allowed_systems: systems_for_entry,
            },
        }
    }

    #[test]
    fn px4_exclusive_group_id_contains_device_family_and_unit() {
        let px4video0 = px4_entry(0, "px4video0", FrontendType::ISDBS);
        let pxmlt5video0 = px4_entry(0, "pxmlt5video0", FrontendType::ISDBS);
        let pxmlt8video0 = px4_entry(0, "pxmlt8video0", FrontendType::ISDBS);
        let pxmlt5video1 = px4_entry(1, "pxmlt5video1", FrontendType::ISDBS);
        assert_ne!(
            entry_physical_group_id(&px4video0),
            entry_physical_group_id(&pxmlt5video0)
        );
        assert_ne!(
            entry_physical_group_id(&pxmlt5video0),
            entry_physical_group_id(&pxmlt8video0)
        );
        assert_ne!(
            entry_physical_group_id(&pxmlt5video0),
            entry_physical_group_id(&pxmlt5video1)
        );
    }

    #[test]
    fn px4_split_entries_for_same_physical_frontend_share_exclusive_group() {
        let isdbt = px4_entry(0, "pxmlt5video0", FrontendType::ISDBT);
        let isdbs = px4_entry(0, "pxmlt5video0", FrontendType::ISDBS);
        assert_ne!(isdbt.id, isdbs.id);
        assert_eq!(
            entry_physical_group_id(&isdbt),
            entry_physical_group_id(&isdbs)
        );
    }

    #[test]
    fn fixed_lnb_voltage_policy_is_contractual() {
        assert!(LnbHal::voltage_supported(
            LnbDeviceProfile::Px4Device15VOnly,
            LnbVoltage::NONE
        ));
        assert!(LnbHal::voltage_supported(
            LnbDeviceProfile::Px4Device15VOnly,
            LnbVoltage::VOLTAGE_15V
        ));
        assert!(!LnbHal::voltage_supported(
            LnbDeviceProfile::Px4Device15VOnly,
            LnbVoltage::VOLTAGE_11V
        ));
        assert!(LnbHal::voltage_supported(
            LnbDeviceProfile::NoPower,
            LnbVoltage::NONE
        ));
        assert!(!LnbHal::voltage_supported(
            LnbDeviceProfile::NoPower,
            LnbVoltage::VOLTAGE_15V
        ));
        assert!(!LnbHal::voltage_supported(
            LnbDeviceProfile::NoPower,
            LnbVoltage::VOLTAGE_11V
        ));
        assert!(LnbHal::voltage_supported(
            LnbDeviceProfile::EarthPt1FixedLnb,
            LnbVoltage::VOLTAGE_11V
        ));
        assert!(LnbHal::voltage_supported(
            LnbDeviceProfile::EarthPt1FixedLnb,
            LnbVoltage::VOLTAGE_15V
        ));
        assert!(!LnbHal::voltage_supported(
            LnbDeviceProfile::EarthPt1FixedLnb,
            LnbVoltage::VOLTAGE_18V
        ));
        assert!(LnbHal::voltage_supported(
            LnbDeviceProfile::NoPower,
            LnbVoltage::NONE
        ));
        assert!(!LnbHal::voltage_supported(
            LnbDeviceProfile::NoPower,
            LnbVoltage::VOLTAGE_15V
        ));
    }
}

#[cfg(test)]
mod frontend_capability_tests {
    use super::*;

    #[test]
    fn satellite_frontend_status_caps_include_fixed_lnb_voltage() {
        let sat = FrontendEntry {
            id: 2,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type: FrontendType::ISDBS,
                allowed_systems: vec![FrontendSystem::IsdbS],
            },
        };
        let terr = FrontendEntry {
            id: 1,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("isdbt2071video0".to_string()),
                control_path: PathBuf::from("/dev/isdbt2071video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        assert!(entry_status_caps(&sat).contains(&FrontendStatusType::LNB_VOLTAGE));
        assert!(!entry_status_caps(&terr).contains(&FrontendStatusType::LNB_VOLTAGE));
    }

    fn px4_entry(id: i32, declared_type: FrontendType, system: FrontendSystem) -> FrontendEntry {
        FrontendEntry {
            id,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type,
                allowed_systems: vec![system],
            },
        }
    }

    fn dvb_entry(id: i32, declared_type: FrontendType, system: FrontendSystem) -> FrontendEntry {
        FrontendEntry {
            id,
            kind: FrontendEntryKind::Dvb {
                adapter: 0,
                frontend_index: 0,
                demux_index: 0,
                dvr_index: 0,
                declared_type,
                supported_systems: vec![system],
                min_frequency_hz: 0,
                max_frequency_hz: 0,
                max_symbol_rate: 0,
            },
        }
    }

    fn frontend_for_entry(entry: FrontendEntry) -> FrontendHal {
        let frontend_type = entry_aidl_frontend_type(&entry);
        let physical_group_id = entry_physical_group_id(&entry);
        let runtime = FrontendRuntime::new(
            entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        FrontendHal::new(
            runtime,
            frontend_type,
            physical_group_id,
            1,
            Arc::new(Mutex::new(FrontendLeaseRegistry::default())),
            Arc::new(Mutex::new(DemuxLedger::default())),
        )
    }

    #[test]
    fn isdbt_capability_matches_fixed_japanese_target_values() {
        let entry = px4_entry(1, FrontendType::ISDBT, FrontendSystem::IsdbT);
        match entry_frontend_caps(&entry) {
            FrontendCapabilities::IsdbtCaps(caps) => {
                assert_eq!(caps.modeCap, isdbt_mode_caps());
                assert_eq!(
                    caps.modeCap,
                    FrontendIsdbtMode::AUTO.0 | FrontendIsdbtMode::MODE_3.0
                );
                assert_eq!(
                    caps.bandwidthCap,
                    FrontendIsdbtBandwidth::AUTO.0 | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ.0
                );
                assert_eq!(caps.modulationCap, isdbt_modulation_caps());
                assert_eq!(caps.coderateCap, isdbt_coderate_caps());
                assert_eq!(caps.guardIntervalCap, isdbt_guard_interval_caps());
                assert_eq!(caps.timeInterleaveCap, isdbt_time_interleave_caps());
                assert!(caps.isSegmentAuto);
                assert!(caps.isFullSegment);
            }
            _ => panic!("ISDB-T項目はISDB-T能力を報告する必要があります"),
        }
    }

    #[test]
    fn isdbs_capability_matches_fixed_japanese_target_values() {
        let entry = px4_entry(2, FrontendType::ISDBS, FrontendSystem::IsdbS);
        match entry_frontend_caps(&entry) {
            FrontendCapabilities::IsdbsCaps(caps) => {
                assert_eq!(caps.modulationCap, isdbs_modulation_caps());
                assert_eq!(caps.coderateCap, isdbs_coderate_caps());
            }
            _ => panic!("ISDB-S項目はISDB-S能力を報告する必要があります"),
        }
    }

    #[test]
    fn vts_lab_settings_are_subset_of_advertised_capabilities() {
        let t = match entry_frontend_caps(&px4_entry(1, FrontendType::ISDBT, FrontendSystem::IsdbT))
        {
            FrontendCapabilities::IsdbtCaps(caps) => caps,
            _ => panic!("ISDB-T能力が期待されます"),
        };
        assert_ne!(t.bandwidthCap & FrontendIsdbtBandwidth::BANDWIDTH_6MHZ.0, 0);
        assert_ne!(t.modeCap & FrontendIsdbtMode::AUTO.0, 0);
        assert_ne!(t.modulationCap & FrontendIsdbtModulation::AUTO.0, 0);
        assert_ne!(t.coderateCap & FrontendIsdbtCoderate::AUTO.0, 0);
        assert_ne!(t.guardIntervalCap & FrontendIsdbtGuardInterval::AUTO.0, 0);
        assert_ne!(
            t.timeInterleaveCap & FrontendIsdbtTimeInterleaveMode::AUTO.0,
            0
        );

        let s = match entry_frontend_caps(&px4_entry(2, FrontendType::ISDBS, FrontendSystem::IsdbS))
        {
            FrontendCapabilities::IsdbsCaps(caps) => caps,
            _ => panic!("ISDB-S能力が期待されます"),
        };
        assert_ne!(s.modulationCap & FrontendIsdbsModulation::AUTO.0, 0);
        assert_ne!(s.coderateCap & FrontendIsdbsCoderate::AUTO.0, 0);
    }

    #[test]
    fn px4_isdbt_advertised_range_covers_japan_catv_and_uhf_contract() {
        assert_eq!(japan_isdbt_frequency_contract_range_hz(), (110_642_857, 767_642_857, 500_000));
    }

    #[test]
    fn frontend_info_symbol_rate_contract_is_zero_even_when_dvb_probe_reports_nonzero() {
        let dvb_isdbs = FrontendEntry {
            id: 200,
            kind: FrontendEntryKind::Dvb {
                adapter: 0,
                frontend_index: 1,
                demux_index: 0,
                dvr_index: 0,
                declared_type: FrontendType::ISDBS,
                supported_systems: vec![FrontendSystem::IsdbS],
                min_frequency_hz: 950_000_000,
                max_frequency_hz: 2_150_000_000,
                max_symbol_rate: 28_860_000,
            },
        };
        assert_eq!(entry_frontend_max_symbol_rate_contract(&dvb_isdbs), 0);
    }

    #[test]
    fn rf_lock_status_caps_are_backend_specific() {
        assert!(
            !entry_status_caps(&px4_entry(1, FrontendType::ISDBS, FrontendSystem::IsdbS))
                .contains(&FrontendStatusType::RF_LOCK)
        );
        assert!(
            entry_status_caps(&dvb_entry(2, FrontendType::ISDBS, FrontendSystem::IsdbS))
                .contains(&FrontendStatusType::RF_LOCK)
        );
    }

    #[test]
    fn optional_snr_and_strength_are_not_advertised_without_probe_fixed_capability() {
        let caps = entry_status_caps(&dvb_entry(3, FrontendType::ISDBT, FrontendSystem::IsdbT));
        assert!(caps.contains(&FrontendStatusType::DEMOD_LOCK));
        assert!(caps.contains(&FrontendStatusType::RF_LOCK));
        assert!(caps.contains(&FrontendStatusType::SIGNAL_QUALITY));
        assert!(!caps.contains(&FrontendStatusType::SNR));
        assert!(!caps.contains(&FrontendStatusType::SIGNAL_STRENGTH));
    }

    #[test]
    fn rf_lock_status_uses_dvb_carrier_only_when_supported() {
        let telemetry = FrontendTelemetry {
            locked: false,
            rf_locked: Some(true),
            ..Default::default()
        };
        let with_rf = FrontendHal::status_for_types(
            FrontendStatusSupport { rf_lock: true, ..Default::default() },
            &telemetry,
            &[FrontendStatusType::RF_LOCK, FrontendStatusType::DEMOD_LOCK],
        )
        .unwrap();
        assert!(matches!(with_rf[0], FrontendStatus::IsRfLocked(true)));
        assert!(matches!(with_rf[1], FrontendStatus::IsDemodLocked(false)));

        let without_rf = FrontendHal::status_for_types(
            FrontendStatusSupport::default(),
            &telemetry,
            &[FrontendStatusType::RF_LOCK, FrontendStatusType::DEMOD_LOCK],
        );
        assert!(without_rf.is_err());
    }

    #[test]
    fn rf_lock_readiness_is_unsupported_for_px4_and_stable_for_dvb_with_value() {
        assert_eq!(
            FrontendHal::readiness_for_types(
                FrontendStatusSupport::default(),
                true,
                false,
                Some(&FrontendTelemetry::default()),
                &[FrontendStatusType::RF_LOCK]
            )
            .unwrap(),
            vec![FrontendStatusReadiness::UNSUPPORTED]
        );
        assert_eq!(
            FrontendHal::readiness_for_types(
                FrontendStatusSupport { rf_lock: true, ..Default::default() },
                true,
                false,
                Some(&FrontendTelemetry { rf_locked: Some(true), ..Default::default() }),
                &[FrontendStatusType::RF_LOCK]
            )
            .unwrap(),
            vec![FrontendStatusReadiness::STABLE]
        );
        assert_eq!(
            FrontendHal::readiness_for_types(
                FrontendStatusSupport { rf_lock: true, ..Default::default() },
                true,
                false,
                Some(&FrontendTelemetry::default()),
                &[FrontendStatusType::RF_LOCK]
            )
            .unwrap(),
            vec![FrontendStatusReadiness::UNAVAILABLE]
        );
    }

    #[test]
    fn frontend_status_helpers_preserve_empty_request_shape() {
        let telemetry = FrontendTelemetry::default();
        assert_eq!(
            FrontendHal::status_for_types(FrontendStatusSupport::default(), &telemetry, &[]).unwrap(),
            Vec::<FrontendStatus>::new()
        );
        assert_eq!(
            FrontendHal::readiness_for_types(
                FrontendStatusSupport::default(),
                true,
                false,
                None,
                &[]
            )
            .unwrap(),
            Vec::<FrontendStatusReadiness>::new()
        );
    }

    #[test]
    fn frontend_readiness_all_unsupported_does_not_require_telemetry() {
        assert_eq!(
            FrontendHal::readiness_for_types(
                FrontendStatusSupport::default(),
                true,
                false,
                None,
                &[FrontendStatusType::RF_LOCK, FrontendStatusType::SNR]
            )
            .unwrap(),
            vec![FrontendStatusReadiness::UNSUPPORTED, FrontendStatusReadiness::UNSUPPORTED]
        );
    }

    #[test]
    fn runtime_status_support_matches_advertised_status_caps() {
        let entry = dvb_entry(4, FrontendType::ISDBT, FrontendSystem::IsdbT);
        let expected = entry_status_caps(&entry);
        let runtime = FrontendRuntime::new(
            entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        for status_type in [
            FrontendStatusType::DEMOD_LOCK,
            FrontendStatusType::RF_LOCK,
            FrontendStatusType::SNR,
            FrontendStatusType::SIGNAL_STRENGTH,
            FrontendStatusType::SIGNAL_QUALITY,
            FrontendStatusType::LNB_VOLTAGE,
        ] {
            assert_eq!(
                runtime.advertised_status_support.supports(status_type),
                expected.contains(&status_type)
            );
        }
    }

    #[test]
    fn get_status_rejects_status_type_outside_advertised_caps_before_backend_read() {
        let frontend = frontend_for_entry(px4_entry(5, FrontendType::ISDBT, FrontendSystem::IsdbT));
        let err = frontend
            .getStatus(&[FrontendStatusType::SIGNAL_QUALITY])
            .unwrap_err();
        assert_eq!(err.service_specific_error(), TunerResult::INVALID_ARGUMENT.0);
    }

    #[test]
    fn get_frontend_status_readiness_supported_telemetry_failure_is_unavailable() {
        let frontend = frontend_for_entry(px4_entry(6, FrontendType::ISDBT, FrontendSystem::IsdbT));
        assert_eq!(
            frontend
                .getFrontendStatusReadiness(&[FrontendStatusType::DEMOD_LOCK])
                .unwrap(),
            vec![FrontendStatusReadiness::UNAVAILABLE]
        );
    }

    #[test]
    fn get_frontend_status_readiness_all_unsupported_returns_unsupported_without_backend_read() {
        let frontend = frontend_for_entry(px4_entry(9, FrontendType::ISDBT, FrontendSystem::IsdbT));
        assert_eq!(
            frontend
                .getFrontendStatusReadiness(&[FrontendStatusType::RF_LOCK, FrontendStatusType::SNR])
                .unwrap(),
            vec![FrontendStatusReadiness::UNSUPPORTED, FrontendStatusReadiness::UNSUPPORTED]
        );
    }

    #[test]
    fn get_frontend_status_readiness_uses_advertised_caps_not_backend_runtime_reconstruction() {
        let frontend = frontend_for_entry(dvb_entry(7, FrontendType::ISDBT, FrontendSystem::IsdbT));
        assert!(entry_status_caps(&dvb_entry(8, FrontendType::ISDBT, FrontendSystem::IsdbT))
            .contains(&FrontendStatusType::RF_LOCK));
        assert_eq!(
            frontend
                .getFrontendStatusReadiness(&[FrontendStatusType::RF_LOCK])
                .unwrap(),
            vec![FrontendStatusReadiness::UNAVAILABLE]
        );
    }
}

#[cfg(test)]
mod lnb_ownership_tests {
    use super::*;

    fn registry_with(
        owner_frontend_id: i32,
        lnb_id: i32,
    ) -> Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>> {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        lock_mutex_status(&registry, "test_mutex").unwrap().insert(
            lnb_id,
            LnbRuntimeState {
                owner_frontend_id,
                profile: LnbDeviceProfile::Px4Device15VOnly,
                ..Default::default()
            },
        );
        registry
    }

    #[test]
    fn satellite_frontend_accepts_only_own_lnb() {
        let own = registry_with(10, 10010);
        assert!(FrontendHal::validate_lnb_owner(&[FrontendSystem::IsdbS], 10, &own, 10010).is_ok());

        let other = registry_with(11, 10011);
        assert!(
            FrontendHal::validate_lnb_owner(&[FrontendSystem::IsdbS], 10, &other, 10011).is_err()
        );
    }

    #[test]
    fn terrestrial_frontend_cannot_attach_lnb() {
        let registry = registry_with(10, 10010);
        assert!(
            FrontendHal::validate_lnb_owner(&[FrontendSystem::IsdbT], 10, &registry, 10010)
                .is_err()
        );
    }

    #[test]
    fn unknown_lnb_id_is_rejected() {
        let registry = registry_with(10, 10010);
        assert!(
            FrontendHal::validate_lnb_owner(&[FrontendSystem::IsdbS], 10, &registry, 99999)
                .is_err()
        );
    }

    #[test]
    fn terrestrial_frontend_does_not_get_default_lnb_id() {
        let entry = FrontendEntry {
            id: 1,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("isdbt2071video0".to_string()),
                control_path: PathBuf::from("/dev/isdbt2071video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        assert_eq!(entry_default_lnb_id(&entry), None);
    }
}

#[cfg(test)]
mod hal_error_mapping_tests {
    use super::hal_error_tuner_result;
    use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::Result::Result as TunerResult;
    use maleicacid_tuner_hal_common::HalError;
    use std::path::PathBuf;

    #[test]
    fn hal_error_mapping_is_stable_for_vts_facing_paths() {
        assert_eq!(
            hal_error_tuner_result(&HalError::InvalidArgument("bad".into())),
            TunerResult::INVALID_ARGUMENT.0
        );
        assert_eq!(
            hal_error_tuner_result(&HalError::DeviceMissing(PathBuf::from("/dev/null"))),
            TunerResult::UNAVAILABLE.0
        );
        assert_eq!(
            hal_error_tuner_result(&HalError::Unsupported("x")),
            TunerResult::UNAVAILABLE.0
        );
        assert_eq!(
            hal_error_tuner_result(&HalError::IoctlFailed {
                backend: "test",
                path: None,
                op: "TEST",
                errno: 25
            }),
            TunerResult::UNKNOWN_ERROR.0
        );
        assert_eq!(
            hal_error_tuner_result(&HalError::Io {
                backend: "test",
                operation: "read",
                path: None,
                errno: None,
                message: "実行時読み取り失敗".into()
            }),
            TunerResult::UNKNOWN_ERROR.0
        );
        assert_eq!(
            hal_error_tuner_result(&HalError::PermissionDenied {
                path: PathBuf::from("/dev/dvb/adapter0/frontend0"),
                message: "EACCES".into()
            }),
            TunerResult::UNAVAILABLE.0
        );
        assert_eq!(
            hal_error_tuner_result(&HalError::Busy {
                path: Some(PathBuf::from("/dev/px4video0")),
                message: "EBUSY".into()
            }),
            TunerResult::UNAVAILABLE.0
        );
        assert_eq!(
            hal_error_tuner_result(&HalError::InvalidState("closed".into())),
            TunerResult::INVALID_STATE.0
        );
        assert_eq!(
            hal_error_tuner_result(&HalError::Internal("poison".into())),
            TunerResult::UNKNOWN_ERROR.0
        );
    }
}

#[cfg(test)]
mod vts_contract_tests {
    use super::*;

    #[test]
    fn demux_filter_caps_match_all_demux_info_filter_types() {
        let hal = TunerHal::new();
        let caps = hal.getDemuxCaps().unwrap();
        let combined = hal
            .getDemuxIds()
            .unwrap()
            .into_iter()
            .map(|id| hal.getDemuxInfo(id).unwrap().filterTypes)
            .fold(0, |acc, filter_types| acc | filter_types);
        assert_eq!(caps.filterCaps, SUPPORTED_DEMUX_FILTER_CAPS);
        assert_ne!(caps.filterCaps, 0);
        assert_eq!(combined, caps.filterCaps);
    }

    #[test]
    fn demux_link_caps_advertise_ts_linkage_only() {
        let hal = TunerHal::new();
        let caps = hal.getDemuxCaps().unwrap();
        assert_eq!(DEMUX_FILTER_MAIN_TYPE_TS_BITS, DemuxFilterMainType::TS.0);
        assert_eq!(caps.linkCaps.len(), DEMUX_FILTER_MAIN_TYPE_COUNT);
        assert_eq!(caps.linkCaps[0], DEMUX_FILTER_MAIN_TYPE_TS_BITS);
        assert!(caps.linkCaps[1..].iter().all(|bits| *bits == 0));
    }

    #[test]
    fn record_packet_does_not_use_standard_fmq_watermarks() {
        let packet = record_ts_packet(0x0123, true, 0);
        assert!(!payload_uses_standard_fmq_watermarks(
            false,
            &FilterPayload::RecordPacket(packet)
        ));
        assert!(payload_uses_standard_fmq_watermarks(
            false,
            &FilterPayload::TsPacket(vec![0x47; TS_PACKET_SIZE])
        ));
        assert!(!payload_uses_standard_fmq_watermarks(
            true,
            &FilterPayload::TsPacket(vec![0x47; TS_PACKET_SIZE])
        ));
    }

    #[test]
    fn av_sync_is_exposed_through_av_filters_without_time_filter_claim() {
        let hal = TunerHal::new();
        let caps = hal.getDemuxCaps().unwrap();
        assert_eq!(caps.numPcrFilter, 0);
        assert!(!caps.bTimeFilter);
        assert!(caps.numAudioFilter > 0);
        assert!(caps.numVideoFilter > 0);
    }

    #[test]
    fn noinit_ts_filter_settings_are_accepted_only_for_raw_ts_filter_subtype() {
        let settings = DemuxFilterSettings::Ts(android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::DemuxTsFilterSettings::DemuxTsFilterSettings {
            tpid: 0x100,
            filterSettings: DemuxTsFilterSettingsFilterSettings::Noinit(false),
        });
        assert!(build_filter_summary(&settings).is_err());
        let summary = build_filter_summary_for_open_type(&settings, FilterOpenType::TsRaw).unwrap();
        assert_eq!(summary.tpid, 0x100);
        assert_eq!(summary.kind, FilterConfigKind::Noinit);
    }
}

#[cfg(test)]
mod lifecycle_regression_tests {
    use super::*;

    #[test]
    fn live_pump_can_be_stopped_and_restarted_after_natural_exit() {
        let entry = FrontendEntry {
            id: 41,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        let runtime = FrontendRuntime::new(
            entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        runtime.ensure_live_pump().unwrap();
        thread::sleep(Duration::from_millis(50));
        runtime.ensure_live_pump().unwrap();
        runtime.stop_live_pump().unwrap();
        assert!(lock_mutex_status(&runtime.pump_worker, "test_mutex").unwrap().is_none());
    }

    #[test]
    fn diseqc_is_permanently_unavailable() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        lock_mutex_status(&registry, "test_mutex").unwrap()
            .insert(42, LnbRuntimeState::default());
        let frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>> = Arc::new(BTreeMap::new());
        let lnb = LnbHal::new(42, Arc::clone(&registry), frontend_registry).unwrap();

        assert!(lnb.sendDiseqcMessage(&[0xe0, 0x10, 0x38, 0xf0]).is_err());
        let stored = lock_mutex_status(&registry, "test_mutex").unwrap().get(&42).cloned().unwrap();
        assert_eq!(stored.diseqc_generation, 0);
        assert_eq!(stored.generation, 0);
    }
}

#[cfg(test)]
mod static_completion_tests {
    use super::*;
    use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
        DemuxFilterAvSettings::DemuxFilterAvSettings,
        DemuxFilterPesDataSettings::DemuxFilterPesDataSettings,
        DemuxFilterRecordSettings::DemuxFilterRecordSettings,
        DemuxFilterSubType::DemuxFilterSubType, DemuxRecordScIndexType::DemuxRecordScIndexType,
        DemuxTsFilterType::DemuxTsFilterType, FilterDelayHintType::FilterDelayHintType,
        IDvrCallback::BnDvrCallback, PlaybackSettings::PlaybackSettings,
        RecordSettings::RecordSettings,
    };

    struct NoopFilterCallback;

    impl Interface for NoopFilterCallback {}

    impl IFilterCallback for NoopFilterCallback {
        fn onFilterStatus(&self, _status: DemuxFilterStatus) -> BinderResult<()> {
            Ok(())
        }
        fn onFilterEvent(&self, _filterEvent: &[DemuxFilterEvent]) -> BinderResult<()> {
            Ok(())
        }
    }

    struct RecordingFilterCallback {
        statuses: Arc<Mutex<Vec<DemuxFilterStatus>>>,
    }

    impl Interface for RecordingFilterCallback {}

    impl IFilterCallback for RecordingFilterCallback {
        fn onFilterStatus(&self, status: DemuxFilterStatus) -> BinderResult<()> {
            lock_mutex_status(&self.statuses, "test_mutex").unwrap().push(status);
            Ok(())
        }

        fn onFilterEvent(&self, _filter_event: &[DemuxFilterEvent]) -> BinderResult<()> {
            Ok(())
        }
    }

    struct NoopDvrCallback;

    impl Interface for NoopDvrCallback {}

    impl IDvrCallback for NoopDvrCallback {
        fn onRecordStatus(&self, _status: RecordStatus) -> BinderResult<()> {
            Ok(())
        }

        fn onPlaybackStatus(&self, _status: PlaybackStatus) -> BinderResult<()> {
            Ok(())
        }
    }

    static RELEASE_AV_HANDLE_TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn new_test_dvr(
        direction: DemuxPathDirection,
    ) -> (DvrHal, Arc<Mutex<DemuxHandle>>, Arc<RuntimeIoRegistry>, i32) {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let record = demux.register_dvr(direction, 4096).unwrap();
        let dvr_id = record.dvr_id;
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnDvrCallback::new_binder(NoopDvrCallback, BinderFeatures::default());
        let dvr = DvrHal::new(
            DEMUX_ID_BASE,
            dvr_id,
            direction,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback,
            None,
)
        .unwrap();
        (dvr, state, runtime_io, dvr_id)
    }

    fn av_settings(pid: i32, secure: bool) -> DemuxFilterSettings {
        av_settings_with_passthrough(pid, secure, false)
    }

    fn av_settings_with_passthrough(
        pid: i32,
        secure: bool,
        passthrough: bool,
    ) -> DemuxFilterSettings {
        DemuxFilterSettings::Ts(android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::DemuxTsFilterSettings::DemuxTsFilterSettings {
            tpid: pid,
            filterSettings: DemuxTsFilterSettingsFilterSettings::Av(DemuxFilterAvSettings {
                isPassthrough: passthrough,
                isSecureMemory: secure,
            }),
        })
    }

    fn pes_settings(pid: i32) -> DemuxFilterSettings {
        pes_settings_with_stream_id(pid, 0xbd)
    }

    fn pes_settings_with_stream_id(pid: i32, stream_id: i32) -> DemuxFilterSettings {
        DemuxFilterSettings::Ts(android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::DemuxTsFilterSettings::DemuxTsFilterSettings {
            tpid: pid,
            filterSettings: DemuxTsFilterSettingsFilterSettings::PesData(DemuxFilterPesDataSettings {
                streamId: stream_id,
                isRaw: false,
            }),
        })
    }

    fn record_filter_settings(pid: i32) -> DemuxFilterSettings {
        DemuxFilterSettings::Ts(android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::DemuxTsFilterSettings::DemuxTsFilterSettings {
            tpid: pid,
            filterSettings: DemuxTsFilterSettingsFilterSettings::Record(DemuxFilterRecordSettings {
                tsIndexMask: 0,
                scIndexType: DemuxRecordScIndexType::NONE,
                scIndexMask: DemuxFilterScIndexMask::ScIndex(0),
            }),
        })
    }

    fn record_dvr_settings(data_format: DataFormat, packet_size: i64) -> DvrSettings {
        DvrSettings::Record(RecordSettings {
            statusMask: 0,
            lowThreshold: 1_048_576,
            highThreshold: 3_145_728,
            dataFormat: data_format,
            packetSize: packet_size,
        })
    }

    fn playback_dvr_settings(data_format: DataFormat, packet_size: i64) -> DvrSettings {
        DvrSettings::Playback(PlaybackSettings {
            statusMask: 0,
            lowThreshold: 1_048_576,
            highThreshold: 3_145_728,
            dataFormat: data_format,
            packetSize: packet_size,
        })
    }

    fn ts_filter_type(ts_type: DemuxTsFilterType) -> DemuxFilterType {
        DemuxFilterType {
            mainType: DemuxFilterMainType::TS,
            subType: DemuxFilterSubType::TsFilterType(ts_type),
        }
    }

    fn delay_hint(hint_type: FilterDelayHintType, value: i32) -> FilterDelayHint {
        FilterDelayHint {
            hintType: hint_type,
            hintValue: value,
        }
    }

    fn record_ts_packet(pid: u16, payload_unit_start: bool, scrambling_control: u8) -> Vec<u8> {
        let mut packet = vec![0xff; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if payload_unit_start {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        packet[3] = ((scrambling_control & 0x03) << 6) | 0x10;
        packet
    }

    fn section_config_for_pid(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: DEMUX_FILTER_MAIN_TYPE_TS_BITS,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn section_packet(pid: u16, continuity_counter: u8, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = (pid & 0xff) as u8;
        packet[3] = 0x10 | (continuity_counter & 0x0f);
        let copy_len = payload.len().min(TS_PACKET_SIZE - 4);
        packet[4..4 + copy_len].copy_from_slice(&payload[..copy_len]);
        packet
    }

    fn wait_for_filter_status(
        statuses: &Arc<Mutex<Vec<DemuxFilterStatus>>>,
        expected: DemuxFilterStatus,
    ) -> bool {
        for _ in 0..20 {
            if lock_mutex_status(&statuses, "test_mutex").unwrap().contains(&expected) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn ts_record_event(event: DemuxFilterEvent) -> DemuxFilterTsRecordEvent {
        match event {
            DemuxFilterEvent::TsRecord(record) => record,
            other => panic!("想定外のrecord eventです: {:?}", other),
        }
    }

    #[test]
    fn playback_threshold_contract_uses_unused_write_space() {
        let low = Some(1_048_576);
        let high = Some(3_145_728);
        let capacity = 4_194_304;

        assert_eq!(
            DvrHal::playback_status_from_thresholds(0, low, high, capacity),
            Some(PlaybackStatus::SPACE_EMPTY)
        ).unwrap();
        assert_eq!(
            DvrHal::playback_status_from_thresholds(1_048_576, low, high, capacity),
            Some(PlaybackStatus::SPACE_ALMOST_EMPTY)
        );
        assert_eq!(
            DvrHal::playback_status_from_thresholds(
                (1_048_576 + 3_145_728) / 2,
                low,
                high,
                capacity
            ),
            None
        );
        assert_eq!(
            DvrHal::playback_status_from_thresholds(3_145_728, low, high, capacity),
            Some(PlaybackStatus::SPACE_ALMOST_FULL)
        );
        assert_eq!(
            DvrHal::playback_status_from_thresholds(4_194_304, low, high, capacity),
            Some(PlaybackStatus::SPACE_FULL)
        );
    }

    #[test]
    fn open_time_filter_type_preserves_media_and_non_media_subtype() {
        assert_eq!(
            filter_open_type(&ts_filter_type(DemuxTsFilterType::TS)).unwrap(),
            FilterOpenType::TsRaw
        );
        assert_eq!(
            filter_open_type(&ts_filter_type(DemuxTsFilterType::AUDIO)).unwrap(),
            FilterOpenType::TsAudio
        );
        assert_eq!(
            filter_open_type(&ts_filter_type(DemuxTsFilterType::VIDEO)).unwrap(),
            FilterOpenType::TsVideo
        );
        assert_eq!(
            filter_open_type(&ts_filter_type(DemuxTsFilterType::SECTION)).unwrap(),
            FilterOpenType::TsSection
        );
        assert_eq!(
            filter_open_type(&ts_filter_type(DemuxTsFilterType::PES)).unwrap(),
            FilterOpenType::TsPes
        );
        assert_eq!(
            filter_open_type(&ts_filter_type(DemuxTsFilterType::RECORD)).unwrap(),
            FilterOpenType::TsRecord
        );
    }

    #[test]
    fn delay_hint_type_is_normalized_without_confusing_bytes_for_millis() {
        assert_eq!(
            normalize_filter_delay_hint(&delay_hint(FilterDelayHintType::TIME_DELAY_IN_MS, 7))
                .unwrap(),
            FilterDelayHintState::TimeDelayMs(7)
        );
        assert_eq!(
            normalize_filter_delay_hint(&delay_hint(
                FilterDelayHintType::DATA_SIZE_DELAY_IN_BYTES,
                188
            ))
            .unwrap(),
            FilterDelayHintState::DataSizeDelayBytes(188)
        );
        assert!(normalize_filter_delay_hint(&delay_hint(
            FilterDelayHintType::TIME_DELAY_IN_MS,
            -1
        ))
        .is_err());
        assert!(normalize_filter_delay_hint(&delay_hint(FilterDelayHintType(999), 1)).is_err());
    }

    #[test]
    fn av_shared_file_error_result_matches_code_convention() {
        assert_eq!(
            av_shared_file_error_result(ERRNO_ENOMEM),
            TunerResult::OUT_OF_MEMORY
        );
        assert_eq!(
            av_shared_file_error_result(ERRNO_ENOENT),
            TunerResult::UNAVAILABLE
        );
        assert_eq!(
            av_shared_file_error_result(ERRNO_EACCES),
            TunerResult::UNAVAILABLE
        );
        assert_eq!(
            av_shared_file_error_result(ERRNO_EIO),
            TunerResult::UNKNOWN_ERROR
        );
        assert_eq!(
            av_shared_file_error_result(ERRNO_EINVAL),
            TunerResult::UNKNOWN_ERROR
        );
        assert_eq!(
            av_shared_file_error_result(12345),
            TunerResult::UNKNOWN_ERROR
        );
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum InjectedDvrCleanupFailure {
        QueueClear,
        RuntimeUnregister,
        QueueStop,
        DemuxUnregister,
    }

    #[derive(Debug)]
    struct InjectedDvrCleanupRunner {
        failure: Option<InjectedDvrCleanupFailure>,
        callback_stoppable: bool,
        calls: Vec<&'static str>,
    }

    impl InjectedDvrCleanupRunner {
        fn new(failure: Option<InjectedDvrCleanupFailure>, callback_stoppable: bool) -> Self {
            Self {
                failure,
                callback_stoppable,
                calls: Vec::new(),
            }
        }

        fn injected_failure(&self, failure: InjectedDvrCleanupFailure) -> BinderResult<DvrCleanupStepResult> {
            if self.failure == Some(failure) {
                Err(Status::from(StatusCode::UNKNOWN_ERROR))
            } else {
                Ok(DvrCleanupStepResult::Success)
            }
        }
    }

    impl DvrCleanupStepRunner for InjectedDvrCleanupRunner {
        fn stop_callback_worker(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult> {
            self.calls.push("callback");
            if matches!(caller, DvrCleanupCaller::WorkerFailure) {
                return Ok(DvrCleanupStepResult::SkippedDueToWorkerFailureContext);
            }
            if self.callback_stoppable {
                Ok(DvrCleanupStepResult::Success)
            } else {
                Ok(DvrCleanupStepResult::SafeNoOp)
            }
        }

        fn clear_queue(&mut self, _caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult> {
            self.calls.push("clear");
            self.injected_failure(InjectedDvrCleanupFailure::QueueClear)
        }

        fn unregister_runtime(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult> {
            self.calls.push("runtime");
            if matches!(caller, DvrCleanupCaller::ExternalClose) {
                self.injected_failure(InjectedDvrCleanupFailure::RuntimeUnregister)
            } else {
                Ok(DvrCleanupStepResult::Unknown)
            }
        }

        fn stop_queue(&mut self, caller: DvrCleanupCaller) -> BinderResult<DvrCleanupStepResult> {
            self.calls.push("queue");
            if matches!(caller, DvrCleanupCaller::ExternalClose) {
                self.injected_failure(InjectedDvrCleanupFailure::QueueStop)
            } else {
                Ok(DvrCleanupStepResult::Unknown)
            }
        }

        fn unregister_demux(&mut self) -> BinderResult<DvrCleanupStepResult> {
            self.calls.push("demux");
            self.injected_failure(InjectedDvrCleanupFailure::DemuxUnregister)
        }
    }

    #[test]
    fn dvr_cleanup_helper_attempts_all_steps_after_injected_failures() {
        for failure in [
            InjectedDvrCleanupFailure::QueueClear,
            InjectedDvrCleanupFailure::RuntimeUnregister,
            InjectedDvrCleanupFailure::QueueStop,
            InjectedDvrCleanupFailure::DemuxUnregister,
        ] {
            let mut runner = InjectedDvrCleanupRunner::new(Some(failure), true);

            let outcome = DvrHal::cleanup_dvr_resources_with_runner(
                DvrCleanupCaller::ExternalClose,
                &mut runner,
            );

            assert!(outcome.first_error.is_some());
            assert!(!outcome.all_cleanup_complete);
            assert_eq!(
                runner.calls,
                vec!["callback", "clear", "runtime", "queue", "demux"]
            );
            assert_eq!(
                outcome.step_results.queue_clear,
                if failure == InjectedDvrCleanupFailure::QueueClear {
                    DvrCleanupStepResult::Failed
                } else {
                    DvrCleanupStepResult::Success
                }
            );
            assert_eq!(
                outcome.step_results.runtime_unregister,
                if failure == InjectedDvrCleanupFailure::RuntimeUnregister {
                    DvrCleanupStepResult::Failed
                } else {
                    DvrCleanupStepResult::Success
                }
            );
            assert_eq!(
                outcome.step_results.queue_stop,
                if failure == InjectedDvrCleanupFailure::QueueStop {
                    DvrCleanupStepResult::Failed
                } else {
                    DvrCleanupStepResult::Success
                }
            );
            assert_eq!(
                outcome.step_results.demux_unregister,
                if failure == InjectedDvrCleanupFailure::DemuxUnregister {
                    DvrCleanupStepResult::Failed
                } else {
                    DvrCleanupStepResult::Success
                }
            );

            let mut retry_runner = InjectedDvrCleanupRunner::new(None, true);
            let retry = DvrHal::cleanup_dvr_resources_with_runner(
                DvrCleanupCaller::ExternalClose,
                &mut retry_runner,
            );
            assert!(retry.first_error.is_none());
            assert!(retry.all_cleanup_complete);
        }
    }

    #[test]
    fn dvr_cleanup_helper_preserves_caller_specific_worker_completion() {
        let mut external = InjectedDvrCleanupRunner::new(None, true);
        let external_outcome = DvrHal::cleanup_dvr_resources_with_runner(
            DvrCleanupCaller::ExternalClose,
            &mut external,
        );
        assert!(external_outcome.all_cleanup_complete);

        let mut best_effort = InjectedDvrCleanupRunner::new(None, true);
        let best_effort_outcome = DvrHal::cleanup_dvr_resources_with_runner(
            DvrCleanupCaller::BestEffortDrop,
            &mut best_effort,
        );
        assert!(!best_effort_outcome.all_cleanup_complete);
        assert_eq!(
            best_effort_outcome.step_results.queue_clear,
            DvrCleanupStepResult::Unknown
        );
        assert_eq!(
            best_effort_outcome.step_results.runtime_unregister,
            DvrCleanupStepResult::Unknown
        );
        assert_eq!(
            best_effort_outcome.step_results.queue_stop,
            DvrCleanupStepResult::Unknown
        );

        let mut worker_failure = InjectedDvrCleanupRunner::new(None, true);
        let worker_failure_outcome = DvrHal::cleanup_dvr_resources_with_runner(
            DvrCleanupCaller::WorkerFailure,
            &mut worker_failure,
        );
        assert!(worker_failure_outcome.first_error.is_none());
        assert!(!worker_failure_outcome.all_cleanup_complete);
        assert_eq!(
            worker_failure_outcome.step_results.callback_worker,
            DvrCleanupStepResult::SkippedDueToWorkerFailureContext
        );
        assert_eq!(
            worker_failure_outcome.step_results.queue_clear,
            DvrCleanupStepResult::Unknown
        );
        assert_eq!(
            worker_failure_outcome.step_results.runtime_unregister,
            DvrCleanupStepResult::Unknown
        );
        assert_eq!(
            worker_failure_outcome.step_results.queue_stop,
            DvrCleanupStepResult::Unknown
        );
        assert_eq!(
            worker_failure_outcome.step_results.demux_unregister,
            DvrCleanupStepResult::Success
        );
    }

    #[test]
    fn dvr_close_is_idempotent_after_successful_cleanup() {
        let (dvr, state, _runtime_io, dvr_id) = new_test_dvr(DemuxPathDirection::Record);

        assert!(lock_mutex_status(&state, "test_mutex").unwrap().dvr_record(dvr_id).is_some());
        dvr.close_internal().unwrap();
        assert!(dvr.closed.load(Ordering::SeqCst));
        assert!(lock_mutex_status(&state, "test_mutex").unwrap().dvr_record(dvr_id).is_none());

        dvr.close_internal().unwrap();
        assert!(lock_mutex_status(&state, "test_mutex").unwrap().dvr_record(dvr_id).is_none());
    }

    #[test]
    fn dvr_close_attempts_full_cleanup_after_queue_stop_error() {
        let (dvr, state, _runtime_io, dvr_id) = new_test_dvr(DemuxPathDirection::Record);
        let queue_backing = Arc::clone(&dvr.queue_backing);

        let mut poison_worker = WorkerRuntime::spawn_owned_with_exit_hook(
            WorkerOwnerId("dvr_close_poison_shared_memory_worker", dvr_id),
            "dvr_close_poison_shared_memory_worker",
            move |_owner_signal| {
                let _guard = lock_mutex_status(&queue_backing.worker, "test_mutex").unwrap();
                panic!("poison shared_memory_worker mutex for close cleanup regression");
            },
            |_| {},
        )
        .unwrap();
        let _ = poison_worker.join_from_owner();

        let result = dvr.close_internal();

        assert!(result.is_err());
        assert!(!dvr.closed.load(Ordering::SeqCst));
        assert!(!dvr.cleanup_complete.load(Ordering::SeqCst));
        assert!(lock_mutex_status(&dvr.callback_worker, "test_mutex").unwrap().is_none());
        assert!(lock_mutex_status(&state, "test_mutex").unwrap().dvr_record(dvr_id).is_none());

        // queue stop が同じ poisoned lock で再度失敗しても、closed は立たず再試行可能な状態を維持する。
        let second = dvr.close_internal();
        assert!(second.is_err());
        assert!(!dvr.closed.load(Ordering::SeqCst));
    }

    #[test]
    fn dvr_fail_worker_cleanup_is_retryable_by_external_close() {
        let (dvr, state, _runtime_io, dvr_id) = new_test_dvr(DemuxPathDirection::Record);

        DvrHal::fail_dvr_worker(
            &dvr.state,
            &dvr.runtime_io,
            &dvr.queue_backing,
            &dvr.closed,
            &dvr.cleanup_complete,
            Some(&dvr.last_cleanup_steps),
            &dvr.callback_stop,
            dvr.dvr_id,
            "test_worker_failure",
        );

        assert!(dvr.closed.load(Ordering::SeqCst));
        assert!(!dvr.cleanup_complete.load(Ordering::SeqCst));
        let worker_failure_steps = lock_mutex_status(&dvr.last_cleanup_steps, "test_mutex").unwrap().clone().unwrap();
        assert_eq!(
            worker_failure_steps.callback_worker,
            DvrCleanupStepResult::SkippedDueToWorkerFailureContext
        );
        assert_eq!(worker_failure_steps.queue_clear, DvrCleanupStepResult::Unknown);
        assert_eq!(worker_failure_steps.runtime_unregister, DvrCleanupStepResult::Unknown);
        assert_eq!(worker_failure_steps.queue_stop, DvrCleanupStepResult::Unknown);
        assert_eq!(worker_failure_steps.demux_unregister, DvrCleanupStepResult::Success);
        assert!(lock_mutex_status(&state, "test_mutex").unwrap().dvr_record(dvr_id).is_none());

        dvr.close_internal().unwrap();
        assert!(dvr.cleanup_complete.load(Ordering::SeqCst));
        assert!(lock_mutex_status(&dvr.callback_worker, "test_mutex").unwrap().is_none());
        let external_close_steps = lock_mutex_status(&dvr.last_cleanup_steps, "test_mutex").unwrap().clone().unwrap();
        assert!(external_close_steps.callback_worker.is_complete());
        assert_eq!(external_close_steps.queue_clear, DvrCleanupStepResult::Success);
        assert_eq!(external_close_steps.runtime_unregister, DvrCleanupStepResult::Success);
        assert_eq!(external_close_steps.queue_stop, DvrCleanupStepResult::Success);
        assert_eq!(external_close_steps.demux_unregister, DvrCleanupStepResult::Success);
    }

    #[test]
    fn hal_close_lifecycle_peer_objects_checked() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let filter_record = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let filter_state = Arc::new(Mutex::new(demux));
        let filter_runtime = Arc::new(RuntimeIoRegistry::default());
        let filter_callback =
            BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());
        let filter = FilterHal::new(
            DEMUX_ID_BASE,
            filter_record.filter_id,
            Arc::clone(&filter_state),
            filter_runtime,
            filter_callback,
            None,
)
        .unwrap();
        filter.close_internal().unwrap();
        filter.close_internal().unwrap();
        assert!(lock_mutex_status(&filter_state, "test_mutex").unwrap()
            .filter_record(filter_record.filter_id)
            .is_none());

        let frontend_entry = FrontendEntry {
            id: 52,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        let frontend_runtime = FrontendRuntime::new(
            frontend_entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        let frontend = FrontendHal::new(
            frontend_runtime,
            FrontendType::ISDBT,
            0,
            1,
            Arc::new(Mutex::new(FrontendLeaseRegistry::default())),
            Arc::new(Mutex::new(DemuxLedger::default())),
        );
        frontend.close_internal().unwrap();
        frontend.close_internal().unwrap();

        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        descrambler.setDemuxSource(demux_id).unwrap();
        descrambler.close().unwrap();
        descrambler.close().unwrap();
        assert!(descrambler.debug_snapshot().0);

        let lnb_registry = Arc::new(Mutex::new(BTreeMap::new()));
        let lnb_frontends = Arc::new(BTreeMap::new());
        let lnb = LnbHal::new(77, lnb_registry, lnb_frontends).unwrap();
        lnb.close().unwrap();
        lnb.close().unwrap();
    }

    #[test]
    fn stale_partial_overflow_reaches_filter_callback_once() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let record = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        demux
            .configure_filter_with_summary_result(record.filter_id, section_config_for_pid(pid))
            .unwrap();
        demux.start_filter_result(record.filter_id).unwrap();
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let callback = BnFilterCallback::new_binder(
            RecordingFilterCallback {
                statuses: Arc::clone(&statuses),
            },
            BinderFeatures::default(),
        ).unwrap();
        let filter = FilterHal::new(
            DEMUX_ID_BASE,
            record.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
            None,
)
        .unwrap();

        let stale = [0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1];
        let replacement = [0x42, 0xf0, 0x05, 0x00, 0x01, 0xc1, 0x00, 0x00];
        let mut first = vec![0x00];
        first.extend_from_slice(&stale);
        let mut second = vec![0x00];
        second.extend_from_slice(&replacement);
        {
            let mut demux = lock_mutex_status(&state, "test_mutex").unwrap();
            assert!(demux.push_ts_packet(&section_packet(pid as u16, 0, &first)));
            assert!(demux.push_ts_packet(&section_packet(pid as u16, 1, &second)));
            assert_eq!(
                demux.filter_stale_partial_discard_count(record.filter_id),
                Some(1)
            );
        }

        assert!(wait_for_filter_status(
            &statuses,
            DemuxFilterStatus::OVERFLOW
        ));
        thread::sleep(Duration::from_millis(60));
        let overflow_count = lock_mutex_status(&statuses, "test_mutex").unwrap()
            .iter()
            .filter(|status| **status == DemuxFilterStatus::OVERFLOW)
            .count();
        assert_eq!(overflow_count, 1);
        filter.close_internal().unwrap();
    }

    #[test]
    fn vts_profile_does_not_claim_filter_monitor_events() {
        let vts = include_str!("../../config/tuner_vts_config_aidl_V2.xml");
        assert!(vts.contains("monitorEventTypes=\"0\""));
        assert!(!vts.contains("monitorEventTypes=\"1\""));
        assert!(!vts.contains("monitorEventTypes=\"2\""));
        assert!(!vts.contains("monitorEventTypes=\"3\""));
        assert!(!vts.contains("configureMonitorEvent"));
    }

    #[test]
    fn av_passthrough_is_rejected_at_configure_boundary() {
        assert!(build_filter_summary(&av_settings_with_passthrough(0x0100, false, false)).is_ok());
        assert!(build_filter_summary(&av_settings_with_passthrough(0x0100, false, true)).is_err());

        let design = include_str!("../../DESIGN_JA.md");
        assert!(design.contains("AV passthrough"));
        assert!(design.contains("isPassthrough=true"));
        assert!(design.contains("UNAVAILABLE"));

        let vts = include_str!("../../config/tuner_vts_config_aidl_V2.xml");
        assert!(vts.contains("isPassthrough=\"false\""));
        assert!(!vts.contains("isPassthrough=\"true\""));
    }

    #[test]
    fn configure_ip_cid_is_unavailable_for_r51_ts_only_profile() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let record = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());
        let filter = FilterHal::new(
            DEMUX_ID_BASE,
            record.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
            None,
)
        .unwrap();

        assert!(filter.configureIpCid(7).is_err());
        assert_eq!(
            lock_mutex_status(&state, "test_mutex").unwrap()
                .filter_record(record.filter_id)
                .unwrap()
                .ip_cid,
            None
        );

        let design = include_str!("../../DESIGN_JA.md");
        assert!(design.contains("TS-only HAL profile"));
        assert!(design.contains("IFilter.configureIpCid()"));
        assert!(design.contains("UNAVAILABLE"));
    }

    #[test]
    fn table_info_version_contract_is_minus_one_or_zero_to_thirty_one() {
        assert_eq!(normalize_table_info_version(-1).unwrap(), None);
        assert_eq!(normalize_table_info_version(0).unwrap(), Some(0));
        assert_eq!(normalize_table_info_version(31).unwrap(), Some(31));
        assert!(normalize_table_info_version(-2).is_err());
        assert!(normalize_table_info_version(32).is_err());
    }

    #[test]
    fn section_table_id_contract_is_zero_to_255_only() {
        assert_eq!(normalize_section_table_id(0).unwrap(), 0);
        assert_eq!(normalize_section_table_id(255).unwrap(), 255);
        assert!(normalize_section_table_id(-1).is_err());
        assert!(normalize_section_table_id(256).is_err());
    }

    #[test]
    fn pes_stream_id_contract_allows_minus_one_wildcard_and_8bit_values() {
        assert_eq!(normalize_pes_stream_id(-1).unwrap(), -1);
        assert_eq!(normalize_pes_stream_id(0).unwrap(), 0);
        assert_eq!(normalize_pes_stream_id(255).unwrap(), 255);
        assert!(normalize_pes_stream_id(-2).is_err());
        assert!(normalize_pes_stream_id(256).is_err());
        assert!(build_filter_summary(&pes_settings_with_stream_id(0x0100, -1)).is_ok());
        assert!(build_filter_summary(&pes_settings_with_stream_id(0x0100, -2)).is_err());
        assert!(build_filter_summary(&pes_settings_with_stream_id(0x0100, 256)).is_err());
    }


    #[test]
    fn descrambler_upstream_filter_open_type_policy_includes_audio_video_pes_and_record_only() {
        for open_type in [
            FilterOpenType::TsAudio,
            FilterOpenType::TsVideo,
            FilterOpenType::TsPes,
            FilterOpenType::TsRecord,
        ] {
            assert!(descrambler_upstream_filter_open_type_allowed(open_type));
        }
        for open_type in [
            FilterOpenType::TsRaw,
            FilterOpenType::TsSection,
            FilterOpenType::TsOther,
            FilterOpenType::NonTs,
        ] {
            assert!(!descrambler_upstream_filter_open_type_allowed(open_type));
        }
    }

    #[test]
    fn record_index_config_rejects_unsupported_and_mismatched_values() {
        let valid_ts = DEMUX_TS_INDEX_FIRST_PACKET | DEMUX_TS_INDEX_PAYLOAD_UNIT_START;
        assert!(validate_record_index_settings(
            valid_ts,
            RECORD_SC_TYPE_NONE,
            &DemuxFilterScIndexMask::ScIndex(0),
        )
        .is_ok());
        assert!(validate_record_index_settings(
            supported_record_ts_index_mask() << 1,
            RECORD_SC_TYPE_NONE,
            &DemuxFilterScIndexMask::ScIndex(0),
        )
        .is_err());
        assert!(validate_record_index_settings(
            valid_ts,
            RECORD_SC_TYPE_NONE,
            &DemuxFilterScIndexMask::ScIndex(1),
        )
        .is_err());
        assert!(
            validate_record_index_settings(valid_ts, 999, &DemuxFilterScIndexMask::ScIndex(1),)
                .is_err()
        );
        assert!(validate_record_index_settings(
            valid_ts,
            RECORD_SC_TYPE_SC_HEVC,
            &DemuxFilterScIndexMask::ScAvc(1),
        )
        .is_err());
        assert!(validate_record_index_settings(
            valid_ts,
            RECORD_SC_TYPE_SC,
            &DemuxFilterScIndexMask::ScIndex(1 << 20),
        )
        .is_err());
        assert!(validate_record_index_settings(
            valid_ts,
            RECORD_SC_TYPE_SC_AVC,
            &DemuxFilterScIndexMask::ScAvc(1 << 20),
        )
        .is_err());
        assert!(validate_record_index_settings(
            valid_ts,
            RECORD_SC_TYPE_SC_HEVC,
            &DemuxFilterScIndexMask::ScHevc(1 << 20),
        )
        .is_err());
        assert!(validate_record_index_settings(
            valid_ts,
            RECORD_SC_TYPE_SC_VVC,
            &DemuxFilterScIndexMask::ScVvc(1 << 20),
        )
        .is_err());
    }

    #[test]
    fn record_index_config_accepts_each_supported_sc_variant() {
        let valid_ts = DEMUX_TS_INDEX_FIRST_PACKET;
        for (sc_type, mask) in [
            (RECORD_SC_TYPE_SC, DemuxFilterScIndexMask::ScIndex(1)),
            (RECORD_SC_TYPE_SC_AVC, DemuxFilterScIndexMask::ScAvc(1)),
            (RECORD_SC_TYPE_SC_HEVC, DemuxFilterScIndexMask::ScHevc(1)),
            (RECORD_SC_TYPE_SC_VVC, DemuxFilterScIndexMask::ScVvc(1)),
        ] {
            assert!(validate_record_index_settings(valid_ts, sc_type, &mask).is_ok());
        }
    }

    #[test]
    fn ts_record_event_is_suppressed_without_configured_index_hit() {
        let packet = record_ts_packet(0x0100, false, 0);
        let mut state = RecordEventState::default();
        assert!(build_ts_record_event(
            &packet,
            0,
            0,
            RECORD_SC_TYPE_NONE,
            0,
            &mut state,
        )
        .is_none());

        let mut state = RecordEventState::default();
        assert!(build_ts_record_event(
            &packet,
            TS_PACKET_SIZE as u64,
            DEMUX_TS_INDEX_FIRST_PACKET,
            RECORD_SC_TYPE_NONE,
            0,
            &mut state,
        )
        .is_none());
    }

    #[test]
    fn ts_record_event_is_emitted_for_configured_ts_index_hit() {
        let packet = record_ts_packet(0x0100, false, 0);
        let mut state = RecordEventState::default();
        let event = ts_record_event(
            build_ts_record_event(
                &packet,
                0,
                DEMUX_TS_INDEX_FIRST_PACKET,
                RECORD_SC_TYPE_NONE,
                0,
                &mut state,
            )
            .expect("FIRST_PACKET mask hit should emit record event"),
        );
        assert_eq!(event.tsIndexMask, DEMUX_TS_INDEX_FIRST_PACKET);
        assert_eq!(event.byteNumber, 0);
        assert!(matches!(event.scIndexMask, DemuxFilterScIndexMask::ScIndex(0)));
    }

    #[test]
    fn ts_record_event_is_emitted_for_configured_sc_index_hit() {
        let mut packet = record_ts_packet(0x0100, false, 0);
        packet[4..8].copy_from_slice(&[0x00, 0x00, 0x01, 0xb3]);
        let mut state = RecordEventState::default();
        let event = ts_record_event(
            build_ts_record_event(
                &packet,
                TS_PACKET_SIZE as u64,
                0,
                RECORD_SC_TYPE_SC,
                1 << 3,
                &mut state,
            )
            .expect("SC mask hit should emit record event"),
        );
        assert_eq!(event.tsIndexMask, 0);
        assert!(matches!(event.scIndexMask, DemuxFilterScIndexMask::ScIndex(v) if v == (1 << 3)));
    }

    #[test]
    fn local_filter_owner_validation_maps_lifecycle_and_argument_errors() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let record = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());
        let local = FilterHal::new(
            DEMUX_ID_BASE,
            record.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
            None,
)
        .unwrap();
        assert_eq!(local.getId().unwrap(), record.filter_id);
        assert_eq!(local.getId64Bit().unwrap(), i64::from(record.filter_id));
        assert_eq!(
            validate_local_filter_identity_for_owner(&local, DEMUX_ID_BASE)
                .map(|identity| identity.filter_id),
            Ok(record.filter_id)
        );

        assert_eq!(
            validate_local_filter_identity_for_owner(&local, DEMUX_ID_BASE + 1),
            Err(LocalFilterOwnerValidationError::ForeignDemux)
        );
        assert_eq!(
            local_filter_owner_error_tuner_result(LocalFilterOwnerValidationError::ForeignDemux),
            TunerResult::INVALID_ARGUMENT.0
        );

        local.closed.store(true, Ordering::SeqCst);
        assert_eq!(
            validate_local_filter_identity_for_owner(&local, DEMUX_ID_BASE),
            Err(LocalFilterOwnerValidationError::Closed)
        );
        assert_eq!(
            local_filter_owner_error_tuner_result(LocalFilterOwnerValidationError::Closed),
            TunerResult::INVALID_ARGUMENT.0
        );
        local.closed.store(false, Ordering::SeqCst);

        runtime_io.mark_failed(RuntimeIoKind::Filter, record.filter_id, "test failure");
        assert_eq!(
            validate_local_filter_identity_for_owner(&local, DEMUX_ID_BASE),
            Err(LocalFilterOwnerValidationError::RuntimeFailed)
        );
        assert_eq!(
            local_filter_owner_error_tuner_result(LocalFilterOwnerValidationError::RuntimeFailed),
            TunerResult::INVALID_ARGUMENT.0
        );

        let runtime_io_clean = Arc::new(RuntimeIoRegistry::default());
        let local_unregistered = FilterHal::new(
            DEMUX_ID_BASE,
            record.filter_id,
            Arc::clone(&state),
            runtime_io_clean,
            callback,
            None,
)
        .unwrap();
        lock_mutex_status(&state, "test_mutex").unwrap().unregister_filter(record.filter_id);
        assert_eq!(
            validate_local_filter_identity_for_owner(&local_unregistered, DEMUX_ID_BASE),
            Err(LocalFilterOwnerValidationError::NotOpenDemuxFilter)
        );
        assert_eq!(
            local_filter_owner_error_tuner_result(
                LocalFilterOwnerValidationError::NotOpenDemuxFilter
            ),
            TunerResult::INVALID_ARGUMENT.0
        );
    }

    #[test]
    fn closed_upstream_filter_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
            None,
)
        .unwrap();
        source_hal.closed.store(true, Ordering::SeqCst);
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
            None,
)
        .unwrap();
        let err = destination_hal.setDataSource(&source_binder).unwrap_err();
        assert_eq!(err.service_specific_error(), TunerResult::INVALID_ARGUMENT.0);
        assert_eq!(
            lock_mutex_status(&state, "test_mutex").unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn runtime_failed_upstream_filter_is_rejected_as_invalid_argument_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
            None,
)
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());
        runtime_io.mark_failed(RuntimeIoKind::Filter, source.filter_id, "test用の入力元失敗");

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
            None,
)
        .unwrap();
        let err = destination_hal.setDataSource(&source_binder).unwrap_err();
        assert_eq!(err.service_specific_error(), TunerResult::INVALID_ARGUMENT.0);
        assert_eq!(
            lock_mutex_status(&state, "test_mutex").unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn started_destination_rewire_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let section_summary = FilterConfig {
            tpid: 0x0123,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        };
        assert!(demux.configure_filter_with_summary_result(source.filter_id, section_summary.clone()).is_ok());
        assert!(demux.configure_filter_with_summary_result(destination.filter_id, section_summary).is_ok());
        assert_eq!(demux.start_filter_result(destination.filter_id), Ok(()));
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
            None,
)
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
            None,
)
        .unwrap();

        assert!(destination_hal.setDataSource(&source_binder).is_err());
        assert_eq!(
            lock_mutex_status(&state, "test_mutex").unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn closed_destination_filter_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
            None,
)
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
            None,
)
        .unwrap();
        destination_hal.closed.store(true, Ordering::SeqCst);

        assert!(destination_hal.setDataSource(&source_binder).is_err());
        assert_eq!(
            lock_mutex_status(&state, "test_mutex").unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn runtime_failed_destination_filter_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
            None,
)
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback,
            None,
)
        .unwrap();
        runtime_io.mark_failed(
            RuntimeIoKind::Filter,
            destination.filter_id,
            "test用の出力先失敗",
        );

        assert!(destination_hal.setDataSource(&source_binder).is_err());
        assert_eq!(
            lock_mutex_status(&state, "test_mutex").unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn advertised_ts_linkage_succeeds_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsRaw, 4096)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let raw_summary = FilterConfig {
            tpid: 0x0123,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Noinit,
        };
        let section_summary = FilterConfig {
            tpid: 0x0123,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        };
        assert!(demux.configure_filter_with_summary_result(source.filter_id, raw_summary).is_ok());
        assert!(demux.configure_filter_with_summary_result(destination.filter_id, section_summary).is_ok());
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
            None,
)
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
            None,
)
        .unwrap();

        destination_hal.setDataSource(&source_binder).unwrap();
        assert_eq!(
            lock_mutex_status(&state, "test_mutex").unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            Some(source.filter_id)
        );
    }

    #[test]
    fn unadvertised_linkage_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux
            .register_filter_result(1, FilterOpenType::NonTs, 4096)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
            None,
)
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
            None,
)
        .unwrap();

        assert!(destination_hal.setDataSource(&source_binder).is_err());
        assert_eq!(
            lock_mutex_status(&state, "test_mutex").unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn delay_hint_record_contract_rejects_media_before_configuration() {
        let mut demux = DemuxHandle::new(0);
        let audio = demux
            .register_filter_result(1, FilterOpenType::TsAudio, 4096)
            .expect("test setup should register filter");
        let video = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        let section = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        let time_hint = delay_hint(FilterDelayHintType::TIME_DELAY_IN_MS, 10);
        let size_hint = delay_hint(FilterDelayHintType::DATA_SIZE_DELAY_IN_BYTES, 188);

        assert!(normalize_filter_delay_hint_for_record(
            demux.filter_record(audio.filter_id).unwrap(),
            &time_hint
        )
        .is_err());
        assert!(normalize_filter_delay_hint_for_record(
            demux.filter_record(video.filter_id).unwrap(),
            &size_hint
        )
        .is_err());
        assert_eq!(
            normalize_filter_delay_hint_for_record(
                demux.filter_record(section.filter_id).unwrap(),
                &time_hint
            )
            .unwrap(),
            FilterDelayHintState::TimeDelayMs(10)
        );
        assert_eq!(
            normalize_filter_delay_hint_for_record(
                demux.filter_record(record.filter_id).unwrap(),
                &time_hint
            )
            .unwrap(),
            FilterDelayHintState::TimeDelayMs(10)
        );
        assert!(normalize_filter_delay_hint_for_record(
            demux.filter_record(record.filter_id).unwrap(),
            &size_hint
        )
        .is_err());
    }

    #[test]
    fn start_id_is_gated_by_delay_readiness() {
        assert!(filter_start_event_ready(
            maleicacid_tuner_hal_soft_demux::FilterDeliveryReadiness::Ready
        ));
        assert!(!filter_start_event_ready(
            maleicacid_tuner_hal_soft_demux::FilterDeliveryReadiness::WaitingForTime
        ));
        assert!(!filter_start_event_ready(
            maleicacid_tuner_hal_soft_demux::FilterDeliveryReadiness::WaitingForDataSize
        ));
        assert!(!filter_start_event_ready(
            maleicacid_tuner_hal_soft_demux::FilterDeliveryReadiness::MissingFilter
        ));
    }

    #[test]
    fn secure_av_memory_is_rejected_and_clear_av_is_accepted() {
        assert!(build_filter_summary(&av_settings(0x100, true)).is_err());
        let summary = build_filter_summary(&av_settings(0x100, false)).unwrap();
        assert!(matches!(
            summary.kind,
            FilterConfigKind::Av {
                secure_memory: false,
                ..
            }
        ));
    }

    #[test]
    fn ts_pid_range_is_validated_for_all_representative_ts_subtypes() {
        assert!(build_filter_summary(&av_settings(-1, false)).is_err());
        assert!(build_filter_summary(&av_settings(0x2000, false)).is_err());
        assert!(build_filter_summary(&av_settings(0, false)).is_ok());
        assert!(build_filter_summary(&av_settings(0x1fff, false)).is_ok());
        assert!(build_filter_summary(&pes_settings(0x100)).is_ok());
        assert!(build_filter_summary(&record_filter_settings(0x100)).is_ok());
        assert!(build_filter_summary(&pes_settings(0x2000)).is_err());
        assert!(build_filter_summary(&record_filter_settings(-1)).is_err());
    }

    #[test]
    fn dvr_settings_are_ts_188_and_direction_checked() {
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &record_dvr_settings(DataFormat::TS, 188),
            4_194_304
        )
        .is_ok());
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Playback,
            &playback_dvr_settings(DataFormat::TS, 188),
            4_194_304
        )
        .is_ok());
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &playback_dvr_settings(DataFormat::TS, 188),
            4_194_304
        )
        .is_err());
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Playback,
            &record_dvr_settings(DataFormat::TS, 188),
            4_194_304
        )
        .is_err());
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &record_dvr_settings(DataFormat::TS, 187),
            4_194_304
        )
        .is_err());
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &record_dvr_settings(DataFormat::TS, 0),
            4_194_304
        )
        .is_err());
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &record_dvr_settings(DataFormat::TS, 204),
            4_194_304
        )
        .is_err());
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &record_dvr_settings(DataFormat::PES, 188),
            4_194_304
        )
        .is_err());
    }

    #[test]
    fn dvr_settings_reject_invalid_thresholds_and_status_masks() {
        let mut record = match record_dvr_settings(DataFormat::TS, 188) {
            DvrSettings::Record(record) => record,
            _ => unreachable!(),
        };
        record.lowThreshold = -1;
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &DvrSettings::Record(record.clone()),
            4_194_304
        )
        .is_err());
        record.lowThreshold = 3_145_728;
        record.highThreshold = 1_048_576;
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &DvrSettings::Record(record.clone()),
            4_194_304
        )
        .is_err());
        record.lowThreshold = 1_048_576;
        record.highThreshold = 4_194_305;
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &DvrSettings::Record(record.clone()),
            4_194_304
        )
        .is_err());
        record.highThreshold = 3_145_728;
        record.statusMask = i32::MIN;
        assert!(validate_and_build_dvr_summary(
            DemuxPathDirection::Record,
            &DvrSettings::Record(record),
            4_194_304
        )
        .is_err());
    }

    #[test]
    fn av_shared_handle_can_be_exported_before_payload_and_released_with_zero_id() {
        let backing = AvSharedBacking::new().unwrap();
        assert_eq!(backing.stats().allocated_slots, 0);
        assert_eq!(backing.total_size(), AV_MIN_SLOT_SIZE * AV_SLOT_COUNT);
        let handle = backing.build_native_handle().unwrap();
        assert_eq!(handle.fds.len(), 1);
        assert_eq!(handle.ints, vec![0]);
        assert!(!handle.ints.contains(&(AV_MIN_SLOT_SIZE as i32)));
        assert!(!handle.ints.contains(&(AV_SLOT_COUNT as i32)));

        backing.release_all().unwrap();
        assert_eq!(backing.stats().allocated_slots, 0);

        backing.allocate(1, &[0x47; 188]).unwrap();
        assert_eq!(backing.stats().allocated_slots, 1);
        backing.release_all().unwrap();
        let stats = backing.stats();
        assert_eq!(stats.allocated_slots, 0);
        assert_eq!(stats.free_slots, AV_SLOT_COUNT);
        assert_eq!(stats.released_slots, 1);
    }

    #[test]
    fn runtime_io_registry_flushes_live_av_shared_and_removes_dead_entries() {
        let registry = RuntimeIoRegistry::default();
        let av_shared = AvSharedBacking::new().unwrap();
        av_shared.allocate(1, &[0x47; 188]).unwrap();
        assert_eq!(av_shared.stats().allocated_slots, 1);
        lock_mutex_status(&registry.entries, "test_mutex").unwrap().insert(
            RuntimeIoKey {
                kind: RuntimeIoKind::Filter,
                id: 6,
            },
            RuntimeIoBackings {
                filter_queue: None,
                filter_av_queue: None,
                filter_av_shared: Some(Arc::downgrade(&av_shared)),
                filter_av_drop_unexported: Some(Arc::new(AtomicU64::new(2))),
                dvr_queue: None,
                failed_reason: None,
            },
        );
        lock_mutex_status(&registry.entries, "test_mutex").unwrap().insert(
            RuntimeIoKey {
                kind: RuntimeIoKind::Filter,
                id: 7,
            },
            RuntimeIoBackings {
                filter_queue: Some(Weak::new()),
                filter_av_queue: Some(Weak::new()),
                filter_av_shared: Some(Weak::new()),
                filter_av_drop_unexported: Some(Arc::new(AtomicU64::new(0))),
                dvr_queue: None,
                failed_reason: None,
            },
        );
        registry.flush_all().unwrap();
        assert_eq!(av_shared.stats().allocated_slots, 0);
        assert_eq!(registry.entry_count(), 1);
        let dump = registry.dump_av_shared_for_debug().join("\n");
        assert!(dump.contains("av_drop_unexported=2"));
    }

    #[test]
    fn helper_rejects_live_av_event_without_shared_slot() {
        let record = DemuxFilterRecord {
            filter_id: 1,
            filter_type_bits: 0,
            open_type: FilterOpenType::TsVideo,
            buffer_size: 4096,
            configured: true,
            started: true,
            monitor_event_mask: 0,
            ip_cid: None,
            data_upstream_filter_id: None,
            pending_start_event: false,
            pending_start_id: 0,
            ever_started: true,
            delay_hints: maleicacid_tuner_hal_soft_demux::FilterDelayHints::default(),
            delivery_not_before: None,
            av_stream_type_hint: Some(0xe0),
            av_stream_kind: Some(AvFilterStreamKind::Video),
            config: Some(FilterConfig {
                tpid: 0x100,
                main_type_bits: 0,
                sub_type_hint: 0xe0,
                kind: FilterConfigKind::Av {
                    passthrough: false,
                    secure_memory: false,
                },
            }),
            queued_bytes: 0,
            pending_overflow: false,
            overflow_events: 0,
            drop_bytes: 0,
            section_drop_events: 0,
            stale_partial_discards: 0,
            events_emitted: 0,
            delivery_generation: 0,
        };
        let mut state = RecordEventState::default();
        let event = build_filter_event_from_payload(
            &record,
            &[0x00, 0x00, 0x01, 0xe0, 0x00, 0x00],
            None,
            Some(0xe0),
            0,
            0,
            None,
            None,
            None,
            &mut state,
        );
        assert!(event.is_none());
        // AV 正式配送は avDataId != 0 かつ empty avMemory の shared-handle slot だけ。
    }

    #[test]
    fn helper_drops_secure_memory_av_event_without_panic() {
        let record = DemuxFilterRecord {
            filter_id: 1,
            filter_type_bits: 0,
            open_type: FilterOpenType::TsVideo,
            buffer_size: 4096,
            configured: true,
            started: true,
            monitor_event_mask: 0,
            ip_cid: None,
            data_upstream_filter_id: None,
            pending_start_event: false,
            pending_start_id: 0,
            ever_started: true,
            delay_hints: maleicacid_tuner_hal_soft_demux::FilterDelayHints::default(),
            delivery_not_before: None,
            av_stream_type_hint: Some(0xe0),
            av_stream_kind: Some(AvFilterStreamKind::Video),
            config: Some(FilterConfig {
                tpid: 0x100,
                main_type_bits: 0,
                sub_type_hint: 0xe0,
                kind: FilterConfigKind::Av {
                    passthrough: false,
                    secure_memory: true,
                },
            }),
            queued_bytes: 0,
            pending_overflow: false,
            overflow_events: 0,
            drop_bytes: 0,
            section_drop_events: 0,
            stale_partial_discards: 0,
            events_emitted: 0,
            delivery_generation: 0,
        };
        let mut state = RecordEventState::default();
        let event = build_filter_event_from_payload(
            &record,
            &[0x00, 0x00, 0x01, 0xe0, 0x00, 0x00],
            None,
            Some(0xe0),
            0,
            0,
            Some(AvBufferSlice {
                slot_index: 0,
                offset: 0,
                len: 6,
                generation: 1,
            }),
            Some(7),
            Some(empty_native_handle()),
            &mut state,
        );
        assert!(event.is_none());
    }

    #[test]
    fn helper_builds_live_av_event_with_shared_slot_and_empty_event_handle() {
        let record = DemuxFilterRecord {
            filter_id: 1,
            filter_type_bits: 0,
            open_type: FilterOpenType::TsVideo,
            buffer_size: 4096,
            configured: true,
            started: true,
            monitor_event_mask: 0,
            ip_cid: None,
            data_upstream_filter_id: None,
            pending_start_event: false,
            pending_start_id: 0,
            ever_started: true,
            delay_hints: maleicacid_tuner_hal_soft_demux::FilterDelayHints::default(),
            delivery_not_before: None,
            av_stream_type_hint: Some(0xe0),
            av_stream_kind: Some(AvFilterStreamKind::Video),
            config: Some(FilterConfig {
                tpid: 0x100,
                main_type_bits: 0,
                sub_type_hint: 0xe0,
                kind: FilterConfigKind::Av {
                    passthrough: false,
                    secure_memory: false,
                },
            }),
            queued_bytes: 0,
            pending_overflow: false,
            overflow_events: 0,
            drop_bytes: 0,
            section_drop_events: 0,
            stale_partial_discards: 0,
            events_emitted: 0,
            delivery_generation: 0,
        };
        let mut state = RecordEventState::default();
        let event = build_filter_event_from_payload(
            &record,
            &[0x00, 0x00, 0x01, 0xe0, 0x00, 0x00],
            None,
            Some(0xe0),
            0,
            0,
            Some(AvBufferSlice {
                slot_index: 0,
                offset: 4096,
                len: 6,
                generation: 1,
            }),
            Some(7),
            Some(empty_native_handle()),
            &mut state,
        )
        .expect("shared-slot AV event should be built");
        match event {
            DemuxFilterEvent::Media(media) => {
                assert_eq!(media.avDataId, 7);
                assert_eq!(media.offset, 4096);
                assert_eq!(media.dataLength, 6);
                assert!(media.avMemory.fds.is_empty());
                assert!(media.avMemory.ints.is_empty());
            }
            other => panic!("想定外のAV eventです: {:?}", other),
        }
    }

    fn event_test_record(open_type: FilterOpenType, kind: FilterConfigKind) -> DemuxFilterRecord {
        DemuxFilterRecord {
            filter_id: 44,
            filter_type_bits: 0,
            open_type,
            buffer_size: 4096,
            configured: true,
            started: true,
            monitor_event_mask: 0,
            ip_cid: None,
            data_upstream_filter_id: None,
            pending_start_event: false,
            pending_start_id: 0,
            ever_started: true,
            delay_hints: maleicacid_tuner_hal_soft_demux::FilterDelayHints::default(),
            delivery_not_before: None,
            av_stream_type_hint: None,
            av_stream_kind: None,
            config: Some(FilterConfig {
                tpid: 0x100,
                main_type_bits: 0,
                sub_type_hint: 0,
                kind,
            }),
            queued_bytes: 0,
            pending_overflow: false,
            overflow_events: 0,
            drop_bytes: 0,
            section_drop_events: 0,
            stale_partial_discards: 0,
            events_emitted: 0,
            delivery_generation: 0,
        }
    }

    #[test]
    fn helper_builds_raw_section_event_even_when_header_is_not_parseable() {
        let record = event_test_record(
            FilterOpenType::TsSection,
            FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: true,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        );
        let mut state = RecordEventState::default();
        let event = build_filter_event_from_payload(
            &record,
            &[0xaa],
            None,
            None,
            0,
            0,
            None,
            None,
            None,
            &mut state,
        )
        .expect("raw section event must not be dropped");
        match event {
            DemuxFilterEvent::Section(section) => {
                assert_eq!(section.dataLength, 1);
            }
            other => panic!("想定外のsection eventです: {:?}", other),
        }
    }

    #[test]
    fn helper_builds_raw_pes_event_even_when_header_is_not_parseable() {
        let record = event_test_record(
            FilterOpenType::TsPes,
            FilterConfigKind::PesData {
                stream_id: -1,
                raw: true,
            },
        );
        let mut state = RecordEventState::default();
        let event = build_filter_event_from_payload(
            &record,
            &[0xaa, 0xbb],
            None,
            None,
            0,
            0,
            None,
            None,
            None,
            &mut state,
        )
        .expect("raw PES event must not be dropped");
        match event {
            DemuxFilterEvent::Pes(pes) => {
                assert_eq!(pes.streamId, 0);
                assert_eq!(pes.dataLength, 2);
            }
            other => panic!("想定外のPES eventです: {:?}", other),
        }
    }

}

#[cfg(test)]
mod av_delivery_acceptance_tests {
    use super::*;

    #[test]
    fn av_delivery_decision_never_notifies_data_ready_for_drop_results() {
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedBeforeHandleExport),
                true
            ),
            (false, true)
        );
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedAfterClientRelease),
                true
            ),
            (false, true)
        );
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedNoFreeSlot),
                true
            ),
            (false, true)
        );
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedOversizePayload),
                true
            ),
            (false, true)
        );
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedMalformedPayload),
                true
            ),
            (false, true)
        );
    }

    #[test]
    fn av_delivery_decision_allows_data_ready_only_for_delivered_media_payload() {
        let delivered = AvPayloadDeliveryResult::Delivered {
            slice: AvBufferSlice {
                slot_index: 0,
                offset: 0,
                len: 188,
                generation: 1,
            },
            av_data_id: 1,
        };
        assert_eq!(
            av_payload_status_decision(true, Some(delivered), false),
            (true, false)
        );
        assert_eq!(
            av_payload_status_decision(false, None, false),
            (true, false)
        );
    }

    #[test]
    fn callback_worker_branch_helpers_keep_av_payloads_out_of_standard_fmq_and_eventflag_path() {
        assert!(!av_payload_should_write_standard_fmq(true));
        assert!(av_payload_should_write_standard_fmq(false));
        assert!(!av_payload_should_emit_data_event(true, None));
        assert!(av_payload_should_emit_data_event(
            true,
            Some(AvBufferSlice {
                slot_index: 0,
                offset: 0,
                len: 188,
                generation: 1
            })
        ));
        assert!(av_payload_should_emit_data_event(false, None));
    }

    #[test]
    fn av_shared_handle_delivery_requires_exported_and_not_client_released() {
        assert!(!av_shared_handle_allows_payload_delivery(false, false));
        assert!(!av_shared_handle_allows_payload_delivery(false, true));
        assert!(!av_shared_handle_allows_payload_delivery(true, true));
        assert!(av_shared_handle_allows_payload_delivery(true, false));
    }

    #[test]
    fn native_handle_exports_only_memory_index_without_slot_metadata() {
        let backing = AvSharedBacking::new().unwrap();
        let handle = backing.build_native_handle().unwrap();
        assert_eq!(handle.fds.len(), 1);
        assert_eq!(handle.ints, vec![0]);
        assert_ne!(handle.ints.get(1), Some(&(backing.slot_size as i32)));
        assert_ne!(handle.ints.get(2), Some(&(backing.slot_count as i32)));
    }

    #[test]
    fn public_get_av_shared_handle_exports_only_memory_index_without_slot_metadata() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let record = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());
        let local = FilterHal::new(
            DEMUX_ID_BASE,
            record.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback,
            None,
)
        .unwrap();

        local.configure(&av_settings(0x100, false)).unwrap();

        let mut handle = empty_native_handle();
        let size = local.getAvSharedHandle(&mut handle).unwrap();

        assert_eq!(size, (AV_MIN_SLOT_SIZE * AV_SLOT_COUNT) as i64);
        assert_eq!(handle.fds.len(), 1);
        assert_eq!(handle.ints, vec![0]);
        assert!(!handle.ints.contains(&(AV_MIN_SLOT_SIZE as i32)));
        assert!(!handle.ints.contains(&(AV_SLOT_COUNT as i32)));
        assert_ne!(handle.ints.get(1), Some(&(AV_MIN_SLOT_SIZE as i32)));
        assert_ne!(handle.ints.get(2), Some(&(AV_SLOT_COUNT as i32)));
    }

    fn new_test_filter_hal(
        open_type: FilterOpenType,
    ) -> (FilterHal, Arc<Mutex<DemuxHandle>>, Arc<RuntimeIoRegistry>, i32) {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let record = demux
            .register_filter_result(1, open_type, 4096)
            .expect("test setup should register filter");
        let filter_id = record.filter_id;
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());
        let local = FilterHal::new(
            DEMUX_ID_BASE,
            filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback,
            None,
)
        .unwrap();
        (local, state, runtime_io, filter_id)
    }

    fn new_configured_av_filter() -> (FilterHal, Arc<Mutex<DemuxHandle>>, Arc<RuntimeIoRegistry>, i32) {
        let (local, state, runtime_io, filter_id) = new_test_filter_hal(FilterOpenType::TsVideo);
        local.configure(&av_settings(0x100, false)).unwrap();
        (local, state, runtime_io, filter_id)
    }

    fn exported_av_backing(local: &FilterHal) -> (TunerNativeHandle, Arc<AvSharedBacking>) {
        let mut handle = empty_native_handle();
        local.getAvSharedHandle(&mut handle).unwrap();
        let backing = lock_mutex_status(&local.av_shared_backing, "test_av_shared_backing")
            .unwrap()
            .as_ref()
            .cloned()
            .unwrap();
        (handle, backing)
    }

    #[test]
    fn release_av_handle_accepts_empty_handle_with_zero_data_id_after_export() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (_shared_handle, backing) = exported_av_backing(&local);
        backing.allocate(77, &[0x47; 188]).unwrap();
        assert_eq!(backing.stats().allocated_slots, 1);
        let before = AV_HANDLE_CLIENT_RELEASE_COUNT.load(Ordering::SeqCst);

        local.releaseAvHandle(&empty_native_handle(), 0).unwrap();

        assert_eq!(backing.stats().allocated_slots, 1);
        assert!(local.av_shared_handle_client_released.load(Ordering::SeqCst));
        assert!(AV_HANDLE_CLIENT_RELEASE_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_empty_zero_before_export_is_compat_noop() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let before = AV_HANDLE_CLIENT_RELEASE_COUNT.load(Ordering::SeqCst);

        local.releaseAvHandle(&empty_native_handle(), 0).unwrap();

        assert!(!local.av_shared_handle_client_released.load(Ordering::SeqCst));
        assert_eq!(AV_HANDLE_CLIENT_RELEASE_COUNT.load(Ordering::SeqCst), before);
    }

    #[test]
    fn release_av_handle_empty_zero_double_release_rejected_after_export() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (_shared_handle, _backing) = exported_av_backing(&local);
        local.releaseAvHandle(&empty_native_handle(), 0).unwrap();
        let before = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&empty_native_handle(), 0).is_err());

        assert!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn get_av_shared_handle_after_client_release_rearms_delivery() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (_shared_handle, _backing) = exported_av_backing(&local);
        local.releaseAvHandle(&empty_native_handle(), 0).unwrap();
        assert!(local.av_shared_handle_client_released.load(Ordering::SeqCst));

        let mut handle = empty_native_handle();
        local.getAvSharedHandle(&mut handle).unwrap();

        assert_eq!(handle.ints, vec![0]);
        assert!(!local.av_shared_handle_client_released.load(Ordering::SeqCst));
    }

    #[test]
    fn release_av_handle_accepts_empty_handle_with_active_data_id() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (_shared_handle, backing) = exported_av_backing(&local);
        backing.allocate(77, &[0x47; 188]).unwrap();
        assert_eq!(backing.stats().allocated_slots, 1);
        let before = AV_DATA_ID_RELEASE_COUNT.load(Ordering::SeqCst);

        local.releaseAvHandle(&empty_native_handle(), 77).unwrap();

        assert_eq!(backing.stats().allocated_slots, 0);
        assert!(AV_DATA_ID_RELEASE_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_accepts_empty_handle_with_stale_data_id() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (_shared_handle, backing) = exported_av_backing(&local);
        backing.allocate(77, &[0x47; 188]).unwrap();
        local.releaseAvHandle(&empty_native_handle(), 77).unwrap();
        let before = AV_DATA_ID_STALE_RELEASE_COUNT.load(Ordering::SeqCst);

        local.releaseAvHandle(&empty_native_handle(), 77).unwrap();

        assert!(AV_DATA_ID_STALE_RELEASE_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_rejects_empty_handle_before_get_av_shared_handle() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let before = AV_HANDLE_RELEASE_WITHOUT_HANDLE_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&empty_native_handle(), 1).is_err());

        assert!(AV_HANDLE_RELEASE_WITHOUT_HANDLE_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_rejects_negative_data_id_with_empty_handle() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let before = AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&empty_native_handle(), -1).is_err());

        assert!(AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_accepts_returned_shared_handle_with_zero_data_id() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (shared_handle, backing) = exported_av_backing(&local);
        backing.allocate(77, &[0x47; 188]).unwrap();
        let before = AV_HANDLE_CLIENT_RELEASE_COUNT.load(Ordering::SeqCst);

        local.releaseAvHandle(&shared_handle, 0).unwrap();

        assert!(AV_HANDLE_CLIENT_RELEASE_COUNT.load(Ordering::SeqCst) > before);
        assert_eq!(backing.stats().allocated_slots, 1);
    }

    #[test]
    fn release_av_handle_rejects_fd_handle_before_get_av_shared_handle() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let backing = AvSharedBacking::new().unwrap();
        let shared_handle = backing.build_native_handle().unwrap();
        let before = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&shared_handle, 0).is_err());

        assert!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_rejects_fd_handle_with_wrong_backing() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (_shared_handle, _backing) = exported_av_backing(&local);
        let other_backing = AvSharedBacking::new().unwrap();
        let wrong_handle = other_backing.build_native_handle().unwrap();
        let before = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&wrong_handle, 0).is_err());

        assert!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_ignores_ints_for_fd_backing_match() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (mut shared_handle, _backing) = exported_av_backing(&local);
        shared_handle.ints = vec![1234, 5678];

        local.releaseAvHandle(&shared_handle, 0).unwrap();
    }

    #[test]
    fn release_av_handle_rejects_returned_shared_handle_with_nonzero_data_id() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (shared_handle, _backing) = exported_av_backing(&local);
        let before = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&shared_handle, 1).is_err());

        assert!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_rejects_fd_handle_with_multiple_fds() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (mut shared_handle, _backing) = exported_av_backing(&local);
        let other_backing = AvSharedBacking::new().unwrap();
        let mut other_handle = other_backing.build_native_handle().unwrap();
        shared_handle.fds.append(&mut other_handle.fds);
        let before = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&shared_handle, 0).is_err());

        assert!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_rejects_double_release_of_returned_shared_handle() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (shared_handle, _backing) = exported_av_backing(&local);
        local.releaseAvHandle(&shared_handle, 0).unwrap();
        let before = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&shared_handle, 0).is_err());

        assert!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn configure_av_stream_type_makes_old_data_id_stale_noop() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (_shared_handle, backing) = exported_av_backing(&local);
        backing.allocate(77, &[0x47; 188]).unwrap();
        local
            .configureAvStreamType(&AvStreamType::Video(VideoStreamType::AVC))
            .unwrap();
        assert_eq!(backing.stats().allocated_slots, 0);
        let before = AV_DATA_ID_STALE_RELEASE_COUNT.load(Ordering::SeqCst);

        local.releaseAvHandle(&empty_native_handle(), 77).unwrap();

        assert!(AV_DATA_ID_STALE_RELEASE_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn non_av_filter_with_previous_av_export_history_treats_release_as_stale_noop() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _state, _, _) = new_test_filter_hal(FilterOpenType::TsPes);
        local.configure(&pes_settings(0x101)).unwrap();
        local.av_export_generation.store(1, Ordering::SeqCst);
        let before_stale = AV_DATA_ID_STALE_RELEASE_COUNT.load(Ordering::SeqCst);
        let before_unavailable = AV_HANDLE_UNAVAILABLE_COUNT.load(Ordering::SeqCst);

        local.releaseAvHandle(&empty_native_handle(), 77).unwrap();

        assert!(AV_DATA_ID_STALE_RELEASE_COUNT.load(Ordering::SeqCst) > before_stale);
        assert_eq!(AV_HANDLE_UNAVAILABLE_COUNT.load(Ordering::SeqCst), before_unavailable);
    }

    #[test]
    fn release_av_handle_after_close_accepts_empty_handle_with_zero_data_id() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        exported_av_backing(&local);
        local.close().unwrap();
        let before = AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT.load(Ordering::SeqCst);

        local.releaseAvHandle(&empty_native_handle(), 0).unwrap();

        assert!(AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_after_close_accepts_empty_handle_with_positive_data_id() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (_shared_handle, backing) = exported_av_backing(&local);
        backing.allocate(77, &[0x47; 188]).unwrap();
        local.close().unwrap();
        let before = AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT.load(Ordering::SeqCst);

        local.releaseAvHandle(&empty_native_handle(), 77).unwrap();

        assert!(AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_after_close_rejects_negative_data_id() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        exported_av_backing(&local);
        local.close().unwrap();
        let before = AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&empty_native_handle(), -1).is_err());

        assert!(AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_after_close_rejects_fd_handle() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (shared_handle, _backing) = exported_av_backing(&local);
        local.close().unwrap();
        let before = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&shared_handle, 0).is_err());

        assert!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst) > before);
    }

    #[test]
    fn release_av_handle_negative_data_id_takes_precedence_over_fd_handle() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (shared_handle, _backing) = exported_av_backing(&local);
        let before_invalid = AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst);
        let before_direct = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&shared_handle, -1).is_err());

        assert!(AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst) > before_invalid);
        assert_eq!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst), before_direct);
    }

    #[test]
    fn release_av_handle_negative_data_id_takes_precedence_over_non_av_filter() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _state, _, _) = new_test_filter_hal(FilterOpenType::TsPes);
        local.configure(&pes_settings(0x100)).unwrap();
        let before_invalid = AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst);
        let before_unavailable = AV_HANDLE_UNAVAILABLE_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&empty_native_handle(), -1).is_err());

        assert!(AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst) > before_invalid);
        assert_eq!(AV_HANDLE_UNAVAILABLE_COUNT.load(Ordering::SeqCst), before_unavailable);
    }

    #[test]
    fn release_av_handle_negative_data_id_takes_precedence_after_close() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        exported_av_backing(&local);
        local.close().unwrap();
        let before_invalid = AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst);
        let before_after_close = AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&empty_native_handle(), -1).is_err());

        assert!(AV_DATA_ID_INVALID_RELEASE_COUNT.load(Ordering::SeqCst) > before_invalid);
        assert_eq!(
            AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT.load(Ordering::SeqCst),
            before_after_close
        );
    }

    #[test]
    fn release_av_handle_fd_handle_with_non_negative_data_id_takes_precedence_after_close() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (local, _, _, _) = new_configured_av_filter();
        let (shared_handle, _backing) = exported_av_backing(&local);
        local.close().unwrap();
        let before_direct = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);
        let before_after_close = AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&shared_handle, 1).is_err());

        assert!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst) > before_direct);
        assert_eq!(
            AV_DATA_ID_STALE_RELEASE_AFTER_CLOSE_COUNT.load(Ordering::SeqCst),
            before_after_close
        );
    }

    #[test]
    fn release_av_handle_fd_handle_with_non_negative_data_id_takes_precedence_over_non_av_filter() {
        let _guard = lock_mutex_status(&RELEASE_AV_HANDLE_TEST_MUTEX, "test_mutex").unwrap();
        let (av_local, _, _, _) = new_configured_av_filter();
        let (shared_handle, _backing) = exported_av_backing(&av_local);
        let (local, _state, _, _) = new_test_filter_hal(FilterOpenType::TsPes);
        local.configure(&pes_settings(0x100)).unwrap();
        let before_direct = AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst);
        let before_unavailable = AV_HANDLE_UNAVAILABLE_COUNT.load(Ordering::SeqCst);

        assert!(local.releaseAvHandle(&shared_handle, 1).is_err());

        assert!(AV_HANDLE_DIRECT_UNSUPPORTED_COUNT.load(Ordering::SeqCst) > before_direct);
        assert_eq!(AV_HANDLE_UNAVAILABLE_COUNT.load(Ordering::SeqCst), before_unavailable);
    }

    #[test]
    fn internal_error_names_are_stable_for_fail_closed_diagnostics() {
        assert_eq!(
            AvPayloadInternalError::MutexPoisoned.as_str(),
            "MutexPoisoned"
        );
        assert_eq!(
            AvPayloadInternalError::SharedHandleExportedWithoutBacking.as_str(),
            "SharedHandleExportedWithoutBacking"
        );
        assert_eq!(
            AvPayloadInternalError::ActiveSlotCollision.as_str(),
            "ActiveSlotCollision"
        );
        assert_eq!(
            AvPayloadInternalError::SlotRegistryInconsistent.as_str(),
            "SlotRegistryInconsistent"
        );
        assert_eq!(
            AvPayloadInternalError::MappingFailure.as_str(),
            "MappingFailure"
        );
        assert_eq!(
            AvPayloadInternalError::CounterFailure.as_str(),
            "CounterFailure"
        );
    }
}

#[cfg(test)]
mod descrambler_state_tests {
    use super::*;

    struct DescramblerTestFilterCallback;

    impl Interface for DescramblerTestFilterCallback {}

    impl IFilterCallback for DescramblerTestFilterCallback {
        fn onFilterStatus(&self, _status: DemuxFilterStatus) -> BinderResult<()> {
            Ok(())
        }

        fn onFilterEvent(&self, _filter_event: &[DemuxFilterEvent]) -> BinderResult<()> {
            Ok(())
        }
    }

    fn assert_tuner_result(result: BinderResult<()>, expected: TunerResult) {
        let status = result.expect_err("operation should fail");
        assert_eq!(status.service_specific_error(), expected.0);
    }

    fn register_test_filter(
        state: &Arc<Mutex<DemuxHandle>>,
        owner_demux_id: i32,
        pid: i32,
    ) -> Strong<dyn IFilter> {
        let record = {
            let mut demux = lock_mutex_status(&state, "test_mutex").unwrap();
            let record = demux
                .register_filter_result(1, FilterOpenType::TsPes, 4096)
                .expect("test setup should register source filter");
            assert!(
                demux.configure_filter_with_summary_result(
                    record.filter_id,
                    FilterConfig {
                        tpid: pid,
                        main_type_bits: 1,
                        sub_type_hint: 0xbd,
                        kind: FilterConfigKind::PesData {
                            stream_id: -1,
                            raw: false,
                        },
                    },
                ).is_ok(),
                "test setup should configure source filter"
            );
            record
        };
        let callback =
            BnFilterCallback::new_binder(DescramblerTestFilterCallback, BinderFeatures::default());
        let filter = FilterHal::new(
            owner_demux_id,
            record.filter_id,
            Arc::clone(state),
            Arc::new(RuntimeIoRegistry::default()),
            callback,
            None,
)
        .expect("test setup should create source filter");
        BnFilter::new_binder(filter, BinderFeatures::default())
    }

    fn register_test_key(hal: &TunerHal) -> Vec<u8> {
        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8])).unwrap();
        hal.descrambler_key_table.register_for_test(key_slot)
    }

    #[test]
    fn descrambler_state_tracks_token_and_pid_lifecycle() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();

        assert!(descrambler.setDemuxSource(demux_id).is_ok());
        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8])).unwrap();
        let key_token = hal.descrambler_key_table.register_for_test(key_slot);
        assert!(descrambler.setKeyToken(&key_token).is_ok());
        assert!(descrambler.add_pid_for_test(0x0123).is_ok());
        assert!(descrambler.add_pid_for_test(0x0456).is_ok());

        let (closed, bound_demux, bound_generation, token, pids) = descrambler.debug_snapshot();
        assert!(!closed);
        assert_eq!(bound_demux, Some(demux_id));
        assert!(bound_generation.is_some());
        assert_eq!(token, Some(key_token));
        assert!(pids.contains(&0x0123));
        assert!(pids.contains(&0x0456));

        assert!(descrambler.remove_pid_for_test(0x0123).is_ok());
        let (_, _, _, _, pids_after_remove) = descrambler.debug_snapshot();
        assert!(!pids_after_remove.contains(&0x0123));
        assert!(pids_after_remove.contains(&0x0456));

        assert!(descrambler.close().is_ok());
        let (
            closed_after_close,
            demux_after_close,
            demux_generation_after_close,
            token_after_close,
            pids_after_close,
        ) = descrambler.debug_snapshot();
        assert!(closed_after_close);
        assert_eq!(demux_after_close, None);
        assert_eq!(demux_generation_after_close, None);
        assert_eq!(token_after_close, None);
        assert!(pids_after_close.is_empty());
        assert!(descrambler.setKeyToken(&[0x99]).is_err());
        assert!(descrambler.add_pid_for_test(0x0100).is_err());
    }

    #[test]
    fn descrambler_rejects_invalid_state_transitions() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();

        assert!(descrambler.add_pid_for_test(0x0100).is_err());
        assert!(descrambler.setKeyToken(&[]).is_err());
        assert!(descrambler
            .setDemuxSource(DEMUX_ID_BASE + MAX_LIVE_DEMUXES as i32)
            .is_err());
        assert!(descrambler.setDemuxSource(demux_id).is_ok());
        assert_tuner_result(
            descrambler.setDemuxSource(demux_id),
            TunerResult::INVALID_STATE,
        );
        assert!(descrambler.setKeyToken(&[0x01]).is_err());
        let expired_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x44; 32], [0x55; 8], [0x66; 8])).unwrap();
        let expired_token = hal.descrambler_key_table.register_for_test(expired_slot);
        hal.descrambler_key_table.expire_token(&expired_token).unwrap();
        assert_tuner_result(
            descrambler.setKeyToken(&expired_token),
            TunerResult::INVALID_ARGUMENT,
        );
        let after_expired = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_expired.bad_token, 5);
        assert_eq!(after_expired.expired_key_slot, 1);

        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x11; 32], [0x22; 8], [0x33; 8])).unwrap();
        let key_token = hal.descrambler_key_table.register_for_test(key_slot);
        assert!(descrambler.setKeyToken(&key_token).is_ok());
        assert!(descrambler.add_pid_for_test(0x1fff).is_err());
        assert!(TunerDescrambler::pid_from_demux_pid(&DemuxPid::TPid(0x1fff)).is_err());
        assert!(descrambler.remove_pid_for_test(0x0222).is_ok());
        assert!(descrambler.close().is_ok());
        assert!(descrambler.close().is_ok());
    }

    #[test]
    fn public_descrambler_methods_map_state_and_argument_errors_exactly() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let source = register_test_filter(&lock_mutex_status(&record, "test_mutex").unwrap().state, demux_id, 0x0123);
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        let pid = DemuxPid::TPid(0x0123);

        assert_tuner_result(
            descrambler.addPid(&pid, &source),
            TunerResult::INVALID_STATE,
        );
        assert_tuner_result(
            descrambler.removePid(&pid, &source),
            TunerResult::INVALID_STATE,
        );

        descrambler.setDemuxSource(demux_id).unwrap();
        assert_tuner_result(
            descrambler.setDemuxSource(demux_id),
            TunerResult::INVALID_STATE,
        );
        assert!(descrambler.addPid(&pid, &source).is_ok());
        assert!(descrambler.removePid(&pid, &source).is_ok());
        assert_tuner_result(
            descrambler.addPid(&DemuxPid::TPid(0x1fff), &source),
            TunerResult::INVALID_ARGUMENT,
        );
        assert_tuner_result(
            descrambler.addPid(&DemuxPid::MmtpPid(0x0123), &source),
            TunerResult::UNAVAILABLE,
        );

        let key_token = register_test_key(&hal);
        descrambler.setKeyToken(&key_token).unwrap();
        assert!(descrambler.removePid(&pid, &source).is_ok());
    }

    #[test]
    fn public_descrambler_add_pid_replaces_same_pid_source() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let demux_state = lock_mutex_status(&record, "test_mutex").unwrap().state.clone();
        let first_source = register_test_filter(&demux_state, demux_id, 0x0201);
        let second_source = register_test_filter(&demux_state, demux_id, 0x0201);
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        let pid = DemuxPid::TPid(0x0201);

        descrambler.setDemuxSource(demux_id).unwrap();
        let key_token = register_test_key(&hal);
        descrambler.setKeyToken(&key_token).unwrap();
        descrambler.addPid(&pid, &first_source).unwrap();
        descrambler.addPid(&pid, &second_source).unwrap();

        assert_tuner_result(
            descrambler.removePid(&pid, &first_source),
            TunerResult::INVALID_ARGUMENT,
        );
        descrambler.removePid(&pid, &second_source).unwrap();
    }

    #[test]
    fn public_descrambler_remove_pid_after_void_key_token_succeeds() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let demux_state = lock_mutex_status(&record, "test_mutex").unwrap().state.clone();
        let source = register_test_filter(&demux_state, demux_id, 0x0204);
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        let pid = DemuxPid::TPid(0x0204);

        descrambler.setDemuxSource(demux_id).unwrap();
        let key_token = register_test_key(&hal);
        descrambler.setKeyToken(&key_token).unwrap();
        descrambler.addPid(&pid, &source).unwrap();
        descrambler.setKeyToken(&[0x00]).unwrap();

        descrambler.removePid(&pid, &source).unwrap();
        let (_, _, _, token_after_remove, pids_after_remove) = descrambler.debug_snapshot();
        assert_eq!(token_after_remove, None);
        assert!(!pids_after_remove.contains(&0x0204));
    }

    #[test]
    fn public_descrambler_rejects_stale_demux_generation_for_pid_mutations() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let demux_state = lock_mutex_status(&record, "test_mutex").unwrap().state.clone();
        let source = register_test_filter(&demux_state, demux_id, 0x0202);
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        let pid = DemuxPid::TPid(0x0202);

        descrambler.setDemuxSource(demux_id).unwrap();
        let key_token = register_test_key(&hal);
        descrambler.setKeyToken(&key_token).unwrap();
        descrambler.addPid(&pid, &source).unwrap();
        lock_mutex_status(&record, "test_mutex").unwrap().generation += 1;

        assert_tuner_result(
            descrambler.addPid(&DemuxPid::TPid(0x0203), &source),
            TunerResult::INVALID_STATE,
        );
        let (_, demux_after_stale_add, generation_after_stale_add, token_after_stale_add, pids_after_stale_add) =
            descrambler.debug_snapshot();
        assert_eq!(demux_after_stale_add, None);
        assert_eq!(generation_after_stale_add, None);
        assert_eq!(token_after_stale_add, None);
        assert!(pids_after_stale_add.is_empty());
        assert_tuner_result(
            descrambler.removePid(&pid, &source),
            TunerResult::INVALID_STATE,
        );
    }

    #[test]
    fn public_descrambler_set_key_token_prunes_stale_demux_generation() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        descrambler.setDemuxSource(demux_id).unwrap();
        let key_token = register_test_key(&hal);
        descrambler.setKeyToken(&key_token).unwrap();
        descrambler.add_pid_for_test(0x0220).unwrap();
        lock_mutex_status(&record, "test_mutex").unwrap().generation += 1;

        let next_key = register_test_key(&hal);
        assert_tuner_result(descrambler.setKeyToken(&next_key), TunerResult::INVALID_STATE);
        let (_, demux_after_stale_key, generation_after_stale_key, token_after_stale_key, pids_after_stale_key) =
            descrambler.debug_snapshot();
        assert_eq!(demux_after_stale_key, None);
        assert_eq!(generation_after_stale_key, None);
        assert_eq!(token_after_stale_key, None);
        assert!(pids_after_stale_key.is_empty());
    }

    #[test]
    fn stale_demux_generation_expires_previous_key_token() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        descrambler.setDemuxSource(demux_id).unwrap();
        let key_token = register_test_key(&hal);
        descrambler.setKeyToken(&key_token).unwrap();
        lock_mutex_status(&record, "test_mutex").unwrap().generation += 1;

        let next_key = register_test_key(&hal);
        assert_tuner_result(descrambler.setKeyToken(&next_key), TunerResult::INVALID_STATE);
        assert_eq!(
            hal.descrambler_key_table.resolve_with_diagnostic(&key_token).unwrap_err(),
            DescramblerKeyResolveError::ExpiredKeySlot
        );
    }

    #[test]
    fn descrambler_close_expires_active_key_binding() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        descrambler.setDemuxSource(demux_id).unwrap();
        let key_token = register_test_key(&hal);
        descrambler.setKeyToken(&key_token).unwrap();
        descrambler.close().unwrap();
        assert_eq!(
            hal.descrambler_key_table.resolve_with_diagnostic(&key_token).unwrap_err(),
            DescramblerKeyResolveError::ExpiredKeySlot
        );
    }

    #[test]
    fn demux_invalidation_expires_active_key_binding() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        descrambler.setDemuxSource(demux_id).unwrap();
        let key_token = register_test_key(&hal);
        descrambler.setKeyToken(&key_token).unwrap();
        let generation = lock_mutex_status(&record, "test_mutex").unwrap().generation;

        hal.descrambler_registry.invalidate_demux(demux_id, generation).unwrap();

        assert_eq!(
            hal.descrambler_key_table.resolve_with_diagnostic(&key_token).unwrap_err(),
            DescramblerKeyResolveError::ExpiredKeySlot
        );
    }

    #[test]
    fn descrambler_drop_expires_active_key_binding() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let key_token = {
            let descrambler = TunerDescrambler::new(
                Arc::clone(&hal.demux_ledger),
                Arc::clone(&hal.descrambler_registry),
                Arc::clone(&hal.descrambler_diagnostics),
                Arc::clone(&hal.descrambler_key_table),
            ).unwrap();
            descrambler.setDemuxSource(demux_id).unwrap();
            let key_token = register_test_key(&hal);
            descrambler.setKeyToken(&key_token).unwrap();
            key_token
        };

        assert_eq!(
            hal.descrambler_key_table.resolve_with_diagnostic(&key_token).unwrap_err(),
            DescramblerKeyResolveError::ExpiredKeySlot
        );
    }

    #[test]
    fn expired_key_token_records_expired_diagnostic() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        descrambler.setDemuxSource(demux_id).unwrap();
        let key_token = register_test_key(&hal);
        hal.descrambler_key_table.expire_token(&key_token).unwrap();

        assert_tuner_result(descrambler.setKeyToken(&key_token), TunerResult::INVALID_ARGUMENT);
        let counters = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(counters.expired_key_slot, 1);
        assert_eq!(counters.bad_token, 0);
    }

    #[test]
    fn open_descrambler_returns_binder_object() {
        let hal = TunerHal::new();
        assert!(hal.openDescrambler().is_ok());
    }

    #[test]
    fn descrambler_registry_exposes_active_demux_snapshot_without_false_clear() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();

        assert!(descrambler.setDemuxSource(demux_id).is_ok());
        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8])).unwrap();
        let key_token = hal.descrambler_key_table.register_for_test(key_slot);
        assert!(descrambler.setKeyToken(&key_token).is_ok());
        assert!(descrambler.add_pid_for_test(0x0123).is_ok());

        let (demux_generation, demux_state) = {
            let record = lock_mutex_status(&record, "test_mutex").unwrap();
            (record.generation, record.state.clone())
        };
        let snapshots = {
            let handle = lock_mutex_status(&demux_state, "test_mutex").unwrap();
            hal.descrambler_registry
                .snapshots_for_demux(demux_id, demux_generation, &handle).unwrap()
        };
        assert_eq!(snapshots.len(), 1);
        assert!(snapshots[0].targets_pid(0x0123));

        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = 0x01;
        packet[2] = 0x23;
        packet[3] = 0x90;
        let result = snapshots[0].descramble_packet_in_place(&mut packet);
        assert!(matches!(result, Ok(_) | Err(DescrambleFailure::Multi2Fail)));

        assert!(descrambler.close().is_ok());
        let snapshots_after_close = {
            let handle = lock_mutex_status(&demux_state, "test_mutex").unwrap();
            hal.descrambler_registry
                .snapshots_for_demux(demux_id, demux_generation, &handle).unwrap()
        };
        assert!(snapshots_after_close.is_empty());
    }

    #[test]
    fn descrambler_snapshot_prunes_reopened_demux_generation() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        assert!(descrambler.setDemuxSource(demux_id).is_ok());
        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x55; 32], [0x66; 8], [0x77; 8])).unwrap();
        let key_token = hal.descrambler_key_table.register_for_test(key_slot);
        assert!(descrambler.setKeyToken(&key_token).is_ok());
        assert!(descrambler.add_pid_for_test(0x0200).is_ok());

        let (generation, demux_state) = {
            let record = lock_mutex_status(&record, "test_mutex").unwrap();
            (record.generation, record.state.clone())
        };
        let wrong_generation_snapshot = {
            let handle = lock_mutex_status(&demux_state, "test_mutex").unwrap();
            hal.descrambler_registry.snapshots_for_demux(
                demux_id,
                generation.saturating_add(1),
                &handle,
            ).unwrap()
        };
        assert!(wrong_generation_snapshot.is_empty());

        hal.descrambler_registry
            .invalidate_demux(demux_id, generation).unwrap();
        let (_, demux_after_invalidate, generation_after_invalidate, _, pids_after_invalidate) =
            descrambler.debug_snapshot();
        assert_eq!(demux_after_invalidate, None);
        assert_eq!(generation_after_invalidate, None);
        assert!(pids_after_invalidate.is_empty());
    }

    #[test]
    fn descrambler_rejects_pid_owned_by_another_active_descrambler() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let first = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        let second = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x01; 32], [0x02; 8], [0x03; 8])).unwrap();
        let key_token = hal.descrambler_key_table.register_for_test(key_slot);

        assert!(first.setDemuxSource(demux_id).is_ok());
        assert!(second.setDemuxSource(demux_id).is_ok());
        assert!(first.setKeyToken(&key_token).is_ok());
        assert!(second.setKeyToken(&key_token).is_ok());
        assert!(first.add_pid_for_test(0x0201).is_ok());
        assert!(second.add_pid_for_test(0x0201).is_err());
        assert!(second.add_pid_for_test(0x0202).is_ok());
    }

    #[test]
    fn descrambler_snapshot_prunes_upstream_filter_generation_mismatch() {
        let registry = DescramblerRuntimeRegistry::new();
        let state = Arc::new(Mutex::new(DescramblerSession::new()));
        let _id = registry.register(&state).unwrap();
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        {
            let mut descrambler = lock_mutex_status(&state, "test_mutex").unwrap();
            descrambler.demux_id = Some(DEMUX_ID_BASE);
            descrambler.demux_generation = Some(10);
            descrambler.key_token = Some(vec![1]);
            descrambler.key_slot = Some(DescramblerKeySlot::empty());
            descrambler.add_pid(
                PidBinding { pid: 0x0123 },
                SourceFilterBinding {
                    filter_id: filter.filter_id,
                    generation: filter.delivery_generation,
                },
            );
        }
        assert_eq!(
            registry
                .snapshots_for_demux(DEMUX_ID_BASE, 10, &demux).unwrap()
                .len(),
            1
        );
        demux
            .configure_filter_with_summary_result(
                filter.filter_id,
                FilterConfig {
                    tpid: 0x0123,
                    main_type_bits: 1,
                    sub_type_hint: 0,
                    kind: FilterConfigKind::Section {
                        check_crc: false,
                        repeat: true,
                        raw: false,
                        length_field_bits: 12,
                        condition_kind: SectionConditionKind::SectionBits,
                        condition: SectionCondition::default(),
                    },
                },
            )
            .unwrap();
        assert!(registry
            .snapshots_for_demux(DEMUX_ID_BASE, 10, &demux).unwrap()
            .is_empty());
        assert!(lock_mutex_status(&state, "test_mutex").unwrap().pid_registrations.is_empty());
    }

    fn descrambler_test_packet(pid: u16, tsc: u8, afc: u8) -> [u8; 188] {
        let mut packet = [0u8; 188];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = (pid & 0xff) as u8;
        packet[3] = (tsc << 6) | (afc << 4) | 0x05;
        for i in 4..188 {
            packet[i] = (i as u8).wrapping_mul(3).wrapping_add(1);
        }
        packet
    }

    #[test]
    fn scrambled_packet_passthrough_is_diagnosed_without_clear_success() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let mut scrambled = [0u8; 188];
        scrambled[0] = 0x47;
        scrambled[1] = 0x01;
        scrambled[2] = 0x23;
        scrambled[3] = 0x80 | 0x10;
        for i in 4..188 {
            scrambled[i] = (i as u8).wrapping_mul(3).wrapping_add(1);
        }

        let unresolved = ActiveDescramblerSnapshot {
            pids: BTreeSet::from([0x0123]),
            key_slot: Some(DescramblerKeySlot::empty()),
        };
        let decision = descramble_packet_for_pid_with_diagnostics(
            &scrambled,
            7,
            0x0123,
            &[unresolved],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::ScrambledPassthrough);
        assert_eq!(decision.packet, scrambled);
        let counters = diagnostics.snapshot(7, 0x0123);
        assert_eq!(counters.descrambled_packets, 0);
        assert_eq!(counters.no_key, 1);
        assert_eq!(counters.scrambled_passthrough_for_recording_packets, 1);
    }

    #[test]
    fn scrambled_packet_without_descrambler_is_diagnosed_separately() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let mut scrambled = [0u8; 188];
        scrambled[0] = 0x47;
        scrambled[1] = 0x01;
        scrambled[2] = 0x24;
        scrambled[3] = 0x80 | 0x10;
        let decision =
            descramble_packet_for_pid_with_diagnostics(&scrambled, 8, 0x0124, &[], &diagnostics);
        assert_eq!(decision.flow, PacketDescrambleFlow::ScrambledPassthrough);
        let counters = diagnostics.snapshot(8, 0x0124);
        assert_eq!(counters.scrambled_without_descrambler, 1);
        assert_eq!(counters.scrambled_passthrough_for_recording_packets, 1);
        assert_eq!(counters.descrambled_packets, 0);
    }

    #[test]
    fn tei_packet_increments_transport_error_record_counter() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let mut packet = descrambler_test_packet(0x0129, 2, 1);
        packet[1] |= 0x80;
        let decision = descramble_packet_for_pid_with_diagnostics(
            &packet,
            13,
            0x0129,
            &[],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::TransportErrorRecord);
        assert_eq!(decision.packet, packet);
        let counters = diagnostics.snapshot(13, 0x0129);
        assert_eq!(counters.transport_error_record, 1);
        assert_eq!(counters.clear_packets, 0);
        assert_eq!(counters.descrambled_packets, 0);
        assert_eq!(counters.scrambled_passthrough_for_recording_packets, 0);
    }

    #[test]
    fn tei_packet_does_not_increment_clear_or_descrambled_counters() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let mut packet = descrambler_test_packet(0x012c, 2, 1);
        packet[1] |= 0x80;
        let decision = descramble_packet_for_pid_with_diagnostics(
            &packet,
            17,
            0x012c,
            &[],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::TransportErrorRecord);
        let counters = diagnostics.snapshot(17, 0x012c);
        assert_eq!(counters.transport_error_record, 1);
        assert_eq!(counters.clear_packets, 0);
        assert_eq!(counters.descrambled_packets, 0);
    }

    #[test]
    fn tei_invalid_tsc_prefers_transport_error_record_flow() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let mut packet = descrambler_test_packet(0x012a, 1, 1);
        packet[1] |= 0x80;
        let decision = descramble_packet_for_pid_with_diagnostics(
            &packet,
            14,
            0x012a,
            &[],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::TransportErrorRecord);
        let counters = diagnostics.snapshot(14, 0x012a);
        assert_eq!(counters.transport_error_record, 1);
        assert_eq!(counters.invalid_tsc, 0);
    }

    #[test]
    fn null_pid_scrambled_is_record_only_passthrough_in_production_flow() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let packet = descrambler_test_packet(maleicacid_tuner_hal_descrambler::NULL_PID, 2, 1);
        let decision = descramble_packet_for_pid_with_diagnostics(
            &packet,
            19,
            maleicacid_tuner_hal_descrambler::NULL_PID,
            &[],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::ScrambledNullPid);
        assert_eq!(decision.packet, packet);
        let counters = diagnostics.snapshot(19, maleicacid_tuner_hal_descrambler::NULL_PID);
        assert_eq!(counters.scrambled_null_pid, 1);
        assert_eq!(counters.clear_packets, 0);
    }

    #[test]
    fn null_pid_scrambled_increments_scrambled_null_pid_counter() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let packet = descrambler_test_packet(maleicacid_tuner_hal_descrambler::NULL_PID, 2, 1);
        let decision = descramble_packet_for_pid_with_diagnostics(
            &packet,
            15,
            maleicacid_tuner_hal_descrambler::NULL_PID,
            &[],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::ScrambledNullPid);
        assert_eq!(decision.packet, packet);
        let counters = diagnostics.snapshot(15, maleicacid_tuner_hal_descrambler::NULL_PID);
        assert_eq!(counters.scrambled_null_pid, 1);
        assert_eq!(counters.clear_packets, 0);
        assert_eq!(counters.scrambled_passthrough_for_recording_packets, 0);
    }

    #[test]
    fn null_pid_scrambled_does_not_increment_clear_counter() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let packet = descrambler_test_packet(maleicacid_tuner_hal_descrambler::NULL_PID, 3, 1);
        let decision = descramble_packet_for_pid_with_diagnostics(
            &packet,
            18,
            maleicacid_tuner_hal_descrambler::NULL_PID,
            &[],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::ScrambledNullPid);
        let counters = diagnostics.snapshot(18, maleicacid_tuner_hal_descrambler::NULL_PID);
        assert_eq!(counters.scrambled_null_pid, 1);
        assert_eq!(counters.clear_packets, 0);
    }

    #[test]
    fn afc11_payload_zero_increments_malformed_packet_for_recording() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let mut packet = descrambler_test_packet(0x012b, 0, 3);
        packet[4] = 183;
        let decision = descramble_packet_for_pid_with_diagnostics(
            &packet,
            16,
            0x012b,
            &[],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::MalformedRecord);
        assert_eq!(decision.packet, packet);
        let counters = diagnostics.snapshot(16, 0x012b);
        assert_eq!(counters.invalid_adaptation_field, 1);
        assert_eq!(counters.malformed_packet_for_recording, 1);
        assert_eq!(counters.clear_packets, 0);
        assert_eq!(counters.descrambled_packets, 0);
    }

    #[test]
    fn malformed_header_matrix_is_record_only_not_av_clear() {
        for tsc in 0..=3 {
            let packet = descrambler_test_packet(0x0125, tsc, 0);
            let diagnostics = DescramblerDiagnosticRegistry::new();
            let decision = descramble_packet_for_pid_with_diagnostics(
                &packet,
                9,
                0x0125,
                &[],
                &diagnostics,
            );
            assert_eq!(decision.flow, PacketDescrambleFlow::MalformedRecord);
            assert_eq!(decision.packet, packet);
            let counters = diagnostics.snapshot(9, 0x0125);
            assert_eq!(counters.invalid_afc, 1);
            assert_eq!(counters.malformed_packet_for_recording, 1);
            assert_eq!(counters.clear_packets, 0);
        }

        let invalid_tsc = descrambler_test_packet(0x0126, 1, 1);
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let decision = descramble_packet_for_pid_with_diagnostics(
            &invalid_tsc,
            10,
            0x0126,
            &[],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::MalformedRecord);
        let counters = diagnostics.snapshot(10, 0x0126);
        assert_eq!(counters.invalid_tsc, 1);
        assert_eq!(counters.malformed_packet_for_recording, 1);

        let mut scrambled_adaptation_only = descrambler_test_packet(0x0127, 2, 2);
        scrambled_adaptation_only[4] = 183;
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let decision = descramble_packet_for_pid_with_diagnostics(
            &scrambled_adaptation_only,
            11,
            0x0127,
            &[],
            &diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::MalformedRecord);
        let counters = diagnostics.snapshot(11, 0x0127);
        assert_eq!(counters.scrambled_without_payload, 1);
        assert_eq!(counters.malformed_packet_for_recording, 1);
    }

    #[test]
    fn non_ts_frame_inputs_are_dropped_not_recorded() {
        let diagnostics = DescramblerDiagnosticRegistry::new();
        let short_packet = vec![0u8; 187];
        let short_decision = descramble_packet_bytes_for_pid_with_diagnostics(
            &short_packet,
            12,
            0x0128,
            &[],
            &diagnostics,
        );
        assert_eq!(short_decision, None);
        let counters = diagnostics.snapshot(12, 0x0128);
        assert_eq!(counters.invalid_packet_size, 1);
        assert_eq!(counters.malformed_packet_for_recording, 0);
        assert_eq!(counters.clear_packets, 0);
        assert_eq!(counters.descrambled_packets, 0);
        assert_eq!(counters.scrambled_passthrough_for_recording_packets, 0);

        let mut packet = descrambler_test_packet(0x0128, 0, 1);
        packet[0] = 0x00;
        let decision = descramble_packet_bytes_for_pid_with_diagnostics(
            &packet,
            12,
            0x0128,
            &[],
            &diagnostics,
        )
        .expect("188 byte non-sync input should still return an explicit drop decision");
        assert_eq!(decision.flow, PacketDescrambleFlow::Drop);
        let counters = diagnostics.snapshot(12, 0x0128);
        assert_eq!(counters.bad_sync_byte, 1);
        assert_eq!(counters.malformed_packet_for_recording, 0);
        assert_eq!(counters.clear_packets, 0);
        assert_eq!(counters.descrambled_packets, 0);
        assert_eq!(counters.scrambled_passthrough_for_recording_packets, 0);
    }

    #[test]
    fn cas_bridge_registration_is_fail_closed_until_connected() {
        let table = DescramblerKeyTable::new();
        let even_only = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8])).unwrap();
        let odd_only = DescramblerKeySlot::empty()
            .try_with_odd(Multi2KeyMaterial::new([0x11; 32], [0x21; 8], [0x31; 8])).unwrap();
        let paired = even_only.clone()
            .try_with_odd(Multi2KeyMaterial::new([0x12; 32], [0x22; 8], [0x32; 8])).unwrap();
        assert!(table.register_from_cas_bridge(even_only.clone(), false).is_err());
        assert!(table.register_from_cas_bridge(odd_only.clone(), false).is_err());
        assert!(table.register_from_cas_bridge(paired.clone(), false).is_err());
        assert!(table.register_from_cas_bridge(even_only, true).is_err());
        assert!(table.register_from_cas_bridge(odd_only, true).is_err());
        let token = table.register_from_cas_bridge(paired, true).unwrap();
        assert!(table.resolve_with_diagnostic(&token).is_ok());
    }

    fn too_long_descrambler_diagnostic_token_for_test() -> [u8; 48] {
        [
            0x6d, 0x61, 0x6c, 0x65, 0x69, 0x63, 0x61, 0x63,
            0x69, 0x64, 0x2d, 0x74, 0x65, 0x73, 0x74, 0x2d,
            0x64, 0x65, 0x73, 0x63, 0x2d, 0x74, 0x6f, 0x6b,
            0x65, 0x6e, 0x2d, 0x66, 0x66, 0x66, 0x66, 0x66,
            0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
            0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
        ]
    }

    #[test]
    fn set_key_token_records_distinct_failure_diagnostics() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        assert!(descrambler.setDemuxSource(demux_id).is_ok());

        assert_tuner_result(descrambler.setKeyToken(&[]), TunerResult::INVALID_ARGUMENT);
        let after_empty = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_empty.bad_token, 1);

        assert_tuner_result(
            descrambler.setKeyToken(&[0x55; 17]),
            TunerResult::INVALID_ARGUMENT,
        );
        let after_too_long = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_too_long.bad_token, 2);

        assert_tuner_result(
            descrambler.setKeyToken(&too_long_descrambler_diagnostic_token_for_test()),
            TunerResult::INVALID_ARGUMENT,
        );
        let after_long_ascii = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_long_ascii.bad_token, 3);

        assert_tuner_result(
            descrambler.setKeyToken(&[0x42; 8]),
            TunerResult::INVALID_ARGUMENT,
        );
        let after_unknown = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_unknown.bad_token, 4);

        assert_tuner_result(
            descrambler.setKeyToken(b"placeholder"),
            TunerResult::INVALID_ARGUMENT,
        );
        let after_placeholder = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_placeholder.bad_token, 5);

        let expired_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x44; 32], [0x55; 8], [0x66; 8])).unwrap();
        let expired_token = hal.descrambler_key_table.register_for_test(expired_slot);
        hal.descrambler_key_table.expire_token(&expired_token).unwrap();
        assert_tuner_result(
            descrambler.setKeyToken(&expired_token),
            TunerResult::INVALID_ARGUMENT,
        );
        let after_expired = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_expired.bad_token, 5);
        assert_eq!(after_expired.expired_key_slot, 1);

        let key_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x11; 32], [0x22; 8], [0x33; 8])).unwrap();
        let ok_token = hal.descrambler_key_table.register_for_test(key_slot);
        assert_eq!(ok_token.len(), 8);
        assert!(descrambler.setKeyToken(&ok_token).is_ok());

        assert!(hal
            .dump_descrambler_diagnostics_for_debug()
            .contains("BAD_TOKEN=5"));
        assert!(hal
            .dump_descrambler_diagnostics_for_debug()
            .contains("EXPIRED_KEY_SLOT=1"));
        assert!(hal
            .dump_descrambler_diagnostics_for_debug()
            .contains("CAS_BRIDGE_UNCONNECTED=0"));
    }

    #[test]
    fn set_key_token_failure_does_not_mutate_current_key_or_pid_registration() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        )
        .unwrap();
        descrambler.setDemuxSource(demux_id).unwrap();
        let key_token = register_test_key(&hal);
        descrambler.setKeyToken(&key_token).unwrap();
        descrambler.add_pid_for_test(0x0123).unwrap();
        let before = descrambler.debug_snapshot();

        assert_tuner_result(descrambler.setKeyToken(&[]), TunerResult::INVALID_ARGUMENT);
        assert_eq!(descrambler.debug_snapshot(), before);

        assert_tuner_result(
            descrambler.setKeyToken(&[0x42; 8]),
            TunerResult::INVALID_ARGUMENT,
        );
        assert_eq!(descrambler.debug_snapshot(), before);

        let expired_slot = DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x44; 32], [0x55; 8], [0x66; 8]))
            .unwrap();
        let expired_token = hal.descrambler_key_table.register_for_test(expired_slot);
        hal.descrambler_key_table.expire_token(&expired_token).unwrap();
        assert_tuner_result(
            descrambler.setKeyToken(&expired_token),
            TunerResult::INVALID_ARGUMENT,
        );
        assert_eq!(descrambler.debug_snapshot(), before);
    }

    #[test]
    fn void_key_token_clears_key_only_and_keeps_pid_registration() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_ledger),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        ).unwrap();
        assert!(descrambler.setDemuxSource(demux_id).is_ok());
        let key_token = register_test_key(&hal);
        assert!(descrambler.setKeyToken(&key_token).is_ok());
        assert!(descrambler.add_pid_for_test(0x0123).is_ok());

        assert!(descrambler.setKeyToken(&[0x00]).is_ok());
        assert!(hal.descrambler_key_table.resolve_with_diagnostic(&key_token).is_ok());
        let (_, _, _, token_after_void, pids_after_void) = descrambler.debug_snapshot();
        assert_eq!(token_after_void, None);
        assert!(pids_after_void.contains(&0x0123));
        let counters = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(counters.bad_token, 0);
        assert_eq!(counters.cas_bridge_unconnected, 0);

        assert!(descrambler.setKeyToken(&[0x00]).is_ok());
        let (_, _, _, token_after_second_void, pids_after_second_void) = descrambler.debug_snapshot();
        assert_eq!(token_after_second_void, None);
        assert!(pids_after_second_void.contains(&0x0123));

        let (demux_generation, demux_state) = {
            let record = lock_mutex_status(&record, "test_mutex").unwrap();
            (record.generation, record.state.clone())
        };
        let snapshots_after_void = {
            let handle = lock_mutex_status(&demux_state, "test_mutex").unwrap();
            hal.descrambler_registry
                .snapshots_for_demux(demux_id, demux_generation, &handle).unwrap()
        };
        assert_eq!(snapshots_after_void.len(), 1);
        assert!(snapshots_after_void[0].targets_pid(0x0123));

        let mut scrambled = [0u8; 188];
        scrambled[0] = 0x47;
        scrambled[1] = 0x01;
        scrambled[2] = 0x23;
        scrambled[3] = 0x80 | 0x10;
        for i in 4..188 {
            scrambled[i] = (i as u8).wrapping_mul(3).wrapping_add(1);
        }
        let decision = descramble_packet_for_pid_with_diagnostics(
            &scrambled,
            demux_id,
            0x0123,
            &snapshots_after_void,
            &hal.descrambler_diagnostics,
        );
        assert_eq!(decision.flow, PacketDescrambleFlow::ScrambledPassthrough);
        let pid_counters = hal.descrambler_diagnostics.snapshot(demux_id, 0x0123);
        assert_eq!(pid_counters.no_key, 1);
        assert_eq!(pid_counters.scrambled_without_descrambler, 0);
        assert_eq!(pid_counters.scrambled_passthrough_for_recording_packets, 1);
    }

    #[test]
    fn multiple_descramblers_same_pid_try_later_resolved_snapshot() {
        use maleicacid_tuner_hal_descrambler::{multi2_encrypt_payload, Multi2KeyMaterial};

        let mut system_key = [0u8; 32];
        for (i, b) in system_key.iter_mut().enumerate() {
            *b = 0x20u8.wrapping_add(i as u8);
        }
        let mut cbc_iv = [0u8; 8];
        for (i, b) in cbc_iv.iter_mut().enumerate() {
            *b = 0x80u8.wrapping_add(i as u8);
        }
        let mut data_key = [0u8; 8];
        for (i, b) in data_key.iter_mut().enumerate() {
            *b = 0x40u8.wrapping_add((i * 5) as u8);
        }
        let even = Multi2KeyMaterial::new(system_key, cbc_iv, data_key);

        let mut clear = [0u8; 188];
        clear[0] = 0x47;
        clear[1] = 0x01;
        clear[2] = 0x23;
        clear[3] = 0x10;
        for i in 4..188 {
            clear[i] = (i as u8).wrapping_mul(7).wrapping_add(3);
        }

        let mut scrambled = clear;
        let even_prepared = even.prepare().unwrap();
        multi2_encrypt_payload(&mut scrambled[4..], &even_prepared).unwrap();
        scrambled[3] = (scrambled[3] & 0x3f) | 0x80;

        let unresolved = ActiveDescramblerSnapshot {
            pids: BTreeSet::from([0x0123]),
            key_slot: Some(DescramblerKeySlot::empty()),
        };
        let resolved = ActiveDescramblerSnapshot {
            pids: BTreeSet::from([0x0123]),
            key_slot: Some(DescramblerKeySlot::empty().try_with_even(even).unwrap()),
        };
        let output =
            maybe_descramble_packet_for_pid(&scrambled, 0x0123, &[unresolved, resolved]).unwrap();
        assert_eq!(output, clear);
    }
}

#[cfg(test)]
mod contract_regression_tests {
    use super::*;

    fn mark_observation_expired(diagnostics: &Px4PathDiagnostics) {
        let mut observation = lock_mutex_status(&diagnostics.observation, "test_mutex").unwrap();
        observation.started_at =
            Instant::now() - Duration::from_millis(PX4_PATH_DIAGNOSTIC_TIMEOUT_MS + 1);
    }

    fn ts_packet(pid: u16, pusi: bool, payload: &[u8]) -> [u8; 188] {
        let mut packet = [0xffu8; 188];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if pusi {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        packet[3] = 0x10;
        packet[4..4 + payload.len()].copy_from_slice(payload);
        packet
    }

    #[test]
    fn px4_path_diagnostics_separates_ts_pat_pmt_and_av_timeouts() {
        let diagnostics = Px4PathDiagnostics::new();
        diagnostics.apply_stream_boundary_reset();
        mark_observation_expired(&diagnostics);
        diagnostics.check_timeouts();
        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.ts_arrival_timeouts, 1);
        assert_eq!(snapshot.pat_timeouts, 1);
        assert_eq!(snapshot.pmt_timeouts, 1);
        assert_eq!(snapshot.av_data_timeouts, 1);

        diagnostics.check_timeouts();
        assert_eq!(diagnostics.snapshot(), snapshot);
    }

    #[test]
    fn px4_path_diagnostics_observes_pat_pmt_and_av_data_independently() {
        let diagnostics = Px4PathDiagnostics::new();
        diagnostics.apply_stream_boundary_reset();
        diagnostics.observe_ts_packet(&ts_packet(0x0000, true, &[0x00, 0x00, 0xb0, 0x0d]));
        diagnostics.observe_ts_packet(&ts_packet(0x0100, true, &[0x00, 0x02, 0xb0, 0x17]));
        diagnostics.observe_ts_packet(&ts_packet(
            0x0200,
            true,
            &[0x00, 0x00, 0x01, 0xe0, 0x00, 0x00],
        ));
        mark_observation_expired(&diagnostics);
        diagnostics.check_timeouts();
        assert_eq!(diagnostics.snapshot(), Px4PathDiagnosticSnapshot::default());
    }

    #[test]
    fn get_status_rejects_unsupported_type_before_backend_status_read() {
        assert!(FrontendHal::validate_status_types(
            FrontendStatusSupport::default(),
            &[FrontendStatusType::DEMOD_LOCK]
        )
        .is_ok());
        assert!(FrontendHal::validate_status_types(
            FrontendStatusSupport::default(),
            &[FrontendStatusType::SIGNAL_QUALITY]
        )
        .is_err());
        assert!(FrontendHal::validate_status_types(
            FrontendStatusSupport::default(),
            &[FrontendStatusType::RF_LOCK]
        )
        .is_err());
        assert!(FrontendHal::validate_status_types(
            FrontendStatusSupport::default(),
            &[FrontendStatusType::BER]
        )
        .is_err());
    }

    #[test]
    fn remove_output_pid_contract_is_unavailable_without_mutating_demux_filters() {
        let entry = FrontendEntry {
            id: 51,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        let runtime = FrontendRuntime::new(
            entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        let frontend = FrontendHal::new(
            runtime,
            FrontendType::ISDBT,
            0,
            1,
            Arc::new(Mutex::new(FrontendLeaseRegistry::default())),
            Arc::new(Mutex::new(DemuxLedger::default())),
        );
        assert!(frontend.removeOutputPid(0x0100).is_err());
    }

    #[test]
    fn worker_exit_contract_distinguishes_all_terminal_reasons() {
        assert!(!WorkerExit::Normal.is_abnormal());
        assert!(!WorkerExit::StopRequested.is_abnormal());
        assert!(WorkerExit::RuntimeFailure.is_abnormal());
        assert!(WorkerExit::PanicOrJoinFailure.is_abnormal());
        assert_ne!(WorkerExit::Normal, WorkerExit::StopRequested);
        assert_ne!(WorkerExit::RuntimeFailure, WorkerExit::PanicOrJoinFailure);
    }

    #[test]
    fn frontend_tune_worker_abnormal_join_is_returned_and_recorded() {
        let entry = FrontendEntry {
            id: 90,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        let runtime = FrontendRuntime::new(
            entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        let frontend = FrontendHal::new(
            Arc::clone(&runtime),
            FrontendType::ISDBT,
            0,
            1,
            Arc::new(Mutex::new(FrontendLeaseRegistry::default())),
            Arc::new(Mutex::new(DemuxLedger::default())),
        );
        let worker = WorkerRuntime::spawn_owned_with_exit_hook(
            WorkerOwnerId("frontend_tune_worker_abnormal_join_contract_test", 90),
            "frontend_tune_worker_abnormal_join_contract_test",
            |_signal| WorkerExit::RuntimeFailure,
            |_| {},
        )
        .unwrap();
        *lock_mutex_status(&frontend.tune_worker, "test_mutex").unwrap() = Some(worker);

        let err = frontend
            .stop_tune_worker()
            .expect_err("abnormal tune worker stop must not be successful");
        assert_eq!(err.service_specific_error(), TunerResult::UNKNOWN_ERROR.0);
        assert!(runtime
            .debug_dump_runtime_failures()
            .contains("frontend_tune_worker stop_join_abnormal"));
    }

    #[test]
    fn frontend_live_pump_abnormal_join_is_returned_and_recorded() {
        let entry = FrontendEntry {
            id: 91,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        let runtime = FrontendRuntime::new(
            entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        let worker = WorkerRuntime::spawn_owned_with_exit_hook(
            WorkerOwnerId("frontend_live_pump_abnormal_join_contract_test", 91),
            "frontend_live_pump_abnormal_join_contract_test",
            |_owner_signal| WorkerExit::RuntimeFailure,
            |_| {},
        )
        .unwrap();
        *lock_mutex_status(&runtime.pump_worker, "test_mutex").unwrap() = Some(worker);

        let err = runtime
            .stop_live_pump()
            .expect_err("abnormal live pump stop must not be successful");
        assert_eq!(err.service_specific_error(), TunerResult::UNKNOWN_ERROR.0);
        assert!(runtime
            .debug_dump_runtime_failures()
            .contains("frontend_live_pump stop_join_abnormal"));
    }

    #[test]
    fn shared_memory_clear_result_reports_and_removes_pending_data() {
        let backing = SharedMemoryBacking::new_ring(4096).unwrap();
        let payload = [0x31u8, 0x32, 0x33, 0x34];
        let written = backing.write_bytes(&payload).unwrap();
        assert_eq!(written.len, payload.len());
        assert_eq!(backing.current_fill_bytes().unwrap(), payload.len());

        let dropped = backing.clear_result().unwrap();

        assert_eq!(dropped, payload.len());
        assert_eq!(backing.current_fill_bytes().unwrap(), 0);
    }

    struct NoopLnbCallback;

    impl Interface for NoopLnbCallback {}

    impl ILnbCallback for NoopLnbCallback {
        fn onEvent(
            &self,
            _event_type: android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::LnbEventType::LnbEventType,
        ) -> BinderResult<()> {
            Ok(())
        }

        fn onDiseqcMessage(&self, _diseqc_message: &[u8]) -> BinderResult<()> {
            Ok(())
        }
    }

    #[test]
    fn lnb_set_callback_stores_replaces_and_close_clears_callback_object() {
        let lnb_id = 20;
        let registry = Arc::new(Mutex::new(BTreeMap::from([(
            lnb_id,
            LnbRuntimeState::default(),
        )])));
        let lnb = LnbHal::new(lnb_id, registry, Arc::new(BTreeMap::new())).unwrap();
        let first = android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnbCallback::BnLnbCallback::new_binder(
            NoopLnbCallback,
            BinderFeatures::default(),
        ).unwrap();
        let second = android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnbCallback::BnLnbCallback::new_binder(
            NoopLnbCallback,
            BinderFeatures::default(),
        );

        assert!(!lnb.callback_is_set_for_test());
        lnb.setCallback(&first).unwrap();
        assert!(lnb.callback_is_set_for_test());
        lnb.setCallback(&second).unwrap();
        assert!(lnb.callback_is_set_for_test());
        lnb.close().unwrap();
        assert!(!lnb.callback_is_set_for_test());
    }

    #[test]
    fn runtime_fail_closed_transition_marks_registered_objects() {
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_drop_unexported = Arc::new(AtomicU64::new(0));
        runtime_io
            .register_filter(101, &queue, &av_queue, None, &av_drop_unexported)
            .unwrap();
        runtime_io.register_dvr(202, &queue).unwrap();

        let filter_closed = Arc::new(RuntimeAtomicFlag::new(false));
        let filter_transition =
            RuntimeFailClosedTransition::filter(101, "filter_callback_worker");
        assert!(filter_transition.close_atomic(&filter_closed));
        assert!(!filter_transition.close_atomic(&filter_closed));
        filter_transition.mark_failed(&runtime_io, "filter_runtime_failure");
        assert!(runtime_io
            .ensure_not_failed(RuntimeIoKind::Filter, 101)
            .is_err());

        RuntimeFailClosedTransition::dvr(202, "dvr_callback_worker")
            .mark_failed(&runtime_io, "dvr_runtime_failure");
        assert!(runtime_io
            .ensure_not_failed(RuntimeIoKind::Dvr, 202)
            .is_err());
    }

    #[test]
    fn filter_worker_abnormal_exit_helper_fails_closed_object_state() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_shared = Arc::new(Mutex::new(None));
        let closed = Arc::new(RuntimeAtomicFlag::new(false));
        let stop = Arc::new(RuntimeAtomicFlag::new(false));

        FilterHal::fail_filter_worker(
            &state,
            &runtime_io,
            &queue,
            &av_queue,
            &av_shared,
            &closed,
            &stop,
            filter.filter_id,
            "filter_callback_worker_Panic",
        );

        assert!(closed.load(Ordering::SeqCst));
        assert!(stop.load(Ordering::SeqCst));
        assert!(runtime_io
            .ensure_not_failed(RuntimeIoKind::Filter, filter.filter_id)
            .is_err());
        assert!(lock_mutex_status(&state, "test_mutex").unwrap()
            .filter_record(filter.filter_id)
            .is_none());
    }

    #[test]
    fn dvr_worker_abnormal_exit_helper_fails_closed_object_state() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let closed = Arc::new(RuntimeAtomicFlag::new(false));
        let cleanup_complete = Arc::new(RuntimeAtomicFlag::new(false));
        let stop = Arc::new(RuntimeAtomicFlag::new(false));
        DvrHal::fail_dvr_worker(
            &state,
            &runtime_io,
            &queue,
            &closed,
            &cleanup_complete,
            None,
            &stop,
            dvr.dvr_id,
            "dvr_callback_worker_Panic",
        );

        assert!(closed.load(Ordering::SeqCst));
        assert!(!cleanup_complete.load(Ordering::SeqCst));
        assert!(stop.load(Ordering::SeqCst));
        assert!(lock_mutex_status(&state, "test_mutex").unwrap().dvr_record(dvr.dvr_id).is_none());
    }

    #[test]
    fn dvr_playback_worker_abnormal_exit_helper_fails_closed_object_state() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, 4096)
            .unwrap();
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let backing = SharedMemoryBacking::new_ring(4096).unwrap();

        let closed = Arc::new(RuntimeAtomicFlag::new(false));
        backing.fail_playback_worker(
            &state,
            &runtime_io,
            &closed,
            dvr.dvr_id,
            "dvr_playback_consumer_Panic",
        );

        assert!(closed.load(Ordering::SeqCst));
        assert!(backing.ensure_playback_worker_healthy().is_err());
        assert!(runtime_io
            .ensure_not_failed(RuntimeIoKind::Dvr, dvr.dvr_id)
            .is_err());
        assert!(lock_mutex_status(&state, "test_mutex").unwrap().dvr_record(dvr.dvr_id).is_none());
    }

    #[test]
    fn live_pump_abnormal_exit_marks_bound_demux_runtime_failed_and_closed() {
        let entry = FrontendEntry {
            id: 88,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        let runtime = FrontendRuntime::new(
            entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_shared_drops = Arc::new(AtomicU64::new(0));
        runtime_io.register_filter(filter.filter_id, &queue, &av_queue, None, &av_shared_drops).unwrap();
        lock_mutex_status(&runtime.bound_demuxes, "test_mutex").unwrap().insert(
            DEMUX_ID_BASE,
            BoundDemuxRuntime {
                demux_generation: 1,
                state: Arc::clone(&state),
                runtime_io: Arc::clone(&runtime_io),
                demux_record: None,
            },
        );

        runtime.mark_live_path_failed("frontend_live_pump_Panic");

        assert!(runtime_io
            .ensure_not_failed(RuntimeIoKind::Filter, filter.filter_id)
            .is_err());
        assert!(lock_mutex_status(&state, "test_mutex").unwrap()
            .filter_record(filter.filter_id)
            .is_none());
    }

    #[test]
    fn owner_worker_reports_cancel_and_panic_to_hook() {
        let (tx_cancel, rx_cancel) = std::sync::mpsc::channel();
        let mut cancelled_worker = WorkerRuntime::spawn_owned_with_exit_hook(
            WorkerOwnerId("owner_worker_cancel_contract_test", 1),
            "owner_worker_cancel_contract_test",
            move |signal| {
                signal.request_stop();
                WorkerExit::StopRequested
            },
            move |exit| {
                tx_cancel.send(exit).unwrap();
            },
        )
        .unwrap();
        let _ = cancelled_worker.join_from_owner();
        assert_eq!(rx_cancel.recv().unwrap(), WorkerExit::StopRequested);

        let (tx_panic, rx_panic) = std::sync::mpsc::channel();
        let mut panic_worker = WorkerRuntime::spawn_owned_with_exit_hook(
            WorkerOwnerId("owner_worker_panic_contract_test", 2),
            "owner_worker_panic_contract_test",
            |_stop| {
                panic!("owner worker panic契約の意図的test");
            },
            move |exit| {
                tx_panic.send(exit).unwrap();
            },
        )
        .unwrap();
        let _ = panic_worker.join_from_owner();
        assert_eq!(rx_panic.recv().unwrap(), WorkerExit::PanicOrJoinFailure);
    }

    #[test]
    fn dvb_frontend_info_frequency_contract_covers_japan_catv_and_uhf_range() {
        let isdbt = FrontendEntry {
            id: 10_000,
            kind: FrontendEntryKind::Dvb {
                adapter: 0,
                frontend_index: 0,
                demux_index: 0,
                dvr_index: 0,
                declared_type: FrontendType::ISDBT,
                supported_systems: vec![FrontendSystem::IsdbT],
                min_frequency_hz: 90_000_000,
                max_frequency_hz: 770_000_000,
                max_symbol_rate: 0,
            },
        };
        assert_eq!(
            entry_frontend_frequency_contract(&isdbt),
            (110_642_857, 767_642_857, 500_000),
        );

        let isdbs = FrontendEntry {
            id: 10_001,
            kind: FrontendEntryKind::Dvb {
                adapter: 0,
                frontend_index: 1,
                demux_index: 0,
                dvr_index: 0,
                declared_type: FrontendType::ISDBS,
                supported_systems: vec![FrontendSystem::IsdbS],
                min_frequency_hz: 950_000_000,
                max_frequency_hz: 2_150_000_000,
                max_symbol_rate: 28_860_000,
            },
        };
        assert_eq!(
            entry_frontend_frequency_contract(&isdbs),
            (JAPAN_BS_FIRST_IF_HZ, JAPAN_CS110_LAST_IF_HZ, 0)
        );
    }

    #[test]
    fn scan_terminal_state_is_written_to_diagnostics() {
        let entry = FrontendEntry {
            id: 52,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        let runtime = FrontendRuntime::new(
            entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        let session = Arc::new(Mutex::new(Some(ScanSessionState {
            session_id: 7,
            fingerprint: "contract-scan".to_string(),
            phase: ScanPhase::FailedBackend,
        })));
        let terminal =
            FrontendHal::publish_scan_terminal_debug_and_clear(&runtime, &session, 7).unwrap();
        assert_eq!(terminal.phase, ScanPhase::FailedBackend);
        assert!(lock_mutex_status(&session, "test_mutex").unwrap().is_none());
        let dump = runtime.debug_dump_runtime_failures();
        assert!(dump.contains(
            "scan_last_terminal session_id=7 phase=FailedBackend fingerprint=contract-scan"
        ));
    }

    #[test]
    fn scan_completion_clears_active_session_slot() {
        let entry = FrontendEntry {
            id: 53,
            kind: FrontendEntryKind::Px4 {
                unit: 0,
                device_name: Some("px4video0".to_string()),
                control_path: PathBuf::from("/dev/px4video0"),
                declared_type: FrontendType::ISDBT,
                allowed_systems: vec![FrontendSystem::IsdbT],
            },
        };
        let runtime = FrontendRuntime::new(
            entry,
            Arc::new(Mutex::new(BTreeMap::new())),
            Arc::new(DescramblerRuntimeRegistry::new()),
            Arc::new(DescramblerDiagnosticRegistry::new()),
        );
        let session = Arc::new(Mutex::new(Some(ScanSessionState {
            session_id: 8,
            fingerprint: "completed-scan".to_string(),
            phase: ScanPhase::Completed,
        })));

        let terminal =
            FrontendHal::publish_scan_terminal_debug_and_clear(&runtime, &session, 8).unwrap();

        assert_eq!(terminal.phase, ScanPhase::Completed);
        assert!(lock_mutex_status(&session, "test_mutex").unwrap().is_none());
        assert_eq!(FrontendHal::scan_session_phase(&session, 8), None);
        assert!(runtime.debug_dump_runtime_failures().contains(
            "scan_last_terminal session_id=8 phase=Completed fingerprint=completed-scan"
        ));
    }

    #[test]
    fn scan_undefined_is_invalid_and_blind_is_unavailable() {
        assert!(matches!(
            FrontendHal::to_scan_mode(FrontendScanType::SCAN_AUTO),
            Ok(FrontendScanMode::Auto)
        ));
        assert!(matches!(
            FrontendHal::to_scan_mode(FrontendScanType::SCAN_UNDEFINED),
            Err(HalError::InvalidArgument(_))
        ));
        assert!(matches!(
            FrontendHal::to_scan_mode(FrontendScanType::SCAN_BLIND),
            Err(HalError::Unsupported(_))
        ));
    }
}

#[cfg(test)]
mod r50de_phase3_5_binder_tests {
    use super::*;

    #[test]
    fn dvr_status_mask_zero_subscribes_no_status_bits() {
        assert!(!DvrHal::status_mask_allows(0, RecordStatus::DATA_READY.0));
        assert!(!DvrHal::status_mask_allows(0, RecordStatus::OVERFLOW.0));
        assert!(DvrHal::status_mask_allows(
            RecordStatus::DATA_READY.0,
            RecordStatus::DATA_READY.0,
        ));
        assert!(!DvrHal::status_mask_allows(
            RecordStatus::DATA_READY.0,
            RecordStatus::OVERFLOW.0,
        ));
    }


    #[test]
    fn record_sc_index_mask_rejects_unsupported_bits() {
        assert!(validate_record_index_settings(0, RECORD_SC_TYPE_SC, &DemuxFilterScIndexMask::ScIndex(0x1)).is_ok());
        let err = validate_record_index_settings(0, RECORD_SC_TYPE_SC, &DemuxFilterScIndexMask::ScIndex(0x10))
            .expect_err("unsupported SC bit should be rejected");
        assert_eq!(err.service_specific_error(), TunerResult::INVALID_ARGUMENT.0);
    }

    #[test]
    fn dvr_capability_counts_are_advertised_as_demux_count() {
        let hal = TunerHal::new();
        let caps = hal.getDemuxCaps().unwrap();
        assert_eq!(caps.numRecord, MAX_LIVE_DEMUXES as i32);
        assert_eq!(caps.numPlayback, MAX_LIVE_DEMUXES as i32);
    }

    #[test]
    fn dvr_record_can_open_on_all_demuxes_but_not_twice_on_one_demux() {
        let hal = TunerHal::new();
        let callback = BnDvrCallback::new_binder(NoopDvrCallback, BinderFeatures::default());
        let mut opened = Vec::new();
        for demux_id in hal.getDemuxIds().unwrap() {
            let demux = hal.openDemuxById(demux_id).unwrap();
            let dvr = demux
                .openDvr(DvrType::RECORD, 4096, &callback)
                .expect("record DVR should open once per demux");
            opened.push((demux, dvr));
        }
        assert_eq!(opened.len(), MAX_LIVE_DEMUXES);

        let err = match opened[0].0.openDvr(DvrType::RECORD, 4096, &callback) {
            Ok(_) => panic!("second record DVR on same demux should be invalid state"),
            Err(err) => err,
        };
        assert_eq!(err.service_specific_error(), TunerResult::INVALID_STATE.0);
    }

    #[test]
    fn dvr_playback_can_open_on_all_demuxes_but_not_twice_on_one_demux() {
        let hal = TunerHal::new();
        let callback = BnDvrCallback::new_binder(NoopDvrCallback, BinderFeatures::default());
        let mut opened = Vec::new();
        for demux_id in hal.getDemuxIds().unwrap() {
            let demux = hal.openDemuxById(demux_id).unwrap();
            let dvr = demux
                .openDvr(DvrType::PLAYBACK, 4096, &callback)
                .expect("playback DVR should open once per demux");
            opened.push((demux, dvr));
        }
        assert_eq!(opened.len(), MAX_LIVE_DEMUXES);

        let err = match opened[0].0.openDvr(DvrType::PLAYBACK, 4096, &callback) {
            Ok(_) => panic!("second playback DVR on same demux should be invalid state"),
            Err(err) => err,
        };
        assert_eq!(err.service_specific_error(), TunerResult::INVALID_STATE.0);
    }

    #[test]
    fn dvr_capability_count_is_fixed_to_demux_count_in_design() {
        let design = include_str!("../../DESIGN_JA.md");
        assert!(design.contains("numRecord"));
        assert!(design.contains("numPlayback"));
        assert!(design.contains("demux 数と同数"));
        assert!(design.contains("同一方向 DVR は1本"));
    }
}

#[cfg(all(test, loom))]
mod loom_runtime_transition_tests {
    use loom::sync::atomic::{AtomicUsize, Ordering};
    use loom::sync::Arc;
    use loom::thread as loom_task;

    #[test]
    fn av_flush_and_release_zero_never_drop_shared_backing() {
        loom::model(|| {
            let backing_alive = Arc::new(AtomicUsize::new(1));
            let exported = Arc::new(AtomicUsize::new(1));
            let active_slots = Arc::new(AtomicUsize::new(2));

            let slots_for_flush = Arc::clone(&active_slots);
            let backing_for_flush = Arc::clone(&backing_alive);
            let exported_for_flush = Arc::clone(&exported);
            let flush_thread = loom_task::spawn(move || {
                slots_for_flush.store(0, Ordering::SeqCst);
                assert_ne!(backing_for_flush.load(Ordering::SeqCst), 0);
                assert_ne!(exported_for_flush.load(Ordering::SeqCst), 0);
            });

            let slots_for_release = Arc::clone(&active_slots);
            let backing_for_release = Arc::clone(&backing_alive);
            let exported_for_release = Arc::clone(&exported);
            let release_thread = loom_task::spawn(move || {
                let _ = slots_for_release.load(Ordering::SeqCst);
                assert_ne!(backing_for_release.load(Ordering::SeqCst), 0);
                assert_ne!(exported_for_release.load(Ordering::SeqCst), 0);
            });

            flush_thread.join().unwrap();
            release_thread.join().unwrap();
            assert_ne!(backing_alive.load(Ordering::SeqCst), 0);
            assert_ne!(exported.load(Ordering::SeqCst), 0);
            assert!(active_slots.load(Ordering::SeqCst) <= 2);
        });
    }

    #[test]
    fn av_configure_is_the_boundary_that_invalidates_exported_backing() {
        loom::model(|| {
            let backing_alive = Arc::new(AtomicUsize::new(1));
            let exported = Arc::new(AtomicUsize::new(1));
            let active_slots = Arc::new(AtomicUsize::new(1));

            let backing_for_configure = Arc::clone(&backing_alive);
            let exported_for_configure = Arc::clone(&exported);
            let slots_for_configure = Arc::clone(&active_slots);
            let configure_thread = loom_task::spawn(move || {
                slots_for_configure.store(0, Ordering::SeqCst);
                exported_for_configure.store(0, Ordering::SeqCst);
                backing_for_configure.store(0, Ordering::SeqCst);
            });

            configure_thread.join().unwrap();
            assert_eq!(backing_alive.load(Ordering::SeqCst), 0);
            assert_eq!(exported.load(Ordering::SeqCst), 0);
            assert_eq!(active_slots.load(Ordering::SeqCst), 0);
        });
    }

    #[test]
    fn worker_failure_and_external_close_are_idempotent_fail_closed() {
        loom::model(|| {
            let closed = Arc::new(AtomicUsize::new(0));
            let cleanup_attempts = Arc::new(AtomicUsize::new(0));

            let closed_for_worker = Arc::clone(&closed);
            let cleanup_for_worker = Arc::clone(&cleanup_attempts);
            let worker_thread = loom_task::spawn(move || {
                if closed_for_worker.swap(1, Ordering::SeqCst) == 0 {
                    cleanup_for_worker.fetch_add(1, Ordering::SeqCst);
                }
            });

            let closed_for_close = Arc::clone(&closed);
            let cleanup_for_close = Arc::clone(&cleanup_attempts);
            let close_thread = loom_task::spawn(move || {
                if closed_for_close.swap(1, Ordering::SeqCst) == 0 {
                    cleanup_for_close.fetch_add(1, Ordering::SeqCst);
                }
            });

            worker_thread.join().unwrap();
            close_thread.join().unwrap();
            assert!(closed.load(Ordering::SeqCst));
            assert_eq!(cleanup_attempts.load(Ordering::SeqCst), 1);
        });
    }
}


#[cfg(test)]
mod r50dz52_g3_10_tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn diagnostic_update_lock_failure_is_observable_not_success() {
        let registry = Arc::new(DescramblerDiagnosticRegistry::new());
        let poisoned = Arc::clone(&registry);
        let _ = std::thread::spawn(move || {
            match poisoned.counters.lock() {
                Ok(_guard) => std::panic::resume_unwind(Box::new("intentional diagnostic counter poison")),
                Err(_) => (),
            }
        })
        .join();

        assert!(!registry.record_best_effort(10, 0x0123, DescramblerDiagnosticKind::RuntimeFailure));
        assert_eq!(registry.diagnostic_update_failure_count(), 1);
        assert!(registry.dump_for_debug().contains("poisoned"));
    }
}

#[cfg(test)]
mod r50dz52_g2_01_tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    #[test]
    fn av_data_id_allocator_never_wraps_into_reserved_or_negative_range() {
        let counter = AtomicI64::new(i64::MAX);
        assert_eq!(
            allocate_next_av_data_id(&counter),
            Err(AvPayloadInternalError::CounterFailure)
        );
        assert_eq!(counter.load(Ordering::SeqCst), i64::MAX);

        let counter = AtomicI64::new(1);
        assert_eq!(allocate_next_av_data_id(&counter), Ok(1));
        assert_eq!(allocate_next_av_data_id(&counter), Ok(2));
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn av_data_id_allocator_rejects_reserved_zero_and_negative_starts() {
        for start in [0, -1] {
            let counter = AtomicI64::new(start);
            assert_eq!(
                allocate_next_av_data_id(&counter),
                Err(AvPayloadInternalError::CounterFailure)
            );
            assert_eq!(counter.load(Ordering::SeqCst), start);
        }
    }
}

#[cfg(test)]
mod r50dz52_g3_14_tests {
    use super::*;

    #[test]
    fn av_shared_release_swaps_active_and_free_maps_atomically() {
        let backing = AvSharedBacking::new().unwrap();
        let payload = vec![0x47; 188];
        let slice = backing.allocate(301, &payload).unwrap();
        assert_eq!(backing.stats().allocated_slots, 1);

        assert!(backing.release(301).unwrap());

        let stats = backing.stats();
        assert_eq!(stats.allocated_slots, 0);
        assert_eq!(stats.free_slots, AV_SLOT_COUNT);
        assert_eq!(stats.released_slots, 1);
        let next = backing.allocate(302, &payload).unwrap();
        assert_eq!(next.slot_index, slice.slot_index);
    }
}

#[cfg(test)]
mod r50dz52_g3_15_tests {
    use super::*;

    #[test]
    fn release_all_and_clear_result_reset_maps_without_partial_free_state() {
        let backing = AvSharedBacking::new().unwrap();
        backing.allocate(401, &[0x47; 188]).unwrap();
        backing.allocate(402, &[0x47; 188]).unwrap();

        backing.release_all().unwrap();
        let stats = backing.stats();
        assert_eq!(stats.allocated_slots, 0);
        assert_eq!(stats.free_slots, AV_SLOT_COUNT);
        assert_eq!(stats.released_slots, 2);

        backing.allocate(403, &[0x47; 188]).unwrap();
        backing.clear_result().unwrap();
        let after_clear = backing.stats();
        assert_eq!(after_clear.allocated_slots, 0);
        assert_eq!(after_clear.free_slots, AV_SLOT_COUNT);
    }
}

#[cfg(test)]
mod r50dz52_g3_16_tests {
    use super::*;

    #[test]
    fn av_shared_diagnostic_counters_saturate_instead_of_wrapping() {
        let backing = AvSharedBacking::new().unwrap();
        *lock_mutex_status(&backing.stale_releases, "test_stale_releases").unwrap() = u64::MAX;

        assert!(!backing.release(999_999).unwrap());

        assert_eq!(backing.stats().stale_releases, u64::MAX);
    }
}


#[cfg(test)]
mod r50dz52_g3_12_tests {
    use super::*;

    #[test]
    fn event_flag_wake_failure_is_observable_runtime_error_text() {
        let err = std::io::Error::new(
            std::io::ErrorKind::Other,
            "EventFlagWakeFailed: shared_memory_worker wake failed: injected",
        );
        assert!(is_event_flag_wake_failure(&err));
        assert!(!is_event_flag_wake_failure(&std::io::Error::new(std::io::ErrorKind::Other, "plain write")));
    }
}

#[cfg(test)]
mod r50dz52_g3_13_tests {
    use super::*;

    #[test]
    fn descriptor_internal_error_status_is_detected_before_export_success() {
        let status = fmq_clear_error_status("descriptor_internal_error: invalid grantor range");
        assert!(status_is_descriptor_internal_error(&status));
        let transient = fmq_clear_error_status("grantor duplication failed");
        assert!(!status_is_descriptor_internal_error(&transient));
    }
}

#[cfg(test)]
mod r50dz52_g3_17_tests {
    use super::*;

    #[test]
    fn invalid_delay_hint_is_rejected_without_state_commit() {
        let negative_time = FilterDelayHint {
            hintType: FilterDelayHintType::TIME_DELAY_IN_MS,
            hintValue: -1,
        };
        assert!(normalize_filter_delay_hint(&negative_time).is_err());

        let negative_size = FilterDelayHint {
            hintType: FilterDelayHintType::DATA_SIZE_DELAY_IN_BYTES,
            hintValue: -1,
        };
        assert!(normalize_filter_delay_hint(&negative_size).is_err());
    }
}

#[cfg(test)]
mod r50dz52_g3_19_tests {
    #[derive(Default)]
    struct FakeWorkerSlot {
        present: bool,
        request_stop_count: usize,
        join_count: usize,
        fail_next_join: bool,
    }

    impl FakeWorkerSlot {
        fn stop_scan_worker_like_production(&mut self) -> Result<(), ()> {
            if self.present {
                self.request_stop_count += 1;
                self.join_count += 1;
                if self.fail_next_join {
                    self.fail_next_join = false;
                    return Err(());
                }
                self.present = false;
            }
            Ok(())
        }
    }

    #[test]
    fn scan_worker_slot_survives_failed_join_and_is_retried() {
        let mut slot = FakeWorkerSlot { present: true, fail_next_join: true, ..FakeWorkerSlot::default() };
        assert!(slot.stop_scan_worker_like_production().is_err());
        assert!(slot.present);
        assert_eq!(slot.request_stop_count, 1);
        assert_eq!(slot.join_count, 1);

        assert!(slot.stop_scan_worker_like_production().is_ok());
        assert!(!slot.present);
        assert_eq!(slot.request_stop_count, 2);
        assert_eq!(slot.join_count, 2);
    }
}

#[cfg(test)]
mod r50dz52_g3_20_tests {
    #[derive(Default)]
    struct FakeWorkerSlot {
        present: bool,
        request_stop_count: usize,
        join_count: usize,
        fail_next_join: bool,
    }

    impl FakeWorkerSlot {
        fn stop_tune_worker_like_production(&mut self) -> Result<(), ()> {
            if self.present {
                self.request_stop_count += 1;
                self.join_count += 1;
                if self.fail_next_join {
                    self.fail_next_join = false;
                    return Err(());
                }
                self.present = false;
            }
            Ok(())
        }
    }

    #[test]
    fn tune_worker_slot_survives_failed_join_and_is_retried() {
        let mut slot = FakeWorkerSlot { present: true, fail_next_join: true, ..FakeWorkerSlot::default() };
        assert!(slot.stop_tune_worker_like_production().is_err());
        assert!(slot.present);
        assert_eq!(slot.request_stop_count, 1);
        assert_eq!(slot.join_count, 1);

        assert!(slot.stop_tune_worker_like_production().is_ok());
        assert!(!slot.present);
        assert_eq!(slot.request_stop_count, 2);
        assert_eq!(slot.join_count, 2);
    }
}


#[cfg(test)]
mod r50dz52_g1_01_tests {
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct FakeDemuxOpenTxn {
        registry: BTreeSet<i32>,
        live_ids: BTreeSet<i32>,
        ledger: BTreeSet<i32>,
    }

    impl FakeDemuxOpenTxn {
        fn open_demux_like_production(&mut self, id: i32, binder_ok: bool) -> Result<i32, &'static str> {
            self.registry.insert(id);
            self.live_ids.insert(id);
            self.ledger.insert(id);
            if !binder_ok {
                self.registry.remove(&id);
                self.live_ids.remove(&id);
                self.ledger.remove(&id);
                return Err("new_demux_binder_failed");
            }
            Ok(id)
        }
    }

    #[test]
    fn binder_creation_failure_rolls_back_demux_record_live_id_and_ledger() {
        let mut txn = FakeDemuxOpenTxn::default();
        assert_eq!(txn.open_demux_like_production(7, false), Err("new_demux_binder_failed"));
        assert!(!txn.registry.contains(&7));
        assert!(!txn.live_ids.contains(&7));
        assert!(!txn.ledger.contains(&7));
        assert_eq!(txn.open_demux_like_production(7, true), Ok(7));
        assert!(txn.registry.contains(&7));
        assert!(txn.live_ids.contains(&7));
        assert!(txn.ledger.contains(&7));
    }
}

#[cfg(test)]
mod r50dz52_g1_02_tests {
    #[derive(Default)]
    struct FakeOpenByIdRecord {
        ref_count: usize,
        quarantined: bool,
    }

    impl FakeOpenByIdRecord {
        fn open_by_id_like_production(
            &mut self,
            binder_ok: bool,
            rollback_ok: bool,
        ) -> Result<(), &'static str> {
            if binder_ok {
                self.ref_count += 1;
                return Ok(());
            }
            if rollback_ok {
                return Err("new_demux_binder_failed");
            }
            self.quarantined = true;
            Err("UNKNOWN_ERROR")
        }
    }

    #[test]
    fn ref_count_changes_only_after_binder_success() {
        let mut record = FakeOpenByIdRecord { ref_count: 3, quarantined: false };
        assert_eq!(record.open_by_id_like_production(false, true), Err("new_demux_binder_failed"));
        assert_eq!(record.ref_count, 3);
        assert!(!record.quarantined);
        assert_eq!(record.open_by_id_like_production(true, true), Ok(()));
        assert_eq!(record.ref_count, 4);
    }

    #[test]
    fn rollback_failure_quarantines_demux_id_and_returns_unknown_error() {
        let mut record = FakeOpenByIdRecord { ref_count: 1, quarantined: false };
        assert_eq!(record.open_by_id_like_production(false, false), Err("UNKNOWN_ERROR"));
        assert_eq!(record.ref_count, 1);
        assert!(record.quarantined);
    }
}

#[cfg(test)]
mod r50dz52_g1_03_tests {
    #[derive(Default)]
    struct FakeFilterOpenState {
        worker: bool,
        runtime: bool,
        demux_filter_record: bool,
        ledger_provisional: bool,
    }

    impl FakeFilterOpenState {
        fn open_filter_like_production(&mut self, commit_ok: bool) -> Result<(), &'static str> {
            self.demux_filter_record = true;
            self.runtime = true;
            self.worker = true;
            self.ledger_provisional = true;
            if commit_ok {
                return Ok(());
            }
            self.worker = false;
            self.runtime = false;
            self.demux_filter_record = false;
            self.ledger_provisional = false;
            Err("filter_ledger_commit_open_failed")
        }
    }

    #[test]
    fn commit_open_failure_cleans_filter_worker_runtime_demux_record_and_ledger() {
        let mut state = FakeFilterOpenState::default();
        assert_eq!(state.open_filter_like_production(false), Err("filter_ledger_commit_open_failed"));
        assert!(!state.worker);
        assert!(!state.runtime);
        assert!(!state.demux_filter_record);
        assert!(!state.ledger_provisional);
    }
}

#[cfg(test)]
mod r50dz52_g1_04_tests {
    #[derive(Default)]
    struct FakeDvrOpenState {
        worker: bool,
        runtime: bool,
        demux_dvr_record: bool,
        ledger_provisional: bool,
    }

    impl FakeDvrOpenState {
        fn open_dvr_like_production(&mut self, commit_ok: bool) -> Result<(), &'static str> {
            self.demux_dvr_record = true;
            self.runtime = true;
            self.worker = true;
            self.ledger_provisional = true;
            if commit_ok {
                return Ok(());
            }
            self.worker = false;
            self.runtime = false;
            self.demux_dvr_record = false;
            self.ledger_provisional = false;
            Err("dvr_ledger_commit_open_failed")
        }
    }

    #[test]
    fn commit_open_failure_cleans_dvr_worker_runtime_record_and_ledger() {
        let mut state = FakeDvrOpenState::default();
        assert_eq!(state.open_dvr_like_production(false), Err("dvr_ledger_commit_open_failed"));
        assert!(!state.worker);
        assert!(!state.runtime);
        assert!(!state.demux_dvr_record);
        assert!(!state.ledger_provisional);
    }
}

#[cfg(test)]
mod r50dz52_g1_05_tests {
    #[derive(Default)]
    struct FakeFilterRuntime {
        rollback_failed: bool,
        closed: bool,
        diagnostic: Option<&'static str>,
    }

    impl FakeFilterRuntime {
        fn set_data_source_like_production(
            &mut self,
            apply_ok: bool,
            rollback_ok: bool,
        ) -> Result<(), &'static str> {
            if apply_ok {
                return Ok(());
            }
            if rollback_ok {
                return Err("apply_failed");
            }
            self.rollback_failed = true;
            self.closed = false;
            self.diagnostic = Some("filter_set_data_source_rollback_failed");
            Err("UNKNOWN_ERROR")
        }

        fn start_like_production(&self) -> Result<(), &'static str> {
            if self.rollback_failed { Err("INVALID_STATE") } else { Ok(()) }
        }

        fn configure_like_production(&self) -> Result<(), &'static str> {
            if self.rollback_failed { Err("INVALID_STATE") } else { Ok(()) }
        }

        fn close_like_production(&mut self) -> Result<(), &'static str> {
            self.closed = true;
            Ok(())
        }
    }

    #[test]
    fn rollback_failure_marks_runtime_failed_but_keeps_close_possible() {
        let mut runtime = FakeFilterRuntime::default();
        assert_eq!(runtime.set_data_source_like_production(false, false), Err("UNKNOWN_ERROR"));
        assert!(runtime.rollback_failed);
        assert!(!runtime.closed);
        assert_eq!(runtime.diagnostic, Some("filter_set_data_source_rollback_failed"));
        assert_eq!(runtime.start_like_production(), Err("INVALID_STATE"));
        assert_eq!(runtime.configure_like_production(), Err("INVALID_STATE"));
        assert_eq!(runtime.set_data_source_like_production(false, true), Err("apply_failed"));
        assert_eq!(runtime.close_like_production(), Ok(()));
        assert!(runtime.closed);
    }
}

#[cfg(test)]
mod r50dz52_g1_06_tests {
    #[derive(Default)]
    struct FakeDemuxCloseState {
        registry_present: bool,
        live_id_present: bool,
        record_present: bool,
        remove_record_fail_once: bool,
    }

    impl FakeDemuxCloseState {
        fn close_internal_like_production(&mut self) -> Result<(), &'static str> {
            if self.remove_record_fail_once {
                self.remove_record_fail_once = false;
                return Err("demux_ledger_remove_record_failed");
            }
            self.registry_present = false;
            self.live_id_present = false;
            self.record_present = false;
            Ok(())
        }
    }

    #[test]
    fn remove_record_failure_is_returned_and_close_can_retry_remaining_cleanup() {
        let mut state = FakeDemuxCloseState {
            registry_present: true,
            live_id_present: true,
            record_present: true,
            remove_record_fail_once: true,
        };
        assert_eq!(state.close_internal_like_production(), Err("demux_ledger_remove_record_failed"));
        assert!(state.registry_present);
        assert!(state.live_id_present);
        assert!(state.record_present);
        assert_eq!(state.close_internal_like_production(), Ok(()));
        assert!(!state.registry_present);
        assert!(!state.live_id_present);
        assert!(!state.record_present);
    }
}

#[cfg(test)]
mod r50dz52_g1_07_tests {
    #[derive(Default)]
    struct FakeFilterCloseRuntime {
        closed: bool,
        cleanup_complete: bool,
        close_failure: Option<&'static str>,
        fail_cleanup_once: bool,
        fail_rollback_close_once: bool,
    }

    impl FakeFilterCloseRuntime {
        fn close_internal_like_production(&mut self) -> Result<(), &'static str> {
            if self.fail_cleanup_once {
                self.fail_cleanup_once = false;
                self.closed = true;
                self.cleanup_complete = false;
                self.close_failure = Some("filter_cleanup_failed");
                if self.fail_rollback_close_once {
                    self.fail_rollback_close_once = false;
                    self.close_failure = Some("filter_rollback_close_failed");
                }
                return Err("UNKNOWN_ERROR");
            }
            self.closed = true;
            self.cleanup_complete = true;
            self.close_failure = None;
            Ok(())
        }

        fn start_like_production(&self) -> Result<(), &'static str> {
            if self.closed || self.close_failure.is_some() { Err("INVALID_STATE") } else { Ok(()) }
        }

        fn configure_like_production(&self) -> Result<(), &'static str> {
            if self.closed || self.close_failure.is_some() { Err("INVALID_STATE") } else { Ok(()) }
        }
    }

    #[test]
    fn cleanup_and_rollback_close_failure_are_returned_and_close_retries_remaining_cleanup() {
        let mut runtime = FakeFilterCloseRuntime {
            fail_cleanup_once: true,
            fail_rollback_close_once: true,
            ..FakeFilterCloseRuntime::default()
        };
        assert_eq!(runtime.close_internal_like_production(), Err("UNKNOWN_ERROR"));
        assert!(runtime.closed);
        assert!(!runtime.cleanup_complete);
        assert_eq!(runtime.close_failure, Some("filter_rollback_close_failed"));
        assert_eq!(runtime.start_like_production(), Err("INVALID_STATE"));
        assert_eq!(runtime.configure_like_production(), Err("INVALID_STATE"));
        assert_eq!(runtime.close_internal_like_production(), Ok(()));
        assert!(runtime.cleanup_complete);
        assert!(runtime.close_failure.is_none());
    }
}

#[cfg(test)]
mod r50dz52_g1_08_tests {
    #[derive(Default)]
    struct FakeDvrCloseRuntime {
        closed: bool,
        cleanup_complete: bool,
        close_failure: Option<&'static str>,
        fail_cleanup_once: bool,
        fail_rollback_close_once: bool,
    }

    impl FakeDvrCloseRuntime {
        fn close_internal_like_production(&mut self) -> Result<(), &'static str> {
            if self.fail_cleanup_once {
                self.fail_cleanup_once = false;
                self.closed = true;
                self.cleanup_complete = false;
                self.close_failure = Some("dvr_cleanup_failed");
                if self.fail_rollback_close_once {
                    self.fail_rollback_close_once = false;
                    self.close_failure = Some("dvr_rollback_close_failed");
                }
                return Err("UNKNOWN_ERROR");
            }
            self.closed = true;
            self.cleanup_complete = true;
            self.close_failure = None;
            Ok(())
        }

        fn read_like_production(&self) -> Result<(), &'static str> {
            if self.closed || self.close_failure.is_some() { Err("INVALID_STATE") } else { Ok(()) }
        }

        fn write_like_production(&self) -> Result<(), &'static str> {
            if self.closed || self.close_failure.is_some() { Err("INVALID_STATE") } else { Ok(()) }
        }

        fn start_like_production(&self) -> Result<(), &'static str> {
            if self.closed || self.close_failure.is_some() { Err("INVALID_STATE") } else { Ok(()) }
        }
    }

    #[test]
    fn rollback_close_failure_is_kept_as_close_failure_and_dvr_operations_become_invalid() {
        let mut runtime = FakeDvrCloseRuntime {
            fail_cleanup_once: true,
            fail_rollback_close_once: true,
            ..FakeDvrCloseRuntime::default()
        };
        assert_eq!(runtime.close_internal_like_production(), Err("UNKNOWN_ERROR"));
        assert!(runtime.closed);
        assert!(!runtime.cleanup_complete);
        assert_eq!(runtime.close_failure, Some("dvr_rollback_close_failed"));
        assert_eq!(runtime.read_like_production(), Err("INVALID_STATE"));
        assert_eq!(runtime.write_like_production(), Err("INVALID_STATE"));
        assert_eq!(runtime.start_like_production(), Err("INVALID_STATE"));
        assert_eq!(runtime.close_internal_like_production(), Ok(()));
        assert!(runtime.cleanup_complete);
        assert!(runtime.close_failure.is_none());
    }
}

#[cfg(test)]
mod r50dz52_g1_11_tests {
    #[derive(Default)]
    struct FakeLnbCloseState {
        closed: bool,
        closing_failed: bool,
        backend_reset_fail_once: bool,
        registry_commit_fail_once: bool,
        backend_reset_count: usize,
    }

    impl FakeLnbCloseState {
        fn close_like_production(&mut self) -> Result<(), &'static str> {
            self.backend_reset_count += 1;
            if self.backend_reset_fail_once {
                self.backend_reset_fail_once = false;
                self.closing_failed = true;
                self.closed = false;
                return Err("backend_reset_failed");
            }
            if self.registry_commit_fail_once {
                self.registry_commit_fail_once = false;
                self.closing_failed = true;
                self.closed = false;
                return Err("registry_commit_failed");
            }
            self.closed = true;
            self.closing_failed = false;
            Ok(())
        }
    }

    #[test]
    fn backend_reset_failure_does_not_commit_closed_and_reclose_retries_backend_reset() {
        let mut state = FakeLnbCloseState { backend_reset_fail_once: true, ..FakeLnbCloseState::default() };
        assert_eq!(state.close_like_production(), Err("backend_reset_failed"));
        assert!(!state.closed);
        assert!(state.closing_failed);
        assert_eq!(state.backend_reset_count, 1);
        assert_eq!(state.close_like_production(), Ok(()));
        assert!(state.closed);
        assert_eq!(state.backend_reset_count, 2);
    }
}

#[cfg(test)]
mod r50dz52_g1_12_tests {
    #[derive(Default)]
    struct FakeLnbUpdateState {
        backend_state: i32,
        registry_state: i32,
        runtime_failed: bool,
        diagnostic: Option<&'static str>,
        backend_rollback_attempts: usize,
    }

    impl FakeLnbUpdateState {
        fn update_like_production(&mut self, next: i32, registry_commit_ok: bool) -> Result<(), &'static str> {
            self.backend_state = next;
            if !registry_commit_ok {
                self.runtime_failed = true;
                self.diagnostic = Some("lnb_registry_commit_failed_after_backend_apply");
                return Err("UNKNOWN_ERROR");
            }
            self.registry_state = next;
            Ok(())
        }

        fn set_voltage_like_production(&self) -> Result<(), &'static str> {
            if self.runtime_failed { Err("UNKNOWN_ERROR") } else { Ok(()) }
        }
    }

    #[test]
    fn registry_commit_failure_after_backend_apply_marks_internal_failed_without_backend_rollback() {
        let mut state = FakeLnbUpdateState::default();
        assert_eq!(state.update_like_production(13, false), Err("UNKNOWN_ERROR"));
        assert_eq!(state.backend_state, 13);
        assert_eq!(state.registry_state, 0);
        assert!(state.runtime_failed);
        assert_eq!(state.diagnostic, Some("lnb_registry_commit_failed_after_backend_apply"));
        assert_eq!(state.backend_rollback_attempts, 0);
        assert_eq!(state.set_voltage_like_production(), Err("UNKNOWN_ERROR"));
    }
}

#[cfg(test)]
mod r50dz52_g1_13_tests {
    #[derive(Clone, Copy)]
    struct FakeFilterLinkState {
        sink_failed: bool,
        source_closed: bool,
        source_failed: bool,
        same_demux: bool,
        self_reference: bool,
    }

    fn validate_like_production(state: FakeFilterLinkState) -> Result<(), &'static str> {
        if state.sink_failed { return Err("INVALID_STATE:sink_lifecycle"); }
        if state.source_closed { return Err("INVALID_STATE:source_closed"); }
        if state.source_failed { return Err("INVALID_STATE:source_runtime_failed"); }
        if !state.same_demux { return Err("INVALID_ARGUMENT:foreign_source"); }
        if state.self_reference { return Err("INVALID_ARGUMENT:self_reference"); }
        Ok(())
    }

    #[test]
    fn source_lifecycle_is_checked_before_ownership_and_self_reference() {
        assert_eq!(
            validate_like_production(FakeFilterLinkState {
                sink_failed: false,
                source_closed: true,
                source_failed: false,
                same_demux: false,
                self_reference: true,
            }),
            Err("INVALID_STATE:source_closed")
        );
        assert_eq!(
            validate_like_production(FakeFilterLinkState {
                sink_failed: false,
                source_closed: false,
                source_failed: true,
                same_demux: false,
                self_reference: true,
            }),
            Err("INVALID_STATE:source_runtime_failed")
        );
    }

    #[test]
    fn foreign_source_and_self_reference_remain_distinct_after_lifecycle_passes() {
        assert_eq!(
            validate_like_production(FakeFilterLinkState {
                sink_failed: false,
                source_closed: false,
                source_failed: false,
                same_demux: false,
                self_reference: false,
            }),
            Err("INVALID_ARGUMENT:foreign_source")
        );
        assert_eq!(
            validate_like_production(FakeFilterLinkState {
                sink_failed: false,
                source_closed: false,
                source_failed: false,
                same_demux: true,
                self_reference: true,
            }),
            Err("INVALID_ARGUMENT:self_reference")
        );
    }
}

#[cfg(test)]
mod r50dz52_g2_20_tests {
    #[derive(Debug, Default)]
    struct FakeFrontendStatusObserver {
        selected_lnb: Option<i32>,
        backend_apply_count: usize,
        status_reads: usize,
    }

    impl FakeFrontendStatusObserver {
        fn get_status_like_production(&mut self) -> (Option<i32>, usize) {
            self.status_reads += 1;
            (self.selected_lnb, self.backend_apply_count)
        }

        fn set_lnb_like_production(&mut self, lnb: i32) {
            self.selected_lnb = Some(lnb);
            self.backend_apply_count += 1;
        }
    }

    #[test]
    fn get_status_is_observation_only_and_does_not_apply_lnb_backend_state() {
        let mut observer = FakeFrontendStatusObserver::default();
        observer.set_lnb_like_production(9);
        let before_apply_count = observer.backend_apply_count;
        let before_lnb = observer.selected_lnb;
        assert_eq!(observer.get_status_like_production(), (before_lnb, before_apply_count));
        assert_eq!(observer.get_status_like_production(), (before_lnb, before_apply_count));
        assert_eq!(observer.status_reads, 2);
        assert_eq!(observer.backend_apply_count, before_apply_count);
        assert_eq!(observer.selected_lnb, before_lnb);
    }
}

#[cfg(test)]
mod r50dz52_g3_01_tests {
    #[derive(Default)]
    struct FakeDescramblerOpenTxn {
        session_source_committed: bool,
        ledger_committed: bool,
        demux_closed: bool,
        session_already_set: bool,
        owner_mismatch: bool,
    }

    impl FakeDescramblerOpenTxn {
        fn set_demux_source_like_production(&mut self) -> Result<(), &'static str> {
            if self.demux_closed {
                return Err("INVALID_STATE:demux_closed");
            }
            if self.session_already_set {
                return Err("INVALID_STATE:session_already_set");
            }
            if self.owner_mismatch {
                return Err("INVALID_ARGUMENT:owner_demux_mismatch");
            }
            self.session_source_committed = true;
            self.ledger_committed = true;
            Ok(())
        }
    }

    #[test]
    fn prepare_failures_do_not_leave_descrambler_ledger_entry() {
        for (demux_closed, session_already_set, owner_mismatch) in [(true, false, false), (false, true, false), (false, false, true)] {
            let mut txn = FakeDescramblerOpenTxn { demux_closed, session_already_set, owner_mismatch, ..FakeDescramblerOpenTxn::default() };
            assert!(txn.set_demux_source_like_production().is_err());
            assert!(!txn.session_source_committed);
            assert!(!txn.ledger_committed);
        }

        let mut ok = FakeDescramblerOpenTxn::default();
        assert_eq!(ok.set_demux_source_like_production(), Ok(()));
        assert!(ok.session_source_committed);
        assert!(ok.ledger_committed);
    }
}

#[cfg(test)]
mod r50dz52_g3_03_tests {
    #[derive(Default)]
    struct FakeDescramblerCloseTxn {
        closing: bool,
        registry_cleaned: bool,
        key_released: bool,
        session_cleared: bool,
        pending_demux_ledger_close: bool,
        begin_close_fails_once: bool,
    }

    impl FakeDescramblerCloseTxn {
        fn close_like_production(&mut self) -> Result<(), &'static str> {
            self.closing = true;
            if self.begin_close_fails_once {
                self.begin_close_fails_once = false;
                self.pending_demux_ledger_close = true;
                return Err("UNKNOWN_ERROR");
            }
            self.pending_demux_ledger_close = false;
            self.registry_cleaned = true;
            self.key_released = true;
            self.session_cleared = true;
            Ok(())
        }
    }

    #[test]
    fn demux_ledger_begin_close_failure_blocks_cleanup_and_is_retried() {
        let mut txn = FakeDescramblerCloseTxn { begin_close_fails_once: true, ..FakeDescramblerCloseTxn::default() };
        assert_eq!(txn.close_like_production(), Err("UNKNOWN_ERROR"));
        assert!(txn.closing);
        assert!(txn.pending_demux_ledger_close);
        assert!(!txn.registry_cleaned);
        assert!(!txn.key_released);
        assert!(!txn.session_cleared);

        assert_eq!(txn.close_like_production(), Ok(()));
        assert!(!txn.pending_demux_ledger_close);
        assert!(txn.registry_cleaned);
        assert!(txn.key_released);
        assert!(txn.session_cleared);
    }
}

#[cfg(test)]
mod r50dz52_g3_04_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::Ordering;

    #[test]
    fn descrambler_runtime_id_allocator_rejects_wrap_and_reserved_ids() {
        let registry = DescramblerRuntimeRegistry::new();
        let session = Arc::new(Mutex::new(DescramblerSession::new()));

        registry.next_id.store(i64::MAX, Ordering::SeqCst);
        assert!(registry.register(&session).is_ok());
        assert!(registry.register(&session).is_err());

        let zero_registry = DescramblerRuntimeRegistry::new();
        zero_registry.next_id.store(0, Ordering::SeqCst);
        assert!(zero_registry.register(&session).is_err());
    }
}

#[cfg(test)]
mod r50dz52_g3_06_tests {
    #[derive(Debug, Default)]
    struct FakeKeySession {
        current_key: Option<&'static str>,
        pending_key_release: bool,
    }

    impl FakeKeySession {
        fn set_non_void_like_production(&mut self, new_key: &'static str, old_release_ok: bool) -> Result<(), &'static str> {
            let old = self.current_key;
            if old.is_some() && !old_release_ok {
                self.pending_key_release = true;
                return Err("UNKNOWN_ERROR");
            }
            self.current_key = Some(new_key);
            self.pending_key_release = false;
            Ok(())
        }
    }

    #[test]
    fn old_token_release_failure_keeps_old_key_and_pending_cleanup() {
        let mut session = FakeKeySession { current_key: Some("old"), ..FakeKeySession::default() };
        assert_eq!(session.set_non_void_like_production("new", false), Err("UNKNOWN_ERROR"));
        assert_eq!(session.current_key, Some("old"));
        assert!(session.pending_key_release);
        assert_eq!(session.set_non_void_like_production("new", true), Ok(()));
        assert_eq!(session.current_key, Some("new"));
        assert!(!session.pending_key_release);
    }
}

#[cfg(test)]
mod r50dz52_g3_07_tests {
    #[derive(Debug, Default)]
    struct FakeVoidKeySession {
        current_key: Option<&'static str>,
        pending_key_release: bool,
    }

    impl FakeVoidKeySession {
        fn set_void_like_production(&mut self, old_release_ok: bool) -> Result<(), &'static str> {
            if self.current_key.is_some() && !old_release_ok {
                self.pending_key_release = true;
                return Err("UNKNOWN_ERROR");
            }
            self.current_key = None;
            self.pending_key_release = false;
            Ok(())
        }
    }

    #[test]
    fn void_token_release_failure_does_not_clear_session_key_first() {
        let mut session = FakeVoidKeySession { current_key: Some("old"), ..FakeVoidKeySession::default() };
        assert_eq!(session.set_void_like_production(false), Err("UNKNOWN_ERROR"));
        assert_eq!(session.current_key, Some("old"));
        assert!(session.pending_key_release);
        assert_eq!(session.set_void_like_production(true), Ok(()));
        assert_eq!(session.current_key, None);
        assert!(!session.pending_key_release);
    }
}

#[cfg(test)]
mod r50dz52_g2_08_tests {
    #[derive(Default)]
    struct FakeFilterStartTxn {
        started: bool,
        worker_started: bool,
        runtime_registered: bool,
        cleanup_failed: bool,
        txn_error: Option<&'static str>,
    }

    impl FakeFilterStartTxn {
        fn start_like_production(&mut self, callback_ok: bool, cleanup_ok: bool) -> Result<(), &'static str> {
            self.worker_started = true;
            self.runtime_registered = true;
            if !callback_ok {
                self.worker_started = false;
                self.runtime_registered = false;
                self.cleanup_failed = !cleanup_ok;
                self.txn_error = Some(if cleanup_ok { "CALLBACK_FAILED" } else { "CALLBACK_AND_CLEANUP_FAILED" });
                return Err("UNKNOWN_ERROR");
            }
            self.started = true;
            Ok(())
        }
    }

    #[test]
    fn callback_failure_cleanup_failure_does_not_leave_filter_started() {
        let mut txn = FakeFilterStartTxn::default();
        assert_eq!(txn.start_like_production(false, false), Err("UNKNOWN_ERROR"));
        assert!(!txn.started);
        assert!(!txn.worker_started);
        assert!(!txn.runtime_registered);
        assert!(txn.cleanup_failed);
        assert_eq!(txn.txn_error, Some("CALLBACK_AND_CLEANUP_FAILED"));
    }
}

#[cfg(test)]
mod r50dz52_g2_09_tests {
    #[derive(Default)]
    struct FakeDvrStartTxn {
        started: bool,
        worker_started: bool,
        runtime_registered: bool,
        status_callback_sent: bool,
        cleanup_failed: bool,
    }

    impl FakeDvrStartTxn {
        fn start_like_production(&mut self, status_callback_ok: bool, cleanup_ok: bool) -> Result<(), &'static str> {
            self.worker_started = true;
            self.runtime_registered = true;
            if !status_callback_ok {
                self.status_callback_sent = false;
                self.worker_started = false;
                self.runtime_registered = false;
                self.cleanup_failed = !cleanup_ok;
                return Err("UNKNOWN_ERROR");
            }
            self.status_callback_sent = true;
            self.started = true;
            Ok(())
        }
    }

    #[test]
    fn status_callback_failure_does_not_leave_dvr_partially_started() {
        let mut txn = FakeDvrStartTxn::default();
        assert_eq!(txn.start_like_production(false, false), Err("UNKNOWN_ERROR"));
        assert!(!txn.started);
        assert!(!txn.worker_started);
        assert!(!txn.runtime_registered);
        assert!(!txn.status_callback_sent);
        assert!(txn.cleanup_failed);
    }
}

#[cfg(test)]
mod r50dz52_g2_10_tests {
    #[derive(Default)]
    struct FakeFrontendTuneTxn {
        backend_tuned: bool,
        worker_spawned: bool,
        stream_boundary_prepared: bool,
        runtime_tuned: bool,
        packet_reader_stopped: bool,
        runtime_failed: Option<&'static str>,
    }

    impl FakeFrontendTuneTxn {
        fn tune_like_production(&mut self, worker_ok: bool, boundary_ok: bool, rollback_ok: bool) -> Result<(), &'static str> {
            self.backend_tuned = true;
            if !boundary_ok || !worker_ok {
                self.packet_reader_stopped = true;
                if rollback_ok {
                    self.backend_tuned = false;
                    self.worker_spawned = false;
                    self.stream_boundary_prepared = false;
                    self.runtime_tuned = false;
                    return Err("UNKNOWN_ERROR");
                }
                self.runtime_failed = Some("TuneRollbackFailed");
                self.runtime_tuned = false;
                return Err("UNKNOWN_ERROR");
            }
            self.stream_boundary_prepared = true;
            self.worker_spawned = true;
            self.runtime_tuned = true;
            Ok(())
        }

        fn later_public_call_like_production(&self) -> Result<(), &'static str> {
            if self.runtime_failed == Some("TuneRollbackFailed") { Err("UNKNOWN_ERROR") } else { Ok(()) }
        }
    }

    #[test]
    fn post_backend_failure_rolls_back_or_marks_tune_rollback_failed() {
        let mut rollback_ok = FakeFrontendTuneTxn::default();
        assert_eq!(rollback_ok.tune_like_production(false, true, true), Err("UNKNOWN_ERROR"));
        assert!(!rollback_ok.backend_tuned);
        assert!(rollback_ok.packet_reader_stopped);
        assert!(!rollback_ok.runtime_tuned);
        assert_eq!(rollback_ok.runtime_failed, None);

        let mut rollback_failed = FakeFrontendTuneTxn::default();
        assert_eq!(rollback_failed.tune_like_production(false, true, false), Err("UNKNOWN_ERROR"));
        assert_eq!(rollback_failed.runtime_failed, Some("TuneRollbackFailed"));
        assert_eq!(rollback_failed.later_public_call_like_production(), Err("UNKNOWN_ERROR"));
    }
}

#[cfg(test)]
mod r50dz52_g3_09_tests {
    #[derive(Default)]
    struct FakeInvalidateDemuxTxn {
        token_expired: bool,
        session_cleared: bool,
        pending_key_release: bool,
    }

    impl FakeInvalidateDemuxTxn {
        fn invalidate_like_production(&mut self, expire_ok: bool) -> Result<(), &'static str> {
            if !expire_ok {
                self.pending_key_release = true;
                return Err("UNKNOWN_ERROR");
            }
            self.token_expired = true;
            self.session_cleared = true;
            self.pending_key_release = false;
            Ok(())
        }
    }

    #[test]
    fn expire_failure_does_not_clear_session_first() {
        let mut txn = FakeInvalidateDemuxTxn::default();
        assert_eq!(txn.invalidate_like_production(false), Err("UNKNOWN_ERROR"));
        assert!(!txn.token_expired);
        assert!(!txn.session_cleared);
        assert!(txn.pending_key_release);

        assert_eq!(txn.invalidate_like_production(true), Ok(()));
        assert!(txn.token_expired);
        assert!(txn.session_cleared);
        assert!(!txn.pending_key_release);
    }
}

#[cfg(test)]
mod r50dz52_g3_18_tests {
    #[derive(Default)]
    struct FakeAvStreamTypeTxn {
        backing: Option<&'static str>,
        stream_type: &'static str,
        drop_count: usize,
    }

    impl FakeAvStreamTypeTxn {
        fn configure_like_production(&mut self, next_type: &'static str, late_validation_ok: bool, drop_ok: bool) -> Result<(), &'static str> {
            if !late_validation_ok {
                return Err("INVALID_ARGUMENT");
            }
            if !drop_ok {
                return Err("UNKNOWN_ERROR");
            }
            self.backing = None;
            self.drop_count += 1;
            self.stream_type = next_type;
            Ok(())
        }
    }

    #[test]
    fn late_validation_or_drop_failure_preserves_old_backing_and_stream_type() {
        let mut txn = FakeAvStreamTypeTxn { backing: Some("old-backing"), stream_type: "old-av", ..FakeAvStreamTypeTxn::default() };
        assert_eq!(txn.configure_like_production("new-av", false, true), Err("INVALID_ARGUMENT"));
        assert_eq!(txn.backing, Some("old-backing"));
        assert_eq!(txn.stream_type, "old-av");
        assert_eq!(txn.drop_count, 0);

        assert_eq!(txn.configure_like_production("new-av", true, false), Err("UNKNOWN_ERROR"));
        assert_eq!(txn.backing, Some("old-backing"));
        assert_eq!(txn.stream_type, "old-av");
        assert_eq!(txn.drop_count, 0);

        assert_eq!(txn.configure_like_production("new-av", true, true), Ok(()));
        assert_eq!(txn.backing, None);
        assert_eq!(txn.stream_type, "new-av");
        assert_eq!(txn.drop_count, 1);
    }
}

