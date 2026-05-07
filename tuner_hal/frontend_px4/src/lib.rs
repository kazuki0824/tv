mod px4_tune_mapping;

use maleicacid_tuner_hal_common::{
    FrontendBackendKind, FrontendDevicePath, FrontendRuntimeState, FrontendScanMode,
    FrontendSelection, FrontendSystem, FrontendTelemetry, FrontendTuneRequest, HalError,
    TsPacketCompletionBuffer, TS_PACKET_SIZE,
};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub use px4_tune_mapping::reportable_bs_tsid_for_scan;
use px4_tune_mapping::{map_tune_request_to_px4, px4_scan_requests};

pub const PX4_PROBE_PREFIXES: &[&str] = &[
    "px4video",
    "pxmlt5video",
    "pxmlt8video",
    "isdb6014video",
    "isdb2056video",
    "pxm1urvideo",
    "pxs1urvideo",
    "isdbt2071video",
];

extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
    fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

const POLLIN: i16 = 0x0001;
const POLLERR: i16 = 0x0008;
const POLLHUP: i16 = 0x0010;
const POLLNVAL: i16 = 0x0020;

fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
}

const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;
const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;
const IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

const fn ioc(dir: u32, typ: u32, nr: u32, size: u32) -> u64 {
    ((dir << IOC_DIRSHIFT) | (typ << IOC_TYPESHIFT) | (nr << IOC_NRSHIFT) | (size << IOC_SIZESHIFT))
        as u64
}
const fn io(typ: u32, nr: u32) -> u64 {
    ioc(IOC_NONE, typ, nr, 0)
}
const fn iow<T>(typ: u32, nr: u32) -> u64 {
    ioc(IOC_WRITE, typ, nr, size_of::<T>() as u32)
}
const fn ior<T>(typ: u32, nr: u32) -> u64 {
    ioc(IOC_READ, typ, nr, size_of::<T>() as u32)
}

const PTX_IOCTL_TYPE_BASIC: u32 = 0x8d;
const PTX_IOCTL_TYPE_EXT: u32 = 0xe7;

const PTX_SET_CHANNEL: u64 = iow::<PtxFreq>(PTX_IOCTL_TYPE_BASIC, 0x01);
const PTX_START_STREAMING: u64 = io(PTX_IOCTL_TYPE_BASIC, 0x02);
const PTX_STOP_STREAMING: u64 = io(PTX_IOCTL_TYPE_BASIC, 0x03);
const PTX_GET_CNR: u64 = ior::<u32>(PTX_IOCTL_TYPE_BASIC, 0x04);
const PTX_ENABLE_LNB_POWER: u64 = iow::<i32>(PTX_IOCTL_TYPE_BASIC, 0x05);
const PTX_DISABLE_LNB_POWER: u64 = io(PTX_IOCTL_TYPE_BASIC, 0x06);
const PTX_SET_SYSTEM_MODE: u64 = iow::<u32>(PTX_IOCTL_TYPE_BASIC, 0x0b);
const PTXT_SET_LNB_VOLTAGE: u64 = iow::<i32>(PTX_IOCTL_TYPE_EXT, 0x05);

const O_NONBLOCK: i32 = 0x800;
const ERRNO_EINVAL: i32 = 22;
const ERRNO_ENOTTY: i32 = 25;
const ERRNO_ENOSYS: i32 = 38;

const PTX_ISDB_T_SYSTEM: u32 = 0x0000_0010;
const PTX_ISDB_S_SYSTEM: u32 = 0x0000_0020;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct PtxFreq {
    freq_no: i32,
    slot: i32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Px4FrontendStatus {
    pub telemetry: FrontendTelemetry,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Px4FrontendProbe {
    pub frontend_index: i32,
    pub device_name: Option<String>,
    pub control_path: PathBuf,
    pub supported_systems: Vec<FrontendSystem>,
}

trait Px4LnbOps {
    fn set_extended_lnb_voltage(&mut self, voltage: i32) -> Result<(), HalError>;
    fn set_legacy_lnb_enabled(&mut self, enabled: bool, voltage: i32) -> Result<(), HalError>;
}

struct RealPx4LnbOps<'a> {
    backend: &'a mut Px4FrontendBackend,
}

impl<'a> Px4LnbOps for RealPx4LnbOps<'a> {
    fn set_extended_lnb_voltage(&mut self, voltage: i32) -> Result<(), HalError> {
        let mut requested = voltage.max(0);
        self.backend
            .ioctl_ptr(PTXT_SET_LNB_VOLTAGE, &mut requested, "PTXT_SET_LNB_VOLTAGE")
    }

    fn set_legacy_lnb_enabled(&mut self, enabled: bool, voltage: i32) -> Result<(), HalError> {
        if enabled {
            let mut legacy_request = voltage;
            self.backend.ioctl_ptr(
                PTX_ENABLE_LNB_POWER,
                &mut legacy_request,
                "PTX_ENABLE_LNB_POWER",
            )
        } else {
            self.backend
                .ioctl_noarg(PTX_DISABLE_LNB_POWER, "PTX_DISABLE_LNB_POWER")
        }
    }
}

fn linux_major(dev: u64) -> u64 {
    ((dev >> 8) & 0x0fff) | ((dev >> 32) & !0x0fff)
}

fn linux_minor(dev: u64) -> u64 {
    (dev & 0x00ff) | ((dev >> 12) & !0x00ff)
}

fn sysfs_devname_for_char_device(path: &std::path::Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let rdev = metadata.rdev();
    let uevent = std::fs::read_to_string(format!(
        "/sys/dev/char/{}:{}/uevent",
        linux_major(rdev),
        linux_minor(rdev)
    ))
    .ok()?;
    for line in uevent.lines() {
        if let Some(value) = line.strip_prefix("DEVNAME=") {
            return value
                .rsplit('/')
                .next()
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string());
        }
    }
    None
}

#[derive(Debug)]
pub struct Px4LiveStreamReaderState {
    reader: File,
    reader_path: PathBuf,
    residual: TsPacketCompletionBuffer,
    malformed_bytes_total: u64,
    last_packet_seen: Option<Instant>,
    stopped: bool,
}

#[derive(Clone, Debug)]
pub struct Px4LiveStreamReader {
    inner: Arc<Mutex<Px4LiveStreamReaderState>>,
}

