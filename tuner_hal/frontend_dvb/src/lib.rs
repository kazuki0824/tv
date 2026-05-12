mod explicit_scan;

use explicit_scan::dvb_scan_requests;
use maleicacid_tuner_hal_common::{
    FrontendBackendKind, FrontendDevicePath, FrontendRuntimeState, FrontendScanMode,
    FrontendSelection, FrontendStreamIdKind, FrontendSystem, FrontendTelemetry,
    FrontendTuneRequest, HalError, TsPacketCompletionBuffer, TS_PACKET_SIZE,
};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
const fn ior<T>(typ: u32, nr: u32) -> u64 {
    ioc(IOC_READ, typ, nr, size_of::<T>() as u32)
}
const fn iow<T>(typ: u32, nr: u32) -> u64 {
    ioc(IOC_WRITE, typ, nr, size_of::<T>() as u32)
}

const FE_IOCTL_TYPE: u32 = b'o' as u32;
const FE_SET_PROPERTY: u64 = iow::<DtvProperties>(FE_IOCTL_TYPE, 82);
const FE_GET_PROPERTY: u64 = ior::<DtvProperties>(FE_IOCTL_TYPE, 83);
const FE_READ_STATUS: u64 = ior::<u32>(FE_IOCTL_TYPE, 69);
const FE_READ_SIGNAL_STRENGTH: u64 = ior::<u16>(FE_IOCTL_TYPE, 71);
const FE_READ_SNR: u64 = ior::<u16>(FE_IOCTL_TYPE, 72);
const FE_SET_TONE: u64 = io(FE_IOCTL_TYPE, 66);
const FE_SET_VOLTAGE: u64 = io(FE_IOCTL_TYPE, 67);
const FE_DISEQC_SEND_MASTER_CMD: u64 = iow::<FeDiseqcMasterCmd>(FE_IOCTL_TYPE, 63);

const DMX_SET_PES_FILTER: u64 = iow::<DmxPesFilterParams>(FE_IOCTL_TYPE, 44);
const DMX_SET_SOURCE: u64 = iow::<u32>(FE_IOCTL_TYPE, 49);
const DMX_STOP: u64 = io(FE_IOCTL_TYPE, 42);

const DTV_TUNE: u32 = 1;
const DTV_CLEAR: u32 = 2;
const DTV_FREQUENCY: u32 = 3;
const DTV_BANDWIDTH_HZ: u32 = 5;
const DTV_SYMBOL_RATE: u32 = 8;
const DTV_DELIVERY_SYSTEM: u32 = 17;
const DTV_STREAM_ID: u32 = 42;
const DTV_ENUM_DELSYS: u32 = 44;
const FE_GET_INFO: u64 = ior::<DvbFrontendInfo>(FE_IOCTL_TYPE, 61);

const FE_HAS_SIGNAL: u32 = 0x01;
const FE_HAS_CARRIER: u32 = 0x02;
const FE_HAS_VITERBI: u32 = 0x04;
const FE_HAS_SYNC: u32 = 0x08;
const FE_HAS_LOCK: u32 = 0x10;

const SYS_DVBS2: u32 = 6;
const SYS_ISDBT: u32 = 8;
const SYS_ISDBS: u32 = 9;

const JAPAN_BS_FIRST_IF_HZ: u64 = 1_049_480_000;
const JAPAN_BS_LAST_IF_HZ: u64 = 1_471_440_000;
const JAPAN_BS_STEP_HZ: u64 = 38_360_000;
const JAPAN_CS110_FIRST_IF_HZ: u64 = 1_613_000_000;
const JAPAN_CS110_LAST_IF_HZ: u64 = 2_053_000_000;
const JAPAN_CS110_STEP_HZ: u64 = 40_000_000;

fn is_japan_bs_if_frequency_hz(if_frequency_hz: u64) -> bool {
    if if_frequency_hz < JAPAN_BS_FIRST_IF_HZ || if_frequency_hz > JAPAN_BS_LAST_IF_HZ {
        return false;
    }
    (if_frequency_hz - JAPAN_BS_FIRST_IF_HZ) % JAPAN_BS_STEP_HZ == 0
}

fn is_japan_cs110_if_frequency_hz(if_frequency_hz: u64) -> bool {
    if if_frequency_hz < JAPAN_CS110_FIRST_IF_HZ || if_frequency_hz > JAPAN_CS110_LAST_IF_HZ {
        return false;
    }
    (if_frequency_hz - JAPAN_CS110_FIRST_IF_HZ) % JAPAN_CS110_STEP_HZ == 0
}


const SEC_TONE_ON: u32 = 0;
const SEC_TONE_OFF: u32 = 1;
const SEC_VOLTAGE_13: u32 = 0;
const SEC_VOLTAGE_18: u32 = 1;
const SEC_VOLTAGE_OFF: u32 = 2;
const O_NONBLOCK: i32 = 0x800;
const DMX_IN_FRONTEND: u32 = 0;
const DMX_OUT_TS_TAP: u32 = 2;
const DMX_PES_OTHER: u32 = 20;
const DMX_IMMEDIATE_START: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct DtvPropertyBuffer {
    data: [u8; 32],
    len: u32,
    reserved1: [u32; 3],
    reserved2: *mut core::ffi::c_void,
}

#[repr(C)]
union DtvPropertyUnion {
    data: u32,
    buffer: DtvPropertyBuffer,
}

#[repr(C)]
struct DtvProperty {
    cmd: u32,
    reserved: [u32; 3],
    u: DtvPropertyUnion,
    result: i32,
}

impl DtvProperty {
    fn with_data(cmd: u32, value: u32) -> Self {
        Self {
            cmd,
            reserved: [0; 3],
            u: DtvPropertyUnion { data: value },
            result: 0,
        }
    }
}

#[repr(C)]
struct DtvProperties {
    num: u32,
    props: *mut DtvProperty,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct DmxPesFilterParams {
    pid: u16,
    input: u32,
    output: u32,
    pes_type: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FeDiseqcMasterCmd {
    msg: [u8; 6],
    msg_len: u8,
}

const MAX_DISEQC_MESSAGE_LEN: usize = 6;

#[repr(C)]
#[derive(Clone, Copy)]
struct DvbFrontendInfo {
    name: [u8; 128],
    fe_type: u32,
    frequency_min: u32,
    frequency_max: u32,
    frequency_stepsize: u32,
    frequency_tolerance: u32,
    symbol_rate_min: u32,
    symbol_rate_max: u32,
    symbol_rate_tolerance: u32,
    notifier_delay: u32,
    caps: u32,
}

fn dvb_frontend_name(info: &DvbFrontendInfo) -> String {
    let nul = info
        .name
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(info.name.len());
    String::from_utf8_lossy(&info.name[..nul])
        .trim()
        .to_string()
}

fn sysfs_driver_basename(adapter_id: i32, frontend_index: i32) -> Option<String> {
    let link = PathBuf::from(format!(
        "/sys/class/dvb/dvb{adapter_id}.frontend{frontend_index}/device/driver"
    ));
    std::fs::read_link(link).ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
    })
}

fn is_supported_earth_pt1_frontend_identity(
    _info: &DvbFrontendInfo,
    driver_basename: Option<&str>,
) -> bool {
    matches!(driver_basename, Some("earth-pt1"))
}

