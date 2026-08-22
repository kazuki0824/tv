pub mod os_abi;

#[cfg(test)]
mod failure_injection_tests;
use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

pub const TUNER_SERVICE_NAME: &str = "android.hardware.tv.tuner.ITuner/default";
pub const TS_PACKET_SIZE: usize = 188;
pub const MAX_ARIB_SHORT_SECTION_LENGTH: usize = 1021;
pub const MAX_ARIB_EIT_SECTION_LENGTH: usize = 4093;
pub const MAX_ARIB_SECTION_TOTAL_BYTES: usize = 3 + MAX_ARIB_EIT_SECTION_LENGTH;
pub const MAX_SECTION_PAYLOAD_BYTES: usize = MAX_ARIB_SECTION_TOTAL_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TransportStreamPid(u16);

impl TransportStreamPid {
    pub fn validate_u16(pid: u16) -> Result<Self, ()> {
        if pid <= 0x1fff {
            Ok(Self(pid))
        } else {
            Err(())
        }
    }

    pub fn validate_i32(pid: i32) -> Result<Self, ()> {
        if (0..=0x1fff).contains(&pid) {
            Ok(Self(pid as u16))
        } else {
            Err(())
        }
    }

    pub const fn to_i32_for_aidl_boundary(self) -> i32 {
        self.0 as i32
    }

    pub const fn matches_i32_config(self, tpid: Option<i32>) -> bool {
        match tpid {
            Some(config_pid) => config_pid == self.0 as i32,
            None => false,
        }
    }
}

/// ARIB STD-B10 の table_id 別 section_length 上限を返す。
/// EIT p/f と EIT schedule は 0x4e..=0x6f、それ以外は短い section として扱う。
pub fn max_arib_section_length_for_table_id(table_id: u8) -> usize {
    match table_id {
        0x4e..=0x6f => MAX_ARIB_EIT_SECTION_LENGTH,
        _ => MAX_ARIB_SHORT_SECTION_LENGTH,
    }
}

fn increment_atomic_counter_with_saturation(
    counter: &AtomicU64,
    saturated: Option<&AtomicBool>,
) -> u64 {
    match counter.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
        value.checked_add(1)
    }) {
        Ok(previous) => previous + 1,
        Err(_) => {
            if let Some(flag) = saturated {
                flag.store(true, Ordering::SeqCst);
            }
            u64::MAX
        }
    }
}

pub fn retry_after_interrupted_read(
    operation: &'static str,
    retry_counter: &AtomicU64,
    f: impl FnMut() -> io::Result<usize>,
) -> io::Result<usize> {
    retry_after_interrupted_read_with_saturation(operation, retry_counter, None, f)
}

pub fn retry_after_interrupted_read_with_saturation(
    _operation: &'static str,
    retry_counter: &AtomicU64,
    retry_counter_saturated: Option<&AtomicBool>,
    mut f: impl FnMut() -> io::Result<usize>,
) -> io::Result<usize> {
    loop {
        match f() {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                let _total = increment_atomic_counter_with_saturation(
                    retry_counter,
                    retry_counter_saturated,
                );
                continue;
            }
            other => return other,
        }
    }
}