impl Px4LiveStreamReader {
    pub fn sample_ts_packets(
        &self,
        max_packets: usize,
        stop_fd: Option<i32>,
    ) -> Result<Vec<[u8; TS_PACKET_SIZE]>, HalError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| HalError::Internal("px4 live stream reader mutex poisoned".into()))?;
        let Px4LiveStreamReaderState {
            reader,
            reader_path,
            residual,
            malformed_bytes_total,
            last_packet_seen,
            stopped,
        } = &mut *inner;
        if *stopped {
            return Ok(Vec::new());
        }
        if !Px4FrontendBackend::poll_reader_ready(
            reader.as_raw_fd(),
            reader_path.as_path(),
            stop_fd,
        )? {
            return Ok(Vec::new());
        }
        let mut packets = Vec::new();
        let malformed_before = residual.malformed_bytes();
        let path = reader_path.clone();
        let pushed = Px4FrontendBackend::pump_reader_packets(
            reader,
            Some(path.as_path()),
            max_packets,
            residual,
            |pkt| {
                let mut packet = [0u8; TS_PACKET_SIZE];
                packet.copy_from_slice(pkt);
                packets.push(packet);
            },
        )?;
        let malformed_delta = residual.malformed_bytes().saturating_sub(malformed_before);
        if malformed_delta > 0 {
            *malformed_bytes_total = malformed_bytes_total.saturating_add(malformed_delta);
            eprintln!(
                "maleicacid-tuner-hal-px4-diagnostic: malformed_ts_bytes={} total_malformed_ts_bytes={}",
                malformed_delta,
                *malformed_bytes_total
            );
        }
        if pushed > 0 {
            *last_packet_seen = Some(Instant::now());
        }
        Ok(packets)
    }
}

#[derive(Debug)]
pub struct Px4FrontendBackend {
    selection: FrontendSelection,
    control: Option<File>,
    ts_reader: Option<Arc<Mutex<Px4LiveStreamReaderState>>>,
    last_tune: Option<FrontendTuneRequest>,
    state: FrontendRuntimeState,
    telemetry: FrontendTelemetry,
    last_packet_seen: Option<Instant>,
    driver_tune_locked: bool,
    last_driver_lock_time: Option<Instant>,
}

impl Px4FrontendBackend {
    pub fn new(frontend_id: i32) -> Self {
        Self::new_with_control_path(frontend_id, Self::default_control_path(frontend_id))
    }

    pub fn new_with_control_path(frontend_id: i32, control_path: PathBuf) -> Self {
        Self {
            selection: Self::selection_with_control_path(frontend_id, control_path),
            control: None,
            ts_reader: None,
            last_tune: None,
            state: FrontendRuntimeState::default(),
            telemetry: FrontendTelemetry::default(),
            last_packet_seen: None,
            driver_tune_locked: false,
            last_driver_lock_time: None,
        }
    }

    pub fn selection(frontend_id: i32) -> FrontendSelection {
        Self::selection_with_control_path(frontend_id, Self::default_control_path(frontend_id))
    }

    pub fn selection_with_control_path(
        frontend_id: i32,
        control_path: PathBuf,
    ) -> FrontendSelection {
        FrontendSelection {
            frontend_id,
            backend: FrontendBackendKind::Px4CharDevice,
            control_path: FrontendDevicePath::new(control_path),
        }
    }

    pub fn selection_ref(&self) -> &FrontendSelection {
        &self.selection
    }
    pub fn set_callback_registered(&mut self, registered: bool) {
        self.state.callback_registered = registered;
    }
    pub fn mark_callback_failed(&mut self, message: impl Into<String>) {
        self.state.callback_registered = false;
        self.state.tuning_active = false;
        self.state.last_error = Some(message.into());
        self.telemetry.locked = false;
        self.telemetry.rf_locked = None;
    }
    pub fn set_lnb_id(&mut self, lnb_id: i32) {
        self.state.lnb_id = Some(lnb_id);
    }

    pub fn hardware_info(&mut self) -> String {
        let mut result = format!(
            "maleicacid-px4 frontend_id={} path={}",
            self.selection.frontend_id,
            self.selection.control_path.display()
        );
        if let Ok(name) = self.device_name() {
            result.push_str(&format!(" name={name}"));
        }
        result
    }

    pub fn runtime_state(&self) -> &FrontendRuntimeState {
        &self.state
    }
    pub fn last_tune(&self) -> Option<&FrontendTuneRequest> {
        self.last_tune.as_ref()
    }
    pub fn probe_device(&self) -> bool {
        self.selection.control_path.as_path().exists()
    }

    pub fn probe_info(&mut self) -> Result<Px4FrontendProbe, HalError> {
        Ok(Px4FrontendProbe {
            frontend_index: self.selection.frontend_id,
            device_name: self.device_name().ok(),
            control_path: self.selection.control_path.as_path().to_path_buf(),
            supported_systems: self.detect_supported_systems()?,
        })
    }

