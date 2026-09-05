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
pub const ARIB_TDT_SECTION_LENGTH: usize = 5;

pub const MAX_ARIB_SECTION_TOTAL_BYTES: usize = 3 + MAX_ARIB_EIT_SECTION_LENGTH;
pub const MAX_SECTION_PAYLOAD_BYTES: usize = MAX_ARIB_SECTION_TOTAL_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TransportStreamPid(u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportStreamPidValidationError {
    OutOfRange,
}

impl TransportStreamPid {
    pub const fn from_mpeg_ts_header_bytes(header_byte_1: u8, header_byte_2: u8) -> Self {
        Self((((header_byte_1 & 0x1f) as u16) << 8) | header_byte_2 as u16)
    }

    pub fn validate_u16(pid: u16) -> Result<Self, TransportStreamPidValidationError> {
        if pid <= 0x1fff {
            Ok(Self(pid))
        } else {
            Err(TransportStreamPidValidationError::OutOfRange)
        }
    }

    pub fn validate_i32(pid: i32) -> Result<Self, TransportStreamPidValidationError> {
        if (0..=0x1fff).contains(&pid) {
            Ok(Self(pid as u16))
        } else {
            Err(TransportStreamPidValidationError::OutOfRange)
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
///
/// HALは表の意味解析を行わずtransport外形だけを検証する。STD-B10で
/// 1021-byte区分として固定される既知tableだけをshort classへ置き、
/// EIT/ST/INT/PCAT/BIT/NBIT/LDT/LIT/ERT/ITT/AMTおよび予約/private/未割当は
/// 4093-byte transport classとして扱う。TDTだけはsection_length=5固定。
pub fn max_arib_section_length_for_table_id(table_id: u8) -> usize {
    match table_id {
        0x70 => ARIB_TDT_SECTION_LENGTH,
        0x00..=0x03 | 0x40 | 0x41 | 0x42 | 0x46 | 0x4a | 0x71 | 0x73 => {
            MAX_ARIB_SHORT_SECTION_LENGTH
        }
        _ => MAX_ARIB_EIT_SECTION_LENGTH,
    }
}

pub fn is_valid_arib_section_length(table_id: u8, section_length: usize) -> bool {
    match table_id {
        0x70 => section_length == ARIB_TDT_SECTION_LENGTH,
        _ => section_length <= max_arib_section_length_for_table_id(table_id),
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
            buf[offset] == 0x47
                && buf[offset + TS_PACKET_SIZE] == 0x47
                && buf[offset + TS_PACKET_SIZE * 2] == 0x47
        })
    }

    fn retain_unconfirmed_tail(&mut self, malformed_bytes: &mut usize) {
        if self.buf.len() <= TS_RESYNC_TAIL_BYTES {
            return;
        }
        let discard = self.buf.len() - TS_RESYNC_TAIL_BYTES;
        self.buf.drain(..discard);
        let mut saturated = false;
        Self::add_local_malformed(malformed_bytes, discard, &mut saturated);
        if saturated {
            self.malformed_bytes_saturated = true;
        }
    }

    pub fn push(&mut self, data: &[u8]) -> TsPacketBufferDrain {
        self.buf.extend_from_slice(data);
        let mut packets = Vec::new();
        let mut malformed_bytes = 0usize;
        loop {
            if self.resync_required || self.buf.first().copied() != Some(0x47) {
                let Some(offset) = Self::confirmed_sync_offset(&self.buf) else {
                    self.resync_required = true;
                    self.retain_unconfirmed_tail(&mut malformed_bytes);
                    break;
                };
                if offset > 0 {
                    self.buf.drain(..offset);
                    let mut saturated = false;
                    Self::add_local_malformed(&mut malformed_bytes, offset, &mut saturated);
                    if saturated {
                        self.malformed_bytes_saturated = true;
                    }
                }
                self.resync_required = false;
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
            packets.push(packet);
        }
        self.add_malformed_bytes(malformed_bytes);
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendBackendKind {
    Px4CharDevice,
    LinuxDvb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

pub fn is_japan_isdbt_frequency_contract_hz(frequency_hz: u64) -> bool {
    const FIRST: u64 = 111_142_857;
    const LAST: u64 = 767_142_857;
    if frequency_hz < FIRST || frequency_hz > LAST {
        return false;
    }
    (frequency_hz - FIRST) % 6_000_000 == 0
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendIsdbtSegmentRequest {
    Unspecified,
    Auto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendIsdbtLayerSetting {
    pub num_of_segment: FrontendIsdbtSegmentRequest,
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
    pub isdbt_layer_settings: Vec<FrontendIsdbtLayerSetting>,
    pub partial_reception: FrontendIsdbtPartialReceptionRequirement,
}

/// Syntactically known frontend values that are not represented directly in
/// `FrontendTuneRequest`. Values are retained losslessly so service/runtime
/// policy can decide support without re-reading AIDL objects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendRequestedSetting {
    IsdbtExplicitBandwidth { bandwidth_hz: u32 },
    IsdbtExplicitMode { value: i32 },
    IsdbtExplicitInversion { value: i32 },
    IsdbtExplicitGuardInterval { value: i32 },
    IsdbtServiceAreaId { value: i32 },
    IsdbtPartialReceptionAuto,
    IsdbtLayerModulation { layer_index: usize, value: i32 },
    IsdbtLayerCoderate { layer_index: usize, value: i32 },
    IsdbtLayerTimeInterleave { layer_index: usize, value: i32 },
    IsdbtExplicitSegmentCount { layer_index: usize, count: i32 },
    IsdbsExplicitModulation { value: i32 },
    IsdbsExplicitCoderate { value: i32 },
    IsdbsExplicitRolloff { value: i32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendSettingsRequest {
    pub request: FrontendTuneRequest,
    pub requested_settings: Vec<FrontendRequestedSetting>,
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
        path: Option<PathBuf>,
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
            },
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
                "{context}: primary=({primary}); cleanup=({cleanup})"
            ),
            HalError::InvalidArgument { kind, detail } => {
                write!(f, "invalid argument ({kind:?}): {}", detail.detail)
            },
            HalError::InvalidState { kind, detail } => {
                write!(f, "invalid state ({kind:?}): {}", detail.detail)
            },
            HalError::Io {
                backend,
                operation,
                path,
                errno,
                detail,
            } => write!(
                f,
                "I/O error: backend={} operation={} path={} errno={} detail={}",
                backend,
                operation,
                display_path(path),
                errno.map(errno_name).unwrap_or("none"),
                detail.detail
            ),
            HalError::Internal { kind, detail } => {
                write!(f, "internal error ({kind:?}): {}", detail.detail)
            },
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

#[cfg(test)]
mod section_length_contract_tests {
    use super::*;

    #[test]
    fn section_length_contract_distinguishes_short_extended_and_tdt_classes() {
        for table_id in [
            0x00, 0x01, 0x02, 0x03, 0x40, 0x41, 0x42, 0x46, 0x4a, 0x71, 0x73,
        ] {
            assert!(is_valid_arib_section_length(table_id, 1021));
            assert!(!is_valid_arib_section_length(table_id, 1022));
        }
        for table_id in [
            0x04, 0x4c, 0x4e, 0x6f, 0x72, 0xc2, 0xc4, 0xc7, 0xd0, 0xd2, 0xfe, 0xff,
        ] {
            assert!(is_valid_arib_section_length(table_id, 4093));
            assert!(!is_valid_arib_section_length(table_id, 4094));
        }
        assert!(is_valid_arib_section_length(0x70, 5));
        assert!(!is_valid_arib_section_length(0x70, 4));
        assert!(!is_valid_arib_section_length(0x70, 6));
    }
}