const TS_RESYNC_CONFIRM_PACKETS: usize = 3;
const TS_RESYNC_TAIL_BYTES: usize = TS_PACKET_SIZE * (TS_RESYNC_CONFIRM_PACKETS - 1);
const TS_RESYNC_CONFIRM_BYTES: usize = TS_RESYNC_TAIL_BYTES + 1;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TsPacketBufferDrain {
    pub packets: Vec<[u8; TS_PACKET_SIZE]>,
    pub malformed_bytes: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TsPacketCompletionBuffer {
    buf: Vec<u8>,
    completed: VecDeque<[u8; TS_PACKET_SIZE]>,
    malformed_bytes: u64,
    malformed_bytes_saturated: bool,
    resync_required: bool,
}

impl TsPacketCompletionBuffer {
    fn add_local_malformed(local: &mut usize, amount: usize, saturated: &mut bool) {
        match local.checked_add(amount) {
            Some(next) => *local = next,
            None => {
                *local = usize::MAX;
                *saturated = true;
            }
        }
    }

    fn add_malformed_bytes(&mut self, amount: usize) {
        if amount == 0 {
            return;
        }
        let amount = u64::try_from(amount).unwrap_or(u64::MAX);
        match self.malformed_bytes.checked_add(amount) {
            Some(next) => self.malformed_bytes = next,
            None => {
                self.malformed_bytes = u64::MAX;
                self.malformed_bytes_saturated = true;
            }
        }
        if amount == u64::MAX {
            self.malformed_bytes_saturated = true;
        }
    }

    fn confirmed_sync_offset(buf: &[u8]) -> Option<usize> {
        if buf.len() < TS_RESYNC_CONFIRM_BYTES {
            return None;
        }
        let last_start = buf.len() - TS_RESYNC_CONFIRM_BYTES;
        (0..=last_start).find(|&offset| {
            (0..TS_RESYNC_CONFIRM_PACKETS)
                .all(|packet_index| buf[offset + packet_index * TS_PACKET_SIZE] == 0x47)
        })
    }

    pub fn push(&mut self, data: &[u8]) -> TsPacketBufferDrain {
        self.buf.extend_from_slice(data);
        let mut malformed_bytes = 0usize;
        let mut local_malformed_saturated = false;
        loop {
            if self.resync_required {
                let Some(offset) = Self::confirmed_sync_offset(&self.buf) else {
                    if self.buf.len() > TS_RESYNC_TAIL_BYTES {
                        let discard = self.buf.len() - TS_RESYNC_TAIL_BYTES;
                        self.buf.drain(..discard);
                        Self::add_local_malformed(
                            &mut malformed_bytes,
                            discard,
                            &mut local_malformed_saturated,
                        );
                    }
                    break;
                };
                if offset > 0 {
                    self.buf.drain(..offset);
                    Self::add_local_malformed(
                        &mut malformed_bytes,
                        offset,
                        &mut local_malformed_saturated,
                    );
                }
                self.resync_required = false;
                continue;
            }

            if self.buf.len() < TS_PACKET_SIZE {
                break;
            }
            if self.buf[0] != 0x47 {
                self.resync_required = true;
                continue;
            }
            let mut packet = [0u8; TS_PACKET_SIZE];
            packet.copy_from_slice(&self.buf[..TS_PACKET_SIZE]);
            self.buf.drain(..TS_PACKET_SIZE);
            self.completed.push_back(packet);
        }
        if malformed_bytes > 0 {
            self.add_malformed_bytes(malformed_bytes);
        }
        if local_malformed_saturated {
            self.malformed_bytes_saturated = true;
        }
        let packets = self.drain_completed(usize::MAX);
        TsPacketBufferDrain {
            packets,
            malformed_bytes,
        }
    }

    pub fn push_limited(&mut self, data: &[u8], max_packets: usize) -> TsPacketBufferDrain {
        let drain = self.push(data);
        if drain.packets.len() <= max_packets {
            return drain;
        }
        let mut packets = drain.packets;
        let pending = packets.split_off(max_packets);
        for packet in pending.into_iter().rev() {
            self.completed.push_front(packet);
        }
        TsPacketBufferDrain {
            packets,
            malformed_bytes: drain.malformed_bytes,
        }
    }

    pub fn drain_completed(&mut self, max_packets: usize) -> Vec<[u8; TS_PACKET_SIZE]> {
        let mut packets = Vec::new();
        while packets.len() < max_packets {
            let Some(packet) = self.completed.pop_front() else {
                break;
            };
            packets.push(packet);
        }
        packets
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.completed.clear();
        self.resync_required = false;
    }

    pub fn tail_len(&self) -> usize {
        self.buf.len()
    }
    pub fn malformed_bytes(&self) -> u64 {
        self.malformed_bytes
    }
    pub fn malformed_bytes_saturated(&self) -> bool {
        self.malformed_bytes_saturated
    }

    pub fn drain_for_boundary(&mut self) -> TsPacketBufferDrain {
        let packets = self.drain_completed(usize::MAX);
        let malformed_bytes = self.buf.len();
        if malformed_bytes > 0 {
            self.add_malformed_bytes(malformed_bytes);
            self.buf.clear();
        }
        self.resync_required = false;
        TsPacketBufferDrain {
            packets,
            malformed_bytes,
        }
    }
}

#[derive(Debug)]
/// Collects the first error among cleanup steps.
///
/// This type is not the source of truth for primary + cleanup failure composition;
/// callers that already have a primary failure must preserve it separately.
pub struct FirstErrorCollector<E> {
    first_error: Option<E>,
}

impl<E> Default for FirstErrorCollector<E> {
    fn default() -> Self {
        Self { first_error: None }
    }
}

impl<E> FirstErrorCollector<E> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_error(&mut self, error: E) {
        if self.first_error.is_none() {
            self.first_error = Some(error);
        }
    }

    pub fn push_result(&mut self, result: Result<(), E>) {
        if let Err(error) = result {
            self.push_error(error);
        }
    }

    pub fn has_error(&self) -> bool {
        self.first_error.is_some()
    }

    pub fn into_result(self) -> Result<(), E> {
        match self.first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// Composes a primary failure with a cleanup/rollback failure.
///
/// This is the shared primary+cleanup composition helper. It is distinct from
/// `FirstErrorCollector`, which only decides the first failure among cleanup
/// steps after cleanup has started.
pub fn compose_primary_cleanup_failure(
    context: &'static str,
    primary: HalError,
    cleanup: HalError,
) -> HalError {
    HalError::composed_failure(context, primary, cleanup)
}

/// Finishes a cleanup/rollback attempt after a primary failure.
///
/// If cleanup succeeds, the original primary failure is returned. If cleanup
/// fails, a composed failure retaining both primary and cleanup failures is
/// returned.
pub fn finish_cleanup_after_primary_failure(
    context: &'static str,
    primary: HalError,
    cleanup: Result<(), HalError>,
) -> HalError {
    match cleanup {
        Ok(()) => primary,
        Err(cleanup) => compose_primary_cleanup_failure(context, primary, cleanup),
    }
}

/// Converts a primary failure plus cleanup result into a result for callers that
/// have no success value after the primary failure.
pub fn fail_after_cleanup<T>(
    context: &'static str,
    primary: HalError,
    cleanup: Result<(), HalError>,
) -> Result<T, HalError> {
    Err(finish_cleanup_after_primary_failure(
        context, primary, cleanup,
    ))
}

#[derive(Debug)]
pub struct IdExhausted {
    pub last_attempted: i32,
}

#[derive(Debug)]
pub struct IdAllocator {
    next: AtomicI32,
    max: i32,
}

impl IdAllocator {
    pub const fn new(start: i32) -> Self {
        Self {
            next: AtomicI32::new(start),
            max: i32::MAX,
        }
    }

    pub const fn new_bounded(start: i32, max: i32) -> Self {
        Self {
            next: AtomicI32::new(start),
            max,
        }
    }

    pub fn try_allocate(&self) -> Result<i32, IdExhausted> {
        loop {
            let current = self.next.load(Ordering::SeqCst);
            if current > self.max {
                return Err(IdExhausted {
                    last_attempted: current,
                });
            }
            let Some(next) = current.checked_add(1) else {
                return Err(IdExhausted {
                    last_attempted: current,
                });
            };
            match self
                .next
                .compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return Ok(current),
                Err(_) => continue,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendBackendKind {
    Px4CharDevice,
    LinuxDvb,
}

impl FrontendBackendKind {
    pub const fn as_hint(self) -> &'static str {
        match self {
            Self::Px4CharDevice => "px4",
            Self::LinuxDvb => "dvb",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FrontendSystem {
    IsdbT,
    IsdbS,
    IsdbS3,
    DvbS,
}

impl FrontendSystem {
    pub const fn as_hint(self) -> &'static str {
        match self {
            Self::IsdbT => "ISDB-T",
            Self::IsdbS => "ISDB-S",
            Self::IsdbS3 => "ISDB-S3",
            Self::DvbS => "DVB-S",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendStreamIdKind {
    AbsoluteStreamId,
    RelativeStreamNumber,
}

pub const JAPAN_CATV_C13_CENTER_HZ: u64 = 111_142_857;
pub const JAPAN_UHF_62_CENTER_HZ: u64 = 767_142_857;
pub const JAPAN_ISDBT_TUNE_TOLERANCE_HZ: u64 = 500_000;

pub fn japan_isdbt_frequency_contract_range_hz() -> (u64, u64, u64) {
    (
        JAPAN_CATV_C13_CENTER_HZ.saturating_sub(JAPAN_ISDBT_TUNE_TOLERANCE_HZ),
        JAPAN_UHF_62_CENTER_HZ.saturating_add(JAPAN_ISDBT_TUNE_TOLERANCE_HZ),
        JAPAN_ISDBT_TUNE_TOLERANCE_HZ,
    )
}

pub fn is_japan_isdbt_frequency_contract_hz(frequency_hz: u64) -> bool {
    let (min_hz, max_hz, _) = japan_isdbt_frequency_contract_range_hz();
    frequency_hz >= min_hz && frequency_hz <= max_hz
}

pub fn is_japan_bs_if_frequency_hz(if_frequency_hz: u64) -> bool {
    let first = 1_049_480_000_u64;
    let last = 1_471_440_000_u64;
    if if_frequency_hz < first || if_frequency_hz > last {
        return false;
    }
    (if_frequency_hz - first) % 38_360_000 == 0
}

pub fn is_japan_cs110_if_frequency_hz(if_frequency_hz: u64) -> bool {
    let first = 1_613_000_000_u64;
    let last = 2_053_000_000_u64;
    if if_frequency_hz < first || if_frequency_hz > last {
        return false;
    }
    (if_frequency_hz - first) % 40_000_000 == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendScanMode {
    Auto,
    Blind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendDevicePath {
    inner: PathBuf,
}

impl FrontendDevicePath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { inner: path.into() }
    }
    pub fn as_path(&self) -> &Path {
        &self.inner
    }
    pub fn display(&self) -> String {
        self.inner.display().to_string()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendIsdbtPartialReceptionRequirement {
    Unspecified,
    Required(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendTuneRequest {
    pub system: FrontendSystem,
    pub frequency: u64,
    pub end_frequency: Option<u64>,
    pub stream_id: Option<u32>,
    pub stream_id_kind: Option<FrontendStreamIdKind>,
    pub bandwidth_hz: Option<u32>,
    pub symbol_rate: Option<u32>,
    pub partial_reception: FrontendIsdbtPartialReceptionRequirement,
}

impl FrontendTuneRequest {
    /// tune と non-blind scan では endFrequency を選局条件に含めない。
    pub fn normalized_for_non_blind_operation(mut self) -> Self {
        self.end_frequency = None;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HalInvalidArgumentKind {
    MissingDeliverySystem,
    UnsupportedStreamSelector,
    InvalidStreamIdRange,
    UnsupportedSymbolRate,
    UnsupportedBandwidth,
    MissingStreamSelector,
    UnsupportedFrequency,
    UnsupportedScanRange,
    NumericRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HalInvalidStateKind {
    InvalidLifecycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HalInternalKind {
    InvariantViolation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HalErrorDetail {
    pub detail: String,
}

impl HalErrorDetail {
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HalError {
    DeviceMissing(PathBuf),
    OpenFailed {
        path: PathBuf,
        detail: HalErrorDetail,
    },
    PermissionDenied {
        path: PathBuf,
        detail: HalErrorDetail,
    },
    Busy {
        path: Option<PathBuf>,
        detail: HalErrorDetail,
    },
    IoctlFailed {
        backend: &'static str,
        path: Option<PathBuf>,
        op: &'static str,
        errno: i32,
    },
    CallbackFailed {
        callback: &'static str,
        detail: HalErrorDetail,
    },
    FmqFailed {
        operation: &'static str,
        detail: HalErrorDetail,
    },
    EventFlagFailed {
        operation: &'static str,
        detail: HalErrorDetail,
    },
    CleanupFailed {
        resource: &'static str,
        detail: HalErrorDetail,
    },
    OutOfMemory {
        resource: &'static str,
        detail: HalErrorDetail,
    },
    ComposedFailure {
        context: &'static str,
        primary: Box<HalError>,
        cleanup: Box<HalError>,
    },
    InvalidArgument {
        kind: HalInvalidArgumentKind,
        detail: HalErrorDetail,
    },
    InvalidState {
        kind: HalInvalidStateKind,
        detail: HalErrorDetail,
    },
    Io {
        backend: &'static str,
        operation: &'static str,
        path: Option<PathBuf>,
        errno: Option<i32>,
        detail: HalErrorDetail,
    },
    Internal {
        kind: HalInternalKind,
        detail: HalErrorDetail,
    },
    Unsupported(&'static str),
    UnsupportedDetail {
        feature: &'static str,
        detail: HalErrorDetail,
    },
}

impl HalError {
    pub fn invalid_argument(kind: HalInvalidArgumentKind, detail: impl Into<String>) -> Self {
        Self::InvalidArgument {
            kind,
            detail: HalErrorDetail::new(detail),
        }
    }

    pub fn invalid_state(kind: HalInvalidStateKind, detail: impl Into<String>) -> Self {
        Self::InvalidState {
            kind,
            detail: HalErrorDetail::new(detail),
        }
    }

    pub fn internal(kind: HalInternalKind, detail: impl Into<String>) -> Self {
        Self::Internal {
            kind,
            detail: HalErrorDetail::new(detail),
        }
    }

    pub fn callback_failed(callback: &'static str, detail: impl Into<String>) -> Self {
        Self::CallbackFailed {
            callback,
            detail: HalErrorDetail::new(detail),
        }
    }

    pub fn fmq_failed(operation: &'static str, detail: impl Into<String>) -> Self {
        Self::FmqFailed {
            operation,
            detail: HalErrorDetail::new(detail),
        }
    }

    pub fn event_flag_failed(operation: &'static str, detail: impl Into<String>) -> Self {
        Self::EventFlagFailed {
            operation,
            detail: HalErrorDetail::new(detail),
        }
    }

    pub fn cleanup_failed(resource: &'static str, detail: impl Into<String>) -> Self {
        Self::CleanupFailed {
            resource,
            detail: HalErrorDetail::new(detail),
        }
    }

    pub fn out_of_memory(resource: &'static str, detail: impl Into<String>) -> Self {
        Self::OutOfMemory {
            resource,
            detail: HalErrorDetail::new(detail),
        }
    }

    pub fn unsupported_detail(feature: &'static str, detail: impl Into<String>) -> Self {
        Self::UnsupportedDetail {
            feature,
            detail: HalErrorDetail::new(detail),
        }
    }

    pub fn composed_failure(context: &'static str, primary: HalError, cleanup: HalError) -> Self {
        Self::ComposedFailure {
            context,
            primary: Box::new(primary),
            cleanup: Box::new(cleanup),
        }
    }

    pub fn primary_error(&self) -> &HalError {
        match self {
            Self::ComposedFailure { primary, .. } => primary.primary_error(),
            other => other,
        }
    }

    pub fn cleanup_error(&self) -> Option<&HalError> {
        match self {
            Self::ComposedFailure { cleanup, .. } => Some(cleanup.as_ref()),
            _ => None,
        }
    }

    pub fn invalid_argument_kind(&self) -> Option<HalInvalidArgumentKind> {
        match self {
            Self::InvalidArgument { kind, .. } => Some(*kind),
            _ => None,
        }
    }
}

pub fn errno_name(errno: i32) -> &'static str {
    match errno {
        1 => "EPERM",
        2 => "ENOENT",
        5 => "EIO",
        6 => "ENXIO",
        11 => "EAGAIN",
        12 => "ENOMEM",
        13 => "EACCES",
        16 => "EBUSY",
        19 => "ENODEV",
        22 => "EINVAL",
        25 => "ENOTTY",
        38 => "ENOSYS",
        110 => "ETIMEDOUT",
        _ => "UNKNOWN_ERRNO",
    }
}

fn display_path(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unavailable>".to_string())
}

impl fmt::Display for HalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HalError::DeviceMissing(path) => write!(f, "device not found: {}", path.display()),
            HalError::OpenFailed { path, detail } => write!(
                f,
                "open に失敗しました {}: {}",
                path.display(),
                detail.detail
            ),
            HalError::PermissionDenied { path, detail } => write!(
                f,
                "permission denied opening {}: {}",
                path.display(),
                detail.detail
            ),
            HalError::Busy { path, detail } => {
                if let Some(path) = path {
                    write!(f, "device busy {}: {}", path.display(), detail.detail)
                } else {
                    write!(f, "device busy: {}", detail.detail)
                }
            }
            HalError::IoctlFailed {
                backend,
                path,
                op,
                errno,
            } => write!(
                f,
                "実行時ioctl失敗: backend={} operation={} device_path={} errno={} errno_name={}",
                backend,
                op,
                display_path(path),
                errno,
                errno_name(*errno),
            ),
            HalError::CallbackFailed { callback, detail } => write!(
                f,
                "callback failed: callback={} detail={}",
                callback, detail.detail
            ),
            HalError::FmqFailed { operation, detail } => write!(
                f,
                "FMQ failed: operation={} detail={}",
                operation, detail.detail
            ),
            HalError::EventFlagFailed { operation, detail } => write!(
                f,
                "EventFlag failed: operation={} detail={}",
                operation, detail.detail
            ),
            HalError::CleanupFailed { resource, detail } => write!(
                f,
                "cleanup failed: resource={} detail={}",
                resource, detail.detail
            ),
            HalError::OutOfMemory { resource, detail } => write!(
                f,
                "out of memory: resource={} detail={}",
                resource, detail.detail
            ),
            HalError::ComposedFailure {
                context,
                primary,
                cleanup,
            } => write!(
                f,
                "composed failure: context={} primary=({}) cleanup=({})",
                context, primary, cleanup
            ),
            HalError::InvalidArgument { kind, detail } => write!(
                f,
                "invalid argument: kind={kind:?} detail={}",
                detail.detail
            ),
            HalError::InvalidState { kind, detail } => {
                write!(f, "invalid state: kind={kind:?} detail={}", detail.detail)
            }
            HalError::Io {
                backend,
                operation,
                path,
                errno,
                detail,
            } => {
                if let Some(errno) = errno {
                    write!(f, "io failed: backend={} operation={} device_path={} errno={} errno_name={} detail={}", backend, operation, display_path(path), errno, errno_name(*errno), detail.detail)
                } else {
                    write!(
                        f,
                        "io failed: backend={} operation={} device_path={} detail={}",
                        backend,
                        operation,
                        display_path(path),
                        detail.detail
                    )
                }
            }
            HalError::Internal { kind, detail } => {
                write!(f, "internal error: kind={kind:?} detail={}", detail.detail)
            }
            HalError::Unsupported(feature) => write!(f, "unsupported feature: {feature}"),
            HalError::UnsupportedDetail { feature, detail } => {
                write!(f, "unsupported feature: {feature}: {}", detail.detail)
            }
        }
    }
}

impl std::error::Error for HalError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(seed: u8) -> [u8; TS_PACKET_SIZE] {
        let mut p = [seed; TS_PACKET_SIZE];
        p[0] = 0x47;
        p
    }

    #[test]
    fn first_error_collector_preserves_first_error() {
        let mut collector = FirstErrorCollector::new();
        collector.push_result(Ok(()));
        collector.push_error("first");
        collector.push_error("second");
        assert_eq!(collector.into_result(), Err("first"));
    }

    #[test]
    fn first_error_collector_reports_success_when_all_steps_succeed() {
        let mut collector = FirstErrorCollector::<&'static str>::new();
        collector.push_result(Ok(()));
        collector.push_result(Ok(()));
        assert_eq!(collector.into_result(), Ok(()));
    }

    #[test]
    fn composed_failure_preserves_primary_and_cleanup() {
        let primary =
            HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, "primary failure");
        let cleanup = HalError::cleanup_failed("test cleanup", "cleanup failure");
        let error = HalError::composed_failure("test composed", primary.clone(), cleanup.clone());

        assert_eq!(error.primary_error(), &primary);
        assert_eq!(error.cleanup_error(), Some(&cleanup));
        let rendered = error.to_string();
        assert!(rendered.contains("primary failure"));
        assert!(rendered.contains("cleanup failure"));
    }

    #[test]
    fn finish_cleanup_after_primary_failure_keeps_primary_when_cleanup_succeeds() {
        let primary = HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "primary-only failure",
        );
        let error = finish_cleanup_after_primary_failure(
            "test cleanup composition",
            primary.clone(),
            Ok(()),
        );
        assert_eq!(error, primary);
    }

    #[test]
    fn finish_cleanup_after_primary_failure_composes_both_failures() {
        let primary =
            HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, "primary failure");
        let cleanup = HalError::cleanup_failed("test cleanup", "cleanup failure");
        let error = finish_cleanup_after_primary_failure(
            "test cleanup composition",
            primary.clone(),
            Err(cleanup.clone()),
        );
        assert_eq!(error.primary_error(), &primary);
        assert_eq!(error.cleanup_error(), Some(&cleanup));
    }

    #[test]
    fn resync_discards_noise_until_three_sync_bytes_are_seen() {
        let p1 = packet(1);
        let p2 = packet(2);
        let p3 = packet(3);
        let mut input = vec![0xaa, 0xbb, 0xcc];
        input.extend_from_slice(&p1);
        input.extend_from_slice(&p2);
        input.extend_from_slice(&p3);
        let mut buf = TsPacketCompletionBuffer::default();
        let drain = buf.push(&input);
        assert_eq!(drain.malformed_bytes, 3);
        assert_eq!(drain.packets, vec![p1, p2, p3]);
    }

    #[test]
    fn drain_for_boundary_returns_completed_and_drops_tail() {
        let p = packet(9);
        let mut buf = TsPacketCompletionBuffer::default();
        drop(buf.push_limited(&p, 0));
        buf.buf.extend_from_slice(&[1, 2, 3]);
        let drain = buf.drain_for_boundary();
        assert_eq!(drain.packets, vec![p]);
        assert_eq!(drain.malformed_bytes, 3);
        assert_eq!(buf.tail_len(), 0);
    }

    #[test]
    fn id_allocator_reports_exhaustion_without_wrapping() {
        let alloc = IdAllocator::new_bounded(i32::MAX, i32::MAX);
        assert!(alloc.try_allocate().is_err());
    }
}