    pub fn enumerate_probes() -> Vec<Px4FrontendProbe> {
        let prefixes = PX4_PROBE_PREFIXES;
        let mut candidates: Vec<(i32, PathBuf, String)> = Vec::new();
        if let Ok(dir) = std::fs::read_dir("/dev") {
            for entry in dir.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                for prefix in prefixes {
                    let Some(idx) = name.strip_prefix(prefix) else {
                        continue;
                    };
                    let Ok(index) = idx.parse::<i32>() else {
                        continue;
                    };
                    candidates.push((index, entry.path(), name.to_string()));
                }
            }
        }
        if candidates.is_empty() && PathBuf::from("/dev/px4video0").exists() {
            candidates.push((0, PathBuf::from("/dev/px4video0"), "px4video0".to_string()));
        }
        candidates.sort_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
        candidates.dedup_by(|a, b| a.1 == b.1);
        let mut probes = Vec::new();
        for (index, path, _name) in candidates {
            let mut backend = Self::new_with_control_path(index, path);
            if !backend.probe_device() {
                continue;
            }
            match backend.probe_info() {
                Ok(probe) if !probe.supported_systems.is_empty() => probes.push(probe),
                Ok(_) | Err(_) => {}
            }
        }
        probes
    }

    fn clear_driver_lock_state(&mut self) {
        self.state.tuning_active = false;
        self.telemetry.locked = false;
        if let Some(reader) = self.ts_reader.as_ref() {
            if let Ok(mut reader) = reader.lock() {
                reader.stopped = true;
            }
        }
        self.ts_reader = None;
        self.last_packet_seen = None;
        self.driver_tune_locked = false;
        self.last_driver_lock_time = None;
    }

    pub fn tune(&mut self, request: FrontendTuneRequest) -> Result<Px4FrontendStatus, HalError> {
        self.ensure_control_open()?;
        let mapped = map_tune_request_to_px4(&request)?;
        if self.state.tuning_active || self.ts_reader.is_some() {
            self.clear_driver_lock_state();
            self.ioctl_noarg(PTX_STOP_STREAMING, "PTX_STOP_STREAMING")?;
        }
        self.clear_driver_lock_state();
        let mut system_mode = mapped.system_code;
        self.ioctl_ptr(PTX_SET_SYSTEM_MODE, &mut system_mode, "PTX_SET_SYSTEM_MODE")?;

        let mut freq = PtxFreq {
            freq_no: mapped.freq_no,
            slot: mapped.slot,
        };
        self.ioctl_ptr(PTX_SET_CHANNEL, &mut freq, "PTX_SET_CHANNEL")?;
        self.driver_tune_locked = true;
        self.last_driver_lock_time = Some(Instant::now());
        if let Err(err) = self.ioctl_noarg(PTX_START_STREAMING, "PTX_START_STREAMING") {
            self.driver_tune_locked = false;
            self.last_driver_lock_time = None;
            return Err(err);
        }

        self.state.tuning_active = true;
        self.state.tune_request_count += 1;
        self.state.last_error = None;
        self.last_tune = Some(request.clone());
        self.telemetry.current_system = Some(request.system);
        self.telemetry.tuned_frequency = Some(request.frequency);

        self.read_status()
    }

    pub fn stop_tune(&mut self) -> Result<(), HalError> {
        if self.control.is_some() {
            let _ = self.ioctl_noarg(PTX_STOP_STREAMING, "PTX_STOP_STREAMING");
        }
        self.clear_driver_lock_state();
        Ok(())
    }

    pub fn set_lnb_voltage(&mut self, voltage: i32) -> Result<(), HalError> {
        self.ensure_control_open()?;
        let resolved = {
            let mut ops = RealPx4LnbOps { backend: self };
            Self::set_lnb_voltage_with_ops(&mut ops, voltage)?
        };
        self.telemetry.lnb_voltage = resolved;
        Ok(())
    }

    fn set_lnb_voltage_with_ops<O: Px4LnbOps>(
        ops: &mut O,
        voltage: i32,
    ) -> Result<Option<i32>, HalError> {
        if voltage != 0 && voltage != 15 {
            return Err(HalError::InvalidArgument(format!(
                "px4 fixed LNB profile accepts only NONE or 15V; got {voltage}V"
            )));
        }
        let extended = ops.set_extended_lnb_voltage(voltage);
        let should_try_legacy = match &extended {
            Err(err) => Self::lnb_voltage_fallback_allowed(err),
            Ok(()) => false,
        };
        let legacy = if should_try_legacy {
            let legacy_request = if voltage > 0 { 2 } else { 0 };
            ops.set_legacy_lnb_enabled(voltage > 0, legacy_request)
        } else {
            Err(HalError::Unsupported(
                "px4 legacy LNB fallback は試行していません",
            ))
        };
        Self::resolve_lnb_voltage_attempts(voltage, extended, legacy)
    }
    fn lnb_voltage_fallback_allowed(err: &HalError) -> bool {
        matches!(err, HalError::IoctlFailed { errno, .. } if *errno == ERRNO_ENOTTY || *errno == ERRNO_EINVAL || *errno == ERRNO_ENOSYS)
    }

    fn resolve_lnb_voltage_attempts(
        voltage: i32,
        extended: Result<(), HalError>,
        legacy: Result<(), HalError>,
    ) -> Result<Option<i32>, HalError> {
        match extended {
            Ok(()) => Ok((voltage > 0).then_some(voltage)),
            Err(extended_err) if Self::lnb_voltage_fallback_allowed(&extended_err) => {
                legacy.map(|()| (voltage > 0).then_some(voltage))
            }
            Err(extended_err) => Err(extended_err),
        }
    }

    pub fn read_status(&mut self) -> Result<Px4FrontendStatus, HalError> {
        self.ensure_control_open()?;
        let mut raw_cnr: u32 = 0;
        let cnr = match self.ioctl_ptr(PTX_GET_CNR, &mut raw_cnr, "PTX_GET_CNR") {
            Ok(()) => Some(raw_cnr),
            Err(err) => {
                self.state.last_error =
                    Some(format!("PTX_GET_CNR optional telemetry failed: {err}"));
                None
            }
        };

        let signal_strength = None;
        self.telemetry.signal_strength = signal_strength;
        self.telemetry.cnr = cnr;
        self.telemetry.signal_quality = cnr.map(Self::quality_from_cnr);
        self.telemetry.locked =
            Self::lock_from_driver_tune_result(self.state.tuning_active, self.driver_tune_locked);
        Ok(Px4FrontendStatus {
            telemetry: self.telemetry.clone(),
        })
    }

    pub fn pump_reader_packets<R, F>(
        reader: &mut R,
        reader_path: Option<&Path>,
        max_packets: usize,
        residual: &mut TsPacketCompletionBuffer,
        mut on_packet: F,
    ) -> Result<usize, HalError>
    where
        R: Read,
        F: FnMut(&[u8]),
    {
        let mut pushed = 0usize;
        let mut scratch = [0u8; TS_PACKET_SIZE * 128];
        while pushed < max_packets {
            for packet in residual
                .drain_completed(max_packets.saturating_sub(pushed))
                .into_iter()
            {
                on_packet(&packet);
                pushed += 1;
            }
            if pushed >= max_packets {
                break;
            }
            match reader.read(&mut scratch) {
                Ok(0) => break,
                Ok(read) => {
                    let drain =
                        residual.push_limited(&scratch[..read], max_packets.saturating_sub(pushed));
                    for packet in drain.packets.into_iter() {
                        on_packet(&packet);
                        pushed += 1;
                    }
                    if read < scratch.len() {
                        break;
                    }
                }
                Err(err)
                    if err.kind() == ErrorKind::UnexpectedEof
                        || err.kind() == ErrorKind::WouldBlock
                        || err.kind() == ErrorKind::Interrupted =>
                {
                    break
                }
                Err(err) => {
                    return Err(HalError::Io {
                        backend: "px4",
                        operation: "ts_reader_read",
                        path: reader_path.map(|p| p.to_path_buf()),
                        errno: err.raw_os_error(),
                        message: format!("px4 ts reader failed: {}", err),
                    })
                }
            }
        }
        Ok(pushed)
    }

    fn drain_stop_fd(stop_fd: i32) {
        let mut buf = [0u8; 64];
        loop {
            let rc = unsafe { read(stop_fd, buf.as_mut_ptr(), buf.len()) };
            if rc <= 0 || (rc as usize) < buf.len() {
                break;
            }
        }
    }

    fn classify_device_revents(path: &Path, revents: i16) -> Result<bool, HalError> {
        if (revents & (POLLERR | POLLHUP | POLLNVAL)) != 0 {
            return Err(HalError::Io {
                backend: "px4",
                operation: "dvr_poll",
                path: Some(path.to_path_buf()),
                errno: None,
                message: format!("px4 dvr poll reported device fd error revents=0x{revents:x}"),
            });
        }
        Ok((revents & POLLIN) != 0)
    }

    fn poll_reader_ready(fd: i32, path: &Path, stop_fd: Option<i32>) -> Result<bool, HalError> {
        let mut pollfds = Vec::with_capacity(if stop_fd.is_some() { 2 } else { 1 });
        pollfds.push(PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        });
        if let Some(stop_fd) = stop_fd {
            pollfds.push(PollFd {
                fd: stop_fd,
                events: POLLIN,
                revents: 0,
            });
        }
        let rc = unsafe { poll(pollfds.as_mut_ptr(), pollfds.len(), 1_000) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            return Err(HalError::Io {
                backend: "px4",
                operation: "dvr_poll",
                path: Some(path.to_path_buf()),
                errno: err.raw_os_error(),
                message: format!("px4 dvr poll failed: {}", err),
            });
        }
        if rc == 0 {
            return Ok(false);
        }
        if pollfds.len() > 1 {
            let stop_revents = pollfds[1].revents;
            if (stop_revents & (POLLIN | POLLHUP)) != 0 {
                if let Some(stop_fd) = stop_fd {
                    Self::drain_stop_fd(stop_fd);
                }
                return Ok(false);
            }
            if (stop_revents & (POLLERR | POLLNVAL)) != 0 {
                return Err(HalError::Io {
                    backend: "px4",
                    operation: "dvr_poll_stop_fd",
                    path: Some(path.to_path_buf()),
                    errno: None,
                    message: "px4 dvr stop fd poll reported error".into(),
                });
            }
        }
        Self::classify_device_revents(path, pollfds[0].revents)
    }

    pub fn live_stream_reader(&mut self) -> Result<Option<Px4LiveStreamReader>, HalError> {
        self.ensure_control_open()?;
        self.ensure_ts_reader_open()?;
        Ok(self.ts_reader.as_ref().map(|inner| Px4LiveStreamReader {
            inner: Arc::clone(inner),
        }))
    }

    pub fn close(&mut self) {
        if let Some(reader) = self.ts_reader.as_ref() {
            if let Ok(mut reader) = reader.lock() {
                reader.stopped = true;
            }
        }
        self.ts_reader = None;
        self.last_packet_seen = None;
        self.control = None;
        self.clear_driver_lock_state();
    }

    fn ensure_control_open(&mut self) -> Result<(), HalError> {
        if self.control.is_some() {
            return Ok(());
        }
        let path = self.selection.control_path.as_path().to_path_buf();
        if !path.exists() {
            let err = HalError::DeviceMissing(path.clone());
            self.state.last_error = Some(err.to_string());
            return Err(err);
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| {
                let err = classify_open_error(&path, &e);
                self.state.last_error = Some(err.to_string());
                err
            })?;
        self.control = Some(file);
        Ok(())
    }

    fn ensure_ts_reader_open(&mut self) -> Result<(), HalError> {
        if self.ts_reader.is_some() {
            return Ok(());
        }
        let path = self.selection.control_path.as_path().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(&path)
            .map_err(|e| classify_open_error(&path, &e))?;
        self.ts_reader = Some(Arc::new(Mutex::new(Px4LiveStreamReaderState {
            reader: file,
            reader_path: path,
            residual: TsPacketCompletionBuffer::default(),
            malformed_bytes_total: 0,
            last_packet_seen: None,
            stopped: false,
        })));
        Ok(())
    }

    fn fd(&self) -> Result<i32, HalError> {
        self.control
            .as_ref()
            .map(|control| control.as_raw_fd())
            .ok_or_else(|| HalError::InvalidState("px4 control device is not open".into()))
    }

    fn ioctl_noarg(&mut self, request: u64, op: &'static str) -> Result<(), HalError> {
        let rc = unsafe { ioctl(self.fd()?, request) };
        if rc == 0 {
            Ok(())
        } else {
            let err = HalError::IoctlFailed {
                backend: "px4",
                path: Some(self.selection.control_path.as_path().to_path_buf()),
                op,
                errno: last_errno(),
            };
            self.state.last_error = Some(err.to_string());
            Err(err)
        }
    }

    fn ioctl_ptr<T>(
        &mut self,
        request: u64,
        data: &mut T,
        op: &'static str,
    ) -> Result<(), HalError> {
        let rc = unsafe { ioctl(self.fd()?, request, data as *mut T) };
        if rc == 0 {
            Ok(())
        } else {
            let err = HalError::IoctlFailed {
                backend: "px4",
                path: Some(self.selection.control_path.as_path().to_path_buf()),
                op,
                errno: last_errno(),
            };
            self.state.last_error = Some(err.to_string());
            Err(err)
        }
    }

    pub fn validate_tune_request(&self, request: &FrontendTuneRequest) -> Result<(), HalError> {
        let _ = map_tune_request_to_px4(request)?;
        Ok(())
    }

    pub fn scan_requests(
        &self,
        base: &FrontendTuneRequest,
        scan_mode: FrontendScanMode,
    ) -> Result<Vec<FrontendTuneRequest>, HalError> {
        if matches!(scan_mode, FrontendScanMode::Blind) {
            return Err(HalError::Unsupported(
                "px4 backend does not provide BLIND_SCAN; TIS owns the Japanese scan SSOT",
            ));
        }
        px4_scan_requests(base)
    }

    fn detect_supported_systems(&mut self) -> Result<Vec<FrontendSystem>, HalError> {
        self.ensure_control_open()?;
        let mut systems = Vec::new();
        for (mode, system) in [
            (PTX_ISDB_T_SYSTEM, FrontendSystem::IsdbT),
            (PTX_ISDB_S_SYSTEM, FrontendSystem::IsdbS),
        ] {
            let mut probe_mode = mode;
            if self
                .ioctl_ptr(PTX_SET_SYSTEM_MODE, &mut probe_mode, "PTX_SET_SYSTEM_MODE")
                .is_ok()
            {
                systems.push(system);
            }
        }
        let mut unspecified: u32 = 0;
        let _ = self.ioctl_ptr(PTX_SET_SYSTEM_MODE, &mut unspecified, "PTX_SET_SYSTEM_MODE");
        Ok(systems)
    }

    fn lock_from_driver_tune_result(tuning_active: bool, driver_tune_locked: bool) -> bool {
        // px4_drv legacy PTX_SET_CHANNEL は driver 内部で ops->check_lock() を待ち、
        // lock 失敗時は ioctl error になる。HAL の疑似 DEMOD_LOCK は TS 到着ではなく、
        // その ioctl 成功結果を source of truth とする。
        tuning_active && driver_tune_locked
    }

    fn quality_from_cnr(cnr: u32) -> u32 {
        (cnr / 100).min(100)
    }

    fn device_name(&mut self) -> Result<String, HalError> {
        if let Some(devname) = sysfs_devname_for_char_device(self.selection.control_path.as_path())
        {
            return Ok(devname);
        }
        self.selection
            .control_path
            .as_path()
            .file_name()
            .and_then(|v| v.to_str())
            .map(|v| v.to_string())
            .ok_or_else(|| HalError::InvalidArgument("px4 device path has no basename".to_string()))
    }

    fn default_control_path(frontend_id: i32) -> PathBuf {
        PathBuf::from(format!("/dev/px4video{frontend_id}"))
    }
}