#[cfg(test)]
fn is_supported_earth_pt1_frontend_info(info: &DvbFrontendInfo) -> bool {
    is_supported_earth_pt1_frontend_identity(info, Some("earth-pt1"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvbFrontendProbe {
    pub adapter_id: i32,
    pub frontend_index: i32,
    pub demux_index: i32,
    pub dvr_index: i32,
    pub supported_systems: Vec<FrontendSystem>,
    pub min_frequency_raw: i32,
    pub max_frequency_raw: i32,
    pub max_symbol_rate: i32,
}

impl DvbFrontendProbe {
    pub fn normalized_frequency_range_hz(&self, system: FrontendSystem) -> (i64, i64) {
        let scale = match system {
            FrontendSystem::IsdbT => 1i64,
            FrontendSystem::IsdbS => 1_000i64,
            FrontendSystem::IsdbS3 | FrontendSystem::DvbS => 1_000i64,
        };
        let min = i64::from(self.min_frequency_raw) * scale;
        let max = i64::from(self.max_frequency_raw) * scale;
        (min, max)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DvbTuneRequest {
    pub frequency_hz: Option<u32>,
    pub stream_id: Option<u16>,
    pub stream_id_kind: Option<FrontendStreamIdKind>,
    pub bandwidth_hz: Option<u32>,
    pub symbol_rate: Option<u32>,
    pub system: Option<FrontendSystem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DvbFrontendStatus {
    pub telemetry: FrontendTelemetry,
}

#[derive(Debug)]
pub struct DvbLiveStreamReaderState {
    dvr: File,
    dvr_path: PathBuf,
    residual: TsPacketCompletionBuffer,
    malformed_bytes_total: u64,
    stopped: bool,
}

#[derive(Clone, Debug)]
pub struct DvbLiveStreamReader {
    inner: Arc<Mutex<DvbLiveStreamReaderState>>,
}

impl DvbLiveStreamReader {
    pub fn sample_ts_packets(
        &self,
        max_packets: usize,
        stop_fd: Option<i32>,
    ) -> Result<Vec<[u8; TS_PACKET_SIZE]>, HalError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| HalError::Internal("dvb live stream reader mutex poisoned".into()))?;
        let DvbLiveStreamReaderState {
            dvr,
            dvr_path,
            residual,
            malformed_bytes_total,
            stopped,
        } = &mut *inner;
        if *stopped {
            return Ok(Vec::new());
        }
        if !DvbFrontendBackend::poll_reader_ready(dvr.as_raw_fd(), dvr_path.as_path(), stop_fd)? {
            return Ok(Vec::new());
        }
        let mut packets = Vec::new();
        let malformed_before = residual.malformed_bytes();
        let path = dvr_path.clone();
        DvbFrontendBackend::pump_reader_packets(
            dvr,
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
                "maleicacid-tuner-hal-dvb-diagnostic: malformed_ts_bytes={} total_malformed_ts_bytes={}",
                malformed_delta,
                *malformed_bytes_total
            );
        }
        Ok(packets)
    }
}

#[derive(Debug)]
pub struct DvbFrontendBackend {
    selection: FrontendSelection,
    adapter_id: i32,
    frontend_index: i32,
    demux_index: i32,
    dvr_index: i32,
    supported_systems: Vec<FrontendSystem>,
    control: Option<File>,
    ts_demux: Option<File>,
    ts_reader: Option<Arc<Mutex<DvbLiveStreamReaderState>>>,
    last_tune: Option<DvbTuneRequest>,
    state: FrontendRuntimeState,
    telemetry: FrontendTelemetry,
}

impl DvbFrontendBackend {
    pub fn new(
        adapter_id: i32,
        frontend_index: i32,
        demux_index: i32,
        dvr_index: i32,
        supported_systems: Vec<FrontendSystem>,
    ) -> Self {
        Self {
            selection: Self::selection_for(adapter_id, frontend_index),
            adapter_id,
            frontend_index,
            demux_index,
            dvr_index,
            supported_systems,
            control: None,
            ts_demux: None,
            ts_reader: None,
            last_tune: None,
            state: FrontendRuntimeState::default(),
            telemetry: FrontendTelemetry::default(),
        }
    }

    pub fn selection(adapter_id: i32) -> FrontendSelection {
        Self::selection_for(adapter_id, 0)
    }

    pub fn selection_for(adapter_id: i32, frontend_index: i32) -> FrontendSelection {
        FrontendSelection {
            frontend_id: adapter_id,
            backend: FrontendBackendKind::LinuxDvb,
            control_path: FrontendDevicePath::new(Self::control_path(adapter_id, frontend_index)),
        }
    }

    pub fn hardware_info(&self) -> String {
        format!(
            "maleicacid-dvb adapter_id={} frontend_index={} demux_index={} dvr_index={} path={}",
            self.adapter_id,
            self.frontend_index,
            self.demux_index,
            self.dvr_index,
            self.selection.control_path.display()
        )
    }

    pub fn probe_device(&self) -> bool {
        self.selection.control_path.as_path().exists()
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

    pub fn runtime_state(&self) -> &FrontendRuntimeState {
        &self.state
    }
    pub fn supported_systems(&self) -> &[FrontendSystem] {
        &self.supported_systems
    }

    fn validate_stream_id(request: &DvbTuneRequest) -> Result<Option<u16>, HalError> {
        match (request.stream_id, request.stream_id_kind) {
            (None, _) => {
                if matches!(request.system, Some(FrontendSystem::IsdbS)) {
                    let Some(driver_frequency) = request.frequency_hz else {
                        return Err(HalError::InvalidArgument(
                            "ISDB-S tune requires a Japan BS/CS110 IF frequency".into(),
                        ));
                    };
                    let if_frequency_hz = u64::from(driver_frequency).saturating_mul(1_000);
                    if is_japan_bs_if_frequency_hz(if_frequency_hz) {
                        return Err(HalError::InvalidArgument(
                            "BS frequency-only tune is intentionally rejected; specify absolute TSID".into(),
                        ));
                    }
                    if is_japan_cs110_if_frequency_hz(if_frequency_hz) { return Ok(None); }
                    return Err(HalError::InvalidArgument(
                        "ISDB-S 周波数のみの選局は日本 BS では禁止し、CS110 でのみ許可します".into(),
                    ));
                }
                Ok(None)
            },
            (Some(_stream_id), Some(FrontendStreamIdKind::RelativeStreamNumber)) => Err(HalError::InvalidArgument(
                "DVB backend rejects relative stream numbers; use TSID for BS and frequency-only for CS110".into(),
            )),
            (Some(stream_id), Some(FrontendStreamIdKind::AbsoluteStreamId) | None) => {
                if matches!(request.system, Some(FrontendSystem::IsdbS)) {
                    if let Some(driver_frequency) = request.frequency_hz {
                        let if_frequency_hz = u64::from(driver_frequency).saturating_mul(1_000);
                        if is_japan_cs110_if_frequency_hz(if_frequency_hz) {
                            return Err(HalError::InvalidArgument("CS110 does not use TSID frontend selection; tune by frequency only".into()));
                        }
                    }
                }
                if matches!(request.system, Some(FrontendSystem::IsdbS)) {
                    let Some(driver_frequency) = request.frequency_hz else {
                        return Err(HalError::InvalidArgument("ISDB-S tune requires a Japan BS/CS110 IF frequency".into()));
                    };
                    let if_frequency_hz = u64::from(driver_frequency).saturating_mul(1_000);
                    if !is_japan_bs_if_frequency_hz(if_frequency_hz) {
                        return Err(HalError::InvalidArgument("ISDB-S TSID selection is valid only for exact Japan BS IF frequencies".into()));
                    }
                    if stream_id < 12 {
                        return Err(HalError::InvalidArgument("BS STREAM_ID must be an absolute TSID; values 0..11 are relative stream numbers".into()));
                    }
                }
                Ok(Some(stream_id))
            },
        }
    }

    fn normalize_stream_id_from_common(
        request: &FrontendTuneRequest,
    ) -> Result<(Option<u16>, Option<FrontendStreamIdKind>), HalError> {
        let Some(raw_stream_id) = request.stream_id else {
            if matches!(request.system, FrontendSystem::IsdbS) {
                if is_japan_bs_if_frequency_hz(request.frequency) {
                    return Err(HalError::InvalidArgument(
                        "BS frequency-only tune is intentionally rejected; specify absolute TSID"
                            .into(),
                    ));
                }
                if is_japan_cs110_if_frequency_hz(request.frequency) {
                    return Ok((None, None));
                }
                return Err(HalError::InvalidArgument(
                    "ISDB-S 周波数のみの選局は日本 BS では禁止し、CS110 でのみ許可します".into(),
                ));
            }
            return Ok((None, None));
        };
        let stream_id = u16::try_from(raw_stream_id).map_err(|_| {
            HalError::InvalidArgument(format!("stream_id out of range: {raw_stream_id}"))
        })?;
        match request.stream_id_kind {
            Some(FrontendStreamIdKind::RelativeStreamNumber) => Err(HalError::InvalidArgument(
                "DVB backend rejects relative stream numbers; use TSID for BS and frequency-only for CS110".into(),
            )),
            Some(FrontendStreamIdKind::AbsoluteStreamId) | None => {
                if matches!(request.system, FrontendSystem::IsdbS) {
                    if is_japan_cs110_if_frequency_hz(request.frequency) {
                        return Err(HalError::InvalidArgument("CS110 does not use TSID frontend selection; tune by frequency only".into()));
                    }
                    if !is_japan_bs_if_frequency_hz(request.frequency) {
                        return Err(HalError::InvalidArgument("ISDB-S TSID selection is valid only for exact Japan BS IF frequencies".into()));
                    }
                    if stream_id < 12 {
                        return Err(HalError::InvalidArgument("BS STREAM_ID must be an absolute TSID; values 0..11 are relative stream numbers".into()));
                    }
                }
                Ok((Some(stream_id), Some(FrontendStreamIdKind::AbsoluteStreamId)))
            },
        }
    }

    fn validate_dvb_tune_symbol_rate(request: &DvbTuneRequest) -> Result<(), HalError> {
        if request.symbol_rate.is_some() {
            return Err(HalError::InvalidArgument(
                "r51 DVB backend contract does not accept explicit symbol_rate".into(),
            ));
        }
        Ok(())
    }

    fn normalize_dvb_tune_bandwidth(request: &DvbTuneRequest) -> Result<Option<u32>, HalError> {
        match request.system {
            Some(FrontendSystem::IsdbT) => match request.bandwidth_hz {
                None | Some(6_000_000) => Ok(Some(6_000_000)),
                Some(other) => Err(HalError::InvalidArgument(format!(
                    "r51 DVB ISDB-T accepts only 6MHz bandwidth; got {other}Hz"
                ))),
            },
            Some(FrontendSystem::IsdbS) => match request.bandwidth_hz {
                None => Ok(None),
                Some(other) => Err(HalError::InvalidArgument(format!(
                    "r51 DVB ISDB-S does not accept bandwidth_hz; got {other}Hz"
                ))),
            },
            Some(FrontendSystem::IsdbS3 | FrontendSystem::DvbS) | None => Ok(None),
        }
    }

    fn tune_property_pairs(request: &DvbTuneRequest) -> Result<Vec<(u32, u32)>, HalError> {
        Self::validate_dvb_tune_symbol_rate(request)?;
        let delivery = Self::delivery_system(request.system)?;
        let normalized_bandwidth = Self::normalize_dvb_tune_bandwidth(request)?;
        let mut pairs = Vec::new();
        pairs.push((DTV_DELIVERY_SYSTEM, delivery));
        if let Some(freq) = request.frequency_hz {
            pairs.push((DTV_FREQUENCY, freq));
        }
        if let Some(bandwidth_hz) = normalized_bandwidth {
            pairs.push((DTV_BANDWIDTH_HZ, bandwidth_hz));
        }
        // r51 の ISDB-T/ISDB-S backend contract は explicit symbol_rate を扱わない。
        // DTV_SYMBOL_RATE は設定しない。
        if let Some(stream_id) = Self::validate_stream_id(request)? {
            pairs.push((DTV_STREAM_ID, u32::from(stream_id)));
        }
        pairs.push((DTV_TUNE, 0));
        Ok(pairs)
    }

    pub fn tune(&mut self, request: DvbTuneRequest) -> Result<DvbFrontendStatus, HalError> {
        let property_pairs = Self::tune_property_pairs(&request)?;
        self.ensure_control_open()?;
        self.stop_stream_reader();
        self.state.tuning_active = false;
        self.telemetry.locked = false;
        self.clear_properties()?;
        let mut props = property_pairs
            .into_iter()
            .map(|(cmd, value)| DtvProperty::with_data(cmd, value))
            .collect::<Vec<_>>();
        self.ioctl_props(FE_SET_PROPERTY, &mut props, "FE_SET_PROPERTY")?;

        self.state.tuning_active = true;
        self.state.tune_request_count += 1;
        self.state.last_error = None;
        self.telemetry.current_system = request.system;
        self.telemetry.tuned_frequency = request.frequency_hz.map(u64::from);
        self.last_tune = Some(request);
        self.read_status()
    }

    fn validate_symbol_rate_from_common(request: &FrontendTuneRequest) -> Result<(), HalError> {
        if request.symbol_rate.is_some() {
            return Err(HalError::InvalidArgument(
                "r51 ISDB-T/ISDB-S backend contract does not accept explicit symbol_rate".into(),
            ));
        }
        Ok(())
    }

    fn normalize_bandwidth_from_common(
        request: &FrontendTuneRequest,
    ) -> Result<Option<u32>, HalError> {
        match request.system {
            FrontendSystem::IsdbT => match request.bandwidth_hz {
                None | Some(6_000_000) => Ok(Some(6_000_000)),
                Some(other) => Err(HalError::InvalidArgument(format!(
                    "r51 DVB ISDB-T accepts only 6MHz bandwidth; got {other}Hz"
                ))),
            },
            FrontendSystem::IsdbS => match request.bandwidth_hz {
                None => Ok(None),
                Some(other) => Err(HalError::InvalidArgument(format!(
                    "r51 DVB ISDB-S does not accept bandwidth_hz; got {other}Hz"
                ))),
            },
            FrontendSystem::IsdbS3 | FrontendSystem::DvbS => Ok(None),
        }
    }

    pub fn validate_tune_request(&self, request: &FrontendTuneRequest) -> Result<(), HalError> {
        let _ = Self::delivery_system(Some(request.system))?;
        Self::validate_symbol_rate_from_common(request)?;
        let _ = Self::normalize_bandwidth_from_common(request)?;
        let _ = Self::validate_driver_frequency_from_common(request)?;
        let _ = Self::normalize_stream_id_from_common(request)?;
        Ok(())
    }

    pub fn tune_from_common(
        &mut self,
        request: FrontendTuneRequest,
    ) -> Result<DvbFrontendStatus, HalError> {
        self.validate_tune_request(&request)?;
        let requested_frequency = request.frequency;
        let normalized_bandwidth = Self::normalize_bandwidth_from_common(&request)?;
        let driver_frequency = Self::validate_driver_frequency_from_common(&request)?;
        let (stream_id, stream_id_kind) = Self::normalize_stream_id_from_common(&request)?;
        let mut status = self.tune(DvbTuneRequest {
            frequency_hz: Some(driver_frequency),
            stream_id,
            stream_id_kind,
            bandwidth_hz: normalized_bandwidth,
            symbol_rate: None,
            system: Some(request.system),
        })?;
        self.telemetry.tuned_frequency = Some(requested_frequency);
        status.telemetry.tuned_frequency = Some(requested_frequency);
        Ok(status)
    }

    pub fn read_status(&mut self) -> Result<DvbFrontendStatus, HalError> {
        self.ensure_control_open()?;
        let mut status: u32 = 0;
        self.ioctl_ptr(FE_READ_STATUS, &mut status, "FE_READ_STATUS")?;

        let mut signal_strength: u16 = 0;
        let signal_strength = self
            .ioctl_ptr(
                FE_READ_SIGNAL_STRENGTH,
                &mut signal_strength,
                "FE_READ_SIGNAL_STRENGTH",
            )
            .map(|()| u32::from(signal_strength))
            .ok();
        let mut snr: u16 = 0;
        let snr = self
            .ioctl_ptr(FE_READ_SNR, &mut snr, "FE_READ_SNR")
            .map(|()| u32::from(snr))
            .ok();

        self.apply_status_word(status, signal_strength, snr);
        Ok(DvbFrontendStatus {
            telemetry: self.telemetry.clone(),
        })
    }

    fn apply_status_word(&mut self, status: u32, signal_strength: Option<u32>, snr: Option<u32>) {
        self.telemetry.rf_locked = Some((status & FE_HAS_CARRIER) != 0);
        self.telemetry.locked = (status & FE_HAS_LOCK) != 0;
        self.telemetry.signal_strength = signal_strength;
        self.telemetry.cnr = snr;
        self.telemetry.signal_quality = Some(Self::quality_from_status(status));
    }

    pub fn stop_tune(&mut self) -> Result<(), HalError> {
        self.stop_stream_reader();
        self.clear_properties()?;
        self.state.tuning_active = false;
        self.telemetry.locked = false;
        self.telemetry.rf_locked = None;
        Ok(())
    }

    pub fn set_lnb_voltage(&mut self, voltage_mv: i32) -> Result<(), HalError> {
        self.ensure_control_open()?;
        let mode = match voltage_mv {
            v if v <= 0 => SEC_VOLTAGE_OFF,
            11 => SEC_VOLTAGE_13,
            15 => SEC_VOLTAGE_18,
            other => {
                return Err(HalError::InvalidArgument(format!(
                    "earth_pt1 fixed LNB profile accepts only NONE, 11V, or 15V; got {other}V"
                )))
            }
        };
        self.ioctl_word(FE_SET_VOLTAGE, mode, "FE_SET_VOLTAGE")?;
        self.telemetry.lnb_voltage = if mode == SEC_VOLTAGE_OFF {
            None
        } else {
            Some(voltage_mv)
        };
        Ok(())
    }

    pub fn set_lnb_tone(&mut self, _on: bool) -> Result<(), HalError> {
        Err(HalError::Unsupported(
            "LNB tone is permanently unsupported by the fixed Japanese tuner profiles",
        ))
    }

    pub fn send_diseqc_message(&mut self, _message: &[u8]) -> Result<(), HalError> {
        Err(HalError::Unsupported(
            "DiSEqC is permanently unsupported by the fixed Japanese tuner profiles",
        ))
    }

    pub fn live_stream_reader(&mut self) -> Result<Option<DvbLiveStreamReader>, HalError> {
        self.ensure_control_open()?;
        self.ensure_stream_open()?;
        Ok(self.ts_reader.as_ref().map(|inner| DvbLiveStreamReader {
            inner: Arc::clone(inner),
        }))
    }

    fn pump_reader_packets<R, F>(
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
                    if err.kind() == ErrorKind::WouldBlock
                        || err.kind() == ErrorKind::UnexpectedEof
                        || err.kind() == ErrorKind::Interrupted =>
                {
                    break
                }
                Err(err) => {
                    return Err(HalError::Io {
                        backend: "dvb",
                        operation: "dvr_read",
                        path: reader_path.map(Path::to_path_buf),
                        errno: err.raw_os_error(),
                        message: format!("dvb dvr read failed: {}", err),
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
                backend: "dvb",
                operation: "dvr_poll",
                path: Some(path.to_path_buf()),
                errno: None,
                message: format!("dvb dvr poll reported device fd error revents=0x{revents:x}"),
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
                backend: "dvb",
                operation: "dvr_poll",
                path: Some(path.to_path_buf()),
                errno: err.raw_os_error(),
                message: format!("dvb dvr poll failed: {}", err),
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
                    backend: "dvb",
                    operation: "dvr_poll_stop_fd",
                    path: Some(path.to_path_buf()),
                    errno: None,
                    message: "dvb dvr stop fd poll reported error".into(),
                });
            }
        }
        Self::classify_device_revents(path, pollfds[0].revents)
    }

    pub fn close(&mut self) -> Result<(), HalError> {
        self.stop_stream_reader();
        self.control = None;
        self.state.tuning_active = false;
        self.telemetry.locked = false;
        self.telemetry.rf_locked = None;
        Ok(())
    }

    pub fn scan_requests(
        &self,
        base: &FrontendTuneRequest,
        scan_mode: FrontendScanMode,
    ) -> Result<Vec<FrontendTuneRequest>, HalError> {
        let requests = dvb_scan_requests(base, scan_mode)?;
        for request in &requests {
            self.validate_tune_request(request)?;
        }
        Ok(requests)
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

    fn ensure_stream_open(&mut self) -> Result<(), HalError> {
        if self.ts_reader.is_some() && self.ts_demux.is_some() {
            return Ok(());
        }
        let demux_path = self.demux_path();
        let dvr_path = self.dvr_path();
        let demux = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&demux_path)
            .map_err(|e| classify_open_error(&demux_path, &e))?;
        let dvr = OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(&dvr_path)
            .map_err(|e| classify_open_error(&dvr_path, &e))?;
        let mut source = self.frontend_index as u32;
        let source_rc =
            unsafe { ioctl(demux.as_raw_fd(), DMX_SET_SOURCE, &mut source as *mut u32) };
        if source_rc != 0 {
            return Err(HalError::IoctlFailed {
                backend: "dvb",
                path: Some(demux_path.clone()),
                op: "DMX_SET_SOURCE",
                errno: last_errno(),
            });
        }
        let mut params = DmxPesFilterParams {
            pid: 0x2000,
            input: DMX_IN_FRONTEND,
            output: DMX_OUT_TS_TAP,
            pes_type: DMX_PES_OTHER,
            flags: DMX_IMMEDIATE_START,
        };
        let rc = unsafe {
            ioctl(
                demux.as_raw_fd(),
                DMX_SET_PES_FILTER,
                &mut params as *mut DmxPesFilterParams,
            )
        };
        if rc != 0 {
            return Err(HalError::IoctlFailed {
                backend: "dvb",
                path: Some(demux_path.clone()),
                op: "DMX_SET_PES_FILTER",
                errno: last_errno(),
            });
        }
        self.ts_demux = Some(demux);
        self.ts_reader = Some(Arc::new(Mutex::new(DvbLiveStreamReaderState {
            dvr,
            dvr_path,
            residual: TsPacketCompletionBuffer::default(),
            malformed_bytes_total: 0,
            stopped: false,
        })));
        Ok(())
    }

    fn stop_stream_reader(&mut self) {
        if let Some(demux) = self.ts_demux.as_ref() {
            let _ = unsafe { ioctl(demux.as_raw_fd(), DMX_STOP) };
        }
        if let Some(reader) = self.ts_reader.as_ref() {
            if let Ok(mut reader) = reader.lock() {
                reader.stopped = true;
            }
        }
        self.ts_reader = None;
        self.ts_demux = None;
    }

    fn driver_frequency_from_common(request: &FrontendTuneRequest) -> Option<u32> {
        let raw = match request.system {
            FrontendSystem::IsdbT => request.frequency,
            FrontendSystem::IsdbS => request.frequency / 1000,
            FrontendSystem::IsdbS3 | FrontendSystem::DvbS => return None,
        };
        u32::try_from(raw).ok()
    }

    fn validate_driver_frequency_from_common(
        request: &FrontendTuneRequest,
    ) -> Result<u32, HalError> {
        let driver_frequency = Self::driver_frequency_from_common(request).ok_or_else(|| {
            HalError::InvalidArgument(format!(
                "DVB frequency cannot be represented for {:?}: {}",
                request.system, request.frequency
            ))
        })?;
        match request.system {
            FrontendSystem::IsdbT => Ok(driver_frequency),
            FrontendSystem::IsdbS => {
                if is_japan_bs_if_frequency_hz(request.frequency)
                    || is_japan_cs110_if_frequency_hz(request.frequency)
                {
                    Ok(driver_frequency)
                } else {
                    Err(HalError::InvalidArgument(format!("earth_pt1 ISDB-S frequency is outside the supported BS/CS110 IF frequency classes: {}", request.frequency)))
                }
            }
            FrontendSystem::IsdbS3 | FrontendSystem::DvbS => Err(HalError::Unsupported(
                "earth_pt1 backend targets ISDB-T/ISDB-S only",
            )),
        }
    }

    fn clear_properties(&mut self) -> Result<(), HalError> {
        let mut props = [DtvProperty::with_data(DTV_CLEAR, 0)];
        self.ioctl_props(FE_SET_PROPERTY, &mut props, "FE_SET_PROPERTY")
    }

    fn ioctl_props(
        &mut self,
        request: u64,
        props: &mut [DtvProperty],
        op: &'static str,
    ) -> Result<(), HalError> {
        let mut property_set = DtvProperties {
            num: props.len() as u32,
            props: props.as_mut_ptr(),
        };
        let rc = unsafe { ioctl(self.fd()?, request, &mut property_set as *mut DtvProperties) };
        if rc == 0 {
            Ok(())
        } else {
            let err = HalError::IoctlFailed {
                backend: "dvb",
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
                backend: "dvb",
                path: Some(self.selection.control_path.as_path().to_path_buf()),
                op,
                errno: last_errno(),
            };
            self.state.last_error = Some(err.to_string());
            Err(err)
        }
    }

    fn ioctl_word(&mut self, request: u64, data: u32, op: &'static str) -> Result<(), HalError> {
        let rc = unsafe { ioctl(self.fd()?, request, data) };
        if rc == 0 {
            Ok(())
        } else {
            let err = HalError::IoctlFailed {
                backend: "dvb",
                path: Some(self.selection.control_path.as_path().to_path_buf()),
                op,
                errno: last_errno(),
            };
            self.state.last_error = Some(err.to_string());
            Err(err)
        }
    }

    fn fd(&self) -> Result<i32, HalError> {
        self.control
            .as_ref()
            .map(|control| control.as_raw_fd())
            .ok_or_else(|| HalError::InvalidState("dvb control device is not open".into()))
    }

    fn delivery_system(system: Option<FrontendSystem>) -> Result<u32, HalError> {
        Ok(match system {
            Some(FrontendSystem::IsdbT) => SYS_ISDBT,
            Some(FrontendSystem::IsdbS) => SYS_ISDBS,
            Some(FrontendSystem::IsdbS3) | Some(FrontendSystem::DvbS) => {
                return Err(HalError::Unsupported("ISDB-S3/DVB-S は製品対象外です"));
            }
            None => {
                return Err(HalError::InvalidArgument(
                    "dvb delivery system not provided".into(),
                ))
            }
        })
    }

    fn quality_from_status(status: u32) -> u32 {
        let mut score = 0u32;
        if (status & FE_HAS_SIGNAL) != 0 {
            score += 20;
        }
        if (status & FE_HAS_CARRIER) != 0 {
            score += 20;
        }
        if (status & FE_HAS_VITERBI) != 0 {
            score += 20;
        }
        if (status & FE_HAS_SYNC) != 0 {
            score += 20;
        }
        if (status & FE_HAS_LOCK) != 0 {
            score += 20;
        }
        score.min(100)
    }

    fn demux_path(&self) -> PathBuf {
        PathBuf::from(format!(
            "/dev/dvb/adapter{}/demux{}",
            self.adapter_id, self.demux_index
        ))
    }

    fn dvr_path(&self) -> PathBuf {
        PathBuf::from(format!(
            "/dev/dvb/adapter{}/dvr{}",
            self.adapter_id, self.dvr_index
        ))
    }

    fn control_path(adapter_id: i32, frontend_index: i32) -> PathBuf {
        PathBuf::from(format!(
            "/dev/dvb/adapter{adapter_id}/frontend{frontend_index}"
        ))
    }

    fn fallback_systems_from_fe_type(fe_type: u32) -> Vec<FrontendSystem> {
        match fe_type {
            2 => vec![FrontendSystem::IsdbT],
            _ => Vec::new(),
        }
    }

    fn enumerate_adapter_nodes(adapter_path: &Path, prefix: &str) -> Vec<i32> {
        let Ok(entries) = std::fs::read_dir(adapter_path) else {
            return Vec::new();
        };
        let mut indexes = Vec::new();
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            let Some(suffix) = file_name.strip_prefix(prefix) else {
                continue;
            };
            let Ok(index) = suffix.parse::<i32>() else {
                continue;
            };
            indexes.push(index);
        }
        indexes.sort_unstable();
        indexes.dedup();
        indexes
    }

    fn strict_stream_pair_for_frontend(
        frontend_indexes: &[i32],
        demux_indexes: &[i32],
        dvr_indexes: &[i32],
        frontend_index: i32,
    ) -> Option<(i32, i32)> {
        if demux_indexes.contains(&frontend_index) && dvr_indexes.contains(&frontend_index) {
            return Some((frontend_index, frontend_index));
        }
        if frontend_indexes.len() == 1 && demux_indexes.len() == 1 && dvr_indexes.len() == 1 {
            return Some((demux_indexes[0], dvr_indexes[0]));
        }
        None
    }

    pub fn enumerate_probes() -> Vec<DvbFrontendProbe> {
        let mut out = Vec::new();
        let Ok(adapter_dir) = std::fs::read_dir("/dev/dvb") else {
            return out;
        };
        let mut adapters = Vec::new();
        for entry in adapter_dir.flatten() {
            let file_type = match entry.file_type() {
                Ok(file_type) if file_type.is_dir() => file_type,
                _ => continue,
            };
            if !file_type.is_dir() {
                continue;
            }
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            let Some(adapter_suffix) = file_name.strip_prefix("adapter") else {
                continue;
            };
            let Ok(adapter_id) = adapter_suffix.parse::<i32>() else {
                continue;
            };
            adapters.push((adapter_id, entry.path()));
        }
        adapters.sort_by_key(|(adapter_id, _)| *adapter_id);

        for (adapter_id, adapter_path) in adapters {
            let frontend_indexes = Self::enumerate_adapter_nodes(&adapter_path, "frontend");
            let demux_indexes = Self::enumerate_adapter_nodes(&adapter_path, "demux");
            let dvr_indexes = Self::enumerate_adapter_nodes(&adapter_path, "dvr");
            if frontend_indexes.is_empty() || demux_indexes.is_empty() || dvr_indexes.is_empty() {
                continue;
            }

            for frontend_index in frontend_indexes.iter().copied() {
                let Some((demux_index, dvr_index)) = Self::strict_stream_pair_for_frontend(
                    &frontend_indexes,
                    &demux_indexes,
                    &dvr_indexes,
                    frontend_index,
                ) else {
                    continue;
                };
                let path = Self::control_path(adapter_id, frontend_index);
                let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
                    continue;
                };
                let fd = file.as_raw_fd();
                let mut info = DvbFrontendInfo {
                    name: [0; 128],
                    fe_type: 0,
                    frequency_min: 0,
                    frequency_max: 0,
                    frequency_stepsize: 0,
                    frequency_tolerance: 0,
                    symbol_rate_min: 0,
                    symbol_rate_max: 0,
                    symbol_rate_tolerance: 0,
                    notifier_delay: 0,
                    caps: 0,
                };
                let _ = unsafe { ioctl(fd, FE_GET_INFO, &mut info) };
                let driver_basename = sysfs_driver_basename(adapter_id, frontend_index);
                if !is_supported_earth_pt1_frontend_identity(&info, driver_basename.as_deref()) {
                    eprintln!(
                        "maleicacid-dvb: earth_pt1 以外の DVB frontend を無視します adapter={} frontend={} name={} driver={:?}",
                        adapter_id,
                        frontend_index,
                        dvb_frontend_name(&info),
                        driver_basename,
                    );
                    continue;
                }
                let mut prop = DtvProperty {
                    cmd: DTV_ENUM_DELSYS,
                    reserved: [0; 3],
                    u: DtvPropertyUnion {
                        buffer: DtvPropertyBuffer {
                            data: [0; 32],
                            len: 0,
                            reserved1: [0; 3],
                            reserved2: std::ptr::null_mut(),
                        },
                    },
                    result: 0,
                };
                let mut props = DtvProperties {
                    num: 1,
                    props: &mut prop,
                };
                let mut systems = Vec::new();
                let rc = unsafe { ioctl(fd, FE_GET_PROPERTY, &mut props) };
                if rc == 0 {
                    let buffer = unsafe { prop.u.buffer };
                    let count = usize::try_from(buffer.len)
                        .unwrap_or(0)
                        .min(buffer.data.len());
                    for delsys in &buffer.data[..count] {
                        match u32::from(*delsys) {
                            SYS_ISDBT => systems.push(FrontendSystem::IsdbT),
                            SYS_ISDBS => systems.push(FrontendSystem::IsdbS),
                            SYS_DVBS2 => {}
                            _ => {}
                        }
                    }
                }
                if systems.is_empty() {
                    systems = Self::fallback_systems_from_fe_type(info.fe_type);
                }
                systems.retain(|s| matches!(s, FrontendSystem::IsdbT | FrontendSystem::IsdbS));
                systems.sort_by_key(|s| match s {
                    FrontendSystem::IsdbT => 0,
                    FrontendSystem::IsdbS => 1,
                    FrontendSystem::IsdbS3 => 2,
                    FrontendSystem::DvbS => 3,
                });
                systems.dedup();
                if systems.is_empty() {
                    continue;
                }
                out.push(DvbFrontendProbe {
                    adapter_id,
                    frontend_index,
                    demux_index,
                    dvr_index,
                    supported_systems: systems,
                    min_frequency_raw: info.frequency_min as i32,
                    max_frequency_raw: info.frequency_max as i32,
                    max_symbol_rate: info.symbol_rate_max as i32,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {

    use super::{
        DvbFrontendBackend, DvbFrontendProbe, DvbTuneRequest, DTV_BANDWIDTH_HZ,
        DTV_DELIVERY_SYSTEM, DTV_FREQUENCY, DTV_STREAM_ID, DTV_SYMBOL_RATE, SYS_ISDBS, SYS_ISDBT,
    };
    use maleicacid_tuner_hal_common::{
        FrontendScanMode, FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest, HalError,
        TsPacketCompletionBuffer, TS_PACKET_SIZE,
    };
    use std::io::{self, Cursor, Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    #[test]
    fn strict_stream_pair_requires_unambiguous_mapping() {
        assert_eq!(
            DvbFrontendBackend::strict_stream_pair_for_frontend(&[0], &[0], &[0], 0),
            Some((0, 0))
        );
        assert_eq!(
            DvbFrontendBackend::strict_stream_pair_for_frontend(&[0, 1], &[0], &[0], 1),
            None
        );
        assert_eq!(
            DvbFrontendBackend::strict_stream_pair_for_frontend(&[0, 1], &[0, 1], &[0, 1], 1),
            Some((1, 1))
        );
    }

    #[test]
    fn selection_uses_linux_dvb_path() {
        let selection = DvbFrontendBackend::selection(2);
        assert_eq!(selection.frontend_id, 2);
        assert_eq!(
            selection.control_path.display(),
            "/dev/dvb/adapter2/frontend0"
        );
    }

    #[test]
    fn delivery_system_maps_supported_frontends() {
        assert_eq!(
            DvbFrontendBackend::delivery_system(Some(FrontendSystem::IsdbT)).unwrap(),
            SYS_ISDBT
        );
        assert_eq!(
            DvbFrontendBackend::delivery_system(Some(FrontendSystem::IsdbS)).unwrap(),
            SYS_ISDBS
        );
        assert!(DvbFrontendBackend::delivery_system(Some(FrontendSystem::IsdbS3)).is_err());
        assert!(DvbFrontendBackend::delivery_system(Some(FrontendSystem::DvbS)).is_err());
    }

    #[test]
    fn tune_request_preserves_frequency_stream_id_without_symbol_rate() {
        let req = DvbTuneRequest {
            frequency_hz: Some(473_142_857),
            stream_id: Some(31),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
            system: Some(FrontendSystem::IsdbS),
        };
        assert_eq!(req.frequency_hz, Some(473_142_857));
        assert_eq!(req.stream_id, Some(31));
        assert_eq!(
            req.stream_id_kind,
            Some(FrontendStreamIdKind::AbsoluteStreamId)
        );
        assert_eq!(req.symbol_rate, None);
    }

    #[test]
    fn relative_stream_number_is_rejected_for_isdbs_dvb() {
        let req = DvbTuneRequest {
            frequency_hz: Some(1_049_480),
            stream_id: Some(0),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            bandwidth_hz: None,
            symbol_rate: None,
            system: Some(FrontendSystem::IsdbS),
        };
        assert!(DvbFrontendBackend::validate_stream_id(&req).is_err());

        let relative_range_absolute = DvbTuneRequest {
            stream_id: Some(11),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            ..req
        };
        assert!(DvbFrontendBackend::validate_stream_id(&relative_range_absolute).is_err());
    }

    #[test]
    fn relative_stream_number_is_rejected_for_non_satellite_dvb_tune() {
        let req = DvbTuneRequest {
            frequency_hz: Some(473_142_857),
            stream_id: Some(0),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            system: Some(FrontendSystem::IsdbT),
        };
        assert!(DvbFrontendBackend::validate_stream_id(&req).is_err());
    }

    #[test]
    fn unknown_relative_stream_number_is_rejected_for_isdbs() {
        let req = DvbTuneRequest {
            frequency_hz: Some(1_049_480),
            stream_id: Some(7),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            bandwidth_hz: None,
            symbol_rate: None,
            system: Some(FrontendSystem::IsdbS),
        };
        assert!(DvbFrontendBackend::validate_stream_id(&req).is_err());
    }

    #[test]
    fn cs110_dvb_tune_accepts_frequency_only() {
        let frequency_only = DvbTuneRequest {
            frequency_hz: Some(1_613_000),
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
            system: Some(FrontendSystem::IsdbS),
        };
        assert_eq!(
            DvbFrontendBackend::validate_stream_id(&frequency_only).unwrap(),
            None
        );

        let relative = DvbTuneRequest {
            stream_id: Some(0),
            stream_id_kind: Some(FrontendStreamIdKind::RelativeStreamNumber),
            ..frequency_only.clone()
        };
        assert!(DvbFrontendBackend::validate_stream_id(&relative).is_err());

        let absolute = DvbTuneRequest {
            stream_id: Some(0x6020),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            ..frequency_only
        };
        assert!(DvbFrontendBackend::validate_stream_id(&absolute).is_err());
    }

    #[test]
    fn satellite_probe_frequency_range_is_normalized_to_hz() {
        let probe = DvbFrontendProbe {
            adapter_id: 0,
            frontend_index: 0,
            demux_index: 0,
            dvr_index: 0,
            supported_systems: vec![FrontendSystem::IsdbS],
            min_frequency_raw: 950_000,
            max_frequency_raw: 2_150_000,
            max_symbol_rate: 45_000_000,
        };
        assert_eq!(
            probe.normalized_frequency_range_hz(FrontendSystem::IsdbS),
            (950_000_000_i64, 2_150_000_000_i64)
        );
    }

    #[test]
    fn explicit_vts_profile_requests_expand_to_one_dvb_scan_candidate() {
        let backend = DvbFrontendBackend::new(
            0,
            0,
            0,
            0,
            vec![FrontendSystem::IsdbT, FrontendSystem::IsdbS],
        );
        let isdbt = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 557_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        let bs = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: Some(1_049_480_000),
            stream_id: Some(0x4010),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        let cs110 = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_613_000_000,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
        };
        assert_eq!(
            backend
                .scan_requests(&isdbt, FrontendScanMode::Auto)
                .unwrap(),
            vec![isdbt]
        );
        assert_eq!(
            backend.scan_requests(&bs, FrontendScanMode::Auto).unwrap(),
            vec![bs]
        );
        assert_eq!(
            backend
                .scan_requests(&cs110, FrontendScanMode::Auto)
                .unwrap(),
            vec![cs110]
        );
    }

    #[test]
    fn dvb_scan_does_not_generate_japanese_tables() {
        let backend = DvbFrontendBackend::new(0, 0, 0, 0, vec![FrontendSystem::IsdbS]);
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: Some(2_100_000_000),
            stream_id: Some(0x4010),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: None,
            symbol_rate: None,
        };
        assert!(backend
            .scan_requests(&request, FrontendScanMode::Auto)
            .is_err());
        assert!(backend
            .scan_requests(&request, FrontendScanMode::Blind)
            .is_err());
    }

    #[test]
    fn dvb_scan_requests_are_backend_validated_for_bandwidth() {
        let backend = DvbFrontendBackend::new(0, 0, 0, 0, vec![FrontendSystem::IsdbT]);
        let request = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(7_000_000),
            symbol_rate: None,
        };
        assert!(backend
            .scan_requests(&request, FrontendScanMode::Auto)
            .is_err());
    }

    #[test]
    fn ambiguous_satellite_without_enum_delsys_is_not_advertised() {
        assert!(DvbFrontendBackend::fallback_systems_from_fe_type(0).is_empty());
        assert_eq!(
            DvbFrontendBackend::fallback_systems_from_fe_type(2),
            vec![FrontendSystem::IsdbT]
        );
    }

    #[test]
    fn quality_from_status_rewards_lock_progressively() {
        assert_eq!(DvbFrontendBackend::quality_from_status(0), 0);
        assert_eq!(DvbFrontendBackend::quality_from_status(0x01), 20);
        assert_eq!(DvbFrontendBackend::quality_from_status(0x1f), 100);
    }

    #[test]
    fn dvb_frontend_driver_gate_accepts_only_earth_pt1_driver() {
        let mut info = DvbFrontendInfo {
            name: [0; 128],
            fe_type: 0,
            frequency_min: 0,
            frequency_max: 0,
            frequency_stepsize: 0,
            frequency_tolerance: 0,
            symbol_rate_min: 0,
            symbol_rate_max: 0,
            symbol_rate_tolerance: 0,
            notifier_delay: 0,
            caps: 0,
        };
        info.name[.."tc90522 isdb-s".len()].copy_from_slice(b"tc90522 isdb-s");
        assert!(super::is_supported_earth_pt1_frontend_identity(
            &info,
            Some("earth-pt1")
        ));
        assert!(!super::is_supported_earth_pt1_frontend_identity(
            &info,
            Some("generic-dvb")
        ));
        assert!(!super::is_supported_earth_pt1_frontend_identity(
            &info, None
        ));
        info.name = [0; 128];
        info.name[.."generic dvb-s2 frontend".len()].copy_from_slice(b"generic dvb-s2 frontend");
        assert!(super::is_supported_earth_pt1_frontend_identity(
            &info,
            Some("earth-pt1")
        ));
    }

    #[test]
    fn dtv_commands_match_expected_uapi_values() {
        assert_eq!(DTV_FREQUENCY, 3);
        assert_eq!(DTV_BANDWIDTH_HZ, 5);
        assert_eq!(DTV_SYMBOL_RATE, 8);
        assert_eq!(DTV_DELIVERY_SYSTEM, 17);
        assert_eq!(DTV_STREAM_ID, 42);
    }
    #[test]
    fn dvb_backend_bandwidth_contract_rejects_non_6mhz_isdbt_and_satellite_bandwidth() {
        let isdbt = DvbTuneRequest {
            frequency_hz: Some(473_142_857),
            bandwidth_hz: Some(6_000_000),
            system: Some(FrontendSystem::IsdbT),
            ..Default::default()
        };
        assert_eq!(
            DvbFrontendBackend::normalize_dvb_tune_bandwidth(&isdbt).unwrap(),
            Some(6_000_000)
        );

        let isdbt_auto = DvbTuneRequest {
            bandwidth_hz: None,
            ..isdbt.clone()
        };
        assert_eq!(
            DvbFrontendBackend::normalize_dvb_tune_bandwidth(&isdbt_auto).unwrap(),
            Some(6_000_000)
        );

        let isdbt_7mhz = DvbTuneRequest {
            bandwidth_hz: Some(7_000_000),
            ..isdbt.clone()
        };
        assert!(DvbFrontendBackend::normalize_dvb_tune_bandwidth(&isdbt_7mhz).is_err());

        let isdbt_8mhz = DvbTuneRequest {
            bandwidth_hz: Some(8_000_000),
            ..isdbt
        };
        assert!(DvbFrontendBackend::normalize_dvb_tune_bandwidth(&isdbt_8mhz).is_err());

        let isdbs_with_bandwidth = DvbTuneRequest {
            frequency_hz: Some(1_049_480),
            stream_id: Some(0x4010),
            stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
            bandwidth_hz: Some(6_000_000),
            system: Some(FrontendSystem::IsdbS),
            ..Default::default()
        };
        assert!(DvbFrontendBackend::normalize_dvb_tune_bandwidth(&isdbs_with_bandwidth).is_err());
    }

    #[test]
    fn dvb_isdbt_tune_properties_apply_6mhz_bandwidth_and_do_not_set_symbol_rate() {
        let pairs = DvbFrontendBackend::tune_property_pairs(&DvbTuneRequest {
            frequency_hz: Some(473_142_857),
            bandwidth_hz: Some(6_000_000),
            system: Some(FrontendSystem::IsdbT),
            ..Default::default()
        })
        .unwrap();
        assert!(pairs.contains(&(DTV_DELIVERY_SYSTEM, SYS_ISDBT)));
        assert!(pairs.contains(&(DTV_FREQUENCY, 473_142_857)));
        assert!(pairs.contains(&(DTV_BANDWIDTH_HZ, 6_000_000)));
        assert!(!pairs.iter().any(|(cmd, _)| *cmd == DTV_SYMBOL_RATE));
    }

    #[test]
    fn dvb_tune_from_common_rejects_symbol_rate_before_device_access() {
        let mut backend = DvbFrontendBackend::new(-9997, 0, 0, 0, vec![FrontendSystem::IsdbS]);
        let err = backend
            .tune_from_common(FrontendTuneRequest {
                system: FrontendSystem::IsdbS,
                frequency: 1_049_480_000,
                end_frequency: None,
                stream_id: Some(0x4010),
                stream_id_kind: Some(FrontendStreamIdKind::AbsoluteStreamId),
                bandwidth_hz: None,
                symbol_rate: Some(28_860_000),
            })
            .expect_err("symbol_rate contract violation must fail before opening the device");
        assert!(matches!(err, HalError::InvalidArgument(_)));
    }

    #[test]
    fn dvb_tune_rejects_invalid_bandwidth_before_device_access() {
        let mut backend = DvbFrontendBackend::new(-9996, 0, 0, 0, vec![FrontendSystem::IsdbT]);
        let err = backend
            .tune(DvbTuneRequest {
                frequency_hz: Some(473_142_857),
                bandwidth_hz: Some(7_000_000),
                system: Some(FrontendSystem::IsdbT),
                ..Default::default()
            })
            .expect_err("bandwidth contract violation must fail before opening the device");
        assert!(matches!(err, HalError::InvalidArgument(_)));
    }
}

#[cfg(test)]
mod diseqc_tests {
    use super::{DvbFrontendBackend, MAX_DISEQC_MESSAGE_LEN};
    use maleicacid_tuner_hal_common::FrontendSystem;

    #[test]
    fn diseqc_is_permanently_unsupported_before_payload_validation() {
        assert_eq!(MAX_DISEQC_MESSAGE_LEN, 6);
        let mut backend = DvbFrontendBackend::new(-1, 0, 0, 0, vec![FrontendSystem::IsdbS]);
        assert!(backend.send_diseqc_message(&[]).is_err());
        assert!(backend.send_diseqc_message(&[0xff; 8]).is_err());
    }
}

#[cfg(test)]
mod device_missing_tests {
    use super::DvbFrontendBackend;
    use maleicacid_tuner_hal_common::{FrontendSystem, HalError};

    #[test]
    fn missing_dvb_device_returns_error_without_panic() {
        let mut backend = DvbFrontendBackend::new(-9999, 0, 0, 0, vec![FrontendSystem::IsdbT]);
        let err = backend
            .read_status()
            .expect_err("missing device should be an error");
        assert!(matches!(err, HalError::DeviceMissing(_)));
    }

    #[test]
    fn missing_dvb_device_tune_returns_error_without_panic() {
        let mut backend = DvbFrontendBackend::new(-9998, 0, 0, 0, vec![FrontendSystem::IsdbT]);
        let err = backend
            .tune(super::DvbTuneRequest {
                frequency_hz: Some(473_142_857),
                system: Some(FrontendSystem::IsdbT),
                ..Default::default()
            })
            .expect_err("missing device tune should be an error");
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

#[cfg(test)]
mod bs_cs_contract_tests {
    use super::*;
    use maleicacid_tuner_hal_common::{
        FrontendStreamIdKind, FrontendSystem, FrontendTuneRequest, HalError,
    };

    fn bs_base_request(
        stream_id: Option<u32>,
        kind: Option<FrontendStreamIdKind>,
    ) -> FrontendTuneRequest {
        FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_049_480_000,
            end_frequency: None,
            stream_id,
            stream_id_kind: kind,
            bandwidth_hz: None,
            symbol_rate: None,
        }
    }

    fn cs_base_request(
        stream_id: Option<u32>,
        kind: Option<FrontendStreamIdKind>,
    ) -> FrontendTuneRequest {
        FrontendTuneRequest {
            system: FrontendSystem::IsdbS,
            frequency: 1_613_000_000,
            end_frequency: None,
            stream_id,
            stream_id_kind: kind,
            bandwidth_hz: None,
            symbol_rate: None,
        }
    }

    #[test]
    fn dvb_bs_frequency_only_is_rejected() {
        let err = DvbFrontendBackend::normalize_stream_id_from_common(&bs_base_request(None, None))
            .expect_err("BS must not accept frequency-only tune");
        assert!(matches!(err, HalError::InvalidArgument(_)));
    }

    #[test]
    fn dvb_bs_rejects_relative_stream_id_and_accepts_absolute_tsid_without_table_match() {
        assert!(
            DvbFrontendBackend::normalize_stream_id_from_common(&bs_base_request(
                Some(0),
                Some(FrontendStreamIdKind::RelativeStreamNumber),
            ))
            .is_err()
        );
        assert!(
            DvbFrontendBackend::normalize_stream_id_from_common(&bs_base_request(
                Some(11),
                Some(FrontendStreamIdKind::AbsoluteStreamId),
            ))
            .is_err()
        );
        assert_eq!(
            DvbFrontendBackend::normalize_stream_id_from_common(&bs_base_request(
                Some(0x4010),
                Some(FrontendStreamIdKind::AbsoluteStreamId),
            ))
            .unwrap(),
            (Some(0x4010), Some(FrontendStreamIdKind::AbsoluteStreamId)),
        );
        assert_eq!(
            DvbFrontendBackend::normalize_stream_id_from_common(&bs_base_request(
                Some(0x4999),
                Some(FrontendStreamIdKind::AbsoluteStreamId),
            ))
            .unwrap(),
            (Some(0x4999), Some(FrontendStreamIdKind::AbsoluteStreamId)),
        );
        let different_bs_if = FrontendTuneRequest {
            frequency: 1_087_840_000,
            ..bs_base_request(Some(0x4010), Some(FrontendStreamIdKind::AbsoluteStreamId))
        };
        assert!(DvbFrontendBackend::normalize_stream_id_from_common(&different_bs_if).is_ok());
    }

    #[test]
    fn dvb_single_explicit_scan_returns_only_the_given_candidate() {
        let backend = DvbFrontendBackend::new(0, 0, 0, 0, vec![FrontendSystem::IsdbS]);
        let request = bs_base_request(Some(0x4010), Some(FrontendStreamIdKind::AbsoluteStreamId));
        let requests = backend
            .scan_requests(&request, FrontendScanMode::Auto)
            .unwrap();
        assert_eq!(requests, vec![request]);
    }

    #[test]
    fn dvb_backend_rejects_internal_symbol_rate_contract_violation() {
        let backend = DvbFrontendBackend::new(0, 0, 0, 0, vec![FrontendSystem::IsdbS]);
        let request = FrontendTuneRequest {
            symbol_rate: Some(28_860_000),
            ..bs_base_request(Some(0x4010), Some(FrontendStreamIdKind::AbsoluteStreamId))
        };
        assert!(DvbFrontendBackend::validate_symbol_rate_from_common(&request).is_err());
        assert!(backend.validate_tune_request(&request).is_err());
    }

    #[test]
    fn dvb_common_validation_accepts_representable_isdbt_frequency_without_scan_table() {
        let valid_isdbt = FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        assert!(DvbFrontendBackend::validate_driver_frequency_from_common(&valid_isdbt).is_ok());

        let arbitrary_representable_isdbt = FrontendTuneRequest {
            frequency: 90_000_000,
            ..valid_isdbt.clone()
        };
        assert!(DvbFrontendBackend::validate_driver_frequency_from_common(&arbitrary_representable_isdbt).is_ok());

        let unrepresentable = FrontendTuneRequest {
            frequency: u64::from(u32::MAX) + 1,
            ..valid_isdbt
        };
        assert!(
            DvbFrontendBackend::validate_driver_frequency_from_common(&unrepresentable).is_err()
        );
    }

    #[test]
    fn dvb_common_validation_keeps_isdbs_frequency_class_boundary() {
        assert!(
            DvbFrontendBackend::validate_driver_frequency_from_common(&bs_base_request(
                Some(0x4010),
                Some(FrontendStreamIdKind::AbsoluteStreamId),
            ))
            .is_ok()
        );
        assert!(
            DvbFrontendBackend::validate_driver_frequency_from_common(&cs_base_request(None, None))
                .is_ok()
        );
        let invalid = FrontendTuneRequest {
            frequency: 1_500_000_000,
            ..cs_base_request(None, None)
        };
        assert!(DvbFrontendBackend::validate_driver_frequency_from_common(&invalid).is_err());
        let near_bs = FrontendTuneRequest {
            frequency: 1_049_480_001,
            ..bs_base_request(Some(0x4010), Some(FrontendStreamIdKind::AbsoluteStreamId))
        };
        assert!(DvbFrontendBackend::validate_driver_frequency_from_common(&near_bs).is_err());
        let near_cs = FrontendTuneRequest {
            frequency: 1_613_000_001,
            ..cs_base_request(None, None)
        };
        assert!(DvbFrontendBackend::validate_driver_frequency_from_common(&near_cs).is_err());
    }

    #[test]
    fn dvb_cs110_uses_frequency_only() {
        assert_eq!(
            DvbFrontendBackend::normalize_stream_id_from_common(&cs_base_request(None, None))
                .unwrap(),
            (None, None)
        );
        assert!(
            DvbFrontendBackend::normalize_stream_id_from_common(&cs_base_request(
                Some(1),
                Some(FrontendStreamIdKind::RelativeStreamNumber),
            ))
            .is_err()
        );
        assert!(
            DvbFrontendBackend::normalize_stream_id_from_common(&cs_base_request(
                Some(0x5001),
                Some(FrontendStreamIdKind::AbsoluteStreamId),
            ))
            .is_err()
        );
    }
}

#[cfg(test)]
mod status_word_tests {
    use super::*;
    use std::io::{self, Cursor, Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    #[test]
    fn status_word_maps_carrier_to_rf_lock_and_lock_to_demod_lock() {
        let mut backend = DvbFrontendBackend::new(0, 0, 0, 0, vec![FrontendSystem::IsdbS]);
        backend.apply_status_word(FE_HAS_CARRIER, Some(123), Some(45));
        assert_eq!(backend.telemetry.rf_locked, Some(true));
        assert!(!backend.telemetry.locked);
        assert_eq!(backend.telemetry.signal_strength, Some(123));
        assert_eq!(backend.telemetry.cnr, Some(45));

        backend.apply_status_word(FE_HAS_CARRIER | FE_HAS_LOCK, Some(321), Some(54));
        assert_eq!(backend.telemetry.rf_locked, Some(true));
        assert!(backend.telemetry.locked);
    }

    #[test]
    fn status_word_distinguishes_no_carrier_from_no_status_support() {
        let mut backend = DvbFrontendBackend::new(0, 0, 0, 0, vec![FrontendSystem::IsdbS]);
        assert_eq!(backend.telemetry.rf_locked, None);
        backend.apply_status_word(FE_HAS_LOCK, 0, 0);
        assert_eq!(backend.telemetry.rf_locked, Some(false));
        assert!(backend.telemetry.locked);
    }

    fn make_test_ts_packet(pid: u16, fill: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [fill; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) & 0x1f) as u8;
        packet[2] = (pid & 0xff) as u8;
        packet[3] = 0x10;
        packet
    }

    struct WouldBlockReader;

    impl Read for WouldBlockReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "not ready"))
        }
    }

    #[test]
    fn dvb_poll_reader_ready_is_woken_by_stop_fd() {
        let (mut device_writer, device_reader) = UnixStream::pair().unwrap();
        let (mut stop_writer, stop_reader) = UnixStream::pair().unwrap();
        device_reader.set_nonblocking(true).unwrap();
        stop_reader.set_nonblocking(true).unwrap();

        stop_writer.write_all(&[1]).unwrap();
        assert!(!DvbFrontendBackend::poll_reader_ready(
            device_reader.as_raw_fd(),
            std::path::Path::new("/dev/dvb/test"),
            Some(stop_reader.as_raw_fd())
        )
        .unwrap());

        device_writer.write_all(&[0x47]).unwrap();
        assert!(DvbFrontendBackend::poll_reader_ready(
            device_reader.as_raw_fd(),
            std::path::Path::new("/dev/dvb/test"),
            None
        )
        .unwrap());
    }

    #[test]
    fn dvb_device_poll_error_revents_are_fatal() {
        let path = std::path::Path::new("/dev/dvb/test");
        assert!(DvbFrontendBackend::classify_device_revents(path, POLLERR).is_err());
        assert!(DvbFrontendBackend::classify_device_revents(path, POLLHUP).is_err());
        assert!(DvbFrontendBackend::classify_device_revents(path, POLLNVAL).is_err());
        assert_eq!(
            DvbFrontendBackend::classify_device_revents(path, 0).unwrap(),
            false
        );
        assert_eq!(
            DvbFrontendBackend::classify_device_revents(path, POLLIN).unwrap(),
            true
        );
    }

    #[test]
    fn stopped_dvb_live_stream_reader_does_not_emit_old_packets() {
        let reader = DvbLiveStreamReader {
            inner: std::sync::Arc::new(std::sync::Mutex::new(DvbLiveStreamReaderState {
                dvr: std::fs::File::open("/dev/null").unwrap(),
                dvr_path: std::path::PathBuf::from("/dev/dvb/old"),
                residual: TsPacketCompletionBuffer::default(),
                malformed_bytes_total: 0,
                stopped: true,
            })),
        };
        assert!(reader.sample_ts_packets(1, None).unwrap().is_empty());
    }

    #[test]
    fn dvb_reader_pump_assembles_split_ts_packet() {
        let packet = make_test_ts_packet(0x0123, 0x55);
        let mut residual = TsPacketCompletionBuffer::default();
        let mut out = Vec::new();

        let mut first = Cursor::new(packet[..1].to_vec());
        assert_eq!(
            DvbFrontendBackend::pump_reader_packets(&mut first, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            0
        );
        assert!(out.is_empty());
        assert_eq!(residual.tail_len(), 1);

        let mut second = Cursor::new(packet[1..].to_vec());
        assert_eq!(
            DvbFrontendBackend::pump_reader_packets(&mut second, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            1
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], packet);
        assert_eq!(residual.tail_len(), 0);
    }

    #[test]
    fn dvb_reader_pump_keeps_over_budget_completed_packets_for_next_call() {
        let first = make_test_ts_packet(0x0126, 0x88);
        let second = make_test_ts_packet(0x0127, 0x99);
        let mut input = Vec::new();
        input.extend_from_slice(&first);
        input.extend_from_slice(&second);
        let mut residual = TsPacketCompletionBuffer::default();
        let mut out = Vec::new();

        let mut reader = Cursor::new(input);
        assert_eq!(
            DvbFrontendBackend::pump_reader_packets(&mut reader, None, 1, &mut residual, |pkt| out
                .push(*pkt))
            .unwrap(),
            1
        );
        assert_eq!(out, vec![first]);

        let mut empty = Cursor::new(Vec::<u8>::new());
        assert_eq!(
            DvbFrontendBackend::pump_reader_packets(&mut empty, None, 1, &mut residual, |pkt| out
                .push(*pkt))
            .unwrap(),
            1
        );
        assert_eq!(out, vec![first, second]);
    }

    #[test]
    fn dvb_reader_pump_keeps_tail_and_treats_would_block_as_nonfatal() {
        let packet = make_test_ts_packet(0x0124, 0x66);
        let mut residual = TsPacketCompletionBuffer::default();
        let mut out = Vec::new();

        let mut partial = Cursor::new(packet[..100].to_vec());
        assert_eq!(
            DvbFrontendBackend::pump_reader_packets(&mut partial, None, 1, &mut residual, |pkt| {
                out.push(pkt.to_vec())
            })
            .unwrap(),
            0
        );
        assert!(out.is_empty());
        assert_eq!(residual.tail_len(), 100);

        let mut blocked = WouldBlockReader;
        assert_eq!(
            DvbFrontendBackend::pump_reader_packets(&mut blocked, None, 1, &mut residual, |pkt| {
                out.push(pkt.to_vec())
            })
            .unwrap(),
            0
        );
        assert!(out.is_empty());
        assert_eq!(residual.tail_len(), 100);

        let mut rest = Cursor::new(packet[100..].to_vec());
        assert_eq!(
            DvbFrontendBackend::pump_reader_packets(&mut rest, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            1
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], packet);
    }

    #[test]
    fn dvb_reader_pump_drops_malformed_full_packet_with_diagnostic_state() {
        let mut malformed = vec![0x11u8; TS_PACKET_SIZE * 3];
        malformed[1] = 0x22;
        let mut residual = TsPacketCompletionBuffer::default();
        let mut out = Vec::new();

        let mut reader = Cursor::new(malformed);
        assert_eq!(
            DvbFrontendBackend::pump_reader_packets(&mut reader, None, 1, &mut residual, |pkt| out
                .push(pkt.to_vec()))
            .unwrap(),
            0
        );
        assert!(out.is_empty());
        assert_eq!(residual.tail_len(), TS_PACKET_SIZE * 2);
        assert!(residual.malformed_bytes() >= TS_PACKET_SIZE as u64);
    }
}
