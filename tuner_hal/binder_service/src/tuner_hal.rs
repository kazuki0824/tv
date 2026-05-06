use crate::descrambler_key_table::{DescramblerKeyResolveError, DescramblerKeyTable};
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
};
use binder::{
    Binder, BinderFeatures, Interface, ParcelFileDescriptor, Result as BinderResult, Status,
    StatusCode, Strong,
};
use maleicacid_tuner_hal_common::{
    is_japan_cs110_if_frequency_hz, FrontendScanMode, FrontendStreamIdKind, FrontendSystem,
    FrontendTelemetry, FrontendTuneRequest, HalError, TsPacketCompletionBuffer,
    DEMUX_MAX_AUDIO_FILTERS, DEMUX_MAX_FILTERS_PER_DEMUX, DEMUX_MAX_PES_FILTERS,
    DEMUX_MAX_SECTION_FILTERS, DEMUX_MAX_TS_FILTERS, DEMUX_MAX_VIDEO_FILTERS,
    MAX_SECTION_FILTER_BYTES, MAX_SECTION_PAYLOAD_BYTES, TS_PACKET_SIZE,
};
use maleicacid_tuner_hal_descrambler::{
    descramble_ts_packet_in_place, DescrambleFailure, DescrambleOutcome, DescramblerKeySlot,
    Multi2KeyMaterial,
};
use maleicacid_tuner_hal_frontend_dvb::{DvbFrontendBackend, DvbLiveStreamReader};
use maleicacid_tuner_hal_frontend_px4::{
    reportable_bs_tsid_for_scan, Px4FrontendBackend, Px4LiveStreamReader,
};
use maleicacid_tuner_hal_soft_demux::{
    demux_link_caps_for_ts_filter_linkage,
    sections::{normalize_length_field_bits, parse_section_header},
    AvFilterStreamKind, AvPayloadMetadata, DemuxConfigError, DemuxCore, DemuxFilterRecord,
    DemuxHandle, DemuxPathDirection, DvrConfig, FilterConfig, FilterConfigKind,
    FilterDelayHintState, FilterOpenType, FilterPayload, SectionCondition, SectionConditionKind,
    DEMUX_FILTER_MAIN_TYPE_COUNT, DEMUX_FILTER_MAIN_TYPE_TS_BITS,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_void, CString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

type TunerQueueDesc = CommonMqDescriptor<i8, CommonSynchronizedReadWrite>;
type TunerNativeHandle = CommonNativeHandle;

fn empty_native_handle() -> TunerNativeHandle {
    TunerNativeHandle {
        fds: Vec::new(),
        ints: Vec::new(),
    }
}

const TUNER_EVENT_DATA_READY: u32 = 1 << 0;
const TUNER_EVENT_DATA_OVERFLOW: u32 = 1 << 1;
const FILTER_MONITOR_MASK_STATUS: i32 = 1 << 0;
const FILTER_MONITOR_MASK_EVENT: i32 = 1 << 1;
const SUPPORTED_FILTER_MONITOR_MASK: i32 = 0;
const AV_SLOT_COUNT: usize = 32;
const AV_MIN_SLOT_SIZE: usize = 256 * 1024;
const AV_DEBUG_LOG_INTERVAL: u64 = 64;
const DVR_DEFAULT_STATUS_CHECK_INTERVAL_MS: i64 = 25;
const LOCK_TIMEOUT_MS: u64 = 5_000;
const PX4_PATH_DIAGNOSTIC_TIMEOUT_MS: u64 = LOCK_TIMEOUT_MS;
const ERRNO_EIO: i32 = 5;
const ERRNO_EACCES: i32 = 13;
const ERRNO_ENOENT: i32 = 2;
const ERRNO_ENOMEM: i32 = 12;
const ERRNO_EINVAL: i32 = 22;
const MAX_LIVE_DEMUXES: usize = 8;
const SUPPORTED_DEMUX_FILTER_CAPS: i32 = DemuxFilterMainType::TS.0;
const DEMUX_ID_BASE: i32 = 0;
const JAPAN_CATV_C13_CENTER_HZ: i64 = 111_142_857;
const JAPAN_CATV_C63_CENTER_HZ: i64 = 465_142_857;
const JAPAN_UHF_13_CENTER_HZ: i64 = 473_142_857;
const JAPAN_UHF_62_CENTER_HZ: i64 = 767_142_857;
const JAPAN_ISDBT_TUNE_TOLERANCE_HZ: i64 = 500_000;
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

fn reset_av_shared_handle_export_epoch(flag: &AtomicBool) {
    flag.store(false, Ordering::SeqCst);
}

#[cfg(target_arch = "x86_64")]
const SYS_MEMFD_CREATE: isize = 319;
#[cfg(target_arch = "x86")]
const SYS_MEMFD_CREATE: isize = 356;
#[cfg(target_arch = "aarch64")]
const SYS_MEMFD_CREATE: isize = 279;

#[repr(C)]
struct TunerFmqQueue(c_void);

fn poisoned_lock_status(name: &'static str) -> Status {
    eprintln!("maleicacid-tuner-hal: mutex poison fail-closed: {name}");
    Status::from(StatusCode::UNKNOWN_ERROR)
}

fn lock_mutex_status<'a, T>(
    mutex: &'a Mutex<T>,
    name: &'static str,
) -> BinderResult<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| poisoned_lock_status(name))
}

fn lock_mutex_hal<'a, T>(
    mutex: &'a Mutex<T>,
    name: &'static str,
) -> Result<MutexGuard<'a, T>, HalError> {
    mutex
        .lock()
        .map_err(|_| HalError::Internal(format!("poisoned mutex fail-closed: {name}")))
}

fn lock_mutex_io<'a, T>(
    mutex: &'a Mutex<T>,
    name: &'static str,
) -> std::io::Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("poisoned mutex fail-closed: {name}"),
        )
    })
}

fn lock_mutex_option<'a, T>(mutex: &'a Mutex<T>, name: &'static str) -> Option<MutexGuard<'a, T>> {
    match mutex.lock() {
        Ok(guard) => Some(guard),
        Err(_) => {
            eprintln!("maleicacid-tuner-hal: mutex poison fail-closed: {name}");
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerExit {
    Normal,
    StopRequested,
    RuntimeFailure,
    PanicOrJoinFailure,
}

impl WorkerExit {
    const Cancelled: WorkerExit = WorkerExit::StopRequested;
    const Error: WorkerExit = WorkerExit::RuntimeFailure;
    const Panic: WorkerExit = WorkerExit::PanicOrJoinFailure;

    fn is_abnormal(self) -> bool {
        matches!(
            self,
            WorkerExit::RuntimeFailure | WorkerExit::PanicOrJoinFailure
        )
    }
}

trait IntoWorkerExit {
    fn into_worker_exit(self) -> WorkerExit;
}

impl IntoWorkerExit for () {
    fn into_worker_exit(self) -> WorkerExit {
        WorkerExit::Normal
    }
}

impl IntoWorkerExit for WorkerExit {
    fn into_worker_exit(self) -> WorkerExit {
        self
    }
}

type WorkerJoinHandle = JoinHandle<WorkerExit>;

static WORKER_PANIC_COUNT: AtomicU64 = AtomicU64::new(0);
static WORKER_ERROR_COUNT: AtomicU64 = AtomicU64::new(0);

fn spawn_worker_with_exit_hook<F, R, H>(
    name: &'static str,
    body: F,
    hook: H,
) -> std::io::Result<WorkerJoinHandle>
where
    F: FnOnce() -> R + Send + 'static,
    R: IntoWorkerExit + Send + 'static,
    H: FnOnce(WorkerExit) + Send + 'static,
{
    std::thread::Builder::new().name(name.to_string()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
        let exit = match result {
            Ok(value) => value.into_worker_exit(),
            Err(_) => {
                let total = WORKER_PANIC_COUNT.fetch_add(1, Ordering::SeqCst).saturating_add(1);
                eprintln!(
                    "maleicacid-tuner-hal-worker: panic stop fail-closed: worker={} worker_panic_count={}",
                    name, total
                );
                WorkerExit::Panic
            }
        };
        if matches!(exit, WorkerExit::Error) {
            let total = WORKER_ERROR_COUNT.fetch_add(1, Ordering::SeqCst).saturating_add(1);
            eprintln!(
                "maleicacid-tuner-hal-worker: error stop fail-closed: worker={} worker_error_count={}",
                name, total
            );
        }
        hook(exit);
        exit
    })
}

fn spawn_worker<F, R>(name: &'static str, body: F) -> std::io::Result<WorkerJoinHandle>
where
    F: FnOnce() -> R + Send + 'static,
    R: IntoWorkerExit + Send + 'static,
{
    spawn_worker_with_exit_hook(name, body, |_| {})
}

fn join_worker_with_diagnostics(handle: WorkerJoinHandle, name: &'static str) -> WorkerExit {
    match handle.join() {
        Ok(exit) => {
            if exit.is_abnormal() {
                eprintln!("maleicacid-tuner-hal-worker: observed abnormal worker stop during join: worker={} exit={:?}", name, exit);
            }
            exit
        }
        Err(_) => {
            let total = WORKER_PANIC_COUNT
                .fetch_add(1, Ordering::SeqCst)
                .saturating_add(1);
            eprintln!(
                "maleicacid-tuner-hal-worker: observed uncaught panic stop during join: worker={} worker_panic_count={}",
                name, total
            );
            WorkerExit::Panic
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Default)]
struct WorkerSignalState {
    stop_requested: bool,
    work_generation: u64,
    active: bool,
    exit_reason: Option<WorkerExit>,
}

#[allow(dead_code)]
#[derive(Debug, Default)]
struct WorkerSignal {
    state: Mutex<WorkerSignalState>,
    cv: Condvar,
}

#[allow(dead_code)]
impl WorkerSignal {
    fn new(active: bool) -> Self {
        Self {
            state: Mutex::new(WorkerSignalState {
                active,
                ..WorkerSignalState::default()
            }),
            cv: Condvar::new(),
        }
    }

    fn clear_for_start(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.stop_requested = false;
            state.active = true;
            state.exit_reason = None;
            state.work_generation = state.work_generation.saturating_add(1);
        }
        self.cv.notify_all();
    }

    fn request_stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.stop_requested = true;
            state.work_generation = state.work_generation.saturating_add(1);
        }
        self.cv.notify_all();
    }

    fn notify_work(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.work_generation = state.work_generation.saturating_add(1);
        }
        self.cv.notify_all();
    }

    fn is_stop_requested(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.stop_requested)
            .unwrap_or(true)
    }

    fn wait_until_work_or_stop(&self, observed_generation: &mut u64, timeout: Duration) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return true;
        };
        loop {
            if state.stop_requested {
                return true;
            }
            if state.work_generation != *observed_generation {
                *observed_generation = state.work_generation;
                return false;
            }
            let Ok((next_state, wait_result)) = self.cv.wait_timeout(state, timeout) else {
                return true;
            };
            state = next_state;
            if wait_result.timed_out() {
                return false;
            }
        }
    }

    fn wait_timeout_or_stop(&self, timeout: Duration) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return true;
        };
        if state.stop_requested {
            return true;
        }
        let Ok((next_state, _)) = self.cv.wait_timeout(state, timeout) else {
            return true;
        };
        state = next_state;
        state.stop_requested
    }

    fn set_exit_reason(&self, exit: WorkerExit) {
        if let Ok(mut state) = self.state.lock() {
            state.exit_reason = Some(exit);
            state.active = false;
        }
        self.cv.notify_all();
    }
}

struct ManagedWorker {
    name: &'static str,
    signal: Arc<WorkerSignal>,
    legacy_stop: Option<Arc<AtomicBool>>,
    handle: Option<WorkerJoinHandle>,
}

#[allow(dead_code)]
impl ManagedWorker {
    fn new(name: &'static str, signal: Arc<WorkerSignal>, handle: WorkerJoinHandle) -> Self {
        Self {
            name,
            signal,
            legacy_stop: None,
            handle: Some(handle),
        }
    }

    fn request_stop(&self) {
        self.signal.request_stop();
        if let Some(stop) = self.legacy_stop.as_ref() {
            stop.store(true, Ordering::SeqCst);
        }
    }

    fn stop_and_join(&mut self) -> WorkerExit {
        self.request_stop();
        let exit = if let Some(handle) = self.handle.take() {
            join_worker_with_diagnostics(handle, self.name)
        } else {
            WorkerExit::Normal
        };
        self.signal.set_exit_reason(exit);
        exit
    }
}

impl Drop for ManagedWorker {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

fn sleep_with_stop(stop: &AtomicBool, interval: Duration) {
    let start = Instant::now();
    let slice = Duration::from_millis(100);
    while start.elapsed() < interval {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let elapsed = start.elapsed();
        if elapsed >= interval {
            break;
        }
        let remaining = interval - elapsed;
        thread::sleep(if remaining < slice { remaining } else { slice });
    }
}

fn spawn_managed_worker_with_exit_hook<F, H>(
    name: &'static str,
    body: F,
    hook: H,
) -> std::io::Result<ManagedWorker>
where
    F: FnOnce(Arc<AtomicBool>) + Send + 'static,
    H: FnOnce(WorkerExit) + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let signal = Arc::new(WorkerSignal::new(true));
    let stop_for_thread = Arc::clone(&stop);
    let handle = spawn_worker_with_exit_hook(
        name,
        move || {
            body(Arc::clone(&stop_for_thread));
            if stop_for_thread.load(Ordering::SeqCst) {
                WorkerExit::Cancelled
            } else {
                WorkerExit::Normal
            }
        },
        hook,
    )?;
    Ok(ManagedWorker {
        name,
        signal,
        legacy_stop: Some(stop),
        handle: Some(handle),
    })
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
    fn tuner_fmq_queue_create(num_bytes: usize, configure_event_flag: bool) -> *mut TunerFmqQueue;
    fn tuner_fmq_queue_destroy(queue: *mut TunerFmqQueue);
    fn tuner_fmq_queue_available_to_read(queue: *const TunerFmqQueue) -> usize;
    fn tuner_fmq_queue_available_to_write(queue: *const TunerFmqQueue) -> usize;
    fn tuner_fmq_queue_write(queue: *mut TunerFmqQueue, data: *const u8, size: usize) -> usize;
    fn tuner_fmq_queue_read(queue: *mut TunerFmqQueue, data: *mut u8, size: usize) -> usize;
    fn tuner_fmq_queue_wake(queue: *mut TunerFmqQueue, bits: u32) -> i32;
    fn tuner_fmq_queue_quantum(queue: *const TunerFmqQueue) -> i32;
    fn tuner_fmq_queue_flags(queue: *const TunerFmqQueue) -> i32;
    fn tuner_fmq_queue_grantor_count(queue: *const TunerFmqQueue) -> usize;
    fn tuner_fmq_queue_grantor_at(
        queue: *const TunerFmqQueue,
        index: usize,
        fd_index: *mut i32,
        offset: *mut i32,
        extent: *mut i64,
    ) -> bool;
    fn tuner_fmq_queue_fd_count(queue: *const TunerFmqQueue) -> usize;
    fn tuner_fmq_queue_dup_fd_at(queue: *const TunerFmqQueue, index: usize) -> i32;
    fn tuner_fmq_queue_int_count(queue: *const TunerFmqQueue) -> usize;
    fn tuner_fmq_queue_int_at(queue: *const TunerFmqQueue, index: usize, value: *mut i32) -> bool;
    fn tuner_dmabuf_heap_alloc_system(len: usize) -> i32;
}