#[cfg(test)]
mod tests {

    use super::{
        map_tune_request_to_px4, Px4FrontendBackend, Px4LiveStreamReader, Px4LiveStreamReaderState,
    };

    fn systems_from_cap(bits: u32) -> Vec<FrontendSystem> {
        let mut systems = Vec::new();
        if (bits & super::PTX_ISDB_T_SYSTEM) != 0 {
            systems.push(FrontendSystem::IsdbT);
        }
        if (bits & super::PTX_ISDB_S_SYSTEM) != 0 {
            systems.push(FrontendSystem::IsdbS);
        }
        systems
    }
    use maleicacid_tuner_hal_common::{
        FrontendScanMode, FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest,
        TsPacketCompletionBuffer, TS_PACKET_SIZE,
    };
    use maleicacid_tuner_hal_soft_demux::{
        DemuxCore, DemuxPathDirection, DvrConfig, FilterConfig, FilterConfigKind, FilterOpenType,
        SectionCondition, SectionConditionKind,
    };
    use std::io::{self, Cursor, Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    fn collect_prefixes_from_ueventd(text: &str) -> std::collections::BTreeSet<String> {
        text.lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter_map(|path| path.strip_prefix("/dev/"))
            .filter_map(|name| name.split("[0-9]").next())
            .filter(|name| super::PX4_PROBE_PREFIXES.contains(name))
            .map(str::to_string)
            .collect()
    }

    fn collect_prefixes_from_file_contexts(text: &str) -> std::collections::BTreeSet<String> {
        text.lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter_map(|path| path.strip_prefix("/dev/"))
            .filter_map(|name| name.split("[0-9]").next())
            .filter(|name| super::PX4_PROBE_PREFIXES.contains(name))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn px4_probe_prefixes_match_ueventd_and_file_contexts() {
        let expected: std::collections::BTreeSet<String> = super::PX4_PROBE_PREFIXES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let ueventd = include_str!("../../config/ueventd.tuner_hal.rc");
        let file_contexts = include_str!("../../sepolicy/file_contexts");
        assert_eq!(collect_prefixes_from_ueventd(ueventd), expected);
        assert_eq!(collect_prefixes_from_file_contexts(file_contexts), expected);
        for prefix in super::PX4_PROBE_PREFIXES {
            let ueventd_prefix = format!("/dev/{prefix}[0-9]*");
            let ueventd_line = ueventd
                .lines()
                .find(|line| line.starts_with(ueventd_prefix.as_str()))
                .unwrap();
            assert!(
                ueventd_line.ends_with("0660 media system"),
                "{ueventd_line}"
            );
            let fc_prefix = format!("/dev/{prefix}[0-9]+");
            let fc_line = file_contexts
                .lines()
                .find(|line| line.starts_with(fc_prefix.as_str()))
                .unwrap();
            assert!(
                fc_line.ends_with("u:object_r:px4_tuner_device:s0"),
                "{fc_line}"
            );
        }
    }

    #[test]
    fn selection_uses_px4_character_device_path() {
        let selection = Px4FrontendBackend::selection(3);
        assert_eq!(selection.frontend_id, 3);
        assert_eq!(selection.control_path.display(), "/dev/px4video3");
    }

    #[test]
    fn terrestrial_frequency_maps_to_px4_legacy_channel_and_addfreq() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 557_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        let mapped = map_tune_request_to_px4(&request).unwrap();
        assert_eq!(mapped.system_code, super::PTX_ISDB_T_SYSTEM);
        assert_eq!(mapped.freq_no, 77);
        assert_eq!(mapped.slot, 0);
    }

    #[test]
    fn systems_from_cap_reports_isdbs_without_dvbs_alias() {
        let systems = systems_from_cap(super::PTX_ISDB_S_SYSTEM);
        assert_eq!(systems, vec![FrontendSystem::IsdbS]);
    }

    #[test]
    fn px4_backend_validation_rejects_internal_symbol_rate_contract_violation() {
        let backend = Px4FrontendBackend::new(0);
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(0x4010),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: Some(28_860_000),
        };
        assert!(backend.validate_tune_request(&request).is_err());
    }

