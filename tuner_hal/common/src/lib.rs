use std::collections::VecDeque;
use std::fmt;
use std::path::{Path, PathBuf};
use std::io;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

pub const TUNER_SERVICE_NAME: &str = "android.hardware.tv.tuner.ITuner/default";
pub const TS_PACKET_SIZE: usize = 188;

/// Retry an I/O read operation after `ErrorKind::Interrupted` without treating EINTR
/// as EOF or a normal short read. The retry counter is updated with saturating
/// semantics so debug/release behavior stays identical at `u64::MAX`.
pub fn retry_after_interrupted_read(
    operation: &'static str,
    retry_counter: &AtomicU64,
    mut f: impl FnMut() -> io::Result<usize>,
) -> io::Result<usize> {
    loop {
        match f() {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                let _ = retry_counter.fetch_update(
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                    |value| Some(value.saturating_add(1)),
                );
                eprintln!(
                    "maleicacid-tuner-hal-read-retry: operation={} error=EINTR action=retry",
                    operation
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
    resync_required: bool,
}

impl TsPacketCompletionBuffer {
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
        loop {
            if self.resync_required {
                let Some(offset) = Self::confirmed_sync_offset(&self.buf) else {
                    if self.buf.len() > TS_RESYNC_TAIL_BYTES {
                        let discard = self.buf.len() - TS_RESYNC_TAIL_BYTES;
                        self.buf.drain(..discard);
                        malformed_bytes = malformed_bytes.saturating_add(discard);
                    }
                    break;
                };
                if offset > 0 {
                    self.buf.drain(..offset);
                    malformed_bytes = malformed_bytes.saturating_add(offset);
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
            self.malformed_bytes = self.malformed_bytes.saturating_add(malformed_bytes as u64);
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

    /// Drain all completed packets at a lifecycle boundary and drop only the
    /// incomplete tail. This prevents a complete packet already held by the
    /// completion buffer from waiting for a later push after stop/flush/close.
    pub fn drain_for_boundary(&mut self) -> TsPacketBufferDrain {
        let packets = self.drain_completed(usize::MAX);
        let malformed_bytes = self.buf.len();
        if malformed_bytes > 0 {
            self.malformed_bytes = self.malformed_bytes.saturating_add(malformed_bytes as u64);
            self.buf.clear();
        }
        self.resync_required = false;
        TsPacketBufferDrain {
            packets,
            malformed_bytes,
        }
    }
}

#[cfg(test)]
mod ts_packet_completion_buffer_tests {
    use super::*;

    fn packet(seed: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [seed; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet
    }

    #[test]
    fn completion_buffer_assembles_one_byte_then_187_bytes() {
        let p = packet(0x11);
        let mut buffer = TsPacketCompletionBuffer::default();
        assert!(buffer.push(&p[..1]).packets.is_empty());
        let out = buffer.push(&p[1..]);
        assert_eq!(out.packets, vec![p]);
        assert_eq!(buffer.tail_len(), 0);
    }

    #[test]
    fn completion_buffer_keeps_tail_until_next_push() {
        let p0 = packet(0x22);
        let p1 = packet(0x33);
        let mut input = Vec::new();
        input.extend_from_slice(&p0);
        input.extend_from_slice(&p1[..17]);
        let mut buffer = TsPacketCompletionBuffer::default();
        let out = buffer.push(&input);
        assert_eq!(out.packets, vec![p0]);
        assert_eq!(buffer.tail_len(), 17);
        let out = buffer.push(&p1[17..]);
        assert_eq!(out.packets, vec![p1]);
        assert_eq!(buffer.tail_len(), 0);
    }

    #[test]
    fn completion_buffer_keeps_over_budget_completed_packets_for_next_drain() {
        let p0 = packet(0x44);
        let p1 = packet(0x55);
        let mut input = Vec::new();
        input.extend_from_slice(&p0);
        input.extend_from_slice(&p1);
        let mut buffer = TsPacketCompletionBuffer::default();

        let first = buffer.push_limited(&input, 1);
        assert_eq!(first.packets, vec![p0]);
        assert_eq!(buffer.drain_completed(1), vec![p1]);
        assert!(buffer.drain_completed(1).is_empty());
    }

    #[test]
    fn completion_buffer_keeps_resync_tail_without_panicking() {
        let mut buffer = TsPacketCompletionBuffer::default();
        let malformed = [0u8; TS_PACKET_SIZE];
        let out = buffer.push(&malformed);
        assert!(out.packets.is_empty());
        assert_eq!(out.malformed_bytes, 0);
        assert_eq!(buffer.tail_len(), TS_PACKET_SIZE);
        assert_eq!(buffer.malformed_bytes(), 0);
    }


    #[test]
    fn completion_buffer_does_not_resync_on_single_payload_sync_byte() {
        let p0 = packet(0x66);
        let p1 = packet(0x77);
        let mut malformed = vec![0x00; TS_PACKET_SIZE + 16];
        malformed[9] = 0x47;
        let mut buffer = TsPacketCompletionBuffer::default();
        let out = buffer.push(&malformed);
        assert!(out.packets.is_empty());
        assert_eq!(out.malformed_bytes, 0);

        let mut tail = Vec::new();
        tail.extend_from_slice(&p0);
        tail.extend_from_slice(&p1[..TS_PACKET_SIZE - 1]);
        let out = buffer.push(&tail);
        assert!(out.packets.is_empty());
        assert!(out.malformed_bytes > 0);
        assert!(buffer.tail_len() >= TS_RESYNC_TAIL_BYTES);
    }

    #[test]
    fn completion_buffer_resyncs_after_three_consecutive_sync_words() {
        let p0 = packet(0x88);
        let p1 = packet(0x99);
        let p2 = packet(0xaa);
        let mut input = vec![0x00, 0x47, 0x01, 0x02, 0x03];
        input.extend_from_slice(&p0);
        input.extend_from_slice(&p1);
        input.extend_from_slice(&p2);

        let mut buffer = TsPacketCompletionBuffer::default();
        let out = buffer.push(&input);
        assert_eq!(out.packets, vec![p0, p1, p2]);
        assert_eq!(out.malformed_bytes, 5);
        assert_eq!(buffer.malformed_bytes(), 5);
        assert_eq!(buffer.tail_len(), 0);
    }
}

#[cfg(test)]
mod r50dz52_g2_19_tests {
    use super::*;

    fn packet(seed: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [seed; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet
    }

    #[test]
    fn boundary_drain_keeps_completed_packets_and_drops_only_incomplete_tail() {
        let p0 = packet(0x21);
        let p1 = packet(0x22);
        let p2 = packet(0x23);
        let mut input = Vec::new();
        input.extend_from_slice(&p0);
        input.extend_from_slice(&p1);
        input.extend_from_slice(&p2[..31]);

        let mut buffer = TsPacketCompletionBuffer::default();
        let initial = buffer.push_limited(&input, 0);
        assert!(initial.packets.is_empty());
        assert_eq!(buffer.tail_len(), 31);

        let drained = buffer.drain_for_boundary();
        assert_eq!(drained.packets, vec![p0, p1]);
        assert_eq!(drained.malformed_bytes, 31);
        assert_eq!(buffer.tail_len(), 0);
    }
}

pub const DEMUX_MAX_FILTERS_PER_DEMUX: usize = 32;
pub const DEMUX_MAX_TS_FILTERS: i32 = DEMUX_MAX_FILTERS_PER_DEMUX as i32;
pub const DEMUX_MAX_SECTION_FILTERS: i32 = 8;
pub const DEMUX_MAX_AUDIO_FILTERS: i32 = 4;
pub const DEMUX_MAX_VIDEO_FILTERS: i32 = 4;
pub const DEMUX_MAX_PES_FILTERS: i32 = 8;
pub const DEMUX_MAX_RECORD_FILTERS: i32 = 32;
pub const MAX_SECTION_FILTER_BYTES: i32 = 16;
/// セクションフィルター 経由で配送する組立済み PSI/SI section payload の上限。
/// `MAX_SECTION_FILTER_BYTES` は mask/filter のbyte幅だけを表すため、payload上限とは分離する。
pub const MAX_SECTION_PAYLOAD_BYTES: usize = 8192;

#[derive(Debug)]
pub struct IdAllocator {
    next: AtomicI32,
}

impl IdAllocator {
    pub const fn new(start: i32) -> Self {
        Self {
            next: AtomicI32::new(start),
        }
    }

    pub fn allocate(&self) -> i32 {
        self.next.fetch_add(1, Ordering::SeqCst)
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendSelection {
    pub frontend_id: i32,
    pub backend: FrontendBackendKind,
    pub control_path: FrontendDevicePath,
}

impl FrontendSelection {
    pub fn backend_hint(&self) -> &'static str {
        self.backend.as_hint()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FrontendRuntimeState {
    pub callback_registered: bool,
    pub lnb_id: Option<i32>,
    pub last_error: Option<String>,
    pub tune_request_count: u64,
    pub tuning_active: bool,
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
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FrontendTelemetry {
    pub locked: bool,
    /// RF/搬送波ロック状態。`None` はこの backend が RF_LOCK を報告できないことを表す。
    pub rf_locked: Option<bool>,
    pub cnr: Option<u32>,
    pub signal_strength: Option<u32>,
    pub signal_quality: Option<u32>,
    pub lna_on: bool,
    pub lnb_voltage: Option<i32>,
    pub current_system: Option<FrontendSystem>,
    pub tuned_frequency: Option<u64>,
}

#[derive(Clone, Debug)]
pub enum HalError {
    DeviceMissing(PathBuf),
    OpenFailed {
        path: PathBuf,
        message: String,
    },
    PermissionDenied {
        path: PathBuf,
        message: String,
    },
    Busy {
        path: Option<PathBuf>,
        message: String,
    },
    IoctlFailed {
        backend: &'static str,
        path: Option<PathBuf>,
        op: &'static str,
        errno: i32,
    },
    InvalidArgument(String),
    InvalidState(String),
    Io {
        backend: &'static str,
        operation: &'static str,
        path: Option<PathBuf>,
        errno: Option<i32>,
        message: String,
    },
    Internal(String),
    Unsupported(&'static str),
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
            HalError::OpenFailed { path, message } => {
                write!(f, "open に失敗しました {}: {}", path.display(), message)
            }
            HalError::PermissionDenied { path, message } => {
                write!(f, "permission denied opening {}: {}", path.display(), message)
            }
            HalError::Busy { path, message } => {
                if let Some(path) = path {
                    write!(f, "device busy {}: {}", path.display(), message)
                } else {
                    write!(f, "device busy: {message}")
                }
            }
            HalError::IoctlFailed { backend, path, op, errno } => write!(
                f,
                "実行時ioctl失敗: backend={} operation={} device_path={} errno={} errno_name={}",
                backend,
                op,
                display_path(path),
                errno,
                errno_name(*errno)
            ),
            HalError::InvalidArgument(message) => write!(f, "不正な引数: {message}"),
            HalError::InvalidState(message) => write!(f, "不正な状態: {message}"),
            HalError::Io { backend, operation, path, errno, message } => {
                if let Some(errno) = errno {
                    write!(
                        f,
                        "実行時I/O失敗: backend={} operation={} device_path={} errno={} errno_name={} message={}",
                        backend,
                        operation,
                        display_path(path),
                        errno,
                        errno_name(*errno),
                        message
                    )
                } else {
                    write!(
                        f,
                        "実行時I/O失敗: backend={} operation={} device_path={} errno=<none> errno_name=<none> message={}",
                        backend,
                        operation,
                        display_path(path),
                        message
                    )
                }
            },
            HalError::Internal(message) => write!(f, "内部エラー: {message}"),
            HalError::Unsupported(message) => write!(f, "対象外: {message}"),
        }
    }
}

impl std::error::Error for HalError {}

#[derive(Clone, Debug, Default)]
pub struct ServiceConfig {
    pub tuner_service_name: &'static str,
}

impl ServiceConfig {
    pub fn new() -> Self {
        Self {
            tuner_service_name: TUNER_SERVICE_NAME,
        }
    }
}

#[cfg(test)]
mod r51_frequency_contract_tests {
    use super::{
        is_japan_cs110_if_frequency_hz, is_japan_isdbt_frequency_contract_hz,
        japan_isdbt_frequency_contract_range_hz,
    };

    #[test]
    fn isdbt_frequency_helper_rejects_vhf_1_to_12_contract() {
        let (min_hz, max_hz, tolerance_hz) = japan_isdbt_frequency_contract_range_hz();
        assert_eq!(min_hz, 110_642_857);
        assert_eq!(max_hz, 767_642_857);
        assert_eq!(tolerance_hz, 500_000);
        assert!(is_japan_isdbt_frequency_contract_hz(111_142_857));
        assert!(is_japan_isdbt_frequency_contract_hz(473_142_857));
        assert!(is_japan_isdbt_frequency_contract_hz(767_142_857));
        assert!(!is_japan_isdbt_frequency_contract_hz(90_000_000));
        assert!(!is_japan_isdbt_frequency_contract_hz(110_642_856));
        assert!(!is_japan_isdbt_frequency_contract_hz(767_642_858));
    }

    #[test]
    fn cs110_frequency_helper_is_exact_for_acquire_range_zero_contract() {
        assert!(is_japan_cs110_if_frequency_hz(1_613_000_000));
        assert!(is_japan_cs110_if_frequency_hz(2_053_000_000));
        assert!(!is_japan_cs110_if_frequency_hz(1_613_000_001));
        assert!(!is_japan_cs110_if_frequency_hz(1_613_499_999));
        assert!(!is_japan_cs110_if_frequency_hz(1_612_999_999));
    }
}
#[cfg(test)]
mod r50dz52_g1_20_tests {
    use super::*;

    fn packet(seed: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [seed; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet
    }

    #[test]
    fn completion_buffer_requires_confirmed_resync_and_emits_aligned_tail() {
        let mut single_sync = TsPacketCompletionBuffer::default();
        let mut malformed = vec![0x00; TS_PACKET_SIZE + 11];
        malformed[5] = 0x47;
        assert!(single_sync.push(&malformed).packets.is_empty());

        let p0 = packet(0x31);
        let p1 = packet(0x32);
        let p2 = packet(0x33);
        let mut confirmed = vec![0x11, 0x22, 0x33];
        confirmed.extend_from_slice(&p0);
        confirmed.extend_from_slice(&p1);
        confirmed.extend_from_slice(&p2);
        let mut buffer = TsPacketCompletionBuffer::default();
        let out = buffer.push(&confirmed);
        assert_eq!(out.packets, vec![p0, p1, p2]);
        assert_eq!(out.malformed_bytes, 3);

        let aligned = packet(0x44);
        let mut tail_buffer = TsPacketCompletionBuffer::default();
        assert_eq!(tail_buffer.push(&aligned).packets, vec![aligned]);
    }
}

#[cfg(test)]
mod r50dz52_g2_15_tests {
    use super::*;
    use std::io;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn interrupted_read_is_retried_and_counted_without_eof_success() {
        let counter = AtomicU64::new(0);
        let mut attempts = 0usize;
        let result = retry_after_interrupted_read("r50dz52_g2_15", &counter, || {
            attempts += 1;
            if attempts <= 2 {
                Err(io::Error::new(io::ErrorKind::Interrupted, "eintr"))
            } else {
                Ok(188)
            }
        });
        assert_eq!(result.unwrap(), 188);
        assert_eq!(attempts, 3);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}