struct SharedMemoryBacking {
    queue: *mut TunerFmqQueue,
    stop: AtomicBool,
    wake: Arc<(Mutex<bool>, Condvar)>,
    worker: Mutex<Option<WorkerJoinHandle>>,
    playback_worker_failed: AtomicBool,
    playback_residual: Mutex<TsPacketCompletionBuffer>,
    playback_malformed_bytes: AtomicU64,
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
struct AvBufferSlice {
    slot_index: usize,
    offset: usize,
    len: usize,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AvSharedStats {
    allocated_slots: usize,
    free_slots: usize,
    evicted_slots: u64,
    released_slots: u64,
    stale_releases: u64,
    alloc_failures: u64,
    av_overflow_no_slot: u64,
    av_invalid_payload: u64,
}

impl AvSharedStats {
    fn summary(&self) -> String {
        format!(
            "allocated={} free={} evicted={} released={} stale={} alloc_failures={} av_overflow_no_slot={} av_invalid_payload={}",
            self.allocated_slots,
            self.free_slots,
            self.evicted_slots,
            self.released_slots,
            self.stale_releases,
            self.alloc_failures,
            self.av_overflow_no_slot,
            self.av_invalid_payload,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AvPayloadDeliveryResult {
    Delivered {
        slice: AvBufferSlice,
        av_data_id: i64,
    },
    DroppedNoSharedHandle,
    DroppedNoFreeSlot,
    DroppedInvalidPayload,
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

fn av_payload_should_emit_data_event(is_media: bool, av_slice: Option<AvBufferSlice>) -> bool {
    !is_media || av_slice.is_some()
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

    fn reset_for_stream_boundary(&self) {
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
    av_invalid_payload: Mutex<u64>,
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

    fn new(slot_size_hint: usize) -> BinderResult<Arc<Self>> {
        let slot_size = slot_size_hint.max(AV_MIN_SLOT_SIZE).next_power_of_two();
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
            av_invalid_payload: Mutex::new(0),
        }))
    }

    fn allocate(
        &self,
        av_data_id: i64,
        payload: &[u8],
    ) -> Result<AvBufferSlice, AvPayloadAllocateError> {
        if payload.is_empty() || payload.len() > self.slot_size {
            self.record_invalid_payload()?;
            return Err(AvPayloadAllocateError::Delivery(
                AvPayloadDeliveryResult::DroppedInvalidPayload,
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
            self.record_invalid_payload()?;
            return Err(AvPayloadAllocateError::Delivery(
                AvPayloadDeliveryResult::DroppedInvalidPayload,
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
        *no_slot += 1;
        drop(no_slot);
        let Ok(mut failures) = lock_mutex_hal(&self.alloc_failures, "av_shared_alloc_failures")
        else {
            return Err(AvPayloadAllocateError::Internal(
                AvPayloadInternalError::CounterFailure,
            ));
        };
        *failures += 1;
        Ok(())
    }

    fn record_invalid_payload(&self) -> Result<(), AvPayloadAllocateError> {
        let Ok(mut invalid) = lock_mutex_hal(&self.av_invalid_payload, "av_invalid_payload") else {
            return Err(AvPayloadAllocateError::Internal(
                AvPayloadInternalError::CounterFailure,
            ));
        };
        *invalid += 1;
        drop(invalid);
        let Ok(mut failures) = lock_mutex_hal(&self.alloc_failures, "av_shared_alloc_failures")
        else {
            return Err(AvPayloadAllocateError::Internal(
                AvPayloadInternalError::CounterFailure,
            ));
        };
        *failures += 1;
        Ok(())
    }

    fn total_size(&self) -> usize {
        self.slot_size.saturating_mul(self.slot_count)
    }

    fn release_all(&self) {
        let released_count = {
            let Some(mut active) = lock_mutex_option(&self.active, "av_shared_active") else {
                return;
            };
            let released_count = active.len() as u64;
            active.clear();
            released_count
        };
        let Some(mut free_slots) = lock_mutex_option(&self.free_slots, "av_shared_free_slots")
        else {
            return;
        };
        free_slots.clear();
        free_slots.extend(0..self.slot_count);
        if released_count > 0 {
            if let Some(mut released) =
                lock_mutex_option(&self.released_slots, "av_shared_released_slots")
            {
                *released += released_count;
            }
        }
    }

    fn release(&self, av_data_id: i64) -> bool {
        let Some(removed) = lock_mutex_option(&self.active, "av_shared_active")
            .map(|mut active| active.remove(&av_data_id))
        else {
            return false;
        };
        if let Some(slice) = removed {
            if let Some(mut free_slots) =
                lock_mutex_option(&self.free_slots, "av_shared_free_slots")
            {
                free_slots.insert(slice.slot_index);
            }
            if let Some(mut released) =
                lock_mutex_option(&self.released_slots, "av_shared_released_slots")
            {
                *released += 1;
            }
            true
        } else {
            let stale_total = {
                let Some(mut stale) =
                    lock_mutex_option(&self.stale_releases, "av_shared_stale_releases")
                else {
                    return false;
                };
                *stale += 1;
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
            false
        }
    }

    fn debug_dump_line(&self, owner: &str) -> String {
        format!("{} av_shared {}", owner, self.stats().summary())
    }

    fn stats(&self) -> AvSharedStats {
        AvSharedStats {
            allocated_slots: lock_mutex_option(&self.active, "av_shared_active")
                .map(|active| active.len())
                .unwrap_or(0),
            free_slots: lock_mutex_option(&self.free_slots, "av_shared_free_slots")
                .map(|free_slots| free_slots.len())
                .unwrap_or(0),
            evicted_slots: lock_mutex_option(&self.evicted_slots, "av_shared_evicted_slots")
                .map(|v| *v)
                .unwrap_or(0),
            released_slots: lock_mutex_option(&self.released_slots, "av_shared_released_slots")
                .map(|v| *v)
                .unwrap_or(0),
            stale_releases: lock_mutex_option(&self.stale_releases, "av_shared_stale_releases")
                .map(|v| *v)
                .unwrap_or(0),
            alloc_failures: lock_mutex_option(&self.alloc_failures, "av_shared_alloc_failures")
                .map(|v| *v)
                .unwrap_or(0),
            av_overflow_no_slot: lock_mutex_option(
                &self.av_overflow_no_slot,
                "av_overflow_no_slot",
            )
            .map(|v| *v)
            .unwrap_or(0),
            av_invalid_payload: lock_mutex_option(&self.av_invalid_payload, "av_invalid_payload")
                .map(|v| *v)
                .unwrap_or(0),
        }
    }

    fn clear(&self) {
        {
            let Some(mut active) = lock_mutex_option(&self.active, "av_shared_active") else {
                return;
            };
            active.clear();
        }
        let Some(mut free_slots) = lock_mutex_option(&self.free_slots, "av_shared_free_slots")
        else {
            return;
        };
        free_slots.clear();
        free_slots.extend(0..self.slot_count);
        if let Some(mut next) =
            lock_mutex_option(&self.next_generation, "av_shared_next_generation")
        {
            *next = 1;
        }
    }

    fn build_native_handle(&self) -> BinderResult<TunerNativeHandle> {
        let dup = lock_mutex_status(&self.file, "av_shared_file")?
            .try_clone()
            .map_err(|_| Status::from(StatusCode::UNKNOWN_ERROR))?;
        Ok(TunerNativeHandle {
            fds: vec![ParcelFileDescriptor::new(dup)],
            ints: vec![0],
        })
    }
}

impl SharedMemoryBacking {
    fn new_ring(len: usize) -> BinderResult<Arc<Self>> {
        let data_len = len.max(4096);
        let queue = unsafe { tuner_fmq_queue_create(data_len, true) };
        if queue.is_null() {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        Ok(Arc::new(Self {
            queue,
            stop: AtomicBool::new(false),
            wake: Arc::new((Mutex::new(false), Condvar::new())),
            worker: Mutex::new(None),
            playback_worker_failed: AtomicBool::new(false),
            playback_residual: Mutex::new(TsPacketCompletionBuffer::default()),
            playback_malformed_bytes: AtomicU64::new(0),
        }))
    }

    fn new_playback_consumer(
        state: Arc<Mutex<DemuxHandle>>,
        runtime_io: Arc<RuntimeIoRegistry>,
        dvr_id: i32,
        len: usize,
    ) -> BinderResult<Arc<Self>> {
        let backing = Self::new_ring(len)?;
        let mut worker_slot = lock_mutex_status(&backing.worker, "shared_memory_worker")?;
        let backing_clone = Arc::clone(&backing);
        let backing_hook = Arc::clone(&backing);
        let runtime_io_hook = Arc::clone(&runtime_io);
        let state_hook = Arc::clone(&state);
        let handle = spawn_worker_with_exit_hook(
            "dvr_playback_consumer",
            move || {
                while !backing_clone.stop.load(Ordering::SeqCst) {
                    match backing_clone.consume_playback_ring(&state, dvr_id) {
                        Ok(PlaybackConsumeState::Consumed) => {}
                        Ok(PlaybackConsumeState::Empty) => {
                            backing_clone.wait_for_stop_or_timeout(Duration::from_millis(10))
                        }
                        Err(err) => {
                            backing_clone.fail_playback_worker(
                                &state,
                                &runtime_io,
                                dvr_id,
                                &format!("dvr_playback_consumer_failed: {err}"),
                            );
                            return WorkerExit::Error;
                        }
                    }
                }
                WorkerExit::Cancelled
            },
            move |exit| {
                if exit.is_abnormal() {
                    backing_hook.fail_playback_worker(
                        &state_hook,
                        &runtime_io_hook,
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
        drop(worker_slot);
        Ok(backing)
    }

    fn write_bytes(&self, bytes: &[u8]) -> std::io::Result<RingWriteResult> {
        if bytes.is_empty() {
            return Ok(RingWriteResult::default());
        }
        let available = unsafe { tuner_fmq_queue_available_to_write(self.queue) };
        if available < bytes.len() {
            unsafe {
                let _ = tuner_fmq_queue_wake(self.queue, TUNER_EVENT_DATA_OVERFLOW);
            }
            return Ok(RingWriteResult {
                start_offset: 0,
                len: 0,
                overflowed: true,
            });
        }
        let written = unsafe { tuner_fmq_queue_write(self.queue, bytes.as_ptr(), bytes.len()) };
        if written > 0 {
            unsafe {
                let _ = tuner_fmq_queue_wake(self.queue, TUNER_EVENT_DATA_READY);
            }
            self.wake_waiters();
        }
        Ok(RingWriteResult {
            start_offset: 0,
            len: written,
            overflowed: written < bytes.len(),
        })
    }

    fn wake_waiters(&self) {
        let (lock, cv) = &*self.wake;
        if let Ok(mut guard) = lock.lock() {
            *guard = true;
        }
        cv.notify_all();
    }

    fn wait_for_stop_or_timeout(&self, interval: Duration) {
        if self.stop.load(Ordering::SeqCst) {
            return;
        }
        let (lock, cv) = &*self.wake;
        let Ok(mut guard) = lock.lock() else {
            return;
        };
        if *guard {
            *guard = false;
            return;
        }
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            let Ok((next_guard, wait_result)) = cv.wait_timeout(guard, interval) else {
                return;
            };
            guard = next_guard;
            if *guard {
                *guard = false;
                return;
            }
            if wait_result.timed_out() {
                return;
            }
        }
    }

    fn consume_playback_ring(
        &self,
        state: &Arc<Mutex<DemuxHandle>>,
        dvr_id: i32,
    ) -> std::io::Result<PlaybackConsumeState> {
        {
            let demux = lock_mutex_io(state, "demux_handle")?;
            let Some(dvr) = demux.dvr_record(dvr_id) else {
                return Ok(PlaybackConsumeState::Empty);
            };
            if !dvr.started || dvr.direction != DemuxPathDirection::Playback {
                return Ok(PlaybackConsumeState::Empty);
            }
        }
        let available = unsafe { tuner_fmq_queue_available_to_read(self.queue) };
        if available == 0 {
            return Ok(PlaybackConsumeState::Empty);
        }
        let mut payload = vec![0u8; available];
        let read = unsafe { tuner_fmq_queue_read(self.queue, payload.as_mut_ptr(), payload.len()) };
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "playback FMQ reported readable bytes but returned no data",
            ));
        }
        payload.truncate(read);
        let drain = {
            let mut residual = self.playback_residual.lock().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "playback residual buffer poisoned",
                )
            })?;
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
            return Ok(PlaybackConsumeState::Consumed);
        }
        let mut aligned = Vec::with_capacity(drain.packets.len() * TS_PACKET_SIZE);
        for packet in drain.packets {
            aligned.extend_from_slice(&packet);
        }
        let mut demux = lock_mutex_io(state, "demux_handle")?;
        if !demux.inject_playback_payload(dvr_id, &aligned) {
            eprintln!(
                "maleicacid-tuner-hal-dvr-playback-diagnostic: dvr_id={} payload_rejected_outside_started_playback_state bytes={}",
                dvr_id,
                aligned.len()
            );
            return Ok(PlaybackConsumeState::Consumed);
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
        dvr_id: i32,
        reason: &str,
    ) {
        self.playback_worker_failed.store(true, Ordering::SeqCst);
        runtime_io.mark_failed(RuntimeIoKind::Dvr, dvr_id, reason);
        self.stop_best_effort();
        eprintln!(
            "maleicacid-tuner-hal-worker: dvr_playback_consumer abnormal stop dvr_id={} reason={}",
            dvr_id, reason
        );
        if let Some(mut demux) = lock_mutex_option(state, "demux_handle") {
            demux.unregister_dvr(dvr_id);
        }
    }

    fn build_queue_desc(&self) -> BinderResult<TunerQueueDesc> {
        let grantor_count = unsafe { tuner_fmq_queue_grantor_count(self.queue) };
        let mut grantors = Vec::with_capacity(grantor_count);
        for i in 0..grantor_count {
            let (mut fd_index, mut offset, mut extent) = (0i32, 0i32, 0i64);
            let ok = unsafe {
                tuner_fmq_queue_grantor_at(self.queue, i, &mut fd_index, &mut offset, &mut extent)
            };
            if !ok {
                return Err(Status::from(StatusCode::UNKNOWN_ERROR));
            }
            grantors.push(CommonGrantorDescriptor {
                fdIndex: fd_index,
                offset,
                extent,
            });
        }
        let fd_count = unsafe { tuner_fmq_queue_fd_count(self.queue) };
        let mut fds = Vec::with_capacity(fd_count);
        for i in 0..fd_count {
            let fd = unsafe { tuner_fmq_queue_dup_fd_at(self.queue, i) };
            if fd < 0 {
                return Err(Status::from(StatusCode::UNKNOWN_ERROR));
            }
            fds.push(ParcelFileDescriptor::new(unsafe { File::from_raw_fd(fd) }));
        }
        let int_count = unsafe { tuner_fmq_queue_int_count(self.queue) };
        let mut ints = Vec::with_capacity(int_count);
        for i in 0..int_count {
            let mut v = 0i32;
            let ok = unsafe { tuner_fmq_queue_int_at(self.queue, i, &mut v) };
            if !ok {
                return Err(Status::from(StatusCode::UNKNOWN_ERROR));
            }
            ints.push(v);
        }
        let handle = CommonNativeHandle { fds, ints };
        let mut desc = TunerQueueDesc::default();
        desc.grantors = grantors;
        desc.handle = handle;
        desc.quantum = unsafe { tuner_fmq_queue_quantum(self.queue) };
        desc.flags = unsafe { tuner_fmq_queue_flags(self.queue) };
        Ok(desc)
    }

    fn clear(&self) {
        let available = unsafe { tuner_fmq_queue_available_to_read(self.queue) };
        if available > 0 {
            let mut sink = vec![0u8; available];
            let _ = unsafe { tuner_fmq_queue_read(self.queue, sink.as_mut_ptr(), sink.len()) };
        }
        if let Ok(mut residual) = self.playback_residual.lock() {
            residual.clear();
        }
        self.playback_malformed_bytes.store(0, Ordering::SeqCst);
    }

    fn current_fill_bytes(&self) -> usize {
        unsafe { tuner_fmq_queue_available_to_read(self.queue) }
    }

    fn stop(&self) -> BinderResult<()> {
        self.stop.store(true, Ordering::SeqCst);
        self.wake_waiters();
        if let Some(handle) = lock_mutex_status(&self.worker, "shared_memory_worker")?.take() {
            join_worker_with_diagnostics(handle, "shared_memory_worker");
        }
        Ok(())
    }

    fn stop_best_effort(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake_waiters();
        if let Some(handle) = lock_mutex_option(&self.worker, "shared_memory_worker")
            .and_then(|mut worker| worker.take())
        {
            join_worker_with_diagnostics(handle, "shared_memory_worker");
        }
    }
}

impl Drop for SharedMemoryBacking {
    fn drop(&mut self) {
        unsafe { tuner_fmq_queue_destroy(self.queue) };
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum RuntimeIoKind {
    Filter,
    Dvr,
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
    ) {
        let Some(mut entries) = lock_mutex_option(&self.entries, "runtime_io_entries") else {
            return;
        };
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
    }

    fn set_filter_av_shared(&self, filter_id: i32, av_shared: &Arc<AvSharedBacking>) {
        let Some(mut entries) = lock_mutex_option(&self.entries, "runtime_io_entries") else {
            return;
        };
        let entry = entries
            .entry(RuntimeIoKey {
                kind: RuntimeIoKind::Filter,
                id: filter_id,
            })
            .or_insert_with(RuntimeIoBackings::default);
        entry.filter_av_shared = Some(Arc::downgrade(av_shared));
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

    fn register_dvr(&self, dvr_id: i32, queue: &Arc<SharedMemoryBacking>) {
        let Some(mut entries) = lock_mutex_option(&self.entries, "runtime_io_entries") else {
            return;
        };
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

    fn is_failed_for_owner_validation(&self, kind: RuntimeIoKind, id: i32) -> bool {
        let Some(entries) = lock_mutex_option(&self.entries, "runtime_io_entries") else {
            return true;
        };
        entries
            .get(&RuntimeIoKey { kind, id })
            .and_then(|entry| entry.failed_reason.as_ref())
            .is_some()
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

    fn flush_all(&self) {
        let Some(mut entries) = lock_mutex_option(&self.entries, "runtime_io_entries") else {
            return;
        };
        entries.retain(|_, backings| {
            let mut alive = false;
            if let Some(backing) = backings.filter_queue.as_ref().and_then(Weak::upgrade) {
                backing.clear();
                alive = true;
            }
            if let Some(backing) = backings.filter_av_queue.as_ref().and_then(Weak::upgrade) {
                backing.clear();
                alive = true;
            }
            if let Some(backing) = backings.filter_av_shared.as_ref().and_then(Weak::upgrade) {
                backing.clear();
                alive = true;
            }
            if let Some(backing) = backings.dvr_queue.as_ref().and_then(Weak::upgrade) {
                backing.clear();
                alive = true;
            }
            alive
        });
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
        lock_mutex_option(&self.entries, "runtime_io_entries")
            .map(|entries| entries.len())
            .unwrap_or(0)
    }
}

#[derive(Clone)]
struct BoundDemuxRuntime {
    demux_generation: u64,
    state: Arc<Mutex<DemuxHandle>>,
    runtime_io: Arc<RuntimeIoRegistry>,
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
        self.reader.lock().ok().map(|reader| reader.as_raw_fd())
    }

    fn wake(&self) {
        if let Ok(mut writer) = self.writer.lock() {
            match writer.write(&[1]) {
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
        }
    }

    #[cfg(test)]
    fn drain_for_test(&self) {
        if let Ok(mut reader) = self.reader.lock() {
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
    DescrambleFailed,
    NoKey,
    BadToken,
    CasBridgeUnconnected,
    ExpiredKeySlot,
    InvalidTsc,
    Multi2Fail,
    ScrambledWithoutDescrambler,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct DescramblerDiagnosticCounters {
    clear_packets: u64,
    descrambled_packets: u64,
    scrambled_passthrough_for_recording_packets: u64,
    descramble_failed_packets: u64,
    no_key: u64,
    bad_token: u64,
    cas_bridge_unconnected: u64,
    expired_key_slot: u64,
    invalid_tsc: u64,
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
            DescramblerDiagnosticKind::DescrambleFailed => {
                self.descramble_failed_packets = self.descramble_failed_packets.saturating_add(1)
            }
            DescramblerDiagnosticKind::NoKey => self.no_key = self.no_key.saturating_add(1),
            DescramblerDiagnosticKind::BadToken => {
                self.bad_token = self.bad_token.saturating_add(1)
            }
            DescramblerDiagnosticKind::CasBridgeUnconnected => {
                self.cas_bridge_unconnected = self.cas_bridge_unconnected.saturating_add(1)
            }
            DescramblerDiagnosticKind::ExpiredKeySlot => {
                self.expired_key_slot = self.expired_key_slot.saturating_add(1)
            }
            DescramblerDiagnosticKind::InvalidTsc => {
                self.invalid_tsc = self.invalid_tsc.saturating_add(1)
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
            "CLEAR_PACKET={} DESCRAMBLED={} SCRAMBLED_PASSTHROUGH_FOR_RECORDING={} DESCRAMBLE_FAILED={} NO_KEY={} BAD_TOKEN={} CAS_BRIDGE_UNCONNECTED={} EXPIRED_KEY_SLOT={} INVALID_TSC={} MULTI2_FAIL={} SCRAMBLED_WITHOUT_DESCRAMBLER={}",
            self.clear_packets,
            self.descrambled_packets,
            self.scrambled_passthrough_for_recording_packets,
            self.descramble_failed_packets,
            self.no_key,
            self.bad_token,
            self.cas_bridge_unconnected,
            self.expired_key_slot,
            self.invalid_tsc,
            self.multi2_fail,
            self.scrambled_without_descrambler,
        )
    }
}

#[derive(Default)]
struct DescramblerDiagnosticRegistry {
    counters: Mutex<BTreeMap<(i32, u16), DescramblerDiagnosticCounters>>,
}

impl DescramblerDiagnosticRegistry {
    fn new() -> Self {
        Self {
            counters: Mutex::new(BTreeMap::new()),
        }
    }

    fn record(&self, demux_id: i32, pid: u16, kind: DescramblerDiagnosticKind) {
        let Some(mut counters) =
            lock_mutex_option(&self.counters, "descrambler_diagnostic_counters")
        else {
            return;
        };
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
    DescrambleFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescramblePacketDecision {
    packet: [u8; 188],
    flow: PacketDescrambleFlow,
}

struct DescramblerRuntimeRegistry {
    next_id: AtomicI64,
    entries: Mutex<BTreeMap<i64, Weak<Mutex<TunerDescramblerState>>>>,
}

impl DescramblerRuntimeRegistry {
    fn new() -> Self {
        Self {
            next_id: AtomicI64::new(1),
            entries: Mutex::new(BTreeMap::new()),
        }
    }
}

#[derive(Clone, Debug)]
struct ActiveDescramblerSnapshot {
    pids: BTreeSet<u16>,
    key_slot: DescramblerKeySlot,
}

impl ActiveDescramblerSnapshot {
    fn targets_pid(&self, pid: u16) -> bool {
        self.pids.contains(&pid)
    }

    fn descramble_packet_in_place(&self, packet: &mut [u8]) -> Result<(), DescrambleFailure> {
        descramble_ts_packet_in_place(packet, &self.pids, &self.key_slot).map(|_| ())
    }
}

fn packet_scrambling_control(packet: &[u8; 188]) -> u8 {
    (packet[3] >> 6) & 0x03
}

fn diagnostic_kind_for_failure(failure: DescrambleFailure) -> DescramblerDiagnosticKind {
    match failure {
        DescrambleFailure::NoKey => DescramblerDiagnosticKind::NoKey,
        DescrambleFailure::BadToken => DescramblerDiagnosticKind::BadToken,
        DescrambleFailure::InvalidScramblingControl => DescramblerDiagnosticKind::InvalidTsc,
        DescrambleFailure::Multi2Fail => DescramblerDiagnosticKind::Multi2Fail,
        DescrambleFailure::ScrambledPidNotRegistered => {
            DescramblerDiagnosticKind::ScrambledWithoutDescrambler
        }
        _ => DescramblerDiagnosticKind::DescrambleFailed,
    }
}

fn descramble_packet_for_pid_with_diagnostics(
    packet: &[u8; 188],
    demux_id: i32,
    pid: u16,
    active_descramblers: &[ActiveDescramblerSnapshot],
    diagnostics: &DescramblerDiagnosticRegistry,
) -> DescramblePacketDecision {
    if packet_scrambling_control(packet) == 0 {
        diagnostics.record(demux_id, pid, DescramblerDiagnosticKind::ClearPacket);
        return DescramblePacketDecision {
            packet: *packet,
            flow: PacketDescrambleFlow::Clear,
        };
    }

    let mut saw_target_descrambler = false;
    for descrambler in active_descramblers.iter().filter(|d| d.targets_pid(pid)) {
        saw_target_descrambler = true;
        let mut candidate = *packet;
        match descramble_ts_packet_in_place(
            &mut candidate,
            &descrambler.pids,
            &descrambler.key_slot,
        ) {
            Ok(DescrambleOutcome::Descrambled { .. }) => {
                diagnostics.record(demux_id, pid, DescramblerDiagnosticKind::Descrambled);
                return DescramblePacketDecision {
                    packet: candidate,
                    flow: PacketDescrambleFlow::Descrambled,
                };
            }
            Ok(DescrambleOutcome::PassedThrough { .. }) => {
                diagnostics.record(
                    demux_id,
                    pid,
                    DescramblerDiagnosticKind::ScrambledPassthroughForRecording,
                );
                return DescramblePacketDecision {
                    packet: *packet,
                    flow: PacketDescrambleFlow::ScrambledPassthrough,
                };
            }
            Err(DescrambleFailure::NoKey) => {
                diagnostics.record(demux_id, pid, DescramblerDiagnosticKind::NoKey);
                continue;
            }
            Err(DescrambleFailure::BadToken) => {
                diagnostics.record(demux_id, pid, DescramblerDiagnosticKind::BadToken);
                continue;
            }
            Err(failure) => {
                diagnostics.record(demux_id, pid, diagnostic_kind_for_failure(failure));
                diagnostics.record(demux_id, pid, DescramblerDiagnosticKind::DescrambleFailed);
                return DescramblePacketDecision {
                    packet: *packet,
                    flow: PacketDescrambleFlow::DescrambleFailed,
                };
            }
        }
    }
    if !saw_target_descrambler {
        diagnostics.record(
            demux_id,
            pid,
            DescramblerDiagnosticKind::ScrambledWithoutDescrambler,
        );
    }
    diagnostics.record(
        demux_id,
        pid,
        DescramblerDiagnosticKind::ScrambledPassthroughForRecording,
    );
    DescramblePacketDecision {
        packet: *packet,
        flow: PacketDescrambleFlow::ScrambledPassthrough,
    }
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

fn av_shared_file_error_status(err: AvSharedFileError) -> Status {
    let detail = err.detail();
    eprintln!("maleicacid-tuner-hal-av-shared: {detail}");
    let result = match err.errno {
        ERRNO_ENOMEM => TunerResult::OUT_OF_MEMORY,
        ERRNO_ENOENT | ERRNO_EACCES | ERRNO_EINVAL => TunerResult::UNAVAILABLE,
        _ => TunerResult::UNAVAILABLE,
    };
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
    match &entry.kind {
        FrontendEntryKind::Px4 {
            allowed_systems, ..
        } => allowed_systems
            .iter()
            .any(|s| matches!(s, FrontendSystem::IsdbS)),
        FrontendEntryKind::Dvb {
            supported_systems, ..
        } => supported_systems
            .iter()
            .any(|s| matches!(s, FrontendSystem::IsdbS)),
    }
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

fn declared_type_to_system(declared_type: FrontendType) -> Option<FrontendSystem> {
    match declared_type {
        FrontendType::ISDBT => Some(FrontendSystem::IsdbT),
        FrontendType::ISDBS => Some(FrontendSystem::IsdbS),
        FrontendType::ISDBS3 | FrontendType::DVBS => None,
        _ => None,
    }
}

fn entry_supports_signal_strength(entry: &FrontendEntry) -> bool {
    matches!(&entry.kind, FrontendEntryKind::Dvb { .. })
}

fn entry_supports_rf_lock(entry: &FrontendEntry) -> bool {
    matches!(&entry.kind, FrontendEntryKind::Dvb { .. })
}

fn entry_status_supported(entry: &FrontendEntry, status_type: FrontendStatusType) -> bool {
    match status_type {
        FrontendStatusType::DEMOD_LOCK
        | FrontendStatusType::SNR
        | FrontendStatusType::SIGNAL_QUALITY => true,
        FrontendStatusType::RF_LOCK => entry_supports_rf_lock(entry),
        FrontendStatusType::SIGNAL_STRENGTH => entry_supports_signal_strength(entry),
        FrontendStatusType::LNB_VOLTAGE => entry_supports_satellite(entry),
        _ => false,
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalFilterOwnerValidationError {
    NotLocalFilter,
    ForeignDemux,
    Closed,
    RuntimeFailed,
    NotOpenDemuxFilter,
}

fn local_filter_owner_error_tuner_result(err: LocalFilterOwnerValidationError) -> i32 {
    match err {
        LocalFilterOwnerValidationError::Closed
        | LocalFilterOwnerValidationError::RuntimeFailed => TunerResult::INVALID_STATE.0,
        LocalFilterOwnerValidationError::NotLocalFilter
        | LocalFilterOwnerValidationError::ForeignDemux
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
        LocalFilterOwnerValidationError::NotOpenDemuxFilter => {
            "source filter is not an open demux filter"
        }
    };
    match local_filter_owner_error_tuner_result(err) {
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
    if local_filter
        .runtime_io
        .is_failed_for_owner_validation(RuntimeIoKind::Filter, local_filter.filter_id)
    {
        return Err(LocalFilterOwnerValidationError::RuntimeFailed);
    }
    let Some(demux) = lock_mutex_option(&local_filter.state, "demux_handle") else {
        return Err(LocalFilterOwnerValidationError::RuntimeFailed);
    };
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

fn isdbt_mode_caps() -> i32 {
    FrontendIsdbtMode::AUTO.0 | FrontendIsdbtMode::MODE_3.0
}

fn isdbt_bandwidth_caps() -> i32 {
    FrontendIsdbtBandwidth::AUTO.0 | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ.0
}

fn isdbt_modulation_caps() -> i32 {
    FrontendIsdbtModulation::AUTO.0
}

fn isdbt_coderate_caps() -> i32 {
    FrontendIsdbtCoderate::AUTO.0
}

fn isdbt_guard_interval_caps() -> i32 {
    FrontendIsdbtGuardInterval::AUTO.0
}

fn isdbt_time_interleave_caps() -> i32 {
    FrontendIsdbtTimeInterleaveMode::AUTO.0
}

fn isdbs_modulation_caps() -> i32 {
    FrontendIsdbsModulation::AUTO.0
}

fn isdbs_coderate_caps() -> i32 {
    FrontendIsdbsCoderate::AUTO.0
}

fn entry_frontend_frequency_contract(entry: &FrontendEntry) -> (i64, i64, i64) {
    match &entry.kind {
        FrontendEntryKind::Px4 { declared_type, .. } if *declared_type == FrontendType::ISDBT => (
            JAPAN_CATV_C13_CENTER_HZ - JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
            JAPAN_UHF_62_CENTER_HZ + JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
            JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
        ),
        FrontendEntryKind::Px4 { declared_type, .. } if *declared_type == FrontendType::ISDBS => {
            (JAPAN_BS_FIRST_IF_HZ, JAPAN_CS110_LAST_IF_HZ, 0)
        }
        FrontendEntryKind::Dvb { declared_type, .. } if *declared_type == FrontendType::ISDBT => (
            JAPAN_UHF_13_CENTER_HZ - JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
            JAPAN_UHF_62_CENTER_HZ + JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
            JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
        ),
        FrontendEntryKind::Dvb { declared_type, .. } if *declared_type == FrontendType::ISDBS => {
            (JAPAN_BS_FIRST_IF_HZ, JAPAN_CS110_LAST_IF_HZ, 0)
        }
        _ => (0, 0, 0),
    }
}

fn entry_frontend_max_symbol_rate_contract(_entry: &FrontendEntry) -> i32 {
    // r51 の ISDB-T / ISDB-S public contract は explicit symbolRate を使わない。
    // driver probe が nonzero を返しても advertise しない。
    0
}

fn entry_frontend_caps(entry: &FrontendEntry) -> FrontendCapabilities {
    match &entry.kind {
        FrontendEntryKind::Px4 { declared_type, .. }
        | FrontendEntryKind::Dvb { declared_type, .. } => match *declared_type {
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
            FrontendType::ISDBS3 | FrontendType::DVBS => Default::default(),
            _ => Default::default(),
        },
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
        let declared_type = if probe
            .supported_systems
            .iter()
            .any(|s| matches!(s, FrontendSystem::IsdbT))
        {
            FrontendType::ISDBT
        } else if probe
            .supported_systems
            .iter()
            .any(|s| matches!(s, FrontendSystem::IsdbS))
        {
            FrontendType::ISDBS
        } else {
            eprintln!(
                "maleicacid-tuner-hal: 対象外 DVB frontend probe を無視します {:?}",
                probe
            );
            continue;
        };
        let id = 10_000 + probe.adapter_id * 10 + probe.frontend_index;
        let (min_frequency_hz, max_frequency_hz) = declared_type_to_system(declared_type)
            .map(|system| probe.normalized_frequency_range_hz(system))
            .unwrap_or((0, 0));
        entries.push(FrontendEntry {
            id,
            kind: FrontendEntryKind::Dvb {
                adapter: probe.adapter_id,
                frontend_index: probe.frontend_index,
                demux_index: probe.demux_index,
                dvr_index: probe.dvr_index,
                declared_type,
                supported_systems: probe
                    .supported_systems
                    .into_iter()
                    .filter(|s| matches!(s, FrontendSystem::IsdbT | FrontendSystem::IsdbS))
                    .collect(),
                min_frequency_hz,
                max_frequency_hz,
                max_symbol_rate: probe.max_symbol_rate,
            },
        });
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
    PxMltDevice15VOnly,
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
    {
        return LnbDeviceProfile::PxMltDevice15VOnly;
    }
    if name.starts_with("isdb2056video")
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
        return 0;
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
}

struct FrontendRuntime {
    frontend_id: i32,
    allowed_systems: Vec<FrontendSystem>,
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
    pump_stop: AtomicBool,
    pump_wake: Arc<(Mutex<bool>, Condvar)>,
    pump_wake_fd: Option<Arc<LivePumpWake>>,
    pump_worker: Mutex<Option<WorkerJoinHandle>>,
}

impl FrontendRuntime {
    fn new(
        entry: FrontendEntry,
        lnb_registry: Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
        descrambler_registry: Arc<DescramblerRuntimeRegistry>,
        descrambler_diagnostics: Arc<DescramblerDiagnosticRegistry>,
    ) -> Arc<Self> {
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
                declared_type_to_system(*declared_type)
                    .into_iter()
                    .collect(),
                FrontendBackendState::Dvb(DvbFrontendBackend::new(
                    *adapter,
                    *frontend_index,
                    *demux_index,
                    *dvr_index,
                    supported_systems.clone(),
                )),
            ),
        };
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
            pump_stop: AtomicBool::new(false),
            pump_wake: Arc::new((Mutex::new(false), Condvar::new())),
            pump_wake_fd: LivePumpWake::new().ok().map(Arc::new),
            pump_worker: Mutex::new(None),
        })
    }

    fn bind_demux(
        self: &Arc<Self>,
        state: Arc<Mutex<DemuxHandle>>,
        runtime_io: Arc<RuntimeIoRegistry>,
        demux_generation: u64,
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
                let _ = self.stop_live_pump();
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

    fn reset_bound_demuxes_for_stream_boundary(&self) {
        if self.is_px4_backend() {
            self.px4_path_diagnostics.reset_for_stream_boundary();
        }
        let demuxes: Vec<BoundDemuxRuntime> =
            lock_mutex_option(&self.bound_demuxes, "frontend_bound_demuxes")
                .map(|demuxes| demuxes.values().cloned().collect())
                .unwrap_or_default();
        for bound in demuxes {
            if let Some(mut handle) = lock_mutex_option(&bound.state, "demux_handle") {
                handle.reset_for_stream_boundary();
            }
            bound.runtime_io.flush_all();
        }
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
            if let Some(mut demux) = lock_mutex_option(&bound.state, "demux_handle") {
                demux.close();
            }
            bound.runtime_io.flush_all();
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
        let (lock, cv) = &*self.pump_wake;
        if let Ok(mut guard) = lock.lock() {
            *guard = true;
        }
        cv.notify_all();
    }

    fn wait_live_pump_interval(&self, interval: Duration) {
        if self.pump_stop.load(Ordering::SeqCst) {
            return;
        }
        let (lock, cv) = &*self.pump_wake;
        let Ok(mut guard) = lock.lock() else {
            return;
        };
        if *guard {
            *guard = false;
            return;
        }
        loop {
            if self.pump_stop.load(Ordering::SeqCst) {
                return;
            }
            let Ok((next_guard, wait_result)) = cv.wait_timeout(guard, interval) else {
                return;
            };
            guard = next_guard;
            if *guard {
                *guard = false;
                return;
            }
            if wait_result.timed_out() {
                return;
            }
        }
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
            let exit = join_worker_with_diagnostics(worker, "frontend_pump_worker");
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
        match spawn_worker_with_exit_hook(
            "frontend_live_pump",
            move || runtime_for_body.live_pump_loop(),
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
            join_worker_with_diagnostics(worker, "frontend_pump_worker");
        }
        Ok(())
    }

    fn stop_live_pump_best_effort(&self) {
        self.pump_stop.store(true, Ordering::SeqCst);
        self.wake_live_pump();
        let worker = lock_mutex_option(&self.pump_worker, "frontend_pump_worker")
            .and_then(|mut worker| worker.take());
        if let Some(worker) = worker {
            join_worker_with_diagnostics(worker, "frontend_pump_worker");
        }
    }

    fn live_pump_loop(self: Arc<Self>) -> WorkerExit {
        while !self.pump_stop.load(Ordering::SeqCst) {
            let reader = {
                let Some(mut backend) = lock_mutex_option(&self.backend, "frontend_backend") else {
                    self.record_runtime_failure(
                        "worker=frontend_live_pump reason=frontend_backend_lock_failed",
                    );
                    self.mark_live_path_failed("frontend_backend_lock_failed");
                    return WorkerExit::Error;
                };
                if !FrontendHal::backend_tuning_active(&backend) {
                    None
                } else {
                    if let Err(err) = FrontendHal::apply_selected_lnb_from_registry(
                        &self.lnb_registry,
                        &mut backend,
                    ) {
                        let detail =
                            format!("worker=frontend_live_pump operation=apply_lnb error={err}");
                        self.record_runtime_failure(detail.clone());
                        self.mark_live_path_failed(&detail);
                        return WorkerExit::Error;
                    }
                    match FrontendHal::backend_live_stream_reader(&mut backend) {
                        Ok(reader) => reader,
                        Err(err) => {
                            let detail = format!(
                                "worker=frontend_live_pump operation=stream_reader error={err}"
                            );
                            self.record_runtime_failure(detail.clone());
                            self.mark_live_path_failed(&detail);
                            return WorkerExit::Error;
                        }
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
                        return WorkerExit::Error;
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
                    return WorkerExit::Error;
                };
                let demuxes: Vec<BoundDemuxRuntime> = demuxes_guard.values().cloned().collect();
                drop(demuxes_guard);
                for demux in demuxes {
                    let Some(mut handle) = lock_mutex_option(&demux.state, "demux_handle") else {
                        self.record_runtime_failure(
                            "worker=frontend_live_pump reason=demux_handle_lock_failed",
                        );
                        self.mark_live_path_failed("demux_handle_lock_failed");
                        return WorkerExit::Error;
                    };
                    let active_descramblers = self.descrambler_registry.snapshots_for_demux(
                        handle.demux_id(),
                        demux.demux_generation,
                        &handle,
                    );
                    for packet in &packets {
                        let pid = (((packet[1] & 0x1f) as i32) << 8) | packet[2] as i32;
                        if let Ok(pid_u16) = u16::try_from(pid) {
                            let decision = descramble_packet_for_pid_with_diagnostics(
                                packet,
                                handle.demux_id(),
                                pid_u16,
                                &active_descramblers,
                                &self.descrambler_diagnostics,
                            );
                            match decision.flow {
                                PacketDescrambleFlow::Clear | PacketDescrambleFlow::Descrambled => {
                                    handle.push_ts_packet(&decision.packet);
                                }
                                PacketDescrambleFlow::ScrambledPassthrough
                                | PacketDescrambleFlow::DescrambleFailed => {
                                    handle.push_ts_packet_record_only(&decision.packet);
                                }
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
                        return WorkerExit::Error;
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
                self.wait_live_pump_interval(Duration::from_millis(sleep_ms));
            }
        }
        self.pump_stop.store(true, Ordering::SeqCst);
        WorkerExit::Cancelled
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
    bound_frontend_id: Option<i32>,
    bound_frontend_generation: Option<u64>,
    ci_cam_diagnostics: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DescramblerPidRegistration {
    source_filter_id: i32,
    source_filter_generation: u64,
}

#[derive(Debug, Default)]
struct TunerDescramblerState {
    closed: bool,
    demux_id: Option<i32>,
    demux_generation: Option<u64>,
    key_token: Option<Vec<u8>>,
    key_slot: Option<DescramblerKeySlot>,
    pids: BTreeMap<u16, DescramblerPidRegistration>,
}

impl DescramblerRuntimeRegistry {
    fn register(&self, state: &Arc<Mutex<TunerDescramblerState>>) -> i64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        if let Some(mut entries) = lock_mutex_option(&self.entries, "descrambler_runtime_entries") {
            entries.insert(id, Arc::downgrade(state));
        }
        id
    }

    fn unregister(&self, id: i64) {
        if let Some(mut entries) = lock_mutex_option(&self.entries, "descrambler_runtime_entries") {
            entries.remove(&id);
        }
    }

    fn snapshots_for_demux(
        &self,
        demux_id: i32,
        demux_generation: u64,
        demux_handle: &DemuxHandle,
    ) -> Vec<ActiveDescramblerSnapshot> {
        let Some(mut entries) = lock_mutex_option(&self.entries, "descrambler_runtime_entries")
        else {
            return Vec::new();
        };
        let mut dead = Vec::new();
        let mut snapshots = Vec::new();
        for (id, weak) in entries.iter() {
            let Some(state_arc) = weak.upgrade() else {
                dead.push(*id);
                continue;
            };
            let Some(mut state) = lock_mutex_option(&state_arc, "descrambler_state") else {
                dead.push(*id);
                continue;
            };
            if state.closed {
                dead.push(*id);
                continue;
            }
            if state.demux_id != Some(demux_id) || state.demux_generation != Some(demux_generation)
            {
                state.pids.clear();
                continue;
            }
            let key_slot = match state.key_slot.clone() {
                Some(key_slot) if state.key_token.is_some() => key_slot,
                _ => continue,
            };
            state.pids.retain(|_, registration| {
                registration.source_filter_id < 0
                    || demux_handle
                        .filter_generation(registration.source_filter_id)
                        .map_or(false, |generation| {
                            generation == registration.source_filter_generation
                        })
            });
            if !state.pids.is_empty() {
                snapshots.push(ActiveDescramblerSnapshot {
                    pids: state.pids.keys().copied().collect(),
                    key_slot,
                });
            }
        }
        for id in dead {
            entries.remove(&id);
        }
        snapshots
    }

    fn invalidate_demux(&self, demux_id: i32, demux_generation: u64) {
        let Some(mut entries) = lock_mutex_option(&self.entries, "descrambler_runtime_entries")
        else {
            return;
        };
        let mut dead = Vec::new();
        for (id, weak) in entries.iter() {
            let Some(state_arc) = weak.upgrade() else {
                dead.push(*id);
                continue;
            };
            let Some(mut state) = lock_mutex_option(&state_arc, "descrambler_state") else {
                dead.push(*id);
                continue;
            };
            if state.closed {
                dead.push(*id);
                continue;
            }
            if state.demux_id == Some(demux_id) && state.demux_generation == Some(demux_generation)
            {
                state.demux_id = None;
                state.demux_generation = None;
                state.pids.clear();
            }
        }
        for id in dead {
            entries.remove(&id);
        }
    }

    fn pid_registered_by_other_descrambler(
        &self,
        current_id: i64,
        demux_id: i32,
        demux_generation: u64,
        pid: u16,
    ) -> bool {
        let Some(mut entries) = lock_mutex_option(&self.entries, "descrambler_runtime_entries")
        else {
            return true;
        };
        let mut dead = Vec::new();
        let mut found = false;
        for (id, weak) in entries.iter() {
            if *id == current_id {
                continue;
            }
            let Some(state_arc) = weak.upgrade() else {
                dead.push(*id);
                continue;
            };
            let Some(state) = lock_mutex_option(&state_arc, "descrambler_state") else {
                found = true;
                break;
            };
            if !state.closed
                && state.demux_id == Some(demux_id)
                && state.demux_generation == Some(demux_generation)
                && state.pids.contains_key(&pid)
            {
                found = true;
                break;
            }
        }
        for id in dead {
            entries.remove(&id);
        }
        found
    }
}

pub struct TunerDescrambler {
    id: i64,
    state: Arc<Mutex<TunerDescramblerState>>,
    demux_registry: Arc<Mutex<BTreeMap<i32, Arc<Mutex<DemuxRecord>>>>>,
    descrambler_registry: Arc<DescramblerRuntimeRegistry>,
    descrambler_diagnostics: Arc<DescramblerDiagnosticRegistry>,
    descrambler_key_table: Arc<DescramblerKeyTable>,
}

impl TunerDescrambler {
    fn new(
        demux_registry: Arc<Mutex<BTreeMap<i32, Arc<Mutex<DemuxRecord>>>>>,
        descrambler_registry: Arc<DescramblerRuntimeRegistry>,
        descrambler_diagnostics: Arc<DescramblerDiagnosticRegistry>,
        descrambler_key_table: Arc<DescramblerKeyTable>,
    ) -> Self {
        let state = Arc::new(Mutex::new(TunerDescramblerState::default()));
        let id = descrambler_registry.register(&state);
        Self {
            id,
            state,
            demux_registry,
            descrambler_registry,
            descrambler_diagnostics,
            descrambler_key_table,
        }
    }

    fn ensure_open_locked(state: &TunerDescramblerState) -> BinderResult<()> {
        if state.closed {
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

    fn record_key_token_error(&self, demux_id: Option<i32>, error: DescramblerKeyResolveError) {
        let diagnostic = match error {
            DescramblerKeyResolveError::CasBridgeUnconnected
            | DescramblerKeyResolveError::RegistryUnavailable => {
                DescramblerDiagnosticKind::CasBridgeUnconnected
            }
            DescramblerKeyResolveError::ExpiredKeySlot => DescramblerDiagnosticKind::ExpiredKeySlot,
            DescramblerKeyResolveError::EmptyToken
            | DescramblerKeyResolveError::MalformedToken
            | DescramblerKeyResolveError::UnknownToken => DescramblerDiagnosticKind::BadToken,
        };
        self.descrambler_diagnostics
            .record(demux_id.unwrap_or(-1), 0x1fff, diagnostic);
    }

    fn status_for_key_token_error(error: DescramblerKeyResolveError) -> Status {
        match error {
            DescramblerKeyResolveError::CasBridgeUnconnected
            | DescramblerKeyResolveError::RegistryUnavailable => {
                Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None)
            }
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
        let state = self.state.lock().unwrap();
        (
            state.closed,
            state.demux_id,
            state.demux_generation,
            state.key_token.clone(),
            state.pids.keys().copied().collect(),
        )
    }

    #[cfg(test)]
    fn add_pid_for_test(&self, pid: u16) -> BinderResult<()> {
        if pid > 0x1ffe {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        let (demux_id, demux_generation, already_registered) = {
            let state = lock_mutex_status(&self.state, "descrambler_state")?;
            Self::ensure_open_locked(&state)?;
            let (Some(demux_id), Some(demux_generation)) = (state.demux_id, state.demux_generation)
            else {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNAVAILABLE.0,
                    None,
                ));
            };
            if state.key_token.is_none() {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNAVAILABLE.0,
                    None,
                ));
            }
            (demux_id, demux_generation, state.pids.contains_key(&pid))
        };
        if !already_registered
            && self
                .descrambler_registry
                .pid_registered_by_other_descrambler(self.id, demux_id, demux_generation, pid)
        {
            return Err(Status::new_service_specific_error(TunerResult::INVALID_STATE.0, Some("PID is already registered by another active descrambler on this demux generation")));
        }
        let mut state = lock_mutex_status(&self.state, "descrambler_state")?;
        Self::ensure_open_locked(&state)?;
        if state.demux_id != Some(demux_id)
            || state.demux_generation != Some(demux_generation)
            || state.key_token.is_none()
        {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        state.pids.insert(
            pid,
            DescramblerPidRegistration {
                source_filter_id: -1,
                source_filter_generation: 0,
            },
        );
        Ok(())
    }

    #[cfg(test)]
    fn remove_pid_for_test(&self, pid: u16) -> BinderResult<()> {
        if pid > 0x1ffe {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        let mut state = lock_mutex_status(&self.state, "descrambler_state")?;
        Self::ensure_open_locked(&state)?;
        state.pids.remove(&pid);
        Ok(())
    }
}

impl Drop for TunerDescrambler {
    fn drop(&mut self) {
        self.descrambler_registry.unregister(self.id);
    }
}

impl Interface for TunerDescrambler {}

impl IDescrambler for TunerDescrambler {
    fn setDemuxSource(&self, demux_id: i32) -> BinderResult<()> {
        if !demux_id_in_pool(demux_id) {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        }
        let record = {
            let registry = lock_mutex_status(&self.demux_registry, "demux_registry")?;
            registry.get(&demux_id).cloned().ok_or_else(|| {
                Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None)
            })?
        };
        let (demux_generation, demux_handle) = {
            let record = lock_mutex_status(&record, "demux_record")?;
            (record.generation, record.state.clone())
        };
        if lock_mutex_status(&demux_handle, "demux_handle")?.is_closed() {
            return Err(invalid_state_status("demux handle is closed"));
        }
        let mut state = lock_mutex_status(&self.state, "descrambler_state")?;
        Self::ensure_open_locked(&state)?;
        if state.demux_id.is_some() {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        state.demux_id = Some(demux_id);
        state.demux_generation = Some(demux_generation);
        Ok(())
    }

    fn setKeyToken(&self, key_token: &[u8]) -> BinderResult<()> {
        let mut state = lock_mutex_status(&self.state, "descrambler_state")?;
        Self::ensure_open_locked(&state)?;
        match self
            .descrambler_key_table
            .resolve_with_diagnostic(key_token)
        {
            Ok(resolved) => {
                state.key_token = Some(key_token.to_vec());
                state.key_slot = Some(resolved.slot);
                Ok(())
            }
            Err(error) => {
                self.record_key_token_error(state.demux_id, error);
                Err(Self::status_for_key_token_error(error))
            }
        }
    }
    fn addPid(
        &self,
        pid: &DemuxPid,
        optional_source_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        let pid = Self::pid_from_demux_pid(pid)?;
        let (demux_id, demux_generation, already_registered) = {
            let state = lock_mutex_status(&self.state, "descrambler_state")?;
            Self::ensure_open_locked(&state)?;
            let Some(demux_id) = state.demux_id else {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNAVAILABLE.0,
                    None,
                ));
            };
            if state.key_token.is_none() {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNAVAILABLE.0,
                    None,
                ));
            }
            let Some(demux_generation) = state.demux_generation else {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNAVAILABLE.0,
                    None,
                ));
            };
            (demux_id, demux_generation, state.pids.contains_key(&pid))
        };
        let source_filter = local_filter_identity_for_owner(optional_source_filter, demux_id)?;
        if !already_registered
            && self
                .descrambler_registry
                .pid_registered_by_other_descrambler(self.id, demux_id, demux_generation, pid)
        {
            return Err(Status::new_service_specific_error(TunerResult::INVALID_STATE.0, Some("PID is already registered by another active descrambler on this demux generation")));
        }
        let mut state = lock_mutex_status(&self.state, "descrambler_state")?;
        Self::ensure_open_locked(&state)?;
        if state.demux_id != Some(demux_id)
            || state.demux_generation != Some(demux_generation)
            || state.key_token.is_none()
        {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        }
        state.pids.insert(
            pid,
            DescramblerPidRegistration {
                source_filter_id: source_filter.filter_id,
                source_filter_generation: source_filter.generation,
            },
        );
        Ok(())
    }

    fn removePid(
        &self,
        pid: &DemuxPid,
        optional_source_filter: &Strong<dyn IFilter>,
    ) -> BinderResult<()> {
        let pid = Self::pid_from_demux_pid(pid)?;
        let mut state = lock_mutex_status(&self.state, "descrambler_state")?;
        Self::ensure_open_locked(&state)?;
        let Some(demux_id) = state.demux_id else {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        };
        let source_filter = local_filter_identity_for_owner(optional_source_filter, demux_id)?;
        match state.pids.get(&pid).copied() {
            Some(stored_source)
                if stored_source.source_filter_id == source_filter.filter_id
                    && stored_source.source_filter_generation == source_filter.generation =>
            {
                state.pids.remove(&pid);
            }
            Some(_) => {
                return Err(Status::new_service_specific_error(
                    TunerResult::INVALID_ARGUMENT.0,
                    Some("PID is registered with a different source filter generation"),
                ))
            }
            None => {}
        }
        Ok(())
    }

    fn close(&self) -> BinderResult<()> {
        let mut state = lock_mutex_status(&self.state, "descrambler_state")?;
        if state.closed {
            return Ok(());
        }
        state.closed = true;
        state.demux_id = None;
        state.demux_generation = None;
        state.key_token = None;
        state.key_slot = None;
        state.pids.clear();
        drop(state);
        self.descrambler_registry.unregister(self.id);
        Ok(())
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
    demux_live_ids: Arc<Mutex<BTreeSet<i32>>>,
    demux_registry: Arc<Mutex<BTreeMap<i32, Arc<Mutex<DemuxRecord>>>>>,
    next_demux_generation: AtomicU64,
    descrambler_registry: Arc<DescramblerRuntimeRegistry>,
    descrambler_diagnostics: Arc<DescramblerDiagnosticRegistry>,
    descrambler_key_table: Arc<DescramblerKeyTable>,
    startup_diagnostics: Arc<StartupDiagnosticRegistry>,
    diagnostic_file_writes: Arc<DiagnosticFileWriteRegistry>,
    demux_core: DemuxCore,
    lnb_registry: Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
    diagnostic_workers: Mutex<Vec<ManagedWorker>>,
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
            match spawn_managed_worker_with_exit_hook(
                "descrambler_diagnostic_file",
                move |stop| {
                    while !stop.load(Ordering::SeqCst) {
                        let dump = diagnostics_for_file.dump_for_debug();
                        diagnostic_file_writes_for_file.write(&path_for_file, dump);
                        sleep_with_stop(&stop, Duration::from_secs(5));
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
        let demux_live_ids = Arc::new(Mutex::new(BTreeSet::new()));
        let demux_registry = Arc::new(Mutex::new(BTreeMap::new()));
        if let Ok(path) = std::env::var("MALEICACID_TUNER_HAL_FRONTEND_DIAGNOSTIC_FILE") {
            let frontend_registry_for_file = frontend_registry.clone();
            let startup_diagnostics_for_file = Arc::clone(&startup_diagnostics);
            let startup_diagnostics_for_hook = Arc::clone(&startup_diagnostics);
            let diagnostic_file_writes_for_file = Arc::clone(&diagnostic_file_writes);
            let path_for_file = path.clone();
            match spawn_managed_worker_with_exit_hook(
                "frontend_diagnostic_file",
                move |stop| {
                    while !stop.load(Ordering::SeqCst) {
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
                        sleep_with_stop(&stop, Duration::from_secs(5));
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
        let demux_registry_for_av_debug = Arc::clone(&demux_registry);
        if let Ok(path) = std::env::var("MALEICACID_TUNER_HAL_AV_SHARED_DIAGNOSTIC_FILE") {
            let diagnostic_file_writes_for_file = Arc::clone(&diagnostic_file_writes);
            let startup_diagnostics_for_hook = Arc::clone(&startup_diagnostics);
            let path_for_file = path.clone();
            match spawn_managed_worker_with_exit_hook(
                "av_shared_diagnostic_file",
                move |stop| {
                    while !stop.load(Ordering::SeqCst) {
                        let dump =
                            lock_mutex_option(&demux_registry_for_av_debug, "demux_registry")
                                .map(|registry| {
                                    registry
                                        .values()
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
                                .unwrap_or_else(|| "demux_registry=poisoned".to_string());
                        diagnostic_file_writes_for_file.write(&path_for_file, dump);
                        sleep_with_stop(&stop, Duration::from_secs(5));
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
            demux_live_ids,
            demux_registry,
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
        lock_mutex_option(&self.demux_registry, "demux_registry")
            .map(|registry| {
                registry
                    .values()
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
            .unwrap_or_else(|| "demux_registry=poisoned".to_string())
    }

    fn entry_declared_type(entry: &FrontendEntry) -> FrontendType {
        match &entry.kind {
            FrontendEntryKind::Px4 { declared_type, .. }
            | FrontendEntryKind::Dvb { declared_type, .. } => *declared_type,
        }
    }

    fn frontend_entry(&self, frontend_id: i32) -> Option<&FrontendEntry> {
        self.frontend_entries
            .iter()
            .find(|entry| entry.id == frontend_id)
    }

    fn default_max_frontends(&self, frontend_type: FrontendType) -> i32 {
        self.frontend_entries
            .iter()
            .filter(|entry| Self::entry_declared_type(entry) == frontend_type)
            .count() as i32
    }

    fn configured_max_frontends(&self, frontend_type: FrontendType) -> i32 {
        lock_mutex_option(&self.max_frontend_overrides, "max_frontend_overrides")
            .and_then(|overrides| overrides.get(&frontend_type.0).copied())
            .unwrap_or_else(|| self.default_max_frontends(frontend_type))
    }

    fn current_open_frontends(&self, frontend_type: FrontendType) -> i32 {
        lock_mutex_option(&self.frontend_leases, "frontend_leases")
            .and_then(|leases| leases.open_counts_by_type.get(&frontend_type.0).copied())
            .unwrap_or(0)
    }

    fn try_acquire_frontend(
        &self,
        frontend_id: i32,
        frontend_type: FrontendType,
        physical_group_id: i32,
    ) -> BinderResult<u64> {
        let max_allowed = self.configured_max_frontends(frontend_type);
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
        let current_open = leases
            .open_counts_by_type
            .get(&frontend_type.0)
            .copied()
            .unwrap_or(0);
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

    fn new_demux_binder(&self, record: Arc<Mutex<DemuxRecord>>) -> Strong<dyn IDemux> {
        BnDemux::new_binder(
            DemuxHal::new(
                record,
                Arc::clone(&self.frontend_registry),
                Arc::clone(&self.frontend_leases),
                Arc::clone(&self.demux_live_ids),
                Arc::clone(&self.demux_registry),
                Arc::clone(&self.descrambler_registry),
            ),
            BinderFeatures::default(),
        )
    }

    fn new_descrambler_binder(&self) -> Strong<dyn IDescrambler> {
        BnDescrambler::new_binder(
            TunerDescrambler::new(
                Arc::clone(&self.demux_registry),
                Arc::clone(&self.descrambler_registry),
                Arc::clone(&self.descrambler_diagnostics),
                Arc::clone(&self.descrambler_key_table),
            ),
            BinderFeatures::default(),
        )
    }

    fn create_demux_record_for_id_locked(
        &self,
        demux_id: i32,
        live_ids: &mut BTreeSet<i32>,
    ) -> BinderResult<Arc<Mutex<DemuxRecord>>> {
        let state = Arc::new(Mutex::new(self.demux_core.new_handle(demux_id)));
        let generation = self.next_demux_generation.fetch_add(1, Ordering::SeqCst);
        let record = Arc::new(Mutex::new(DemuxRecord {
            demux_id,
            generation,
            state,
            runtime_io: Arc::new(RuntimeIoRegistry::default()),
            ref_count: 1,
            bound_frontend_id: None,
            bound_frontend_generation: None,
            ci_cam_diagnostics: Vec::new(),
        }));
        live_ids.insert(demux_id);
        lock_mutex_status(&self.demux_registry, "demux_registry")?
            .insert(demux_id, Arc::clone(&record));
        Ok(record)
    }

    fn allocate_demux_record(&self) -> BinderResult<(i32, Arc<Mutex<DemuxRecord>>)> {
        let mut live_ids = lock_mutex_status(&self.demux_live_ids, "demux_live_ids")?;
        let Some(demux_id) = all_demux_ids()
            .into_iter()
            .find(|id| !live_ids.contains(id))
        else {
            return Err(Status::new_service_specific_error(
                TunerResult::UNAVAILABLE.0,
                None,
            ));
        };
        let record = self.create_demux_record_for_id_locked(demux_id, &mut live_ids)?;
        Ok((demux_id, record))
    }

    fn open_or_create_demux_record_by_id(
        &self,
        demux_id: i32,
    ) -> BinderResult<Arc<Mutex<DemuxRecord>>> {
        if !demux_id_in_pool(demux_id) {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        }
        let mut live_ids = lock_mutex_status(&self.demux_live_ids, "demux_live_ids")?;
        if live_ids.contains(&demux_id) {
            drop(live_ids);
            let record = {
                let registry = lock_mutex_status(&self.demux_registry, "demux_registry")?;
                registry.get(&demux_id).cloned()
            }
            .ok_or_else(|| Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, None))?;
            {
                let mut entry = lock_mutex_status(&record, "demux_record")?;
                entry.ref_count = entry.ref_count.saturating_add(1);
            }
            return Ok(record);
        }
        self.create_demux_record_for_id_locked(demux_id, &mut live_ids)
    }

    #[cfg(test)]
    fn first_available_demux_id(&self) -> Option<i32> {
        let live = self.demux_live_ids.lock().unwrap();
        all_demux_ids().into_iter().find(|id| !live.contains(id))
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
                worker.stop_and_join();
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
        let frontend_type = Self::entry_declared_type(entry);
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
                Arc::clone(&self.demux_registry),
            ),
            BinderFeatures::default(),
        ))
    }

    fn openDemux(&self, demux_id: &mut Vec<i32>) -> BinderResult<Strong<dyn IDemux>> {
        let (allocated, record) = self.allocate_demux_record()?;
        demux_id.clear();
        demux_id.push(allocated);
        Ok(self.new_demux_binder(record))
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
        Ok(self.new_descrambler_binder())
    }

    fn getFrontendInfo(&self, frontend_id: i32) -> BinderResult<FrontendInfo> {
        let Some(entry) = self.frontend_entries.iter().find(|e| e.id == frontend_id) else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        let (min_freq, max_freq, acquire_range) = entry_frontend_frequency_contract(entry);
        let (ty, max_symbol_rate, exclusive_group_id): (FrontendType, i32, i32) = match &entry.kind
        {
            FrontendEntryKind::Px4 { declared_type, .. } => (
                *declared_type,
                entry_frontend_max_symbol_rate_contract(entry),
                entry_physical_group_id(entry),
            ),
            FrontendEntryKind::Dvb { declared_type, .. } => (
                *declared_type,
                entry_frontend_max_symbol_rate_contract(entry),
                entry_physical_group_id(entry),
            ),
        };
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
            ),
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
        if self.current_open_frontends(frontend_type) > max_number {
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
        Ok(self.configured_max_frontends(frontend_type))
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
        let record = self.open_or_create_demux_record_by_id(demux_id)?;
        Ok(self.new_demux_binder(record))
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
    demux_registry: Arc<Mutex<BTreeMap<i32, Arc<Mutex<DemuxRecord>>>>>,
    callback: Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
    scan_signal: Arc<WorkerSignal>,
    scan_worker: Mutex<Option<ManagedWorker>>,
    scan_session: Arc<Mutex<Option<ScanSessionState>>>,
    scan_last_terminal: Arc<Mutex<Option<ScanSessionState>>>,
    next_scan_session_id: AtomicI64,
    tune_signal: Arc<WorkerSignal>,
    tune_worker: Mutex<Option<ManagedWorker>>,
    closed: AtomicBool,
}

impl FrontendHal {
    fn new(
        shared: Arc<FrontendRuntime>,
        frontend_type: FrontendType,
        physical_group_id: i32,
        session_generation: u64,
        lease_registry: Arc<Mutex<FrontendLeaseRegistry>>,
        demux_registry: Arc<Mutex<BTreeMap<i32, Arc<Mutex<DemuxRecord>>>>>,
    ) -> Self {
        Self {
            shared,
            frontend_type,
            physical_group_id,
            session_generation,
            lease_registry,
            demux_registry,
            callback: Arc::new(Mutex::new(None)),
            scan_signal: Arc::new(WorkerSignal::new(false)),
            scan_worker: Mutex::new(None),
            scan_session: Arc::new(Mutex::new(None)),
            scan_last_terminal: Arc::new(Mutex::new(None)),
            next_scan_session_id: AtomicI64::new(1),
            tune_signal: Arc::new(WorkerSignal::new(false)),
            tune_worker: Mutex::new(None),
            closed: AtomicBool::new(false),
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
            "DiSEqC is permanently unsupported by the fixed Japanese tuner profiles",
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

    fn backend_close(backend: &mut FrontendBackendState) {
        match backend {
            FrontendBackendState::Px4(inner) => inner.close(),
            FrontendBackendState::Dvb(inner) => inner.close(),
            FrontendBackendState::Unavailable { .. } => {}
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
    ) {
        if let Some(callback) = Self::current_callback_from_registry(callback_registry) {
            if let Err(err) = callback.onEvent(event) {
                Self::handle_frontend_callback_failure(
                    callback_registry,
                    shared,
                    scan_session,
                    session_id,
                    "onEvent",
                    err,
                );
            }
        }
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
    ) {
        if let Some(callback) = Self::current_callback_from_registry(callback_registry) {
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
            }
        }
    }

    fn notify_scan_end_with_callback(
        callback_registry: &Arc<Mutex<Option<Strong<dyn IFrontendCallback>>>>,
        shared: &Arc<FrontendRuntime>,
        scan_session: &Arc<Mutex<Option<ScanSessionState>>>,
        session_id: i64,
    ) {
        Self::notify_scan_message_with_callback(
            callback_registry,
            shared,
            Some(scan_session),
            Some(session_id),
            FrontendScanMessageType::END,
            FrontendScanMessage::IsEnd(true),
        );
    }

    fn stop_scan_worker(&self) -> BinderResult<()> {
        self.scan_signal.request_stop();
        if let Some(mut worker) =
            lock_mutex_status(&self.scan_worker, "frontend_scan_worker")?.take()
        {
            worker.stop_and_join();
        }
        self.scan_signal.clear_for_start();
        Ok(())
    }

    fn cancel_scan_session(&self) -> BinderResult<()> {
        self.stop_scan_worker()?;
        self.remember_scan_terminal_from_current();
        *lock_mutex_status(&self.scan_session, "frontend_scan_session")? = None;
        Ok(())
    }

    fn cancel_scan_session_best_effort(&self) {
        self.scan_signal.request_stop();
        if let Some(mut worker) = lock_mutex_option(&self.scan_worker, "frontend_scan_worker")
            .and_then(|mut worker| worker.take())
        {
            worker.stop_and_join();
        }
        self.remember_scan_terminal_from_current();
        if let Some(mut session) = lock_mutex_option(&self.scan_session, "frontend_scan_session") {
            *session = None;
        }
        self.scan_signal.clear_for_start();
    }

    fn stop_tune_worker(&self) -> BinderResult<()> {
        self.tune_signal.request_stop();
        if let Some(mut worker) =
            lock_mutex_status(&self.tune_worker, "frontend_tune_worker")?.take()
        {
            worker.stop_and_join();
        }
        self.tune_signal.clear_for_start();
        Ok(())
    }

    fn stop_tune_worker_best_effort(&self) {
        self.tune_signal.request_stop();
        if let Some(mut worker) = lock_mutex_option(&self.tune_worker, "frontend_tune_worker")
            .and_then(|mut worker| worker.take())
        {
            worker.stop_and_join();
        }
        self.tune_signal.clear_for_start();
    }

    fn start_tune_worker(&self, request: FrontendTuneRequest) -> BinderResult<()> {
        let mut worker_slot = lock_mutex_status(&self.tune_worker, "frontend_tune_worker")?;
        if worker_slot.is_some() {
            return Err(Status::from(StatusCode::UNKNOWN_ERROR));
        }
        let shared = Arc::clone(&self.shared);
        let callback_registry = Arc::clone(&self.callback);
        self.tune_signal.clear_for_start();
        let tune_signal = Arc::clone(&self.tune_signal);
        let shared_for_hook = Arc::clone(&shared);
        let shared_for_spawn_failure = Arc::clone(&shared);
        let handle = spawn_worker_with_exit_hook(
            "frontend_tune_worker",
            move || {
            let outcome = FrontendHal::wait_for_lock(&shared, request.system, LockWaitMode::Tune, Some(&tune_signal));
            let Ok(outcome) = outcome else {
                if !tune_signal.is_stop_requested() {
                    let detail = format!("worker=frontend_tune_worker operation=wait_for_lock error=runtime_backend_failure");
                    shared.record_runtime_failure(detail.clone());
                    shared.mark_live_path_failed(&detail);
                    return WorkerExit::Error;
                }
                return WorkerExit::Cancelled;
            };
            if outcome.cancelled || tune_signal.is_stop_requested() {
                return WorkerExit::Cancelled;
            }
            if outcome.locked {
                FrontendHal::notify_event_with_callback(&callback_registry, &shared, None, None, FrontendEventType::LOCKED);
                if !lock_mutex_option(&shared.bound_demuxes, "frontend_bound_demuxes").map(|demuxes| demuxes.is_empty()).unwrap_or(true) {
                    if let Err(status) = shared.ensure_live_pump() {
                        let detail = format!("worker=frontend_tune_worker operation=ensure_live_pump status={:?}", status);
                        shared.record_runtime_failure(detail.clone());
                        shared.mark_live_path_failed(&detail);
                        return WorkerExit::Error;
                    }
                }
            } else {
                FrontendHal::notify_event_with_callback(&callback_registry, &shared, None, None, FrontendEventType::NO_SIGNAL);
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
            shared_for_spawn_failure.stop_live_pump_best_effort();
            shared_for_spawn_failure.reset_bound_demuxes_for_stream_boundary();
            if let Some(mut backend) = lock_mutex_option(&shared_for_spawn_failure.backend, "frontend_backend") {
                if let Err(stop_err) = FrontendHal::backend_stop_tune(&mut backend) {
                    shared_for_spawn_failure.record_runtime_failure(format!("worker=frontend_tune_worker spawn_failure_cleanup=backend_stop_tune error={stop_err}"));
                }
            } else {
                shared_for_spawn_failure.record_runtime_failure("worker=frontend_tune_worker spawn_failure_cleanup=frontend_backend_lock_failed".to_string());
            }
            Status::from(StatusCode::UNKNOWN_ERROR)
        })?;
        *worker_slot = Some(ManagedWorker::new(
            "frontend_tune_worker",
            Arc::clone(&self.tune_signal),
            handle,
        ));
        Ok(())
    }

    fn settings_fingerprint(settings: &FrontendSettings, scan_type: FrontendScanType) -> String {
        format!("{:?}|{:?}", settings, scan_type)
    }

    fn wait_interruptibly(stop_signal: Option<&WorkerSignal>, duration: Duration) -> bool {
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
        stop_signal: Option<&WorkerSignal>,
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
        let deadline = Instant::now() + Duration::from_millis(config.timeout_ms);
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
            if now >= deadline {
                return Ok(LockWaitOutcome {
                    telemetry: last_telemetry,
                    locked: false,
                    cancelled: false,
                });
            }
            let sleep_for = Duration::from_millis(config.poll_interval_ms)
                .min(deadline.saturating_duration_since(now));
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

    fn publish_scan_terminal_debug(
        shared: &Arc<FrontendRuntime>,
        scan_session: &Arc<Mutex<Option<ScanSessionState>>>,
        session_id: i64,
    ) -> Option<ScanSessionState> {
        let terminal =
            lock_mutex_option(scan_session, "frontend_scan_session").and_then(|session| {
                session
                    .as_ref()
                    .filter(|state| {
                        state.session_id == session_id && state.phase != ScanPhase::Running
                    })
                    .cloned()
            });
        if let Some(state) = terminal.as_ref() {
            Self::publish_scan_terminal_state(shared, state);
        }
        terminal
    }

    fn remember_scan_terminal_from_current(&self) {
        let Some(session_id) = lock_mutex_option(&self.scan_session, "frontend_scan_session")
            .and_then(|session| session.as_ref().map(|state| state.session_id))
        else {
            return;
        };
        if let Some(state) =
            FrontendHal::publish_scan_terminal_debug(&self.shared, &self.scan_session, session_id)
        {
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
            let record = lock_mutex_status(&self.demux_registry, "demux_registry")?
                .get(&demux_id)
                .cloned();
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
                        state.reset_for_stream_boundary();
                    }
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
            let record = lock_mutex_option(&self.demux_registry, "demux_registry")
                .and_then(|registry| registry.get(&demux_id).cloned());
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
                        state.reset_for_stream_boundary();
                    }
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
        let count = leases
            .open_counts_by_type
            .get(&self.frontend_type.0)
            .copied()
            .unwrap_or(0);
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
        if self.closed.load(Ordering::SeqCst) {
            return Err(invalid_state_status("frontend is closed"));
        }
        Ok(())
    }

    fn close_internal(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.cancel_scan_session()?;
        self.stop_tune_worker()?;
        self.shared.stop_live_pump()?;
        {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            Self::backend_set_callback_registered(&mut backend, false);
            Self::backend_close(&mut backend);
        }
        *lock_mutex_status(&self.callback, "frontend_callback")? = None;
        self.unbind_frontend_demuxes()?;
        self.release_frontend_lease()?;
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn close_internal_best_effort(&self) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        self.cancel_scan_session_best_effort();
        self.stop_tune_worker_best_effort();
        self.shared.stop_live_pump_best_effort();
        if let Some(mut backend) = lock_mutex_option(&self.shared.backend, "frontend_backend") {
            Self::backend_set_callback_registered(&mut backend, false);
            Self::backend_close(&mut backend);
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
    ) {
        if !matches!(tune_request.system, FrontendSystem::IsdbS) {
            return;
        }
        let Some(stream_id) = Self::reported_scan_input_stream_id(tune_request) else {
            return;
        };
        Self::notify_scan_message_with_callback(
            callback_registry,
            shared,
            Some(scan_session),
            Some(session_id),
            FrontendScanMessageType::INPUT_STREAM_IDS,
            FrontendScanMessage::InputStreamIds(vec![stream_id]),
        );
    }

    fn validate_isdbt_fixed_settings(
        s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbtSettings::FrontendIsdbtSettings,
    ) -> Result<(), HalError> {
        if !matches!(
            s.bandwidth,
            FrontendIsdbtBandwidth::AUTO | FrontendIsdbtBandwidth::BANDWIDTH_6MHZ
        ) {
            return Err(HalError::InvalidArgument(
                "r51 ISDB-T accepts only AUTO or 6MHz bandwidth".into(),
            ));
        }
        if !matches!(s.mode, FrontendIsdbtMode::AUTO | FrontendIsdbtMode::MODE_3) {
            return Err(HalError::InvalidArgument(
                "r51 ISDB-T accepts only AUTO or MODE_3 transmission mode".into(),
            ));
        }
        if !matches!(s.guardInterval, FrontendIsdbtGuardInterval::AUTO) {
            return Err(HalError::InvalidArgument(
                "r51 ISDB-T guard interval capability is AUTO only".into(),
            ));
        }
        for layer in &s.layerSettings {
            if !matches!(layer.modulation, FrontendIsdbtModulation::AUTO) {
                return Err(HalError::InvalidArgument(
                    "r51 ISDB-T layer modulation capability is AUTO only".into(),
                ));
            }
            if !matches!(layer.coderate, FrontendIsdbtCoderate::AUTO) {
                return Err(HalError::InvalidArgument(
                    "r51 ISDB-T layer coderate capability is AUTO only".into(),
                ));
            }
            if !matches!(layer.timeInterleave, FrontendIsdbtTimeInterleaveMode::AUTO) {
                return Err(HalError::InvalidArgument(
                    "r51 ISDB-T layer time interleave capability is AUTO only".into(),
                ));
            }
        }
        Ok(())
    }

    fn validate_isdbs_fixed_settings(
        s: &android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::FrontendIsdbsSettings::FrontendIsdbsSettings,
    ) -> Result<(), HalError> {
        if !matches!(s.modulation, FrontendIsdbsModulation::AUTO) {
            return Err(HalError::InvalidArgument(
                "r51 ISDB-S modulation capability is AUTO only".into(),
            ));
        }
        if !matches!(s.coderate, FrontendIsdbsCoderate::AUTO) {
            return Err(HalError::InvalidArgument(
                "r51 ISDB-S coderate capability is AUTO only".into(),
            ));
        }
        if s.symbolRate != 0 {
            return Err(HalError::InvalidArgument(
                "r51 ISDB-S public settings do not use explicit symbolRate; use 0/unspecified"
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

    fn status_type_supported_by_flags(
        supports_signal_strength: bool,
        supports_rf_lock: bool,
        supports_satellite: bool,
        ty: FrontendStatusType,
    ) -> bool {
        match ty {
            FrontendStatusType::DEMOD_LOCK
            | FrontendStatusType::SNR
            | FrontendStatusType::SIGNAL_QUALITY => true,
            FrontendStatusType::RF_LOCK => supports_rf_lock,
            FrontendStatusType::SIGNAL_STRENGTH => supports_signal_strength,
            FrontendStatusType::LNB_VOLTAGE => supports_satellite,
            _ => false,
        }
    }

    fn validate_status_types(
        supports_signal_strength: bool,
        supports_rf_lock: bool,
        supports_satellite: bool,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<()> {
        if status_types.iter().any(|ty| {
            !Self::status_type_supported_by_flags(
                supports_signal_strength,
                supports_rf_lock,
                supports_satellite,
                *ty,
            )
        }) {
            return Err(invalid_argument_status(
                "unsupported frontend status type requested",
            ));
        }
        Ok(())
    }

    fn status_for_types(
        supports_signal_strength: bool,
        supports_rf_lock: bool,
        supports_satellite: bool,
        status: &FrontendTelemetry,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<Vec<FrontendStatus>> {
        Self::validate_status_types(
            supports_signal_strength,
            supports_rf_lock,
            supports_satellite,
            status_types,
        )?;
        let mut out = Vec::with_capacity(status_types.len());
        for ty in status_types {
            match *ty {
                FrontendStatusType::DEMOD_LOCK => {
                    out.push(FrontendStatus::IsDemodLocked(status.locked))
                }
                FrontendStatusType::RF_LOCK => out.push(FrontendStatus::IsRfLocked(
                    status.rf_locked.unwrap_or(false),
                )),
                FrontendStatusType::SNR => out.push(FrontendStatus::Snr(
                    i32::try_from(status.cnr.unwrap_or(0)).unwrap_or(i32::MAX),
                )),
                FrontendStatusType::SIGNAL_STRENGTH => out.push(FrontendStatus::SignalStrength(
                    i32::try_from(status.signal_strength.unwrap_or(0)).unwrap_or(i32::MAX),
                )),
                FrontendStatusType::SIGNAL_QUALITY => out.push(FrontendStatus::SignalQuality(
                    i32::try_from(status.signal_quality.unwrap_or(0)).unwrap_or(i32::MAX),
                )),
                FrontendStatusType::LNB_VOLTAGE => out.push(FrontendStatus::LnbVoltage(
                    match status.lnb_voltage.unwrap_or(0) {
                        11 => LnbVoltage::VOLTAGE_11V,
                        15 => LnbVoltage::VOLTAGE_15V,
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
        supports_signal_strength: bool,
        supports_rf_lock: bool,
        supports_satellite: bool,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<Vec<FrontendStatusReadiness>> {
        Self::validate_status_types(
            supports_signal_strength,
            supports_rf_lock,
            supports_satellite,
            status_types,
        )?;
        Ok(status_types
            .iter()
            .map(|_| FrontendStatusReadiness::STABLE)
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
            return Err(StatusCode::INVALID_OPERATION.into());
        }
        let lnb = lock_mutex_status(lnb_registry, "lnb_registry")?
            .get(&lnb_id)
            .cloned()
            .ok_or(StatusCode::NAME_NOT_FOUND)?;
        if lnb.owner_frontend_id != frontend_id {
            return Err(StatusCode::BAD_VALUE.into());
        }
        Ok(lnb)
    }
}

impl Drop for FrontendHal {
    fn drop(&mut self) {
        self.close_internal_best_effort();
    }
}

impl Interface for FrontendHal {}

impl IFrontend for FrontendHal {
    fn setCallback(&self, callback: &Strong<dyn IFrontendCallback>) -> BinderResult<()> {
        self.ensure_open()?;
        *lock_mutex_status(&self.callback, "frontend_callback")? = Some(callback.clone());
        {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            Self::backend_set_callback_registered(&mut backend, true);
        }
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
        {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            Self::backend_validate_tune_request(&mut backend, &request)
                .map_err(hal_error_status)?;
        }
        self.cancel_scan_session()?;
        self.stop_tune_worker()?;
        self.shared.stop_live_pump()?;
        {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            Self::backend_stop_tune(&mut backend).map_err(hal_error_status)?;
            self.apply_selected_lnb(&mut backend)
                .map_err(hal_error_status)?;
            Self::backend_submit_tune(&mut backend, request.clone()).map_err(hal_error_status)?;
        }
        self.shared.reset_bound_demuxes_for_stream_boundary();
        self.start_tune_worker(request)?;
        Ok(())
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
        self.shared.reset_bound_demuxes_for_stream_boundary();
        Ok(())
    }

    fn close(&self) -> BinderResult<()> {
        self.close_internal()
    }

    fn scan(&self, settings: &FrontendSettings, scan_type: FrontendScanType) -> BinderResult<()> {
        self.ensure_open()?;
        let fingerprint = Self::settings_fingerprint(settings, scan_type);

        {
            let session = lock_mutex_status(&self.scan_session, "frontend_scan_session")?;
            if let Some(state) = session.as_ref() {
                if state.fingerprint == fingerprint && state.phase == ScanPhase::Running {
                    return Ok(());
                }
            }
        }

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
        self.scan_signal.clear_for_start();
        let scan_signal = Arc::clone(&self.scan_signal);
        let scan_session = Arc::clone(&self.scan_session);
        let callback_registry_for_hook = Arc::clone(&callback_registry);
        let shared_for_hook = Arc::clone(&shared);
        let scan_session_for_hook = Arc::clone(&scan_session);
        let scan_last_terminal_for_hook = Arc::clone(&self.scan_last_terminal);
        let handle = match spawn_worker_with_exit_hook(
            "frontend_scan_worker",
            move || {
                let total = requests.len().max(1) as i32;
                let mut scan_failed = false;
                for index in start_index..requests.len() {
                    if scan_signal.is_stop_requested() {
                        FrontendHal::mark_scan_session_phase(
                            &scan_session,
                            session_id,
                            ScanPhase::Cancelled,
                        );
                        FrontendHal::notify_scan_end_with_callback(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                        );
                        break;
                    }
                    let request = requests[index].clone();
                    let progress = (((index + 1) as i32) * 100) / total;
                    FrontendHal::notify_scan_message_with_callback(
                        &callback_registry,
                        &shared,
                        Some(&scan_session),
                        Some(session_id),
                        FrontendScanMessageType::PROGRESS_PERCENT,
                        FrontendScanMessage::ProgressPercent(progress),
                    );
                    FrontendHal::notify_scan_message_with_callback(
                        &callback_registry,
                        &shared,
                        Some(&scan_session),
                        Some(session_id),
                        FrontendScanMessageType::FREQUENCY,
                        FrontendScanMessage::Frequencies(vec![request.frequency as i64]),
                    );
                    shared.stop_live_pump_best_effort();
                    let stop_result = match lock_mutex_hal(&shared.backend, "frontend_backend") {
                        Ok(mut backend) => FrontendHal::backend_stop_tune(&mut backend),
                        Err(err) => Err(err),
                    };
                    let tune_result = stop_result.and_then(|_| {
                        shared.reset_bound_demuxes_for_stream_boundary();
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
                        FrontendHal::notify_scan_end_with_callback(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                        );
                        scan_failed = true;
                        break;
                    }
                    let outcome = FrontendHal::wait_for_lock(
                        &shared,
                        request.system,
                        LockWaitMode::Scan,
                        Some(&scan_signal),
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
                        FrontendHal::notify_scan_end_with_callback(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                        );
                        scan_failed = true;
                        break;
                    };
                    if outcome.cancelled {
                        FrontendHal::mark_scan_session_phase(
                            &scan_session,
                            session_id,
                            ScanPhase::Cancelled,
                        );
                        FrontendHal::notify_scan_end_with_callback(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                        );
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
                            FrontendHal::notify_scan_message_with_callback(
                                &callback_registry,
                                &shared,
                                Some(&scan_session),
                                Some(session_id),
                                message_type,
                                message,
                            );
                        }
                        FrontendHal::notify_event_with_callback(
                            &callback_registry,
                            &shared,
                            Some(&scan_session),
                            Some(session_id),
                            FrontendEventType::LOCKED,
                        );
                        FrontendHal::emit_scan_stream_id_message_with_callback(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                            &request,
                        );
                        continue;
                    }
                    FrontendHal::notify_event_with_callback(
                        &callback_registry,
                        &shared,
                        Some(&scan_session),
                        Some(session_id),
                        FrontendEventType::NO_SIGNAL,
                    );
                }
                match lock_mutex_hal(&shared.backend, "frontend_backend") {
                    Ok(mut backend) => {
                        if let Err(err) = FrontendHal::backend_stop_tune(&mut backend) {
                            shared.record_runtime_failure(format!(
                                "worker=frontend_scan_worker cleanup=backend_stop_tune error={err}"
                            ));
                            FrontendHal::mark_scan_session_phase(
                                &scan_session,
                                session_id,
                                ScanPhase::FailedBackend,
                            );
                            shared.mark_live_path_failed("scan_cleanup_backend_stop_tune_failed");
                            FrontendHal::notify_scan_end_with_callback(
                                &callback_registry,
                                &shared,
                                &scan_session,
                                session_id,
                            );
                            scan_failed = true;
                        }
                    }
                    Err(err) => {
                        shared.record_runtime_failure(format!(
                            "worker=frontend_scan_worker cleanup=frontend_backend_lock error={err}"
                        ));
                        FrontendHal::mark_scan_session_phase(
                            &scan_session,
                            session_id,
                            ScanPhase::FailedBackend,
                        );
                        shared.mark_live_path_failed("scan_cleanup_frontend_backend_lock_failed");
                        FrontendHal::notify_scan_end_with_callback(
                            &callback_registry,
                            &shared,
                            &scan_session,
                            session_id,
                        );
                        scan_failed = true;
                    }
                }
                if !scan_signal.is_stop_requested()
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
                    FrontendHal::notify_scan_end_with_callback(
                        &callback_registry,
                        &shared,
                        &scan_session,
                        session_id,
                    );
                }
                match FrontendHal::scan_session_phase(&scan_session, session_id) {
                    Some(
                        ScanPhase::FailedBackend
                        | ScanPhase::FailedCallback
                        | ScanPhase::FailedPanic,
                    ) => WorkerExit::Error,
                    Some(ScanPhase::Cancelled) => WorkerExit::Cancelled,
                    _ => WorkerExit::Normal,
                }
            },
            move |exit| {
                if let Some(state) = FrontendHal::publish_scan_terminal_debug(
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
                if exit.is_abnormal() {
                    let detail = format!("worker=frontend_scan_worker exit={:?}", exit);
                    shared_for_hook.record_runtime_failure(detail.clone());
                    match exit {
                        WorkerExit::Panic => {
                            FrontendHal::mark_scan_session_phase(
                                &scan_session_for_hook,
                                session_id,
                                ScanPhase::FailedPanic,
                            );
                        }
                        WorkerExit::Error => {
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
                    FrontendHal::notify_scan_end_with_callback(
                        &callback_registry_for_hook,
                        &shared_for_hook,
                        &scan_session_for_hook,
                        session_id,
                    );
                    if let Some(state) = FrontendHal::publish_scan_terminal_debug(
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
                }
            },
        ) {
            Ok(handle) => handle,
            Err(err) => {
                let detail = format!("worker=frontend_scan_worker spawn_failed error={err}");
                eprintln!("maleicacid-tuner-hal-worker: {detail}");
                self.shared.record_runtime_failure(detail.clone());
                self.shared.mark_live_path_failed(&detail);
                let terminal = if let Some(state) = scan_session_guard.as_mut() {
                    if state.session_id == session_id {
                        state.phase = ScanPhase::FailedBackend;
                        Some(state.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(terminal) = terminal {
                    FrontendHal::publish_scan_terminal_state(&self.shared, &terminal);
                    if let Some(mut last) =
                        lock_mutex_option(&self.scan_last_terminal, "frontend_scan_last_terminal")
                    {
                        *last = Some(terminal);
                    }
                    *scan_session_guard = None;
                }
                return Err(Status::from(StatusCode::UNKNOWN_ERROR));
            }
        };
        *scan_worker_slot = Some(ManagedWorker::new(
            "frontend_scan_worker",
            Arc::clone(&self.scan_signal),
            handle,
        ));
        drop(scan_session_guard);
        drop(scan_worker_slot);
        Ok(())
    }

    fn stopScan(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.cancel_scan_session()?;
        // stopScan owns only the scan operation.  The scan worker performs its own
        // backend stop during cancellation; when no scan is active this must not
        // stop a normal tune/live pump.
        Ok(())
    }

    fn getStatus(&self, status_types: &[FrontendStatusType]) -> BinderResult<Vec<FrontendStatus>> {
        self.ensure_open()?;
        let (supports_signal_strength, supports_rf_lock) = {
            let backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            let is_dvb = matches!(&*backend, FrontendBackendState::Dvb(_));
            (is_dvb, is_dvb)
        };
        let supports_satellite = self
            .shared
            .allowed_systems
            .iter()
            .any(|system| matches!(system, FrontendSystem::IsdbS));
        Self::validate_status_types(
            supports_signal_strength,
            supports_rf_lock,
            supports_satellite,
            status_types,
        )?;
        let telemetry = {
            let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
            self.apply_selected_lnb(&mut backend)
                .and_then(|_| Self::backend_read_status(&mut backend))
        }
        .map_err(hal_error_status)?;
        Self::status_for_types(
            supports_signal_strength,
            supports_rf_lock,
            supports_satellite,
            &telemetry,
            status_types,
        )
    }

    fn setLnb(&self, lnb_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        let _lnb = Self::validate_lnb_owner(
            &self.shared.allowed_systems,
            self.shared.frontend_id,
            &self.shared.lnb_registry,
            lnb_id,
        )?;
        let mut backend = lock_mutex_status(&self.shared.backend, "frontend_backend")?;
        Self::backend_set_lnb_id(&mut backend, lnb_id);
        self.apply_selected_lnb(&mut backend)
            .map_err(hal_error_status)?;
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
        let supports_signal_strength = lock_mutex_status(&self.shared.backend, "frontend_backend")
            .map(|backend| matches!(&*backend, FrontendBackendState::Dvb(_)))?;
        let supports_rf_lock = lock_mutex_status(&self.shared.backend, "frontend_backend")
            .map(|backend| matches!(&*backend, FrontendBackendState::Dvb(_)))?;
        let supports_satellite = self
            .shared
            .allowed_systems
            .iter()
            .any(|system| matches!(system, FrontendSystem::IsdbS));
        Self::readiness_for_types(
            supports_signal_strength,
            supports_rf_lock,
            supports_satellite,
            status_types,
        )
    }
}

pub struct DemuxHal {
    demux_id: i32,
    record: Arc<Mutex<DemuxRecord>>,
    frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>>,
    lease_registry: Arc<Mutex<FrontendLeaseRegistry>>,
    demux_live_ids: Arc<Mutex<BTreeSet<i32>>>,
    demux_registry: Arc<Mutex<BTreeMap<i32, Arc<Mutex<DemuxRecord>>>>>,
    descrambler_registry: Arc<DescramblerRuntimeRegistry>,
    close_lock: Mutex<()>,
    closed: AtomicBool,
}

impl DemuxHal {
    fn new(
        record: Arc<Mutex<DemuxRecord>>,
        frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>>,
        lease_registry: Arc<Mutex<FrontendLeaseRegistry>>,
        demux_live_ids: Arc<Mutex<BTreeSet<i32>>>,
        demux_registry: Arc<Mutex<BTreeMap<i32, Arc<Mutex<DemuxRecord>>>>>,
        descrambler_registry: Arc<DescramblerRuntimeRegistry>,
    ) -> Self {
        let demux_id = lock_mutex_option(&record, "demux_record")
            .map(|record| record.demux_id)
            .unwrap_or(DEMUX_ID_BASE);
        Self {
            demux_id,
            record,
            frontend_registry,
            lease_registry,
            demux_live_ids,
            demux_registry,
            descrambler_registry,
            close_lock: Mutex::new(()),
            closed: AtomicBool::new(false),
        }
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
        let state = lock_mutex_status(&self.record, "demux_record")?
            .state
            .clone();
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

        let (ref_count, bound_frontend_id, state, demux_generation) = {
            let record = lock_mutex_status(&self.record, "demux_record")?;
            (
                record.ref_count,
                record.bound_frontend_id,
                record.state.clone(),
                record.generation,
            )
        };

        if ref_count > 1 {
            let mut record = lock_mutex_status(&self.record, "demux_record")?;
            record.ref_count -= 1;
            self.closed.store(true, Ordering::SeqCst);
            return Ok(());
        }

        if let Some(frontend_id) = bound_frontend_id {
            if let Some(runtime) = self.frontend_registry.get(&frontend_id) {
                runtime.unbind_demux(self.demux_id).map_err(|status| {
                    eprintln!(
                        "maleicacid-tuner-hal-demux-close: demux={} step=unbind_demux frontend={} status={:?}",
                        self.demux_id, frontend_id, status
                    );
                    status
                })?;
            }
        }
        lock_mutex_status(&state, "demux_handle")
            .map_err(|status| {
                eprintln!(
                    "maleicacid-tuner-hal-demux-close: demux={} step=lock_demux_handle status={:?}",
                    self.demux_id, status
                );
                status
            })?
            .close();
        self.descrambler_registry
            .invalidate_demux(self.demux_id, demux_generation);
        lock_mutex_status(&self.demux_live_ids, "demux_live_ids")
            .map_err(|status| {
                eprintln!(
                "maleicacid-tuner-hal-demux-close: demux={} step=lock_demux_live_ids status={:?}",
                self.demux_id, status
            );
                status
            })?
            .remove(&self.demux_id);
        lock_mutex_status(&self.demux_registry, "demux_registry")
            .map_err(|status| {
                eprintln!(
                "maleicacid-tuner-hal-demux-close: demux={} step=lock_demux_registry status={:?}",
                self.demux_id, status
            );
                status
            })?
            .remove(&self.demux_id);

        {
            let mut record = lock_mutex_status(&self.record, "demux_record")?;
            record.ref_count = 0;
            record.bound_frontend_id = None;
            record.bound_frontend_generation = None;
        }
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn release_registration_best_effort(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
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
                record.bound_frontend_generation = None;
                (
                    true,
                    record.bound_frontend_id.take(),
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
            self.descrambler_registry
                .invalidate_demux(self.demux_id, demux_generation);
        }
        if let Some(mut live_ids) = lock_mutex_option(&self.demux_live_ids, "demux_live_ids") {
            live_ids.remove(&self.demux_id);
        }
        if let Some(mut registry) = lock_mutex_option(&self.demux_registry, "demux_registry") {
            registry.remove(&self.demux_id);
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
            self.descrambler_registry
                .invalidate_demux(self.demux_id, demux_generation);
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
            if let Some(mut record) = lock_mutex_option(&self.record, "demux_record") {
                record.bound_frontend_id = old_frontend_id;
                record.bound_frontend_generation = old_frontend_generation;
            }
            if let Some(mut handle) = lock_mutex_option(&state, "demux_handle") {
                match old_frontend_id {
                    Some(old_id) => handle.bind_frontend(old_id),
                    None => handle.unbind_frontend(),
                }
            } else {
                return fail_closed_transition("rollback_demux_handle_lock_failed");
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
        )?;
        if let Some(old_frontend_id) = old_frontend_id.filter(|old| *old != frontend_id) {
            let Some(old_runtime) = self.frontend_registry.get(&old_frontend_id) else {
                rollback_to_old("old_frontend_missing")?;
                return Err(StatusCode::NAME_NOT_FOUND.into());
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
                state.reset_for_stream_boundary();
            }
            runtime_io.flush_all();
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
                "openFilter bufferSize must be positive",
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
        let record = demux
            .register_filter_result(filter_type_bits, open_type, buffer_size)
            .map_err(demux_config_error_status)?;
        let filter_id = record.filter_id;
        drop(demux);
        let filter_hal = match FilterHal::new(
            self.demux_id,
            filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            cb.clone(),
        ) {
            Ok(filter_hal) => filter_hal,
            Err(err) => {
                runtime_io.unregister_filter_best_effort(filter_id);
                if let Some(mut state) = lock_mutex_option(&state, "demux_handle") {
                    state.unregister_filter(filter_id);
                }
                return Err(err);
            }
        };
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
                "openDvr bufferSize must be positive",
            ));
        }
        let direction = normalize_dvr_type(dvr_type)?;
        let state = self.state()?;
        let runtime_io = self.runtime_io()?;
        let mut demux = lock_mutex_status(&state, "demux_handle")?;
        let record = demux
            .register_dvr(direction, buffer_size)
            .map_err(demux_config_error_status)?;
        let dvr_id = record.dvr_id;
        drop(demux);
        let dvr_hal = match DvrHal::new(
            self.demux_id,
            dvr_id,
            direction,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            cb.clone(),
        ) {
            Ok(dvr_hal) => dvr_hal,
            Err(err) => {
                runtime_io.unregister_dvr_best_effort(dvr_id);
                if let Some(mut state) = lock_mutex_option(&state, "demux_handle") {
                    state.unregister_dvr(dvr_id);
                }
                return Err(err);
            }
        };
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

pub struct FilterHal {
    owner_demux_id: i32,
    filter_id: i32,
    state: Arc<Mutex<DemuxHandle>>,
    callback: Strong<dyn IFilterCallback>,
    runtime_io: Arc<RuntimeIoRegistry>,
    queue_backing: Arc<SharedMemoryBacking>,
    av_queue_backing: Arc<SharedMemoryBacking>,
    av_shared_backing: Arc<Mutex<Option<Arc<AvSharedBacking>>>>,
    av_shared_handle_exported: Arc<AtomicBool>,
    av_drop_unexported: Arc<AtomicU64>,
    callback_stop: Arc<AtomicBool>,
    callback_worker: Mutex<Option<WorkerJoinHandle>>,
    closed: Arc<AtomicBool>,
}

impl FilterHal {
    fn new(
        owner_demux_id: i32,
        filter_id: i32,
        state: Arc<Mutex<DemuxHandle>>,
        runtime_io: Arc<RuntimeIoRegistry>,
        callback: Strong<dyn IFilterCallback>,
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
        let av_shared_handle_exported = Arc::new(AtomicBool::new(false));
        let av_drop_unexported = Arc::new(AtomicU64::new(0));
        runtime_io.register_filter(
            filter_id,
            &queue_backing,
            &av_queue_backing,
            None,
            &av_drop_unexported,
        );
        let callback_stop = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let next_av_data_id = Arc::new(AtomicI64::new(1));
        let callback_worker = {
            let state_clone = Arc::clone(&state);
            let callback_clone = callback.clone();
            let stop_clone = Arc::clone(&callback_stop);
            let queue_backing_clone = Arc::clone(&queue_backing);
            let av_queue_backing_clone = Arc::clone(&av_queue_backing);
            let av_shared_backing_clone = Arc::clone(&av_shared_backing);
            let av_shared_handle_exported_clone = Arc::clone(&av_shared_handle_exported);
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
            let handle = spawn_worker_with_exit_hook("filter_callback_worker", move || {
                let mut cumulative_bytes = 0u64;
                let mut record_event_state = RecordEventState::default();
                let mut observed_delivery_generation = 0u64;
                while !stop_clone.load(Ordering::SeqCst) && !closed_clone.load(Ordering::SeqCst) {
                    let (record, start_event_ready, pending_overflow, payloads) = {
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
                            return WorkerExit::Error;
                        };
                        let start_event_ready = demux.take_filter_start_event_if_ready(filter_id);
                        let pending_overflow = demux.take_filter_pending_overflow(filter_id);
                        let record = demux.filter_record(filter_id).cloned();
                        let payloads = demux.drain_filter_payloads_for_delivery(filter_id);
                        (record, start_event_ready, pending_overflow, payloads)
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
                        return WorkerExit::Error;
                    };
                    if record.delivery_generation != observed_delivery_generation {
                        cumulative_bytes = 0;
                        record_event_state = RecordEventState::default();
                        observed_delivery_generation = record.delivery_generation;
                    }
                    let _monitor_mask = record.monitor_event_mask;
                    let send_status = true;
                    let send_event = true;
                    if start_event_ready && send_event {
                        if let Err(err) = callback_clone.onFilterEvent(&[DemuxFilterEvent::StartId(filter_id)]) {
                            eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterEvent(StartId) binder_status={:?}", filter_id, err);
                            FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on StartId");
                            return WorkerExit::Error;
                        }
                    }
                    if payloads.is_empty() {
                        if pending_overflow && send_status {
                            if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::OVERFLOW) {
                                eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(OVERFLOW) binder_status={:?}", filter_id, err);
                                FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on OVERFLOW");
                                return WorkerExit::Error;
                            }
                        }
                        queue_backing_clone.wait_for_stop_or_timeout(Duration::from_millis(20));
                        continue;
                    }
                    let mut internal_overflow_pending = pending_overflow;
                    for payload in payloads {
                        let payload_bytes = payload.bytes().to_vec();
                        let is_media = matches!(record.config.as_ref().map(|c| &c.kind), Some(FilterConfigKind::Av { .. }));
                        let mut queue_ring = RingWriteResult::default();
                        let mut overflow = internal_overflow_pending;
                        internal_overflow_pending = false;
                        let mut av_slice = None;
                        let mut av_data_id = None;
                        let mut av_memory = None;
                        let mut av_delivery = if is_media { Some(AvPayloadDeliveryResult::DroppedNoSharedHandle) } else { None };
                        if is_media {
                            if av_shared_handle_exported_clone.load(Ordering::SeqCst) {
                                let shared_backing = match lock_mutex_status(&av_shared_backing_clone, "filter_av_shared_backing") {
                                    Ok(backing) => backing.clone(),
                                    Err(_) => {
                                        let err = AvPayloadInternalError::MutexPoisoned;
                                        let reason = format!("filter AV shared allocation internal_error={}", err.as_str());
                                        eprintln!("maleicacid-tuner-hal: filter_id={} AV shared allocation internal_error={}", filter_id, err.as_str());
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, &reason);
                                        return WorkerExit::Error;
                                    }
                                };
                                let Some(shared_backing) = shared_backing else {
                                    let err = AvPayloadInternalError::SharedHandleExportedWithoutBacking;
                                    let reason = format!("filter AV shared allocation internal_error={}", err.as_str());
                                    eprintln!("maleicacid-tuner-hal: filter_id={} AV shared allocation internal_error={}", filter_id, err.as_str());
                                    FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, &reason);
                                    return WorkerExit::Error;
                                };
                                let id = next_av_data_id_clone.fetch_add(1, Ordering::SeqCst);
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
                                        return WorkerExit::Error;
                                    }
                                    Err(AvPayloadAllocateError::Delivery(result)) => {
                                        av_delivery = Some(result);
                                        overflow = true;
                                    }
                                }
                            } else {
                                // 呼び出し側が shared fd をまだ取得していない。framework/JNI が消費できない成功風 MediaEvent は出さない。
                                let drops = av_drop_unexported_clone.fetch_add(1, Ordering::SeqCst).saturating_add(1);
                                if AvSharedBacking::should_log_counter(drops) {
                                    eprintln!("maleicacid-tuner-hal: AV payload dropped before shared handle export filter_id={} av_drop_unexported={}", filter_id, drops);
                                }
                                av_delivery = Some(AvPayloadDeliveryResult::DroppedNoSharedHandle);
                                overflow = true;
                            }
                        } else if av_payload_should_write_standard_fmq(is_media) {
                            queue_ring = queue_backing_clone.write_bytes(&payload_bytes).unwrap_or_default();
                            overflow |= queue_ring.overflowed;
                        }
                        let (notify_data_ready, notify_overflow) = av_payload_status_decision(is_media, av_delivery, overflow);
                        let fill = if is_media { av_queue_backing_clone.current_fill_bytes() } else { queue_backing_clone.current_fill_bytes() };
                        if send_status {
                            if notify_data_ready {
                                if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::DATA_READY) {
                                    eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(DATA_READY) binder_status={:?}", filter_id, err);
                                    FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on DATA_READY");
                                    return WorkerExit::Error;
                                }
                            }
                            if notify_overflow {
                                if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::OVERFLOW) {
                                    eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(OVERFLOW) binder_status={:?}", filter_id, err);
                                    FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on OVERFLOW");
                                    return WorkerExit::Error;
                                }
                            }
                            if !is_media {
                                let high_water = record.buffer_size.max(0) as usize * 3 / 4;
                                let low_water = record.buffer_size.max(0) as usize / 4;
                                if high_water > 0 && fill >= high_water {
                                    if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::HIGH_WATER) {
                                        eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(HIGH_WATER) binder_status={:?}", filter_id, err);
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on HIGH_WATER");
                                        return WorkerExit::Error;
                                    }
                                } else if low_water > 0 && fill <= low_water {
                                    if let Err(err) = callback_clone.onFilterStatus(DemuxFilterStatus::LOW_WATER) {
                                        eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterStatus(LOW_WATER) binder_status={:?}", filter_id, err);
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on LOW_WATER");
                                        return WorkerExit::Error;
                                    }
                                }
                            }
                        }
                        if send_event {
                            // AV filter の正式deliveryは MediaEvent + shared handle である。
                            // shared slot を確保できなかった AV payload は OVERFLOW として扱い、
                            // avDataId=0 / shared handleなしの MediaEvent を出して FMQ-only delivery を
                            // live AV 成功経路にしてはならない。
                            if av_payload_should_emit_data_event(is_media, av_slice) {
                                let event_offset = av_slice.map(|slice| slice.offset as i64).unwrap_or(queue_ring.start_offset as i64);
                                if let Some(event) = build_filter_event_from_entry(&record, &payload, event_offset, cumulative_bytes, av_slice, av_data_id, av_memory, &mut record_event_state) {
                                    if let Err(err) = callback_clone.onFilterEvent(&[event]) {
                                        eprintln!("maleicacid-tuner-hal-callback: filter_id={} api=onFilterEvent(data) binder_status={:?}", filter_id, err);
                                        FilterHal::fail_filter_worker(&state_clone, &runtime_io_clone, &queue_backing_clone, &av_queue_backing_clone, &av_shared_backing_clone, &closed_clone, &stop_clone, filter_id, "filter callback failure on data event");
                                        return WorkerExit::Error;
                                    }
                                }
                            }
                        }
                        cumulative_bytes = cumulative_bytes.saturating_add(payload_bytes.len() as u64);
                    }
                }
                WorkerExit::Cancelled
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
            queue_backing,
            av_queue_backing,
            av_shared_backing,
            av_shared_handle_exported,
            av_drop_unexported,
            callback_stop,
            callback_worker,
            closed,
        })
    }

    fn fail_filter_worker(
        state: &Arc<Mutex<DemuxHandle>>,
        runtime_io: &Arc<RuntimeIoRegistry>,
        queue_backing: &Arc<SharedMemoryBacking>,
        av_queue_backing: &Arc<SharedMemoryBacking>,
        av_shared_backing: &Arc<Mutex<Option<Arc<AvSharedBacking>>>>,
        closed: &Arc<AtomicBool>,
        callback_stop: &Arc<AtomicBool>,
        filter_id: i32,
        reason: &str,
    ) {
        if closed.swap(true, Ordering::SeqCst) {
            return;
        }
        eprintln!(
            "maleicacid-tuner-hal-worker: filter_callback_worker abnormal stop filter_id={} reason={}",
            filter_id, reason
        );
        callback_stop.store(true, Ordering::SeqCst);
        if let Some(backing) = lock_mutex_option(av_shared_backing, "filter_av_shared_backing")
            .and_then(|mut backing| backing.take())
        {
            backing.clear();
        }
        runtime_io.mark_failed(RuntimeIoKind::Filter, filter_id, reason);
        runtime_io.clear_filter_av_shared_best_effort(filter_id);
        queue_backing.stop_best_effort();
        av_queue_backing.stop_best_effort();
        if let Some(mut demux) = lock_mutex_option(state, "demux_handle") {
            demux.unregister_filter(filter_id);
        }
    }

    fn fail_from_callback(&self, api: &str, err: Status) -> Status {
        let status = callback_failure_status("filter", self.filter_id, api, &err);
        self.close_internal_best_effort();
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
            join_worker_with_diagnostics(handle, "filter_callback_worker");
        }
        Ok(())
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
        let Some(demux) = lock_mutex_option(&self.state, "demux_handle") else {
            return Err(invalid_state_status(
                "AV shared handle requested after demux state failure",
            ));
        };
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
        if record.av_stream_kind.is_none() || record.av_stream_type_hint.is_none() {
            return Err(invalid_state_status(
                "AV shared handle requested before configureAvStreamType",
            ));
        }
        Ok(())
    }

    fn ensure_av_shared_backing(&self) -> BinderResult<Arc<AvSharedBacking>> {
        self.ensure_configured_av_filter()?;
        if let Some(existing) =
            lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")?.clone()
        {
            return Ok(existing);
        }
        let buffer_size = lock_mutex_status(&self.state, "demux_handle")?
            .filter_record(self.filter_id)
            .map(|r| r.buffer_size.max(0) as usize)
            .unwrap_or(4096);
        let backing = AvSharedBacking::new(buffer_size)?;
        self.runtime_io
            .set_filter_av_shared(self.filter_id, &backing);
        *lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")? =
            Some(Arc::clone(&backing));
        Ok(backing)
    }

    fn clear_av_shared_backing(&self) {
        if let Some(backing) =
            lock_mutex_option(&self.av_shared_backing, "filter_av_shared_backing")
                .and_then(|backing| backing.as_ref().cloned())
        {
            backing.clear();
        }
    }

    fn drop_av_shared_backing(&self) -> BinderResult<()> {
        if let Some(backing) =
            lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")?.take()
        {
            backing.clear();
        }
        self.runtime_io.clear_filter_av_shared(self.filter_id)
    }

    fn drop_av_shared_backing_best_effort(&self) {
        if let Some(backing) =
            lock_mutex_option(&self.av_shared_backing, "filter_av_shared_backing")
                .and_then(|mut backing| backing.take())
        {
            backing.clear();
        }
        self.runtime_io
            .clear_filter_av_shared_best_effort(self.filter_id);
    }

    fn release_all_av_shared_handles(&self) {
        if let Some(backing) =
            lock_mutex_option(&self.av_shared_backing, "filter_av_shared_backing")
                .and_then(|backing| backing.as_ref().cloned())
        {
            backing.release_all();
        }
    }

    fn release_av_shared_handle(&self, av_data_id: i64) -> BinderResult<()> {
        let Some(backing) =
            lock_mutex_status(&self.av_shared_backing, "filter_av_shared_backing")?.clone()
        else {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        };
        if !backing.release(av_data_id) {
            return Err(StatusCode::NAME_NOT_FOUND.into());
        }
        Ok(())
    }

    fn close_internal(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            self.stop_callback_worker()?;
            reset_av_shared_handle_export_epoch(&self.av_shared_handle_exported);
            self.drop_av_shared_backing()?;
            self.runtime_io.unregister_filter(self.filter_id)?;
            self.queue_backing.stop()?;
            self.av_queue_backing.stop()?;
            if let Some(mut state) = lock_mutex_option(&self.state, "demux_handle") {
                state.unregister_filter(self.filter_id);
            }
            return Ok(());
        }
        self.stop_callback_worker()?;
        reset_av_shared_handle_export_epoch(&self.av_shared_handle_exported);
        self.drop_av_shared_backing()?;
        self.runtime_io.unregister_filter(self.filter_id)?;
        self.queue_backing.stop()?;
        self.av_queue_backing.stop()?;
        lock_mutex_status(&self.state, "demux_handle")?.unregister_filter(self.filter_id);
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn close_internal_best_effort(&self) {
        self.callback_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = lock_mutex_option(&self.callback_worker, "filter_callback_worker")
            .and_then(|mut worker| worker.take())
        {
            join_worker_with_diagnostics(handle, "filter_callback_worker");
        }
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        reset_av_shared_handle_export_epoch(&self.av_shared_handle_exported);
        self.drop_av_shared_backing_best_effort();
        self.runtime_io
            .unregister_filter_best_effort(self.filter_id);
        self.queue_backing.stop_best_effort();
        self.av_queue_backing.stop_best_effort();
        if let Some(mut state) = lock_mutex_option(&self.state, "demux_handle") {
            state.unregister_filter(self.filter_id);
        }
        self.closed.store(true, Ordering::SeqCst);
    }
}

impl Interface for FilterHal {}

impl Drop for FilterHal {
    fn drop(&mut self) {
        self.close_internal_best_effort();
    }
}

impl IFilter for FilterHal {
    fn getQueueDesc(&self, queue: &mut TunerQueueDesc) -> BinderResult<()> {
        self.ensure_open()?;
        *queue = self.queue_backing.build_queue_desc()?;
        Ok(())
    }

    fn close(&self) -> BinderResult<()> {
        self.close_internal()
    }

    fn configure(&self, settings: &DemuxFilterSettings) -> BinderResult<()> {
        self.ensure_open()?;
        let summary = build_filter_summary(settings)?;
        lock_mutex_status(&self.state, "demux_handle")?
            .configure_filter_with_summary_result(self.filter_id, summary)
            .map_err(demux_config_error_status)?;
        // 再 configure は既に export 済みの AV shared fd / slot lifetime を無効化する。
        // 後続の AV 配送は呼び出し側に getAvSharedHandle() を再実行させる。
        reset_av_shared_handle_export_epoch(&self.av_shared_handle_exported);
        self.drop_av_shared_backing()?;
        Ok(())
    }

    fn configureAvStreamType(&self, av_stream_type: &AvStreamType) -> BinderResult<()> {
        self.ensure_open()?;
        let (av_stream_type_hint, av_stream_kind) = match av_stream_type {
            AvStreamType::Video(value) => (value.0, AvFilterStreamKind::Video),
            AvStreamType::Audio(value) => (value.0, AvFilterStreamKind::Audio),
        };
        lock_mutex_status(&self.state, "demux_handle")?
            .set_filter_av_stream_type_hint_result(
                self.filter_id,
                av_stream_type_hint,
                av_stream_kind,
            )
            .map_err(demux_config_error_status)
    }
    fn configureIpCid(&self, _ip_cid: i32) -> BinderResult<()> {
        self.ensure_open()?;
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            Some("IP CID monitor/filter is unsupported in the r51 TS-only HAL profile"),
        ))
    }

    fn configureMonitorEvent(&self, monitor_event_types: i32) -> BinderResult<()> {
        self.ensure_open()?;
        if monitor_event_types != 0 {
            return Err(Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, Some("filter monitor events are unsupported in r51; normal callbacks are always delivered")));
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
        let (is_av, stream_type_configured) = self.av_filter_state();
        if is_av {
            if !stream_type_configured {
                return Err(invalid_state_status(
                    "AV filter start requires configureAvStreamType",
                ));
            }
        }
        let (ready, start_event_ready, monitor_mask, is_media) = {
            let mut state = lock_mutex_status(&self.state, "demux_handle")?;
            state
                .start_filter_result(self.filter_id)
                .map_err(demux_config_error_status)?;
            let readiness = state.filter_delivery_readiness(self.filter_id);
            let ready = state.has_filter_payload_ready(self.filter_id)
                && matches!(
                    readiness,
                    maleicacid_tuner_hal_soft_demux::FilterDeliveryReadiness::Ready
                );
            let start_event_ready = filter_start_event_ready(readiness);
            let record = state.filter_record(self.filter_id).cloned();
            // configureMonitorEvent() is not a normal callback gating API.
            // r51 supports no monitor-event bits, so DATA_READY / OVERFLOW / data events remain always enabled.
            let monitor_mask = 0;
            let is_media = matches!(
                record
                    .as_ref()
                    .and_then(|r| r.config.as_ref())
                    .map(|c| &c.kind),
                Some(FilterConfigKind::Av { .. })
            );
            let send_event = true;
            if send_event {
                state.set_filter_start_event_pending(self.filter_id, !start_event_ready);
            } else {
                state.set_filter_start_event_pending(self.filter_id, false);
            }
            (ready, start_event_ready, monitor_mask, is_media)
        };
        let send_status = monitor_mask == 0 || (monitor_mask & FILTER_MONITOR_MASK_STATUS) != 0;
        let send_event = monitor_mask == 0 || (monitor_mask & FILTER_MONITOR_MASK_EVENT) != 0;
        if ready && !is_media && send_status {
            if let Err(err) = self.callback.onFilterStatus(DemuxFilterStatus::DATA_READY) {
                return Err(self.fail_from_callback("onFilterStatus(DATA_READY)", err));
            }
        }
        if start_event_ready && send_event {
            if let Err(err) = self
                .callback
                .onFilterEvent(&[DemuxFilterEvent::StartId(self.filter_id)])
            {
                return Err(self.fail_from_callback("onFilterEvent(StartId)", err));
            }
        }
        Ok(())
    }

    fn stop(&self) -> BinderResult<()> {
        self.ensure_open()?;
        if lock_mutex_status(&self.state, "demux_handle")?.stop_filter(self.filter_id) {
            return Ok(());
        }
        Err(StatusCode::NAME_NOT_FOUND.into())
    }

    fn flush(&self) -> BinderResult<()> {
        self.ensure_open()?;
        if lock_mutex_status(&self.state, "demux_handle")?.flush_filter(self.filter_id) {
            self.queue_backing.clear();
            self.av_queue_backing.clear();
            reset_av_shared_handle_export_epoch(&self.av_shared_handle_exported);
            self.drop_av_shared_backing()?;
            return Ok(());
        }
        Err(StatusCode::NAME_NOT_FOUND.into())
    }

    fn getAvSharedHandle(&self, av_memory: &mut TunerNativeHandle) -> BinderResult<i64> {
        self.ensure_open()?;
        self.ensure_configured_av_filter()?;
        let backing = self.ensure_av_shared_backing()?;
        *av_memory = backing.build_native_handle()?;
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

    fn releaseAvHandle(&self, _av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {
        self.ensure_open()?;
        if av_data_id == 0 {
            reset_av_shared_handle_export_epoch(&self.av_shared_handle_exported);
            self.release_all_av_shared_handles();
            return Ok(());
        }
        if av_data_id < 0 {
            return Err(invalid_argument_status("invalid AV data id"));
        }
        self.release_av_shared_handle(av_data_id)
    }

    fn setDataSource(&self, filter: &Strong<dyn IFilter>) -> BinderResult<()> {
        self.ensure_open()?;
        self.runtime_io
            .ensure_not_failed(RuntimeIoKind::Filter, self.filter_id)?;
        let upstream_id = local_filter_id_for_owner(filter, self.owner_demux_id)?;
        lock_mutex_status(&self.state, "demux_handle")?
            .set_filter_data_source_result(self.filter_id, upstream_id)
            .map_err(demux_config_error_status)
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

pub struct DvrHal {
    owner_demux_id: i32,
    dvr_id: i32,
    direction: DemuxPathDirection,
    state: Arc<Mutex<DemuxHandle>>,
    callback: Strong<dyn IDvrCallback>,
    runtime_io: Arc<RuntimeIoRegistry>,
    queue_backing: Arc<SharedMemoryBacking>,
    callback_stop: Arc<AtomicBool>,
    callback_wake: Arc<(Mutex<bool>, Condvar)>,
    callback_worker: Mutex<Option<WorkerJoinHandle>>,
    closed: Arc<AtomicBool>,
}

impl DvrHal {
    fn new(
        owner_demux_id: i32,
        dvr_id: i32,
        direction: DemuxPathDirection,
        state: Arc<Mutex<DemuxHandle>>,
        runtime_io: Arc<RuntimeIoRegistry>,
        callback: Strong<dyn IDvrCallback>,
    ) -> BinderResult<Self> {
        let buffer_size = lock_mutex_status(&state, "demux_handle")?
            .dvr_record(dvr_id)
            .map(|r| r.buffer_size.max(0) as usize)
            .unwrap_or(4096);
        let queue_backing = match direction {
            DemuxPathDirection::Playback => SharedMemoryBacking::new_playback_consumer(
                Arc::clone(&state),
                Arc::clone(&runtime_io),
                dvr_id,
                buffer_size,
            ),
            DemuxPathDirection::Record => SharedMemoryBacking::new_ring(buffer_size),
        }?;
        runtime_io.register_dvr(dvr_id, &queue_backing);
        let callback_stop = Arc::new(AtomicBool::new(false));
        let callback_wake = Arc::new((Mutex::new(false), Condvar::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let callback_worker = {
            let state = Arc::clone(&state);
            let callback = callback.clone();
            let callback_stop = Arc::clone(&callback_stop);
            let callback_wake_clone = Arc::clone(&callback_wake);
            let closed_clone = Arc::clone(&closed);
            let runtime_io_clone = Arc::clone(&runtime_io);
            let queue_backing_clone = Arc::clone(&queue_backing);
            let state_hook = Arc::clone(&state);
            let runtime_io_hook = Arc::clone(&runtime_io);
            let queue_backing_hook = Arc::clone(&queue_backing);
            let closed_hook = Arc::clone(&closed);
            let callback_stop_hook = Arc::clone(&callback_stop);
            let callback_wake_hook = Arc::clone(&callback_wake);
            spawn_worker_with_exit_hook("dvr_callback_worker", move || {
                while !callback_stop.load(Ordering::SeqCst) && !closed_clone.load(Ordering::SeqCst) {
                    let (thresholds, status_mask, interval_hint_ms, running, pending_overflow, payloads) = {
                        let Some(mut demux) = lock_mutex_option(&state, "demux_handle") else {
                            DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &callback_stop, &callback_wake_clone, dvr_id, "dvr_callback_worker lost demux state");
                            return WorkerExit::Error;
                        };
                        let Some(record) = demux.dvr_record(dvr_id).cloned() else {
                            DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &callback_stop, &callback_wake_clone, dvr_id, "dvr_callback_worker missing dvr record");
                            return WorkerExit::Error;
                        };
                        let running = record.started;
                        let interval = record.status_check_interval_hint_ms;
                        let status_mask = record.config.as_ref().map(|config| config.status_mask).unwrap_or(0);
                        let pending_overflow = demux.take_dvr_pending_overflow(dvr_id);
                        let payloads = if running && matches!(direction, DemuxPathDirection::Record) { demux.drain_dvr_payloads(dvr_id) } else { Vec::new() };
                        (demux.dvr_threshold_state(dvr_id), status_mask, interval, running, pending_overflow, payloads)
                    };
                    if running && matches!(direction, DemuxPathDirection::Record) {
                        let mut overflow = pending_overflow;
                        let mut any = false;
                        for payload in payloads {
                            let ring = queue_backing_clone.write_bytes(&payload).unwrap_or_default();
                            overflow |= ring.overflowed;
                            any |= ring.len > 0;
                        }
                        if any && Self::status_mask_allows(status_mask, RecordStatus::DATA_READY.0) {
                            if let Err(err) = callback.onRecordStatus(RecordStatus::DATA_READY) {
                                eprintln!("maleicacid-tuner-hal-callback: dvr_id={} api=onRecordStatus(DATA_READY) binder_status={:?}", dvr_id, err);
                                DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &callback_stop, &callback_wake_clone, dvr_id, "dvr callback failure on DATA_READY");
                                return WorkerExit::Error;
                            }
                        }
                        if overflow && Self::status_mask_allows(status_mask, RecordStatus::OVERFLOW.0) {
                            if let Err(err) = callback.onRecordStatus(RecordStatus::OVERFLOW) {
                                eprintln!("maleicacid-tuner-hal-callback: dvr_id={} api=onRecordStatus(OVERFLOW) binder_status={:?}", dvr_id, err);
                                DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &callback_stop, &callback_wake_clone, dvr_id, "dvr callback failure on OVERFLOW");
                                return WorkerExit::Error;
                            }
                        }
                    }
                    if running {
                        match (direction, thresholds) {
                            (DemuxPathDirection::Record, Some((_fill, low, high, _capacity))) => {
                                let fill = queue_backing_clone.current_fill_bytes();
                                let status = Self::record_status_from_thresholds(fill, low, high);
                                if Self::status_mask_allows(status_mask, status.0) {
                                    if let Err(err) = callback.onRecordStatus(status) {
                                    eprintln!("maleicacid-tuner-hal-callback: dvr_id={} api=onRecordStatus(threshold) binder_status={:?}", dvr_id, err);
                                    DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &callback_stop, &callback_wake_clone, dvr_id, "dvr callback failure on record threshold");
                                    return WorkerExit::Error;
                                }
                                }
                            }
                            (DemuxPathDirection::Playback, Some((fill, low, high, capacity))) => {
                                if let Some(status) = Self::playback_status_from_thresholds(fill, low, high, capacity) {
                                    if Self::status_mask_allows(status_mask, status.0) {
                                        if let Err(err) = callback.onPlaybackStatus(status) {
                                        eprintln!("maleicacid-tuner-hal-callback: dvr_id={} api=onPlaybackStatus(threshold) binder_status={:?}", dvr_id, err);
                                        DvrHal::fail_dvr_worker(&state, &runtime_io_clone, &queue_backing_clone, &closed_clone, &callback_stop, &callback_wake_clone, dvr_id, "dvr callback failure on playback threshold");
                                        return WorkerExit::Error;
                                    }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    let sleep_ms = u64::try_from(interval_hint_ms).unwrap_or(DVR_DEFAULT_STATUS_CHECK_INTERVAL_MS as u64);
                    DvrHal::wait_for_callback_interval(&callback_stop, &callback_wake_clone, Duration::from_millis(sleep_ms));
                }
                WorkerExit::Cancelled
            }, move |exit| {
                if exit.is_abnormal() {
                    DvrHal::fail_dvr_worker(
                        &state_hook,
                        &runtime_io_hook,
                        &queue_backing_hook,
                        &closed_hook,
                        &callback_stop_hook,
                        &callback_wake_hook,
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
            queue_backing,
            callback_stop,
            callback_wake,
            callback_worker: Mutex::new(Some(callback_worker)),
            closed,
        })
    }

    fn wait_for_callback_interval(
        stop: &AtomicBool,
        wake: &Arc<(Mutex<bool>, Condvar)>,
        interval: Duration,
    ) {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let (lock, cv) = &**wake;
        let Ok(mut guard) = lock.lock() else {
            return;
        };
        if *guard {
            *guard = false;
            return;
        }
        loop {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            let Ok((next_guard, wait_result)) = cv.wait_timeout(guard, interval) else {
                return;
            };
            guard = next_guard;
            if *guard {
                *guard = false;
                return;
            }
            if wait_result.timed_out() {
                return;
            }
        }
    }

    fn wake_callback_wait(wake: &Arc<(Mutex<bool>, Condvar)>) {
        let (lock, cv) = &**wake;
        if let Ok(mut guard) = lock.lock() {
            *guard = true;
        }
        cv.notify_all();
    }

    fn fail_dvr_worker(
        state: &Arc<Mutex<DemuxHandle>>,
        runtime_io: &Arc<RuntimeIoRegistry>,
        queue_backing: &Arc<SharedMemoryBacking>,
        closed: &Arc<AtomicBool>,
        callback_stop: &Arc<AtomicBool>,
        callback_wake: &Arc<(Mutex<bool>, Condvar)>,
        dvr_id: i32,
        reason: &str,
    ) {
        if closed.swap(true, Ordering::SeqCst) {
            return;
        }
        eprintln!(
            "maleicacid-tuner-hal-worker: dvr_callback_worker abnormal stop dvr_id={} reason={}",
            dvr_id, reason
        );
        callback_stop.store(true, Ordering::SeqCst);
        DvrHal::wake_callback_wait(callback_wake);
        runtime_io.mark_failed(RuntimeIoKind::Dvr, dvr_id, reason);
        queue_backing.clear();
        queue_backing.stop_best_effort();
        if let Some(mut demux) = lock_mutex_option(state, "demux_handle") {
            demux.unregister_dvr(dvr_id);
        }
    }

    fn fail_from_callback(&self, api: &str, err: Status) -> Status {
        let status = callback_failure_status("dvr", self.dvr_id, api, &err);
        self.close_internal_best_effort();
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
        DvrHal::wake_callback_wait(&self.callback_wake);
        if let Some(handle) =
            lock_mutex_status(&self.callback_worker, "dvr_callback_worker")?.take()
        {
            join_worker_with_diagnostics(handle, "dvr_callback_worker");
        }
        Ok(())
    }

    fn stop_callback_worker_best_effort(&self) {
        self.callback_stop.store(true, Ordering::SeqCst);
        DvrHal::wake_callback_wait(&self.callback_wake);
        if let Some(handle) = lock_mutex_option(&self.callback_worker, "dvr_callback_worker")
            .and_then(|mut worker| worker.take())
        {
            join_worker_with_diagnostics(handle, "dvr_callback_worker");
        }
    }

    fn close_internal(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            self.stop_callback_worker()?;
            self.queue_backing.clear();
            self.runtime_io.unregister_dvr(self.dvr_id)?;
            self.queue_backing.stop()?;
            if let Some(mut state) = lock_mutex_option(&self.state, "demux_handle") {
                state.unregister_dvr(self.dvr_id);
            }
            return Ok(());
        }
        self.stop_callback_worker()?;
        self.queue_backing.clear();
        self.runtime_io.unregister_dvr(self.dvr_id)?;
        self.queue_backing.stop()?;
        lock_mutex_status(&self.state, "demux_handle")?.unregister_dvr(self.dvr_id);
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn close_internal_best_effort(&self) {
        self.stop_callback_worker_best_effort();
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        self.queue_backing.clear();
        self.runtime_io.unregister_dvr_best_effort(self.dvr_id);
        self.queue_backing.stop_best_effort();
        if let Some(mut state) = lock_mutex_option(&self.state, "demux_handle") {
            state.unregister_dvr(self.dvr_id);
        }
        self.closed.store(true, Ordering::SeqCst);
    }

    fn status_mask_allows(status_mask: i32, status_bit: i32) -> bool {
        status_mask == 0 || (status_mask & status_bit) != 0
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
        // playback status は queued data bytes ではなく未使用 write space で定義する。
        // 呼び出し側は playback TS を FMQ へ書くため、callback は追加入力可能な空き容量を表す。
        let available_space = capacity.saturating_sub(fill);
        if capacity > 0 && available_space == 0 {
            Some(PlaybackStatus::SPACE_EMPTY)
        } else if capacity > 0 && available_space >= capacity {
            Some(PlaybackStatus::SPACE_FULL)
        } else if low.map_or(false, |limit| available_space <= limit) {
            Some(PlaybackStatus::SPACE_ALMOST_EMPTY)
        } else if high.map_or(false, |limit| available_space >= limit) {
            Some(PlaybackStatus::SPACE_ALMOST_FULL)
        } else {
            None
        }
    }
}

impl Interface for DvrHal {}

impl Drop for DvrHal {
    fn drop(&mut self) {
        self.close_internal_best_effort();
    }
}

impl IDvr for DvrHal {
    fn getQueueDesc(&self, queue: &mut TunerQueueDesc) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        *queue = self.queue_backing.build_queue_desc()?;
        Ok(())
    }

    fn configure(&self, settings: &DvrSettings) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        let mut demux = lock_mutex_status(&self.state, "demux_handle")?;
        let buffer_size = demux
            .dvr_record(self.dvr_id)
            .map(|record| record.buffer_size)
            .ok_or_else(|| StatusCode::NAME_NOT_FOUND.into())?;
        let summary = validate_and_build_dvr_summary(self.direction, settings, buffer_size)?;
        demux
            .configure_dvr_with_summary_result(self.dvr_id, summary)
            .map_err(demux_config_error_status)
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
        if lock_mutex_status(&self.state, "demux_handle")?
            .detach_filter_from_dvr(self.dvr_id, filter_id)
        {
            return Ok(());
        }
        Err(StatusCode::NAME_NOT_FOUND.into())
    }

    fn start(&self) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        let (state, status_mask) = {
            let mut demux = lock_mutex_status(&self.state, "demux_handle")?;
            demux
                .start_dvr_result(self.dvr_id)
                .map_err(demux_config_error_status)?;
            let status_mask = demux
                .dvr_record(self.dvr_id)
                .and_then(|record| record.config.as_ref().map(|config| config.status_mask))
                .unwrap_or(0);
            (demux.dvr_threshold_state(self.dvr_id), status_mask)
        };
        match (self.direction, state) {
            (DemuxPathDirection::Record, Some((_fill, low, high, _capacity))) => {
                let fill = self.queue_backing.current_fill_bytes();
                let status = Self::record_status_from_thresholds(fill, low, high);
                if Self::status_mask_allows(status_mask, status.0) {
                    if let Err(err) = self.callback.onRecordStatus(status) {
                        return Err(self.fail_from_callback("onRecordStatus(start)", err));
                    }
                }
            }
            (DemuxPathDirection::Playback, Some((_fill, low, high, capacity))) => {
                let fill = self.queue_backing.current_fill_bytes();
                if let Some(status) =
                    Self::playback_status_from_thresholds(fill, low, high, capacity)
                {
                    if Self::status_mask_allows(status_mask, status.0) {
                        if let Err(err) = self.callback.onPlaybackStatus(status) {
                            return Err(self.fail_from_callback("onPlaybackStatus(start)", err));
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn stop(&self) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        if lock_mutex_status(&self.state, "demux_handle")?.stop_dvr(self.dvr_id) {
            return Ok(());
        }
        Err(StatusCode::NAME_NOT_FOUND.into())
    }

    fn flush(&self) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(self.direction, DemuxPathDirection::Playback) {
            self.queue_backing.ensure_playback_worker_healthy()?;
        }
        if lock_mutex_status(&self.state, "demux_handle")?.flush_dvr(self.dvr_id) {
            self.queue_backing.clear();
            return Ok(());
        }
        Err(StatusCode::NAME_NOT_FOUND.into())
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
                "DVR statusCheckIntervalHint must be non-negative",
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
    closed: AtomicBool,
    callback_set: Mutex<bool>,
    registry: Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
    frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>>,
}

impl LnbHal {
    fn new(
        lnb_id: i32,
        registry: Arc<Mutex<BTreeMap<i32, LnbRuntimeState>>>,
        frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>>,
    ) -> Self {
        Self {
            lnb_id,
            closed: AtomicBool::new(false),
            callback_set: Mutex::new(false),
            registry,
            frontend_registry,
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
            let already_sent =
                lock_mutex_hal(&runtime.sent_diseqc_generations, "sent_diseqc_generations")?
                    .get(&self.lnb_id)
                    .copied()
                    .unwrap_or(0)
                    >= generation;
            if already_sent {
                continue;
            }
            FrontendHal::backend_send_diseqc_message(&mut backend, message)?;
            lock_mutex_hal(&runtime.sent_diseqc_generations, "sent_diseqc_generations")?
                .insert(self.lnb_id, generation);
        }
        Ok(())
    }

    fn restore_state(&self, old: Option<LnbRuntimeState>) {
        let Some(mut registry) = lock_mutex_option(&self.registry, "lnb_registry") else {
            return;
        };
        match old {
            Some(old) => {
                registry.insert(self.lnb_id, old);
            }
            None => {
                registry.remove(&self.lnb_id);
            }
        }
    }

    fn ensure_open(&self) -> BinderResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(invalid_state_status("LNB is closed"));
        }
        Ok(())
    }

    fn voltage_supported(profile: LnbDeviceProfile, voltage: LnbVoltage) -> bool {
        match profile {
            LnbDeviceProfile::Px4Device15VOnly | LnbDeviceProfile::PxMltDevice15VOnly => {
                matches!(voltage, LnbVoltage::NONE | LnbVoltage::VOLTAGE_15V)
            }
            LnbDeviceProfile::EarthPt1FixedLnb => matches!(
                voltage,
                LnbVoltage::NONE | LnbVoltage::VOLTAGE_11V | LnbVoltage::VOLTAGE_15V
            ),
            LnbDeviceProfile::NoPower => matches!(voltage, LnbVoltage::NONE),
        }
    }
}

impl Interface for LnbHal {}

impl ILnb for LnbHal {
    fn setCallback(&self, _callback: &Strong<dyn ILnbCallback>) -> BinderResult<()> {
        self.ensure_open()?;
        *lock_mutex_status(&self.callback_set, "lnb_callback_set")? = true;
        Ok(())
    }

    fn setVoltage(&self, voltage: LnbVoltage) -> BinderResult<()> {
        self.ensure_open()?;
        let (old_state, new_state) = {
            let mut registry = lock_mutex_status(&self.registry, "lnb_registry")?;
            let old = registry.get(&self.lnb_id).cloned();
            let state = registry.entry(self.lnb_id).or_default();
            if !Self::voltage_supported(state.profile, voltage) {
                return Err(Status::new_service_specific_error(
                    TunerResult::UNAVAILABLE.0,
                    None,
                ));
            }
            state.voltage = Some(voltage);
            state.generation = state.generation.saturating_add(1);
            (old, state.clone())
        };
        if let Err(err) = self.apply_to_matching_frontends(&new_state) {
            self.restore_state(old_state.clone());
            if let Some(old) = old_state.as_ref() {
                let _ = self.apply_to_matching_frontends(old);
            }
            return Err(hal_error_status(err));
        }
        Ok(())
    }

    fn setTone(&self, tone: LnbTone) -> BinderResult<()> {
        self.ensure_open()?;
        if matches!(tone, LnbTone::NONE) {
            let mut registry = lock_mutex_status(&self.registry, "lnb_registry")?;
            let state = registry.entry(self.lnb_id).or_default();
            state.tone = Some(LnbTone::NONE);
            state.generation = state.generation.saturating_add(1);
            return Ok(());
        }
        Err(Status::new_service_specific_error(
            TunerResult::UNAVAILABLE.0,
            None,
        ))
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
        let mut registry = lock_mutex_status(&self.registry, "lnb_registry")?;
        let state = registry.entry(self.lnb_id).or_default();
        state.position = Some(LnbPosition::UNDEFINED);
        state.generation = state.generation.saturating_add(1);
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
        self.closed.store(true, Ordering::SeqCst);
        *lock_mutex_status(&self.callback_set, "lnb_callback_set")? = false;
        Ok(())
    }
}

const DEMUX_TS_INDEX_FIRST_PACKET: i32 = 1 << 0;
const DEMUX_TS_INDEX_PAYLOAD_UNIT_START: i32 = 1 << 1;
const DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED: i32 = 1 << 2;
const DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED: i32 = 1 << 3;
const DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED: i32 = 1 << 4;
const DEMUX_TS_INDEX_DISCONTINUITY: i32 = 1 << 5;
const DEMUX_TS_INDEX_RANDOM_ACCESS: i32 = 1 << 6;
const DEMUX_TS_INDEX_PRIORITY: i32 = 1 << 7;
const DEMUX_TS_INDEX_PCR: i32 = 1 << 8;
const DEMUX_TS_INDEX_OPCR: i32 = 1 << 9;
const DEMUX_TS_INDEX_SPLICING_POINT: i32 = 1 << 10;
const DEMUX_TS_INDEX_PRIVATE_DATA: i32 = 1 << 11;
const DEMUX_TS_INDEX_ADAPTATION_EXTENSION: i32 = 1 << 12;
const INVALID_FIRST_MB_IN_SLICE: i32 = -1;
const AVC_SC_I_SLICE: i32 = 1 << 0;
const AVC_SC_P_SLICE: i32 = 1 << 1;
const AVC_SC_B_SLICE: i32 = 1 << 2;
const AVC_SC_SI_SLICE: i32 = 1 << 3;
const AVC_SC_SP_SLICE: i32 = 1 << 4;
const HEVC_SC_SPS: i32 = 1 << 0;
const HEVC_SC_AUD: i32 = 1 << 1;
const HEVC_SC_BLA_W_LP: i32 = 1 << 2;
const HEVC_SC_BLA_W_RADL: i32 = 1 << 3;
const HEVC_SC_BLA_N_LP: i32 = 1 << 4;
const HEVC_SC_IDR_W_RADL: i32 = 1 << 5;
const HEVC_SC_IDR_N_LP: i32 = 1 << 6;
const HEVC_SC_TRAIL_CRA: i32 = 1 << 7;
const VVC_SC_IDR_W_RADL: i32 = 1 << 0;
const VVC_SC_IDR_N_LP: i32 = 1 << 1;
const VVC_SC_CRA: i32 = 1 << 2;
const VVC_SC_GDR: i32 = 1 << 3;
const VVC_SC_VPS: i32 = 1 << 4;
const VVC_SC_SPS: i32 = 1 << 5;
const VVC_SC_AUD: i32 = 1 << 6;
const RECORD_SC_TYPE_NONE: i32 = 0;
const RECORD_SC_TYPE_SC: i32 = 1;
const RECORD_SC_TYPE_SC_HEVC: i32 = 2;
const RECORD_SC_TYPE_SC_AVC: i32 = 3;
const RECORD_SC_TYPE_SC_VVC: i32 = 4;

#[derive(Clone, Copy, Debug, Default)]
struct RecordEventState {
    last_transport_scrambling_control: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
struct TsPacketRecordView<'a> {
    pid: i32,
    payload_unit_start: bool,
    priority: bool,
    scrambling_control: u8,
    discontinuity_indicator: bool,
    random_access_indicator: bool,
    pcr_flag: bool,
    opcr_flag: bool,
    splicing_point_flag: bool,
    private_data_flag: bool,
    adaptation_extension_flag: bool,
    payload: &'a [u8],
}

impl<'a> TsPacketRecordView<'a> {
    fn parse(packet: &'a [u8]) -> Option<Self> {
        if packet.len() < 4 || packet[0] != 0x47 {
            return None;
        }
        let payload_unit_start = (packet[1] & 0x40) != 0;
        let priority = (packet[1] & 0x20) != 0;
        let pid = (((packet[1] & 0x1f) as i32) << 8) | packet[2] as i32;
        let scrambling_control = (packet[3] >> 6) & 0x03;
        let adaptation_control = (packet[3] >> 4) & 0x03;
        if adaptation_control == 0 {
            return None;
        }
        let mut offset = 4usize;
        let mut discontinuity_indicator = false;
        let mut random_access_indicator = false;
        let mut pcr_flag = false;
        let mut opcr_flag = false;
        let mut splicing_point_flag = false;
        let mut private_data_flag = false;
        let mut adaptation_extension_flag = false;
        if adaptation_control == 2 || adaptation_control == 3 {
            if offset >= packet.len() {
                return None;
            }
            let adaptation_len = packet[offset] as usize;
            if offset + 1 + adaptation_len > packet.len() {
                return None;
            }
            if adaptation_len > 0 {
                let flags = packet[offset + 1];
                discontinuity_indicator = (flags & 0x80) != 0;
                random_access_indicator = (flags & 0x40) != 0;
                pcr_flag = (flags & 0x10) != 0;
                opcr_flag = (flags & 0x08) != 0;
                splicing_point_flag = (flags & 0x04) != 0;
                private_data_flag = (flags & 0x02) != 0;
                adaptation_extension_flag = (flags & 0x01) != 0;
            }
            offset += 1 + adaptation_len;
            if adaptation_control == 2 {
                return Some(Self {
                    pid,
                    payload_unit_start,
                    priority,
                    scrambling_control,
                    discontinuity_indicator,
                    random_access_indicator,
                    pcr_flag,
                    opcr_flag,
                    splicing_point_flag,
                    private_data_flag,
                    adaptation_extension_flag,
                    payload: &[],
                });
            }
        }
        Some(Self {
            pid,
            payload_unit_start,
            priority,
            scrambling_control,
            discontinuity_indicator,
            random_access_indicator,
            pcr_flag,
            opcr_flag,
            splicing_point_flag,
            private_data_flag,
            adaptation_extension_flag,
            payload: packet.get(offset..).unwrap_or(&[]),
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct StartCodeInfo {
    mask: i32,
    first_mb_in_slice: i32,
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
        payload.bytes(),
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
            raw: false,
            length_field_bits,
            ..
        } => {
            let (table_id, version, section_num, data_len) =
                parse_section_event(payload, *length_field_bits)?;
            Some(DemuxFilterEvent::Section(DemuxFilterSectionEvent {
                tableId: table_id,
                version,
                sectionNum: section_num,
                dataLength: data_len,
            }))
        }
        FilterConfigKind::PesData { raw: false, .. } => {
            let stream_id = pes_stream_id
                .or_else(|| parse_pes_stream_id(payload))
                .unwrap_or(0);
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
            debug_assert!(!*secure_memory);
            let av_slice = av_slice?;
            let av_data_id = av_data_id.filter(|id| *id != 0)?;
            let av_memory = av_memory?;
            let (pts, dts, stream_id) = if let Some(metadata) = av_metadata {
                (metadata.pts_90khz, metadata.dts_90khz, metadata.stream_id)
            } else {
                let (pts, dts) = parse_pes_timestamps(payload);
                (
                    pts,
                    dts,
                    parse_pes_stream_id(payload).unwrap_or(config.sub_type_hint),
                )
            };
            let event = DemuxFilterMediaEvent {
                streamId: stream_id,
                isPtsPresent: pts.is_some(),
                pts: pts.map(|value| value as i64).unwrap_or(0),
                isDtsPresent: dts.is_some(),
                dts: dts.map(|value| value as i64).unwrap_or(0),
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
    let packet_view = TsPacketRecordView::parse(packet)?;
    let mut observed_ts_index = 0i32;
    if cumulative_bytes == 0 {
        observed_ts_index |= DEMUX_TS_INDEX_FIRST_PACKET;
    }
    if packet_view.payload_unit_start {
        observed_ts_index |= DEMUX_TS_INDEX_PAYLOAD_UNIT_START;
    }
    if packet_view.priority {
        observed_ts_index |= DEMUX_TS_INDEX_PRIORITY;
    }
    if packet_view.discontinuity_indicator {
        observed_ts_index |= DEMUX_TS_INDEX_DISCONTINUITY;
    }
    if packet_view.random_access_indicator {
        observed_ts_index |= DEMUX_TS_INDEX_RANDOM_ACCESS;
    }
    if packet_view.pcr_flag {
        observed_ts_index |= DEMUX_TS_INDEX_PCR;
    }
    if packet_view.opcr_flag {
        observed_ts_index |= DEMUX_TS_INDEX_OPCR;
    }
    if packet_view.splicing_point_flag {
        observed_ts_index |= DEMUX_TS_INDEX_SPLICING_POINT;
    }
    if packet_view.private_data_flag {
        observed_ts_index |= DEMUX_TS_INDEX_PRIVATE_DATA;
    }
    if packet_view.adaptation_extension_flag {
        observed_ts_index |= DEMUX_TS_INDEX_ADAPTATION_EXTENSION;
    }
    if let Some(previous) = record_state
        .last_transport_scrambling_control
        .replace(packet_view.scrambling_control)
    {
        if previous != packet_view.scrambling_control {
            observed_ts_index |= match packet_view.scrambling_control {
                0 => DEMUX_TS_INDEX_CHANGE_TO_NOT_SCRAMBLED,
                2 => DEMUX_TS_INDEX_CHANGE_TO_EVEN_SCRAMBLED,
                3 => DEMUX_TS_INDEX_CHANGE_TO_ODD_SCRAMBLED,
                _ => 0,
            };
        }
    }
    let pts = parse_record_packet_pts(packet_view.payload).unwrap_or(0);
    let start_code = parse_start_code_info(
        packet_view.payload,
        sc_index_type,
        configured_sc_index_mask_bits,
    );
    let first_mb_in_slice = start_code
        .map(|info| info.first_mb_in_slice)
        .unwrap_or(INVALID_FIRST_MB_IN_SLICE);
    let sc_index_mask = build_sc_index_mask(
        sc_index_type,
        start_code.map(|info| info.mask).unwrap_or(0),
        configured_sc_index_mask_bits,
    );
    Some(DemuxFilterEvent::TsRecord(DemuxFilterTsRecordEvent {
        pid: DemuxPid::TPid(packet_view.pid),
        tsIndexMask: observed_ts_index & configured_ts_index_mask,
        scIndexMask: sc_index_mask,
        byteNumber: cumulative_bytes as i64,
        pts,
        firstMbInSlice: first_mb_in_slice,
    }))
}

fn parse_record_packet_pts(payload: &[u8]) -> Option<i64> {
    if payload.starts_with(&[0x00, 0x00, 0x01]) {
        parse_pes_timestamps(payload).0.map(|value| value as i64)
    } else {
        None
    }
}

fn build_sc_index_mask(
    sc_index_type: i32,
    observed_mask: i32,
    configured_mask: i32,
) -> DemuxFilterScIndexMask {
    let masked = observed_mask & configured_mask;
    match sc_index_type {
        RECORD_SC_TYPE_SC => DemuxFilterScIndexMask::ScIndex(masked),
        RECORD_SC_TYPE_SC_HEVC => DemuxFilterScIndexMask::ScHevc(masked),
        RECORD_SC_TYPE_SC_AVC => DemuxFilterScIndexMask::ScAvc(masked),
        RECORD_SC_TYPE_SC_VVC => DemuxFilterScIndexMask::ScVvc(masked),
        _ => DemuxFilterScIndexMask::ScIndex(0),
    }
}

fn parse_start_code_info(
    payload: &[u8],
    sc_index_type: i32,
    configured_mask: i32,
) -> Option<StartCodeInfo> {
    if sc_index_type == RECORD_SC_TYPE_NONE || configured_mask == 0 {
        return None;
    }
    let es_payload = pes_payload_bytes(payload).unwrap_or(payload);
    let (offset, prefix_len) = find_start_code_prefix(es_payload)?;
    let nal = &es_payload[offset + prefix_len..];
    match sc_index_type {
        RECORD_SC_TYPE_SC => parse_generic_sc_index(nal),
        RECORD_SC_TYPE_SC_AVC => parse_avc_sc_index(nal),
        RECORD_SC_TYPE_SC_HEVC => parse_hevc_sc_index(nal),
        RECORD_SC_TYPE_SC_VVC => parse_vvc_sc_index(nal),
        _ => None,
    }
}

fn pes_payload_bytes(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < 9 || !payload.starts_with(&[0x00, 0x00, 0x01]) {
        return None;
    }
    let header_len = payload[8] as usize;
    payload.get(9 + header_len..)
}

fn find_start_code_prefix(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0usize;
    while i + 3 < bytes.len() {
        if bytes[i] == 0x00 && bytes[i + 1] == 0x00 {
            if bytes[i + 2] == 0x01 {
                return Some((i, 3));
            }
            if i + 4 < bytes.len() && bytes[i + 2] == 0x00 && bytes[i + 3] == 0x01 {
                return Some((i, 4));
            }
        }
        i += 1;
    }
    None
}

fn parse_generic_sc_index(nal: &[u8]) -> Option<StartCodeInfo> {
    let code = *nal.first()?;
    let mask = match code {
        0x00 if nal.len() >= 3 => {
            let picture_header = u16::from_be_bytes([nal[1], nal[2]]);
            match (picture_header >> 3) & 0x07 {
                1 => 1 << 0,
                2 => 1 << 1,
                3 => 1 << 2,
                _ => 0,
            }
        }
        0xb3 => 1 << 3,
        _ => 0,
    };
    (mask != 0).then_some(StartCodeInfo {
        mask,
        first_mb_in_slice: INVALID_FIRST_MB_IN_SLICE,
    })
}

fn parse_avc_sc_index(nal: &[u8]) -> Option<StartCodeInfo> {
    let header = *nal.first()?;
    let nal_type = header & 0x1f;
    if !(1..=5).contains(&nal_type) {
        return None;
    }
    let rbsp = nal_to_rbsp(&nal[1..]);
    let mut reader = BitReader::new(&rbsp);
    let first_mb = reader.read_ue()? as i32;
    let slice_type = (reader.read_ue()? % 5) as u8;
    let mask = match slice_type {
        0 => AVC_SC_P_SLICE,
        1 => AVC_SC_B_SLICE,
        2 => AVC_SC_I_SLICE,
        3 => AVC_SC_SP_SLICE,
        4 => AVC_SC_SI_SLICE,
        _ => 0,
    };
    (mask != 0).then_some(StartCodeInfo {
        mask,
        first_mb_in_slice: first_mb,
    })
}

fn parse_hevc_sc_index(nal: &[u8]) -> Option<StartCodeInfo> {
    if nal.len() < 2 {
        return None;
    }
    let nal_type = (nal[0] >> 1) & 0x3f;
    let mask = match nal_type {
        33 => HEVC_SC_SPS,
        35 => HEVC_SC_AUD,
        16 => HEVC_SC_BLA_W_LP,
        17 => HEVC_SC_BLA_W_RADL,
        18 => HEVC_SC_BLA_N_LP,
        19 => HEVC_SC_IDR_W_RADL,
        20 => HEVC_SC_IDR_N_LP,
        0..=9 | 21 => HEVC_SC_TRAIL_CRA,
        _ => 0,
    };
    (mask != 0).then_some(StartCodeInfo {
        mask,
        first_mb_in_slice: INVALID_FIRST_MB_IN_SLICE,
    })
}

fn parse_vvc_sc_index(nal: &[u8]) -> Option<StartCodeInfo> {
    let header = *nal.first()?;
    let nal_type = (header >> 3) & 0x1f;
    let mask = match nal_type {
        14 => VVC_SC_VPS,
        15 => VVC_SC_SPS,
        20 => VVC_SC_AUD,
        7 => VVC_SC_GDR,
        8 => VVC_SC_IDR_W_RADL,
        9 => VVC_SC_IDR_N_LP,
        10 => VVC_SC_CRA,
        _ => 0,
    };
    (mask != 0).then_some(StartCodeInfo {
        mask,
        first_mb_in_slice: INVALID_FIRST_MB_IN_SLICE,
    })
}

fn nal_to_rbsp(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut zero_run = 0usize;
    for &byte in bytes {
        if zero_run >= 2 && byte == 0x03 {
            zero_run = 0;
            continue;
        }
        out.push(byte);
        if byte == 0x00 {
            zero_run += 1;
        } else {
            zero_run = 0;
        }
    }
    out
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit_offset: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_offset: 0,
        }
    }

    fn read_bit(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.bit_offset / 8)?;
        let bit = 7 - (self.bit_offset % 8);
        self.bit_offset += 1;
        Some((byte >> bit) & 1)
    }

    fn read_bits(&mut self, count: usize) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Some(value)
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zero_bits = 0usize;
        while self.read_bit()? == 0 {
            leading_zero_bits += 1;
            if leading_zero_bits > 31 {
                return None;
            }
        }
        let suffix = if leading_zero_bits == 0 {
            0
        } else {
            self.read_bits(leading_zero_bits)?
        };
        Some(((1u32 << leading_zero_bits) - 1) + suffix)
    }
}

fn parse_section_event(payload: &[u8], length_field_bits: i32) -> Option<(i32, i32, i32, i64)> {
    let header = parse_section_header(payload, length_field_bits)?;
    let version = header.version.unwrap_or(0) as i32;
    let section_num = header.section_number.unwrap_or(0) as i32;
    Some((
        header.table_id as i32,
        version,
        section_num,
        header.total_length as i64,
    ))
}

fn parse_pes_stream_id(payload: &[u8]) -> Option<i32> {
    if payload.len() >= 4 && payload[0] == 0x00 && payload[1] == 0x00 && payload[2] == 0x01 {
        Some(payload[3] as i32)
    } else {
        None
    }
}

fn parse_pes_timestamps(payload: &[u8]) -> (Option<u64>, Option<u64>) {
    if payload.len() < 14 || payload[0] != 0x00 || payload[1] != 0x00 || payload[2] != 0x01 {
        return (None, None);
    }
    let flags = payload[7];
    let header_len = payload[8] as usize;
    let pts_dts = (flags >> 6) & 0x03;
    let header_data = payload.get(9..9 + header_len).unwrap_or(&[]);
    let pts = if pts_dts & 0x02 != 0 && header_data.len() >= 5 {
        decode_pts_dts(&header_data[0..5])
    } else {
        None
    };
    let dts = if pts_dts == 0x03 && header_data.len() >= 10 {
        decode_pts_dts(&header_data[5..10])
    } else {
        None
    };
    (pts, dts)
}

fn decode_pts_dts(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 5 {
        return None;
    }
    let value = (((bytes[0] >> 1) & 0x07) as u64) << 30
        | ((bytes[1] as u64) << 22)
        | (((bytes[2] >> 1) as u64) << 15)
        | ((bytes[3] as u64) << 7)
        | ((bytes[4] >> 1) as u64);
    Some(value)
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

fn map_isdbs_stream_selector(
    stream_id: i32,
    stream_id_type: FrontendIsdbsStreamIdType,
    frequency_hz: u64,
) -> Result<(Option<u32>, Option<FrontendStreamIdKind>), HalError> {
    match stream_id_type {
        FrontendIsdbsStreamIdType::UNDEFINED => {
            if stream_id != 0 {
                return Err(HalError::InvalidArgument(
                    "ISDB-S streamId must be zero when streamIdType is UNDEFINED".into(),
                ));
            }
            Ok((None, None))
        }
        FrontendIsdbsStreamIdType::STREAM_ID
        | FrontendIsdbsStreamIdType::RELATIVE_STREAM_NUMBER => {
            if stream_id < 0 {
                return Err(HalError::InvalidArgument(
                    "ISDB-S stream selector must be non-negative when streamIdType is specified"
                        .into(),
                ));
            }
            if is_japan_cs110_if_frequency_hz(frequency_hz) {
                return Err(HalError::InvalidArgument(
                    "CS110 frontend tune must not carry TSID or relative stream-number selector"
                        .into(),
                ));
            }
            let value = u32::try_from(stream_id).map_err(|_| {
                HalError::InvalidArgument(format!(
                    "ISDB-S stream selector out of range: {stream_id}"
                ))
            })?;
            let kind = if matches!(stream_id_type, FrontendIsdbsStreamIdType::STREAM_ID) {
                FrontendStreamIdKind::AbsoluteStreamId
            } else {
                FrontendStreamIdKind::RelativeStreamNumber
            };
            Ok((Some(value), Some(kind)))
        }
        _ => Err(HalError::InvalidArgument(format!(
            "unsupported ISDB-S streamIdType: {:?}",
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
    eprintln!("maleicacid-tuner-hal: backend error: {err}");
    Status::new_service_specific_error(hal_error_tuner_result(&err), None)
}

fn invalid_argument_status(message: &str) -> Status {
    eprintln!("maleicacid-tuner-hal: invalid argument: {message}");
    Status::new_service_specific_error(TunerResult::INVALID_ARGUMENT.0, None)
}

fn invalid_state_status(message: &str) -> Status {
    eprintln!("maleicacid-tuner-hal: invalid state: {message}");
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
    normalize_filter_delay_hint(hint)
}

fn normalize_filter_delay_hint(hint: &FilterDelayHint) -> BinderResult<FilterDelayHintState> {
    if hint.hintValue < 0 {
        return Err(invalid_argument_status(
            "filter delay hint must be non-negative",
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
        Err(invalid_argument_status("TS PID out of range"))
    }
}

fn build_filter_summary(settings: &DemuxFilterSettings) -> BinderResult<FilterConfig> {
    let config = match settings {
        DemuxFilterSettings::Ts(ts) => {
            let tpid = validate_ts_pid(ts.tpid)?;
            FilterConfig {
                tpid,
                main_type_bits: DemuxFilterMainType::TS.0,
                sub_type_hint: 0,
                kind: match &ts.filterSettings {
                    DemuxTsFilterSettingsFilterSettings::Noinit(_) => {
                        return Err(Status::new_service_specific_error(
                            TunerResult::UNAVAILABLE.0,
                            None,
                        ));
                    }
                    DemuxTsFilterSettingsFilterSettings::Section(section) => {
                        let Some(length_field_bits) =
                            normalize_length_field_bits(section.bitWidthOfLengthField)
                        else {
                            return Err(Status::new_service_specific_error(
                                TunerResult::UNAVAILABLE.0,
                                None,
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
                            return Err(Status::new_service_specific_error(TunerResult::UNAVAILABLE.0, Some("AV passthrough is unsupported in r51; use MediaEvent/shared-memory delivery")));
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
                        if pes.streamId > 255 {
                            return Err(invalid_argument_status(
                                "PES streamId must be <=255, with <=0 reserved for wildcard",
                            ));
                        }
                        FilterConfigKind::PesData {
                            stream_id: pes.streamId,
                            raw: pes.isRaw,
                        }
                    }
                    DemuxTsFilterSettingsFilterSettings::Record(record) => {
                        let ts_supported_mask = DEMUX_TS_INDEX_FIRST_PACKET
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
                            | DEMUX_TS_INDEX_ADAPTATION_EXTENSION;
                        if (record.tsIndexMask & !ts_supported_mask) != 0 {
                            return Err(invalid_argument_status(
                                "record tsIndexMask contains unsupported bits",
                            ));
                        }
                        let (expected_type, sc_index_mask_bits) = match &record.scIndexMask {
                            DemuxFilterScIndexMask::ScIndex(v) => (RECORD_SC_TYPE_SC, *v),
                            DemuxFilterScIndexMask::ScAvc(v) => (RECORD_SC_TYPE_SC_AVC, *v),
                            DemuxFilterScIndexMask::ScHevc(v) => (RECORD_SC_TYPE_SC_HEVC, *v),
                            DemuxFilterScIndexMask::ScVvc(v) => (RECORD_SC_TYPE_SC_VVC, *v),
                        };
                        let sc_index_type = record.scIndexType.0;
                        if sc_index_type == RECORD_SC_TYPE_NONE {
                            if sc_index_mask_bits != 0 {
                                return Err(invalid_argument_status(
                                    "record SC index NONE requires zero mask",
                                ));
                            }
                        } else if !matches!(
                            sc_index_type,
                            RECORD_SC_TYPE_SC
                                | RECORD_SC_TYPE_SC_AVC
                                | RECORD_SC_TYPE_SC_HEVC
                                | RECORD_SC_TYPE_SC_VVC
                        ) {
                            return Err(invalid_argument_status(
                                "unsupported record SC index type",
                            ));
                        } else if sc_index_type != expected_type {
                            return Err(invalid_argument_status(
                                "record SC index type and mask union variant mismatch",
                            ));
                        }
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
            "section table version must be -1 or 0..31",
        ));
    }
    Ok((version >= 0).then_some(version))
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
            if !(0..=255).contains(&table.tableId) {
                return Err(invalid_argument_status(
                    "section tableId must be in 0..=255",
                ));
            }
            let version = normalize_table_info_version(table.version)?;
            Ok(SectionCondition {
                filter_bytes: vec![table.tableId as u8],
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
        return Err(invalid_argument_status("DVR dataFormat must be TS"));
    }
    if packet_size != 188 {
        return Err(invalid_argument_status("DVR packetSize must be 188"));
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
        return Err(invalid_argument_status("DVR bufferSize must be positive"));
    }
    let capacity = i64::from(buffer_size);
    if low_threshold < 0 {
        return Err(invalid_argument_status(
            "DVR lowThreshold must be non-negative",
        ));
    }
    if high_threshold < 0 {
        return Err(invalid_argument_status(
            "DVR highThreshold must be non-negative",
        ));
    }
    if low_threshold > high_threshold {
        return Err(invalid_argument_status(
            "DVR lowThreshold must be <= highThreshold",
        ));
    }
    if low_threshold > capacity || high_threshold > capacity {
        return Err(invalid_argument_status(
            "DVR thresholds must be <= bufferSize",
        ));
    }
    if (status_mask & !supported_status_mask) != 0 {
        return Err(invalid_argument_status(
            "DVR statusMask contains unsupported bits",
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
    fn relative_scan_stream_id_is_reported_as_tsid_not_slot() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(0),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        assert_eq!(
            FrontendHal::reported_scan_input_stream_id(&request),
            Some(0x4010)
        );
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
        let backing = AvSharedBacking::new(16).unwrap();
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
        assert!(backing.release(1));
        assert!(!backing.release(1));
        assert_eq!(backing.stats().stale_releases, 1);
    }

    #[test]
    fn av_shared_backing_offsets_lengths_and_release_reuse_are_bounded() {
        let backing = AvSharedBacking::new(188).unwrap();
        let payload = vec![0x47; 188];
        let first = backing.allocate(100, &payload).expect("first AV slot");
        assert_eq!(first.offset % backing.slot_size, 0);
        assert_eq!(first.len, payload.len());
        assert!(first.offset + first.len < backing.total_size());
        assert_eq!(backing.stats().allocated_slots, 1);
        assert!(backing.release(100));
        assert_eq!(backing.stats().allocated_slots, 0);
        assert_eq!(backing.stats().free_slots, AV_SLOT_COUNT);
        let second = backing.allocate(101, &payload).expect("reused AV slot");
        assert_eq!(second.offset % backing.slot_size, 0);
        assert_eq!(second.len, payload.len());
        assert!(second.offset + second.len < backing.total_size());
        assert_eq!(backing.stats().allocated_slots, 1);
    }

    #[test]
    fn active_slot_collision_fails_before_overwriting_existing_entry() {
        let backing = AvSharedBacking::new(188).unwrap();
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
        let active = backing.active.lock().unwrap();
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
            Some(AvPayloadDeliveryResult::DroppedNoSharedHandle)
        ));
        assert!(!av_payload_can_notify_data_ready(
            true,
            Some(AvPayloadDeliveryResult::DroppedNoFreeSlot)
        ));
        assert!(!av_payload_can_notify_data_ready(
            true,
            Some(AvPayloadDeliveryResult::DroppedInvalidPayload)
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
                Some(AvPayloadDeliveryResult::DroppedNoSharedHandle),
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
                Some(AvPayloadDeliveryResult::DroppedInvalidPayload),
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
    fn av_stats_report_no_slot_and_invalid_payload_without_evicting() {
        let backing = AvSharedBacking::new(AV_MIN_SLOT_SIZE).unwrap();
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
                super::AvPayloadDeliveryResult::DroppedInvalidPayload
            ))
        ));
        let stats = backing.stats();
        assert_eq!(stats.evicted_slots, 0);
        assert_eq!(stats.av_overflow_no_slot, 1);
        assert_eq!(stats.av_invalid_payload, 1);
        assert!(stats.summary().contains("av_overflow_no_slot=1"));
        assert!(stats.summary().contains("av_invalid_payload=1"));
        assert!(backing.debug_dump_line("unit").contains("unit av_shared"));
        assert!(backing
            .debug_dump_line("unit")
            .contains("av_overflow_no_slot=1"));
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
            Arc::clone(&hal.demux_live_ids),
            Arc::clone(&hal.demux_registry),
            Arc::clone(&hal.descrambler_registry),
        );
        demux_hal.release_registration_best_effort();

        assert_eq!(hal.first_available_demux_id(), Some(released_id));
        assert!(!hal.demux_live_ids.lock().unwrap().contains(&released_id));
        assert!(!hal
            .demux_registry
            .lock()
            .unwrap()
            .contains_key(&released_id));
    }

    #[test]
    fn open_demux_by_id_refcounts_existing_record() {
        let hal = TunerHal::new();
        let demux_id = all_demux_ids()[0];
        let record = hal.open_or_create_demux_record_by_id(demux_id).unwrap();
        assert_eq!(record.lock().unwrap().ref_count, 1);

        let _second = hal
            .openDemuxById(demux_id)
            .expect("pool member should be reopenable by ID");
        assert_eq!(record.lock().unwrap().ref_count, 2);
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
        registry.lock().unwrap().insert(
            42,
            LnbRuntimeState {
                profile: LnbDeviceProfile::Px4Device15VOnly,
                ..Default::default()
            },
        );
        let frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>> = Arc::new(BTreeMap::new());
        let lnb = LnbHal::new(42, Arc::clone(&registry), frontend_registry);

        lnb.setVoltage(LnbVoltage::VOLTAGE_15V).unwrap();
        let stored = registry.lock().unwrap().get(&42).cloned().unwrap();
        assert_eq!(stored.voltage, Some(LnbVoltage::VOLTAGE_15V));
        assert_eq!(stored.generation, 1);
    }

    #[test]
    fn unsupported_lnb_tone_is_rejected_before_state_change() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        registry
            .lock()
            .unwrap()
            .insert(43, LnbRuntimeState::default());
        let frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>> = Arc::new(BTreeMap::new());
        let lnb = LnbHal::new(43, Arc::clone(&registry), frontend_registry);

        assert!(lnb.setTone(LnbTone::CONTINUOUS).is_err());
        let stored = registry.lock().unwrap().get(&43).cloned().unwrap();
        assert_eq!(stored.tone, None);
        assert_eq!(stored.generation, 0);
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
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("pxmlt5video0")),
            LnbDeviceProfile::PxMltDevice15VOnly
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("pxmlt8video7")),
            LnbDeviceProfile::PxMltDevice15VOnly
        );
        assert_eq!(
            px4_lnb_profile_from_devname(Some("isdb6014video0")),
            LnbDeviceProfile::PxMltDevice15VOnly
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
            LnbDeviceProfile::PxMltDevice15VOnly
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
                allowed_systems: vec![declared_type_to_system(declared_type).unwrap()],
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
            LnbDeviceProfile::PxMltDevice15VOnly,
            LnbVoltage::VOLTAGE_15V
        ));
        assert!(!LnbHal::voltage_supported(
            LnbDeviceProfile::PxMltDevice15VOnly,
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
                assert_eq!(caps.modulationCap, FrontendIsdbtModulation::AUTO.0);
                assert_eq!(caps.coderateCap, FrontendIsdbtCoderate::AUTO.0);
                assert_eq!(caps.guardIntervalCap, FrontendIsdbtGuardInterval::AUTO.0);
                assert_eq!(
                    caps.timeInterleaveCap,
                    FrontendIsdbtTimeInterleaveMode::AUTO.0
                );
                assert!(caps.isSegmentAuto);
                assert!(caps.isFullSegment);
            }
            _ => panic!("ISDB-T entry must report ISDB-T capabilities"),
        }
    }

    #[test]
    fn isdbs_capability_matches_fixed_japanese_target_values() {
        let entry = px4_entry(2, FrontendType::ISDBS, FrontendSystem::IsdbS);
        match entry_frontend_caps(&entry) {
            FrontendCapabilities::IsdbsCaps(caps) => {
                assert_eq!(caps.modulationCap, FrontendIsdbsModulation::AUTO.0);
                assert_eq!(caps.coderateCap, FrontendIsdbsCoderate::AUTO.0);
            }
            _ => panic!("ISDB-S entry must report ISDB-S capabilities"),
        }
    }

    #[test]
    fn vts_lab_settings_are_subset_of_advertised_capabilities() {
        let t = match entry_frontend_caps(&px4_entry(1, FrontendType::ISDBT, FrontendSystem::IsdbT))
        {
            FrontendCapabilities::IsdbtCaps(caps) => caps,
            _ => panic!("ISDB-T caps expected"),
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
            _ => panic!("ISDB-S caps expected"),
        };
        assert_ne!(s.modulationCap & FrontendIsdbsModulation::AUTO.0, 0);
        assert_ne!(s.coderateCap & FrontendIsdbsCoderate::AUTO.0, 0);
    }

    #[test]
    fn px4_isdbt_advertised_range_covers_japan_catv_and_uhf_contract() {
        assert_eq!(
            JAPAN_CATV_C13_CENTER_HZ - JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
            110_642_857
        );
        assert_eq!(JAPAN_CATV_C63_CENTER_HZ, 465_142_857);
        assert_eq!(JAPAN_UHF_13_CENTER_HZ, 473_142_857);
        assert_eq!(
            JAPAN_UHF_62_CENTER_HZ + JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
            767_642_857
        );
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
    fn isdbs_symbol_rate_and_stream_selector_validation_are_not_silently_ignored() {
        let source = include_str!("tuner_hal.rs");
        assert!(source.contains("if s.symbolRate != 0"));
        assert!(map_isdbs_stream_selector(
            0,
            FrontendIsdbsStreamIdType::UNDEFINED,
            JAPAN_BS_FIRST_IF_HZ as u64
        )
        .unwrap()
        .0
        .is_none());
        assert!(map_isdbs_stream_selector(
            1,
            FrontendIsdbsStreamIdType::UNDEFINED,
            JAPAN_BS_FIRST_IF_HZ as u64
        )
        .is_err());
        assert!(map_isdbs_stream_selector(
            1,
            FrontendIsdbsStreamIdType::UNDEFINED,
            JAPAN_CS110_LAST_IF_HZ as u64
        )
        .is_err());
        assert!(map_isdbs_stream_selector(
            1,
            FrontendIsdbsStreamIdType::STREAM_ID,
            JAPAN_CS110_LAST_IF_HZ as u64
        )
        .is_err());
    }

    #[test]
    fn isdbs_symbol_rate_negative_is_rejected_by_contract() {
        let source = include_str!("tuner_hal.rs");
        assert!(source.contains("if s.symbolRate != 0"));
        assert!(!source.contains(&["if s.symbolRate", " > 0"].concat()));
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
    fn rf_lock_status_uses_dvb_carrier_only_when_supported() {
        let telemetry = FrontendTelemetry {
            locked: false,
            rf_locked: Some(true),
            ..Default::default()
        };
        let with_rf = FrontendHal::status_for_types(
            false,
            true,
            false,
            &telemetry,
            &[FrontendStatusType::RF_LOCK, FrontendStatusType::DEMOD_LOCK],
        )
        .unwrap();
        assert!(matches!(with_rf[0], FrontendStatus::IsRfLocked(true)));
        assert!(matches!(with_rf[1], FrontendStatus::IsDemodLocked(false)));

        let without_rf = FrontendHal::status_for_types(
            false,
            false,
            false,
            &telemetry,
            &[FrontendStatusType::RF_LOCK, FrontendStatusType::DEMOD_LOCK],
        );
        assert!(without_rf.is_err());
    }

    #[test]
    fn rf_lock_readiness_is_unsupported_for_px4_and_stable_for_dvb() {
        assert!(FrontendHal::readiness_for_types(
            false,
            false,
            false,
            &[FrontendStatusType::RF_LOCK]
        )
        .is_err());
        assert_eq!(
            FrontendHal::readiness_for_types(false, true, false, &[FrontendStatusType::RF_LOCK])
                .unwrap(),
            vec![FrontendStatusReadiness::STABLE]
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
        registry.lock().unwrap().insert(
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
                message: "runtime read failed".into()
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
    fn av_sync_is_exposed_through_av_filters_without_time_filter_claim() {
        let hal = TunerHal::new();
        let caps = hal.getDemuxCaps().unwrap();
        assert_eq!(caps.numPcrFilter, 0);
        assert!(!caps.bTimeFilter);
        assert!(caps.numAudioFilter > 0);
        assert!(caps.numVideoFilter > 0);
    }

    #[test]
    fn noinit_ts_filters_used_by_pcr_ts_temi_are_rejected_at_configure_time() {
        let settings = DemuxFilterSettings::Ts(android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::DemuxTsFilterSettings::DemuxTsFilterSettings {
            tpid: 0x100,
            filterSettings: DemuxTsFilterSettingsFilterSettings::Noinit(false),
        });
        assert!(build_filter_summary(&settings).is_err());
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
        assert!(runtime.pump_worker.lock().unwrap().is_none());
    }

    #[test]
    fn diseqc_is_permanently_unavailable() {
        let registry = Arc::new(Mutex::new(BTreeMap::new()));
        registry
            .lock()
            .unwrap()
            .insert(42, LnbRuntimeState::default());
        let frontend_registry: Arc<BTreeMap<i32, Arc<FrontendRuntime>>> = Arc::new(BTreeMap::new());
        let lnb = LnbHal::new(42, Arc::clone(&registry), frontend_registry);

        assert!(lnb.sendDiseqcMessage(&[0xe0, 0x10, 0x38, 0xf0]).is_err());
        let stored = registry.lock().unwrap().get(&42).cloned().unwrap();
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
        PlaybackSettings::PlaybackSettings, RecordSettings::RecordSettings,
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

    fn av_settings(pid: i32, secure: bool) -> DemuxFilterSettings {
        DemuxFilterSettings::Ts(android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::DemuxTsFilterSettings::DemuxTsFilterSettings {
            tpid: pid,
            filterSettings: DemuxTsFilterSettingsFilterSettings::Av(DemuxFilterAvSettings {
                isPassthrough: false,
                isSecureMemory: secure,
            }),
        })
    }

    fn pes_settings(pid: i32) -> DemuxFilterSettings {
        DemuxFilterSettings::Ts(android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::DemuxTsFilterSettings::DemuxTsFilterSettings {
            tpid: pid,
            filterSettings: DemuxTsFilterSettingsFilterSettings::PesData(DemuxFilterPesDataSettings {
                streamId: 0xbd,
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

    #[test]
    fn playback_threshold_contract_uses_unused_write_space() {
        let low = Some(1_048_576);
        let high = Some(3_145_728);
        let capacity = 4_194_304;

        assert_eq!(
            DvrHal::playback_status_from_thresholds(0, low, high, capacity),
            Some(PlaybackStatus::SPACE_FULL)
        );
        assert_eq!(
            DvrHal::playback_status_from_thresholds(1_048_576, low, high, capacity),
            Some(PlaybackStatus::SPACE_ALMOST_FULL)
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
            Some(PlaybackStatus::SPACE_ALMOST_EMPTY)
        );
        assert_eq!(
            DvrHal::playback_status_from_thresholds(4_194_304, low, high, capacity),
            Some(PlaybackStatus::SPACE_EMPTY)
        );
    }

    #[test]
    fn open_time_filter_type_preserves_media_and_non_media_subtype() {
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
    fn table_info_version_contract_is_minus_one_or_zero_to_thirty_one() {
        assert_eq!(normalize_table_info_version(-1).unwrap(), None);
        assert_eq!(normalize_table_info_version(0).unwrap(), Some(0));
        assert_eq!(normalize_table_info_version(31).unwrap(), Some(31));
        assert!(normalize_table_info_version(-2).is_err());
        assert!(normalize_table_info_version(32).is_err());
    }

    #[test]
    fn local_filter_owner_validation_maps_lifecycle_and_argument_errors() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let record = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());
        let local = FilterHal::new(
            DEMUX_ID_BASE,
            record.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
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
            TunerResult::INVALID_STATE.0
        );
        local.closed.store(false, Ordering::SeqCst);

        runtime_io.mark_failed(RuntimeIoKind::Filter, record.filter_id, "test failure");
        assert_eq!(
            validate_local_filter_identity_for_owner(&local, DEMUX_ID_BASE),
            Err(LocalFilterOwnerValidationError::RuntimeFailed)
        );
        assert_eq!(
            local_filter_owner_error_tuner_result(LocalFilterOwnerValidationError::RuntimeFailed),
            TunerResult::INVALID_STATE.0
        );

        let runtime_io_clean = Arc::new(RuntimeIoRegistry::default());
        let local_unregistered = FilterHal::new(
            DEMUX_ID_BASE,
            record.filter_id,
            Arc::clone(&state),
            runtime_io_clean,
            callback,
        )
        .unwrap();
        state.lock().unwrap().unregister_filter(record.filter_id);
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
    fn closed_source_filter_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let destination = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
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
        )
        .unwrap();
        assert!(destination_hal.setDataSource(&source_binder).is_err());
        assert_eq!(
            state
                .lock()
                .unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_source_filter_id,
            None
        );
    }

    #[test]
    fn started_destination_rewire_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let destination = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
        assert!(demux.configure_filter_with_summary(source.filter_id, section_summary.clone()));
        assert!(demux.configure_filter_with_summary(destination.filter_id, section_summary));
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
        )
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
        )
        .unwrap();

        assert!(destination_hal.setDataSource(&source_binder).is_err());
        assert_eq!(
            state
                .lock()
                .unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_source_filter_id,
            None
        );
    }

    #[test]
    fn closed_destination_filter_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let destination = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
        )
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
        )
        .unwrap();
        destination_hal.closed.store(true, Ordering::SeqCst);

        assert!(destination_hal.setDataSource(&source_binder).is_err());
        assert_eq!(
            state
                .lock()
                .unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_source_filter_id,
            None
        );
    }

    #[test]
    fn runtime_failed_destination_filter_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let destination = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
        )
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback,
        )
        .unwrap();
        runtime_io.mark_failed(
            RuntimeIoKind::Filter,
            destination.filter_id,
            "destination failed for test",
        );

        assert!(destination_hal.setDataSource(&source_binder).is_err());
        assert_eq!(
            state
                .lock()
                .unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_source_filter_id,
            None
        );
    }

    #[test]
    fn advertised_ts_linkage_succeeds_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let destination = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
        )
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
        )
        .unwrap();

        destination_hal.setDataSource(&source_binder).unwrap();
        assert_eq!(
            state
                .lock()
                .unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_source_filter_id,
            Some(source.filter_id)
        );
    }

    #[test]
    fn unadvertised_linkage_is_rejected_on_public_set_data_source_path() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let source = demux.register_filter(1, FilterOpenType::NonTs, 4096);
        let destination = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());

        let source_hal = FilterHal::new(
            DEMUX_ID_BASE,
            source.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback.clone(),
        )
        .unwrap();
        let source_binder = BnFilter::new_binder(source_hal, BinderFeatures::default());

        let destination_hal = FilterHal::new(
            DEMUX_ID_BASE,
            destination.filter_id,
            Arc::clone(&state),
            runtime_io,
            callback,
        )
        .unwrap();

        assert!(destination_hal.setDataSource(&source_binder).is_err());
        assert_eq!(
            state
                .lock()
                .unwrap()
                .filter_record(destination.filter_id)
                .unwrap()
                .data_source_filter_id,
            None
        );
    }

    #[test]
    fn delay_hint_record_contract_rejects_media_before_configuration() {
        let mut demux = DemuxHandle::new(0);
        let audio = demux.register_filter(1, FilterOpenType::TsAudio, 4096);
        let video = demux.register_filter(1, FilterOpenType::TsVideo, 4096);
        let section = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let record = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
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
                &size_hint
            )
            .unwrap(),
            FilterDelayHintState::DataSizeDelayBytes(188)
        );
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
        let backing = AvSharedBacking::new(188).unwrap();
        assert_eq!(backing.stats().allocated_slots, 0);
        assert_eq!(backing.total_size(), AV_MIN_SLOT_SIZE * AV_SLOT_COUNT);
        let handle = backing.build_native_handle().unwrap();
        assert_eq!(handle.fds.len(), 1);
        assert_eq!(handle.ints, vec![0]);
        assert!(!handle.ints.contains(&(AV_MIN_SLOT_SIZE as i32)));
        assert!(!handle.ints.contains(&(AV_SLOT_COUNT as i32)));

        backing.release_all();
        assert_eq!(backing.stats().allocated_slots, 0);

        backing.allocate(1, &[0x47; 188]).unwrap();
        assert_eq!(backing.stats().allocated_slots, 1);
        backing.release_all();
        let stats = backing.stats();
        assert_eq!(stats.allocated_slots, 0);
        assert_eq!(stats.free_slots, AV_SLOT_COUNT);
        assert_eq!(stats.released_slots, 1);
    }

    #[test]
    fn runtime_io_registry_flushes_live_av_shared_and_removes_dead_entries() {
        let registry = RuntimeIoRegistry::default();
        let av_shared = AvSharedBacking::new(188).unwrap();
        av_shared.allocate(1, &[0x47; 188]).unwrap();
        assert_eq!(av_shared.stats().allocated_slots, 1);
        registry.entries.lock().unwrap().insert(
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
        registry.entries.lock().unwrap().insert(
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
        registry.flush_all();
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
            data_source_filter_id: None,
            pending_start_event: false,
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
        // AV 正式配送は avDataId != 0 かつ NativeHandle fd を持つ shared slot だけ。
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
            data_source_filter_id: None,
            pending_start_event: false,
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
            other => panic!("unexpected AV event: {:?}", other),
        }
    }
}

#[cfg(test)]
mod r50ao4_av_acceptance_tests {
    use super::*;

    #[test]
    fn av_delivery_decision_never_notifies_data_ready_for_drop_results() {
        assert_eq!(
            av_payload_status_decision(
                true,
                Some(AvPayloadDeliveryResult::DroppedNoSharedHandle),
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
                Some(AvPayloadDeliveryResult::DroppedInvalidPayload),
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
    fn native_handle_exports_memory_index_only_without_slot_metadata() {
        let backing = AvSharedBacking::new(188).unwrap();
        let handle = backing.build_native_handle().unwrap();
        assert_eq!(handle.fds.len(), 1);
        assert_eq!(handle.ints, vec![0]);
        assert_ne!(handle.ints.get(1), Some(&(backing.slot_size as i32)));
        assert_ne!(handle.ints.get(2), Some(&(backing.slot_count as i32)));
    }

    #[test]
    fn public_get_av_shared_handle_exports_memory_index_only_without_slot_metadata() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let record = demux.register_filter(1, FilterOpenType::TsVideo, 4096);
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let callback = BnFilterCallback::new_binder(NoopFilterCallback, BinderFeatures::default());
        let local = FilterHal::new(
            DEMUX_ID_BASE,
            record.filter_id,
            Arc::clone(&state),
            Arc::clone(&runtime_io),
            callback,
        )
        .unwrap();

        local.configure(&av_settings(0x100, false)).unwrap();
        state
            .lock()
            .unwrap()
            .set_filter_av_stream_type_hint_result(
                record.filter_id,
                0xe0,
                AvFilterStreamKind::Video,
            )
            .unwrap();

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

    #[test]
    fn descrambler_state_tracks_token_and_pid_lifecycle() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_registry),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        );

        assert!(descrambler.setDemuxSource(demux_id).is_ok());
        let key_slot = DescramblerKeySlot::empty()
            .with_even(Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8]));
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
            Arc::clone(&hal.demux_registry),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        );

        assert!(descrambler.add_pid_for_test(0x0100).is_err());
        assert!(descrambler.setKeyToken(&[]).is_err());
        assert!(descrambler
            .setDemuxSource(DEMUX_ID_BASE + MAX_LIVE_DEMUXES as i32)
            .is_err());
        assert!(descrambler.setDemuxSource(demux_id).is_ok());
        assert!(descrambler.setDemuxSource(demux_id).is_err());
        assert!(descrambler.setKeyToken(&[0x01]).is_err());
        let key_slot = DescramblerKeySlot::empty()
            .with_even(Multi2KeyMaterial::new([0x11; 32], [0x22; 8], [0x33; 8]));
        let key_token = hal.descrambler_key_table.register_for_test(key_slot);
        assert!(descrambler.setKeyToken(&key_token).is_ok());
        assert!(descrambler.add_pid_for_test(0x1fff).is_err());
        assert!(TunerDescrambler::pid_from_demux_pid(&DemuxPid::TPid(0x1fff)).is_err());
        assert!(descrambler.remove_pid_for_test(0x0222).is_ok());
        assert!(descrambler.close().is_ok());
        assert!(descrambler.close().is_ok());
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
            Arc::clone(&hal.demux_registry),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        );

        assert!(descrambler.setDemuxSource(demux_id).is_ok());
        let key_slot = DescramblerKeySlot::empty()
            .with_even(Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8]));
        let key_token = hal.descrambler_key_table.register_for_test(key_slot);
        assert!(descrambler.setKeyToken(&key_token).is_ok());
        assert!(descrambler.add_pid_for_test(0x0123).is_ok());

        let (demux_generation, demux_state) = {
            let record = record.lock().unwrap();
            (record.generation, record.state.clone())
        };
        let snapshots = {
            let handle = demux_state.lock().unwrap();
            hal.descrambler_registry
                .snapshots_for_demux(demux_id, demux_generation, &handle)
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
            let handle = demux_state.lock().unwrap();
            hal.descrambler_registry
                .snapshots_for_demux(demux_id, demux_generation, &handle)
        };
        assert!(snapshots_after_close.is_empty());
    }

    #[test]
    fn descrambler_snapshot_prunes_reopened_demux_generation() {
        let hal = TunerHal::new();
        let (demux_id, record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_registry),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        );
        assert!(descrambler.setDemuxSource(demux_id).is_ok());
        let key_slot = DescramblerKeySlot::empty()
            .with_even(Multi2KeyMaterial::new([0x55; 32], [0x66; 8], [0x77; 8]));
        let key_token = hal.descrambler_key_table.register_for_test(key_slot);
        assert!(descrambler.setKeyToken(&key_token).is_ok());
        assert!(descrambler.add_pid_for_test(0x0200).is_ok());

        let (generation, demux_state) = {
            let record = record.lock().unwrap();
            (record.generation, record.state.clone())
        };
        let wrong_generation_snapshot = {
            let handle = demux_state.lock().unwrap();
            hal.descrambler_registry.snapshots_for_demux(
                demux_id,
                generation.saturating_add(1),
                &handle,
            )
        };
        assert!(wrong_generation_snapshot.is_empty());

        assert!(descrambler.add_pid_for_test(0x0200).is_ok());
        hal.descrambler_registry
            .invalidate_demux(demux_id, generation);
        let (_, demux_after_invalidate, generation_after_invalidate, _, pids_after_invalidate) =
            descrambler.debug_snapshot();
        assert_eq!(demux_after_invalidate, None);
        assert_eq!(generation_after_invalidate, None);
        assert!(pids_after_invalidate.is_empty());
    }

    #[test]
    fn descrambler_rejects_duplicate_active_pid_on_same_demux_generation() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let first = TunerDescrambler::new(
            Arc::clone(&hal.demux_registry),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        );
        let second = TunerDescrambler::new(
            Arc::clone(&hal.demux_registry),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        );
        let key_slot = DescramblerKeySlot::empty()
            .with_even(Multi2KeyMaterial::new([0x01; 32], [0x02; 8], [0x03; 8]));
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
    fn descrambler_snapshot_prunes_source_filter_generation_mismatch() {
        let registry = DescramblerRuntimeRegistry::new();
        let state = Arc::new(Mutex::new(TunerDescramblerState::default()));
        let _id = registry.register(&state);
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        {
            let mut descrambler = state.lock().unwrap();
            descrambler.demux_id = Some(DEMUX_ID_BASE);
            descrambler.demux_generation = Some(10);
            descrambler.key_token = Some(vec![1]);
            descrambler.key_slot = Some(DescramblerKeySlot::empty());
            descrambler.pids.insert(
                0x0123,
                DescramblerPidRegistration {
                    source_filter_id: filter.filter_id,
                    source_filter_generation: filter.delivery_generation,
                },
            );
        }
        assert_eq!(
            registry
                .snapshots_for_demux(DEMUX_ID_BASE, 10, &demux)
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
            .snapshots_for_demux(DEMUX_ID_BASE, 10, &demux)
            .is_empty());
        assert!(state.lock().unwrap().pids.is_empty());
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
            key_slot: DescramblerKeySlot::empty(),
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
    fn cas_bridge_registration_is_fail_closed_until_connected() {
        let table = DescramblerKeyTable::new();
        let key_slot = DescramblerKeySlot::empty()
            .with_even(Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8]));
        assert!(table
            .register_from_cas_bridge(key_slot.clone(), false)
            .is_err());
        let token = table.register_from_cas_bridge(key_slot, true).unwrap();
        assert!(table.resolve_with_diagnostic(&token).is_ok());
    }

    #[test]
    fn set_key_token_records_distinct_failure_diagnostics() {
        let hal = TunerHal::new();
        let (demux_id, _record) = hal.allocate_demux_record().unwrap();
        let descrambler = TunerDescrambler::new(
            Arc::clone(&hal.demux_registry),
            Arc::clone(&hal.descrambler_registry),
            Arc::clone(&hal.descrambler_diagnostics),
            Arc::clone(&hal.descrambler_key_table),
        );
        assert!(descrambler.setDemuxSource(demux_id).is_ok());

        assert!(descrambler.setKeyToken(b"malformed-token").is_err());
        let bad = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(bad.bad_token, 1);

        assert!(descrambler
            .setKeyToken(b"maleicacid-placeholder-desc-token")
            .is_err());
        let after_placeholder = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_placeholder.cas_bridge_unconnected, 1);

        assert!(descrambler
            .setKeyToken(b"maleicacid-cas-desc-token-0000000000000001")
            .is_err());
        let after_cas_unconnected = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_cas_unconnected.cas_bridge_unconnected, 2);

        assert!(descrambler
            .setKeyToken(b"maleicacid-expired-desc-token-0000000000000001")
            .is_err());
        let after_expired = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_expired.expired_key_slot, 1);

        assert!(descrambler
            .setKeyToken(b"maleicacid-test-desc-token-ffffffffffffffff")
            .is_err());
        let after_unknown = hal.descrambler_diagnostics.snapshot(demux_id, 0x1fff);
        assert_eq!(after_unknown.bad_token, 2);

        let key_slot = DescramblerKeySlot::empty()
            .with_even(Multi2KeyMaterial::new([0x11; 32], [0x22; 8], [0x33; 8]));
        let ok_token = hal.descrambler_key_table.register_for_test(key_slot);
        assert!(descrambler.setKeyToken(&ok_token).is_ok());

        assert!(hal
            .dump_descrambler_diagnostics_for_debug()
            .contains("BAD_TOKEN=2"));
        assert!(hal
            .dump_descrambler_diagnostics_for_debug()
            .contains("CAS_BRIDGE_UNCONNECTED=2"));
        assert!(hal
            .dump_descrambler_diagnostics_for_debug()
            .contains("EXPIRED_KEY_SLOT=1"));
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
        multi2_encrypt_payload(&mut scrambled[4..], &even).unwrap();
        scrambled[3] = (scrambled[3] & 0x3f) | 0x80;

        let unresolved = ActiveDescramblerSnapshot {
            pids: BTreeSet::from([0x0123]),
            key_slot: DescramblerKeySlot::empty(),
        };
        let resolved = ActiveDescramblerSnapshot {
            pids: BTreeSet::from([0x0123]),
            key_slot: DescramblerKeySlot::empty().with_even(even),
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
        let mut observation = diagnostics.observation.lock().unwrap();
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
        diagnostics.reset_for_stream_boundary();
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
        diagnostics.reset_for_stream_boundary();
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
            false,
            false,
            false,
            &[
                FrontendStatusType::DEMOD_LOCK,
                FrontendStatusType::SIGNAL_QUALITY
            ]
        )
        .is_ok());
        assert!(FrontendHal::validate_status_types(
            false,
            false,
            false,
            &[FrontendStatusType::RF_LOCK]
        )
        .is_err());
        assert!(FrontendHal::validate_status_types(
            false,
            false,
            false,
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
            Arc::new(Mutex::new(BTreeMap::new())),
        );
        assert!(frontend.removeOutputPid(0x0100).is_err());
    }

    #[test]
    fn worker_exit_contract_distinguishes_all_terminal_reasons() {
        assert!(!WorkerExit::Normal.is_abnormal());
        assert!(!WorkerExit::Cancelled.is_abnormal());
        assert!(WorkerExit::Error.is_abnormal());
        assert!(WorkerExit::Panic.is_abnormal());
        assert_ne!(WorkerExit::Normal, WorkerExit::Cancelled);
        assert_ne!(WorkerExit::Error, WorkerExit::Panic);
    }

    #[test]
    fn filter_worker_abnormal_exit_helper_fails_closed_object_state() {
        let mut demux = DemuxHandle::new(DEMUX_ID_BASE);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_shared = Arc::new(Mutex::new(None));
        let closed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));

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
        assert!(state
            .lock()
            .unwrap()
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
        let closed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let wake = Arc::new((Mutex::new(false), Condvar::new()));

        DvrHal::fail_dvr_worker(
            &state,
            &runtime_io,
            &queue,
            &closed,
            &stop,
            &wake,
            dvr.dvr_id,
            "dvr_callback_worker_Panic",
        );

        assert!(closed.load(Ordering::SeqCst));
        assert!(stop.load(Ordering::SeqCst));
        assert!(runtime_io
            .ensure_not_failed(RuntimeIoKind::Dvr, dvr.dvr_id)
            .is_err());
        assert!(state.lock().unwrap().dvr_record(dvr.dvr_id).is_none());
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

        backing.fail_playback_worker(
            &state,
            &runtime_io,
            dvr.dvr_id,
            "dvr_playback_consumer_Panic",
        );

        assert!(backing.ensure_playback_worker_healthy().is_err());
        assert!(runtime_io
            .ensure_not_failed(RuntimeIoKind::Dvr, dvr.dvr_id)
            .is_err());
        assert!(state.lock().unwrap().dvr_record(dvr.dvr_id).is_none());
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
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let state = Arc::new(Mutex::new(demux));
        let runtime_io = Arc::new(RuntimeIoRegistry::default());
        let queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_queue = SharedMemoryBacking::new_ring(4096).unwrap();
        let av_shared_drops = Arc::new(AtomicU64::new(0));
        runtime_io.register_filter(filter.filter_id, &queue, &av_queue, None, &av_shared_drops);
        runtime.bound_demuxes.lock().unwrap().insert(
            DEMUX_ID_BASE,
            BoundDemuxRuntime {
                demux_generation: 1,
                state: Arc::clone(&state),
                runtime_io: Arc::clone(&runtime_io),
            },
        );

        runtime.mark_live_path_failed("frontend_live_pump_Panic");

        assert!(runtime_io
            .ensure_not_failed(RuntimeIoKind::Filter, filter.filter_id)
            .is_err());
        assert!(state
            .lock()
            .unwrap()
            .filter_record(filter.filter_id)
            .is_none());
    }

    #[test]
    fn worker_abnormal_exit_hooks_are_wired_to_fail_closed_helpers() {
        let source = include_str!("tuner_hal.rs");
        for worker in [
            "dvr_playback_consumer",
            "frontend_live_pump",
            "filter_callback_worker",
            "dvr_callback_worker",
        ] {
            assert!(
                source.contains(&format!("spawn_worker_with_exit_hook(\"{}\"", worker)),
                "missing explicit exit hook for {worker}"
            );
        }
        assert!(source.contains("if exit.is_abnormal()"));
        assert!(source.contains("fail_playback_worker("));
        assert!(source.contains("mark_live_path_failed(&detail)"));
        assert!(source.contains("FilterHal::fail_filter_worker("));
        assert!(source.contains("DvrHal::fail_dvr_worker("));
    }

    #[test]
    fn managed_diagnostic_worker_reports_cancel_and_panic_to_hook() {
        let (tx_cancel, rx_cancel) = std::sync::mpsc::channel();
        let mut cancelled_worker = spawn_managed_worker_with_exit_hook(
            "managed_worker_cancel_contract_test",
            move |stop| {
                stop.store(true, Ordering::SeqCst);
            },
            move |exit| {
                tx_cancel.send(exit).unwrap();
            },
        )
        .unwrap();
        cancelled_worker.stop_and_join();
        assert_eq!(rx_cancel.recv().unwrap(), WorkerExit::Cancelled);

        let (tx_panic, rx_panic) = std::sync::mpsc::channel();
        let mut panic_worker = spawn_managed_worker_with_exit_hook(
            "managed_worker_panic_contract_test",
            |_stop| {
                panic!("intentional managed worker panic contract test");
            },
            move |exit| {
                tx_panic.send(exit).unwrap();
            },
        )
        .unwrap();
        panic_worker.stop_and_join();
        assert_eq!(rx_panic.recv().unwrap(), WorkerExit::Panic);
    }

    #[test]
    fn dvb_frontend_info_frequency_contract_is_narrower_than_driver_probe_range() {
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
            (
                JAPAN_UHF_13_CENTER_HZ - JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
                JAPAN_UHF_62_CENTER_HZ + JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
                JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
            ),
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
        let terminal = FrontendHal::publish_scan_terminal_debug(&runtime, &session, 7).unwrap();
        assert_eq!(terminal.phase, ScanPhase::FailedBackend);
        let dump = runtime.debug_dump_runtime_failures();
        assert!(dump.contains(
            "scan_last_terminal session_id=7 phase=FailedBackend fingerprint=contract-scan"
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

    #[test]
    fn frontend_tune_worker_spawn_failure_is_fail_closed_and_diagnostic() {
        let source = include_str!("tuner_hal.rs");
        let start = source
            .find("fn start_tune_worker")
            .expect("start_tune_worker must exist");
        let end = source[start..]
            .find("fn settings_fingerprint")
            .expect("settings_fingerprint follows start_tune_worker")
            + start;
        let body = &source[start..end];
        assert!(body.contains("shared_for_spawn_failure.record_runtime_failure"));
        assert!(body.contains("shared_for_spawn_failure.mark_live_path_failed"));
        assert!(body.contains("shared_for_spawn_failure.stop_live_pump_best_effort"));
        assert!(body.contains("shared_for_spawn_failure.reset_bound_demuxes_for_stream_boundary"));
        assert!(body.contains("FrontendHal::backend_stop_tune"));
        assert!(body.contains("Status::from(StatusCode::UNKNOWN_ERROR)"));
    }

    #[test]
    fn release_service_registration_does_not_panic_on_add_service_failure() {
        let main_source = include_str!("main.rs");
        assert!(main_source.contains("if let Err(e) = binder::add_service"));
        assert!(main_source.contains("std::process::exit(1)"));
        assert!(!main_source.contains("panic!(\"Tuner HAL service 登録に失敗しました"));
        assert!(!main_source.contains("unwrap_or_else(|e| panic!"));
    }

    #[test]
    fn r50ap5_worker_policy_has_managed_worker_signal_primitive() {
        let source = include_str!("tuner_hal.rs");
        assert!(source.contains("struct WorkerSignal"));
        assert!(source.contains("struct ManagedWorker"));
        assert!(source.contains("Mutex<WorkerSignalState>"));
        assert!(source.contains("cv: Condvar"));
        assert!(source.contains("fn request_stop(&self)"));
        assert!(source.contains("fn notify_work(&self)"));
        assert!(source.contains("fn wait_until_work_or_stop"));
        assert!(source.contains("fn wait_timeout_or_stop"));
        assert!(source.contains("fn stop_and_join(&mut self)"));
        assert!(source.contains("StopRequested"));
        assert!(source.contains("RuntimeFailure"));
        assert!(source.contains("PanicOrJoinFailure"));
    }

    #[test]
    fn r50ap5_frontend_scan_tune_stop_boundaries_are_static_locked() {
        let source = include_str!("tuner_hal.rs");
        let stop_tune_start = source
            .find("fn stopTune(&self)")
            .expect("stopTune must exist");
        let stop_tune_end = source[stop_tune_start..]
            .find("fn close(&self)")
            .expect("close follows stopTune")
            + stop_tune_start;
        let stop_tune = &source[stop_tune_start..stop_tune_end];
        assert!(stop_tune.contains("stopTune does not cancel an active scan"));
        assert!(!stop_tune.contains("cancel_scan_session()?"));

        let stop_scan_start = source
            .find("fn stopScan(&self)")
            .expect("stopScan must exist");
        let stop_scan_end = source[stop_scan_start..]
            .find("fn getStatus(&self")
            .expect("getStatus follows stopScan")
            + stop_scan_start;
        let stop_scan = &source[stop_scan_start..stop_scan_end];
        assert!(stop_scan.contains("cancel_scan_session()?"));
        assert!(!stop_scan.contains("stop_tune_worker"));
        assert!(!stop_scan.contains("stop_live_pump"));
    }

    #[test]
    fn r50ap5_filter_and_dvr_ensure_open_check_parent_demux_lifecycle() {
        let source = include_str!("tuner_hal.rs");
        assert!(source.contains("parent_demux_closed"));
        assert!(source.contains("filter_unregistered_from_parent_demux"));
        assert!(source.contains("dvr_unregistered_from_parent_demux"));
        assert!(source.contains("filter_owner_demux_mismatch"));
        assert!(source.contains("dvr_owner_demux_mismatch"));
        assert!(source.contains("runtime_io.unregister_filter_best_effort(filter_id)"));
        assert!(source.contains("runtime_io.unregister_dvr_best_effort(dvr_id)"));
    }

    #[test]
    fn r50ap5_frontend_source_switch_rolls_back_partial_binding() {
        let source = include_str!("tuner_hal.rs");
        let bind_start = source
            .find("fn bind_demux(self: &Arc<Self>")
            .expect("bind_demux must exist");
        let bind_end = source[bind_start..]
            .find("fn unbind_demux(&self")
            .expect("unbind_demux follows bind_demux")
            + bind_start;
        let bind_body = &source[bind_start..bind_end];
        assert!(bind_body.contains("if let Err(err) = self.ensure_live_pump()"));
        assert!(bind_body.contains("demuxes.remove(&demux_id)"));

        let switch_start = source
            .find("fn setFrontendDataSource(&self")
            .expect("setFrontendDataSource must exist");
        let switch_end = source[switch_start..]
            .find("fn openFilter(")
            .expect("openFilter follows setFrontendDataSource")
            + switch_start;
        let switch_body = &source[switch_start..switch_end];
        assert!(switch_body.contains("rollback_to_old"));
        assert!(switch_body.contains("fail_closed_transition"));
        assert!(switch_body.contains("rollback_unbind_new_frontend_failed"));
        assert!(switch_body.contains("rollback_old_frontend_missing"));
        assert!(switch_body.contains("rollback_bind_old_frontend_failed"));
        assert!(switch_body.contains("old_runtime.bind_demux"));
        assert!(switch_body.contains("handle.close()"));
        assert!(switch_body.contains("runtime_io.flush_all()"));
        assert!(switch_body.contains("state.reset_for_stream_boundary()"));
    }
}