    #[test]
    fn px4_backend_validation_rejects_invalid_bandwidth_contract_violations() {
        let backend = Px4FrontendBackend::new(0);
        let valid_isdbt = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 557_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        assert!(backend.validate_tune_request(&valid_isdbt).is_ok());
        assert!(backend
            .validate_tune_request(&FrontendTuneRequest {
                bandwidth_hz: Some(7_000_000),
                ..valid_isdbt.clone()
            })
            .is_err());
        assert!(backend
            .validate_tune_request(&FrontendTuneRequest {
                bandwidth_hz: Some(8_000_000),
                ..valid_isdbt
            })
            .is_err());

        let isdbs_with_bandwidth = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(0x4010),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        assert!(backend
            .validate_tune_request(&isdbs_with_bandwidth)
            .is_err());
    }

    #[test]
    fn satellite_tsid_maps_to_px4_bs_legacy_carrier_and_slot() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(0x4010),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let mapped = map_tune_request_to_px4(&request).unwrap();
        assert_eq!(mapped.system_code, super::PTX_ISDB_S_SYSTEM);
        assert_eq!(mapped.freq_no, 0);
        assert_eq!(mapped.slot, 0);
    }

    #[test]
    fn satellite_bs_frequency_only_is_rejected_for_px4() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let err = map_tune_request_to_px4(&request).unwrap_err().to_string();
        assert!(
            err.contains("requires TSID or relative stream number"),
            "{err}"
        );
    }

    #[test]
    fn relative_stream_number_maps_to_px4_legacy_slot() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id: Some(1),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let mapped = map_tune_request_to_px4(&request).unwrap();
        assert_eq!(mapped.slot, 1);
    }

    #[test]
    fn cs110_px4_tune_is_frequency_only_with_fixed_zero_slot() {
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_613_000_000,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let mapped = map_tune_request_to_px4(&request).unwrap();
        assert_eq!(mapped.freq_no, 12);
        assert_eq!(mapped.slot, 0);

        let with_tsid = FrontendTuneRequest {
            stream_id: Some(0x6020),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            ..request
        };
        assert!(map_tune_request_to_px4(&with_tsid).is_err());
    }

    #[test]
    fn driver_tune_ioctl_success_is_lock_source_without_ts_sampling() {
        assert!(Px4FrontendBackend::lock_from_driver_tune_result(true, true));
        assert!(!Px4FrontendBackend::lock_from_driver_tune_result(
            true, false
        ));
        assert!(!Px4FrontendBackend::lock_from_driver_tune_result(
            false, true
        ));
    }

    #[test]
    fn clear_driver_lock_state_resets_ioctl_lock_and_ts_flow_state() {
        let mut backend =
            Px4FrontendBackend::new_with_control_path(0, std::path::PathBuf::from("/dev/null"));
        backend.state.tuning_active = true;
        backend.telemetry.locked = true;
        backend.driver_tune_locked = true;
        backend.last_driver_lock_time = Some(std::time::Instant::now());
        backend.last_packet_seen = Some(std::time::Instant::now());
        backend.clear_driver_lock_state();
        assert!(!backend.state.tuning_active);
        assert!(!backend.telemetry.locked);
        assert!(!backend.driver_tune_locked);
        assert!(backend.last_driver_lock_time.is_none());
        assert!(backend.last_packet_seen.is_none());
    }

    #[test]
    fn quality_is_capped_to_100() {
        assert_eq!(Px4FrontendBackend::quality_from_cnr(4_200), 42);
        assert_eq!(Px4FrontendBackend::quality_from_cnr(30_000), 100);
    }

    fn make_ts_packet(pid: u16, payload_unit_start: bool, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0xff; 188];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if payload_unit_start {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        packet[3] = 0x10;
        let mut offset = 4usize;
        if payload_unit_start {
            packet[offset] = 0x00;
            offset += 1;
        }
        let end = (offset + payload.len()).min(packet.len());
        packet[offset..end].copy_from_slice(&payload[..end - offset]);
        packet
    }

    fn section_with_table_id(table_id: u8, body: &[u8]) -> Vec<u8> {
        let section_len = body.len();
        let mut out = vec![
            table_id,
            0xB0 | (((section_len >> 8) & 0x0f) as u8),
            (section_len & 0xff) as u8,
        ];
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn reader_pump_routes_section_to_filter_but_not_to_record_dvr() {
        let core = DemuxCore::new();
        let mut demux = core.new_handle(0);
        let filter = demux.register_filter_result(1, FilterOpenType::TsSection, 1024).expect("test setup should register filter");
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert!(demux.configure_filter_with_summary(
            filter.filter_id,
            FilterConfig {
                tpid: 0,
                main_type_bits: 1,
                sub_type_hint: 0,
                kind: FilterConfigKind::Section {
                    check_crc: false,
                    repeat: true,
                    raw: false,
                    length_field_bits: 12,
                    condition_kind: SectionConditionKind::SectionBits,
                    condition: SectionCondition {
                        filter_bytes: vec![0x00],
                        mask_bytes: vec![0xff],
                        mode_bytes: vec![0],
                        table_id: Some(0x00),
                        version: None,
                    },
                },
            },
        ));
        assert!(demux.configure_dvr_with_summary(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 8,
                high_threshold: 32,
                data_format: 0,
                packet_size: 188,
            },
        ));
        assert!(!demux.attach_filter_to_dvr(dvr.dvr_id, filter.filter_id));
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.start_dvr(dvr.dvr_id));

        let pat = section_with_table_id(0x00, &[0x00, 0x01, 0xc1, 0x00, 0x00]);
        let packet = make_ts_packet(0, true, &pat);
        let mut cursor = Cursor::new(packet);
        let mut residual = TsPacketCompletionBuffer::default();
        let count =
            Px4FrontendBackend::pump_reader_packets(&mut cursor, None, 1, &mut residual, |pkt| {
                demux.push_ts_packet(pkt);
            })
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(demux.pop_filter_payload(filter.filter_id).unwrap(), pat);
        assert!(demux.pop_dvr_payload(dvr.dvr_id).is_none());
    }

    #[test]
    fn reader_pump_mirrors_record_ts_packets_to_dvr() {
        let core = DemuxCore::new();
        let mut demux = core.new_handle(0);
        let filter = demux.register_filter_result(1, FilterOpenType::TsRecord, 2048).expect("test setup should register filter");
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert!(demux.configure_filter_with_summary(
            filter.filter_id,
            FilterConfig {
                tpid: 0,
                main_type_bits: 1,
                sub_type_hint: 0,
                kind: FilterConfigKind::Record {
                    ts_index_mask: 1,
                    sc_index_type: 0,
                    sc_index_mask_bits: 0,
                },
            },
        ));
        assert!(demux.configure_dvr_with_summary(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 8,
                high_threshold: 32,
                data_format: 0,
                packet_size: 188,
            },
        ));
        assert!(demux.attach_filter_to_dvr(dvr.dvr_id, filter.filter_id));
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.start_dvr(dvr.dvr_id));

        let packet = make_ts_packet(0, true, &[0x00, 0xb0, 0x05, 0, 0, 0, 0, 0]);
        let mut cursor = Cursor::new(packet.clone());
        let mut residual = TsPacketCompletionBuffer::default();
        let count =
            Px4FrontendBackend::pump_reader_packets(&mut cursor, None, 1, &mut residual, |pkt| {
                demux.push_ts_packet(pkt);
            })
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(demux.pop_filter_payload(filter.filter_id).unwrap(), packet);
        assert_eq!(demux.pop_dvr_payload(dvr.dvr_id).unwrap(), packet);
    }

    #[test]
    fn reader_pump_stops_on_partial_packet() {
        let mut cursor = Cursor::new(vec![0u8; 100]);
        let mut residual = TsPacketCompletionBuffer::default();
        let count =
            Px4FrontendBackend::pump_reader_packets(&mut cursor, None, 1, &mut residual, |_| {})
                .unwrap();
        assert_eq!(count, 0);
        assert_eq!(residual.tail_len(), 100);
    }

    struct WouldBlockReader;

    impl Read for WouldBlockReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "not ready"))
        }
    }

    #[test]
    fn reader_pump_assembles_one_byte_then_187_bytes() {
        let packet = make_ts_packet(0x0124, false, &[0x66; 184]);
        let mut residual = TsPacketCompletionBuffer::default();
        let mut out = Vec::new();

        let mut first = Cursor::new(packet[..1].to_vec());
        assert_eq!(
            Px4FrontendBackend::pump_reader_packets(&mut first, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            0
        );
        assert!(out.is_empty());
        assert_eq!(residual.tail_len(), 1);

        let mut second = Cursor::new(packet[1..].to_vec());
        assert_eq!(
            Px4FrontendBackend::pump_reader_packets(&mut second, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            1
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], packet);
        assert_eq!(residual.tail_len(), 0);
    }

    #[test]
    fn reader_pump_keeps_over_budget_completed_packets_for_next_call() {
        let first = make_ts_packet(0x0126, false, &[0x88; 184]);
        let second = make_ts_packet(0x0127, false, &[0x99; 184]);
        let mut input = Vec::new();
        input.extend_from_slice(&first);
        input.extend_from_slice(&second);
        let mut residual = TsPacketCompletionBuffer::default();
        let mut out = Vec::new();

        let mut reader = Cursor::new(input);
        assert_eq!(
            Px4FrontendBackend::pump_reader_packets(&mut reader, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            1
        );
        assert_eq!(out, vec![first.clone()]);

        let mut empty = Cursor::new(Vec::<u8>::new());
        assert_eq!(
            Px4FrontendBackend::pump_reader_packets(&mut empty, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            1
        );
        assert_eq!(out, vec![first, second]);
    }

    #[test]
    fn reader_pump_assembles_split_ts_packet_and_keeps_tail_across_would_block() {
        let packet = make_ts_packet(0x0125, false, &[0x77; 184]);
        let mut residual = TsPacketCompletionBuffer::default();
        let mut out = Vec::new();

        let mut first = Cursor::new(packet[..100].to_vec());
        assert_eq!(
            Px4FrontendBackend::pump_reader_packets(&mut first, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            0
        );
        assert_eq!(residual.tail_len(), 100);

        let mut blocked = WouldBlockReader;
        assert_eq!(
            Px4FrontendBackend::pump_reader_packets(&mut blocked, None, 1, &mut residual, |pkt| {
                out.push(pkt.to_vec())
            })
            .unwrap(),
            0
        );
        assert_eq!(residual.tail_len(), 100);

        let mut rest = Cursor::new(packet[100..].to_vec());
        assert_eq!(
            Px4FrontendBackend::pump_reader_packets(&mut rest, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            1
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], packet);
        assert_eq!(residual.tail_len(), 0);
    }

    #[test]
    fn reader_pump_drops_malformed_full_packet_with_diagnostic_state() {
        let malformed = [0x22u8; TS_PACKET_SIZE];
        let mut residual = TsPacketCompletionBuffer::default();
        let mut out = Vec::new();

        let mut reader = Cursor::new(malformed.to_vec());
        assert_eq!(
            Px4FrontendBackend::pump_reader_packets(&mut reader, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            0
        );
        assert!(out.is_empty());
        assert_eq!(residual.tail_len(), 0);
        assert!(residual.malformed_bytes() >= TS_PACKET_SIZE as u64);
    }

    #[test]
    fn px4_poll_reader_ready_is_woken_by_stop_fd() {
        let (mut device_writer, device_reader) = UnixStream::pair().unwrap();
        let (mut stop_writer, stop_reader) = UnixStream::pair().unwrap();
        device_reader.set_nonblocking(true).unwrap();
        stop_reader.set_nonblocking(true).unwrap();

        stop_writer.write_all(&[1]).unwrap();
        assert!(!Px4FrontendBackend::poll_reader_ready(
            device_reader.as_raw_fd(),
            std::path::Path::new("/dev/px4/test"),
            Some(stop_reader.as_raw_fd())
        )
        .unwrap());

        device_writer.write_all(&[0x47]).unwrap();
        assert!(Px4FrontendBackend::poll_reader_ready(
            device_reader.as_raw_fd(),
            std::path::Path::new("/dev/px4/test"),
            None
        )
        .unwrap());
    }

    #[test]
    fn px4_device_poll_error_revents_are_fatal() {
        let path = std::path::Path::new("/dev/px4/test");
        assert!(Px4FrontendBackend::classify_device_revents(path, POLLERR).is_err());
        assert!(Px4FrontendBackend::classify_device_revents(path, POLLHUP).is_err());
        assert!(Px4FrontendBackend::classify_device_revents(path, POLLNVAL).is_err());
        assert_eq!(
            Px4FrontendBackend::classify_device_revents(path, 0).unwrap(),
            false
        );
        assert_eq!(
            Px4FrontendBackend::classify_device_revents(path, POLLIN).unwrap(),
            true
        );
    }

    #[test]
    fn stopped_px4_live_stream_reader_does_not_emit_old_packets() {
        let reader = Px4LiveStreamReader {
            inner: std::sync::Arc::new(std::sync::Mutex::new(Px4LiveStreamReaderState {
                reader: std::fs::File::open("/dev/null").unwrap(),
                reader_path: std::path::PathBuf::from("/dev/px4/old"),
                residual: TsPacketCompletionBuffer::default(),
                malformed_bytes_total: 0,
                last_packet_seen: None,
                stopped: true,
            })),
        };
        assert!(reader.sample_ts_packets(1, None).unwrap().is_empty());
    }
}

#[cfg(test)]
mod lnb_fallback_tests {
    use super::Px4FrontendBackend;
    use maleicacid_tuner_hal_common::HalError;

    fn unsupported() -> HalError {
        HalError::IoctlFailed {
            backend: "px4",
            path: None,
            op: "PTXT_SET_LNB_VOLTAGE",
            errno: super::ERRNO_ENOTTY,
        }
    }

    fn hard_failure() -> HalError {
        HalError::IoctlFailed {
            backend: "px4",
            path: None,
            op: "PTXT_SET_LNB_VOLTAGE",
            errno: 5,
        }
    }

    #[test]
    fn lnb_extended_success_sets_15v_without_legacy_requirement() {
        assert_eq!(
            Px4FrontendBackend::resolve_lnb_voltage_attempts(
                15,
                Ok(()),
                Err(HalError::Unsupported("未試行"))
            )
            .unwrap(),
            Some(15)
        );
        assert_eq!(
            Px4FrontendBackend::resolve_lnb_voltage_attempts(
                0,
                Ok(()),
                Err(HalError::Unsupported("未試行"))
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn lnb_extended_unsupported_falls_back_to_legacy_on_off() {
        assert_eq!(
            Px4FrontendBackend::resolve_lnb_voltage_attempts(15, Err(unsupported()), Ok(()))
                .unwrap(),
            Some(15)
        );
        assert_eq!(
            Px4FrontendBackend::resolve_lnb_voltage_attempts(0, Err(unsupported()), Ok(()))
                .unwrap(),
            None
        );
    }

    #[test]
    fn lnb_hard_extended_failure_does_not_mask_with_legacy() {
        assert!(
            Px4FrontendBackend::resolve_lnb_voltage_attempts(15, Err(hard_failure()), Ok(()))
                .is_err()
        );
    }

    #[test]
    fn lnb_both_extended_and_legacy_failure_returns_error() {
        assert!(Px4FrontendBackend::resolve_lnb_voltage_attempts(
            15,
            Err(unsupported()),
            Err(hard_failure())
        )
        .is_err());
    }
}

#[cfg(test)]
mod lnb_fallback_mock_tests {
    use super::{Px4FrontendBackend, Px4LnbOps};
    use maleicacid_tuner_hal_common::HalError;

    #[derive(Default)]
    struct MockLnbOps {
        extended_result: Option<Result<(), HalError>>,
        legacy_result: Option<Result<(), HalError>>,
        extended_calls: Vec<i32>,
        legacy_calls: Vec<(bool, i32)>,
    }

    impl Px4LnbOps for MockLnbOps {
        fn set_extended_lnb_voltage(&mut self, voltage: i32) -> Result<(), HalError> {
            self.extended_calls.push(voltage);
            self.extended_result.clone().unwrap_or(Ok(()))
        }
        fn set_legacy_lnb_enabled(&mut self, enabled: bool, voltage: i32) -> Result<(), HalError> {
            self.legacy_calls.push((enabled, voltage));
            self.legacy_result.clone().unwrap_or(Ok(()))
        }
    }

    fn unsupported() -> HalError {
        HalError::IoctlFailed {
            backend: "px4",
            path: None,
            op: "PTXT_SET_LNB_VOLTAGE",
            errno: super::ERRNO_ENOTTY,
        }
    }
    fn hard_failure() -> HalError {
        HalError::IoctlFailed {
            backend: "px4",
            path: None,
            op: "PTXT_SET_LNB_VOLTAGE",
            errno: 5,
        }
    }

    #[test]
    fn extended_success_does_not_call_legacy() {
        let mut ops = MockLnbOps {
            extended_result: Some(Ok(())),
            legacy_result: Some(Err(hard_failure())),
            ..Default::default()
        };
        assert_eq!(
            Px4FrontendBackend::set_lnb_voltage_with_ops(&mut ops, 15).unwrap(),
            Some(15)
        );
        assert_eq!(ops.extended_calls, vec![15]);
        assert!(ops.legacy_calls.is_empty());
    }

    #[test]
    fn fixed_px4_lnb_profile_rejects_non_15v_powered_voltage_before_ioctl() {
        let mut ops = MockLnbOps {
            extended_result: Some(Ok(())),
            legacy_result: Some(Ok(())),
            ..Default::default()
        };
        assert!(Px4FrontendBackend::set_lnb_voltage_with_ops(&mut ops, 11).is_err());
        assert!(Px4FrontendBackend::set_lnb_voltage_with_ops(&mut ops, 13).is_err());
        assert!(Px4FrontendBackend::set_lnb_voltage_with_ops(&mut ops, 18).is_err());
        assert!(ops.extended_calls.is_empty());
        assert!(ops.legacy_calls.is_empty());
    }

    #[test]
    fn unsupported_extended_falls_back_to_legacy_on_and_off() {
        let mut on_ops = MockLnbOps {
            extended_result: Some(Err(unsupported())),
            legacy_result: Some(Ok(())),
            ..Default::default()
        };
        assert_eq!(
            Px4FrontendBackend::set_lnb_voltage_with_ops(&mut on_ops, 15).unwrap(),
            Some(15)
        );
        assert_eq!(on_ops.legacy_calls, vec![(true, 2)]);
        let mut off_ops = MockLnbOps {
            extended_result: Some(Err(unsupported())),
            legacy_result: Some(Ok(())),
            ..Default::default()
        };
        assert_eq!(
            Px4FrontendBackend::set_lnb_voltage_with_ops(&mut off_ops, 0).unwrap(),
            None
        );
        assert_eq!(off_ops.legacy_calls, vec![(false, 0)]);
    }

    #[test]
    fn extended_einval_and_enosys_fall_back_but_unrelated_errno_does_not() {
        for errno in [super::ERRNO_EINVAL, super::ERRNO_ENOSYS] {
            let mut ops = MockLnbOps {
                extended_result: Some(Err(HalError::IoctlFailed {
                    backend: "px4",
                    path: None,
                    op: "PTXT_SET_LNB_VOLTAGE",
                    errno,
                })),
                legacy_result: Some(Ok(())),
                ..Default::default()
            };
            assert_eq!(
                Px4FrontendBackend::set_lnb_voltage_with_ops(&mut ops, 15).unwrap(),
                Some(15)
            );
            assert_eq!(ops.legacy_calls, vec![(true, 2)]);
        }

        let mut unrelated = MockLnbOps {
            extended_result: Some(Err(HalError::IoctlFailed {
                backend: "px4",
                path: None,
                op: "PTXT_SET_LNB_VOLTAGE",
                errno: 1,
            })),
            legacy_result: Some(Ok(())),
            ..Default::default()
        };
        assert!(Px4FrontendBackend::set_lnb_voltage_with_ops(&mut unrelated, 15).is_err());
        assert!(unrelated.legacy_calls.is_empty());
    }

    #[test]
    fn hard_extended_failure_and_legacy_failure_return_error() {
        let mut hard = MockLnbOps {
            extended_result: Some(Err(hard_failure())),
            legacy_result: Some(Ok(())),
            ..Default::default()
        };
        assert!(Px4FrontendBackend::set_lnb_voltage_with_ops(&mut hard, 15).is_err());
        assert!(hard.legacy_calls.is_empty());
        let mut legacy_fail = MockLnbOps {
            extended_result: Some(Err(unsupported())),
            legacy_result: Some(Err(hard_failure())),
            ..Default::default()
        };
        assert!(Px4FrontendBackend::set_lnb_voltage_with_ops(&mut legacy_fail, 15).is_err());
        assert_eq!(legacy_fail.legacy_calls, vec![(true, 2)]);
    }
}

#[cfg(test)]
mod px4_device_missing_tests {
    use super::Px4FrontendBackend;
    use maleicacid_tuner_hal_common::HalError;

    #[test]
    fn missing_px4_device_status_returns_error_without_panic() {
        let mut backend = Px4FrontendBackend::new(99_999);
        let err = backend
            .read_status()
            .expect_err("missing px4 device should be an error");
        assert!(matches!(err, HalError::DeviceMissing(_)));
    }

    #[test]
    fn missing_px4_device_lnb_returns_error_without_panic() {
        let mut backend = Px4FrontendBackend::new(99_998);
        let err = backend
            .set_lnb_voltage(15)
            .expect_err("missing px4 device should be an error");
        assert!(matches!(err, HalError::DeviceMissing(_)));
    }
}

fn classify_open_error(path: &std::path::Path, err: &std::io::Error) -> HalError {
    match err.kind() {
        ErrorKind::NotFound => HalError::DeviceMissing(path.to_path_buf()),
        ErrorKind::PermissionDenied => HalError::PermissionDenied {
            path: path.to_path_buf(),
            message: err.to_string(),
        },
        _ if err.raw_os_error() == Some(16) => HalError::Busy {
            path: Some(path.to_path_buf()),
            message: err.to_string(),
        },
        _ => HalError::OpenFailed {
            path: path.to_path_buf(),
            message: err.to_string(),
        },
    }
}
