use std::io::{BufRead, Write};
use std::os::fd::AsRawFd;
use std::thread;
use std::time::{Duration, Instant};

use android_hardware_common_fmq::aidl::android::hardware::common::fmq::{
    MQDescriptor::MQDescriptor, SynchronizedReadWrite::SynchronizedReadWrite,
};
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    DemuxFilterEvent::DemuxFilterEvent, DemuxFilterMainType::DemuxFilterMainType,
    DemuxFilterSectionSettings::DemuxFilterSectionSettings,
    DemuxFilterSectionSettingsCondition::DemuxFilterSectionSettingsCondition,
    DemuxFilterSectionSettingsConditionTableInfo::DemuxFilterSectionSettingsConditionTableInfo,
    DemuxFilterSettings::DemuxFilterSettings, DemuxFilterStatus::DemuxFilterStatus,
    DemuxFilterSubType::DemuxFilterSubType, DemuxFilterType::DemuxFilterType,
    DemuxTsFilterSettings::DemuxTsFilterSettings,
    DemuxTsFilterSettingsFilterSettings::DemuxTsFilterSettingsFilterSettings,
    DemuxTsFilterType::DemuxTsFilterType, FrontendEventType::FrontendEventType,
    FrontendIsdbsCoderate::FrontendIsdbsCoderate,
    FrontendIsdbsModulation::FrontendIsdbsModulation,
    FrontendIsdbsRolloff::FrontendIsdbsRolloff,
    FrontendIsdbsSettings::FrontendIsdbsSettings,
    FrontendIsdbsStreamIdType::FrontendIsdbsStreamIdType,
    FrontendIsdbtBandwidth::FrontendIsdbtBandwidth,
    FrontendIsdbtCoderate::FrontendIsdbtCoderate,
    FrontendIsdbtGuardInterval::FrontendIsdbtGuardInterval,
    FrontendIsdbtLayerSettings::FrontendIsdbtLayerSettings,
    FrontendIsdbtMode::FrontendIsdbtMode,
    FrontendIsdbtModulation::FrontendIsdbtModulation,
    FrontendIsdbtPartialReceptionFlag::FrontendIsdbtPartialReceptionFlag,
    FrontendIsdbtSettings::FrontendIsdbtSettings,
    FrontendIsdbtTimeInterleaveMode::FrontendIsdbtTimeInterleaveMode,
    FrontendScanMessage::FrontendScanMessage,
    FrontendScanMessageType::FrontendScanMessageType,
    FrontendSettings::FrontendSettings, FrontendSpectralInversion::FrontendSpectralInversion,
    FrontendStatus::FrontendStatus, FrontendStatusType::FrontendStatusType,
    FrontendType::FrontendType, IDemux::IDemux,
    IFilterCallback::{BnFilterCallback, IFilterCallback}, IFrontend::IFrontend,
    IFrontendCallback::{BnFrontendCallback, IFrontendCallback}, ITuner::ITuner,
};
use binder::{BinderFeatures, Interface, Strong};
use serde_json::{json, Value};

const TUNER_SERVICE: &str = "android.hardware.tv.tuner.ITuner/default";
const FILTER_BUFFER_BYTES: i32 = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const QUIET_AFTER_DATA: Duration = Duration::from_millis(200);

#[repr(C)]
struct FmqGrantor {
    fd_index: i32,
    offset: i32,
    extent: i64,
}

#[repr(C)]
struct ImportedFmq {
    _private: [u8; 0],
}

extern "C" {
    fn vts_agent_fmq_import(
        quantum: i32,
        flags: i32,
        grantors: *const FmqGrantor,
        grantor_count: usize,
        fds: *const i32,
        fd_count: usize,
        ints: *const i32,
        int_count: usize,
    ) -> *mut ImportedFmq;
    fn vts_agent_fmq_destroy(queue: *mut ImportedFmq);
    fn vts_agent_fmq_available_to_read(queue: *const ImportedFmq) -> usize;
    fn vts_agent_fmq_read(queue: *mut ImportedFmq, data: *mut u8, size: usize) -> usize;
}

struct FmqReader(*mut ImportedFmq);

impl Drop for FmqReader {
    fn drop(&mut self) {
        unsafe { vts_agent_fmq_destroy(self.0) };
    }
}

impl FmqReader {
    fn import(desc: &MQDescriptor<i8, SynchronizedReadWrite>) -> Result<Self, String> {
        let grantors = desc
            .grantors
            .iter()
            .map(|g| FmqGrantor {
                fd_index: g.fdIndex,
                offset: g.offset,
                extent: g.extent,
            })
            .collect::<Vec<_>>();
        let fds = desc
            .handle
            .fds
            .iter()
            .map(AsRawFd::as_raw_fd)
            .collect::<Vec<_>>();
        let ints = &desc.handle.ints;
        let queue = unsafe {
            vts_agent_fmq_import(
                desc.quantum,
                desc.flags,
                grantors.as_ptr(),
                grantors.len(),
                fds.as_ptr(),
                fds.len(),
                ints.as_ptr(),
                ints.len(),
            )
        };
        if queue.is_null() {
            Err("failed to import filter FMQ".to_string())
        } else {
            Ok(Self(queue))
        }
    }

    fn available(&self) -> usize {
        unsafe { vts_agent_fmq_available_to_read(self.0) }
    }

    fn read_available(&mut self) -> Result<Vec<u8>, String> {
        let available = self.available();
        if available == 0 {
            return Ok(Vec::new());
        }
        let mut bytes = vec![0u8; available];
        let read = unsafe { vts_agent_fmq_read(self.0, bytes.as_mut_ptr(), bytes.len()) };
        if read == 0 {
            return Err("filter FMQ read failed".to_string());
        }
        bytes.truncate(read);
        Ok(bytes)
    }
}

#[derive(Default)]
struct FrontendCallback;

impl Interface for FrontendCallback {}

impl IFrontendCallback for FrontendCallback {
    fn onEvent(&self, _event: FrontendEventType) -> binder::Result<()> {
        Ok(())
    }

    fn onScanMessage(
        &self,
        _kind: FrontendScanMessageType,
        _message: &FrontendScanMessage,
    ) -> binder::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct FilterCallback;

impl Interface for FilterCallback {}

impl IFilterCallback for FilterCallback {
    fn onFilterEvent(&self, _events: &[DemuxFilterEvent]) -> binder::Result<()> {
        Ok(())
    }

    fn onFilterStatus(&self, _status: DemuxFilterStatus) -> binder::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct Args {
    delivery_system: String,
    frequency_hz: i64,
    timeout_ms: u64,
    stream_id: i32,
    stream_id_type: i32,
    symbol_rate: i32,
    modulation: i32,
    coderate: i32,
    rolloff: i32,
}

fn parse_i64(text: &str, label: &str) -> Result<i64, String> {
    text.parse::<i64>()
        .map_err(|_| format!("{label} is not an integer"))
}

fn parse_args() -> Result<Args, String> {
    let mut values = std::collections::BTreeMap::<String, String>::new();
    let mut args = std::env::args().skip(1);
    while let Some(key) = args.next() {
        if !key.starts_with("--") {
            return Err(format!("unexpected argument: {key}"));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{key} requires a value"))?;
        values.insert(key, value);
    }

    let delivery_system = values
        .remove("--delivery-system")
        .ok_or("--delivery-system is required")?;
    if delivery_system != "ISDBT" && delivery_system != "ISDBS" {
        return Err("--delivery-system must be ISDBT or ISDBS".to_string());
    }
    let frequency_hz = parse_i64(
        &values
            .remove("--frequency-hz")
            .ok_or("--frequency-hz is required")?,
        "frequency",
    )?;
    if frequency_hz <= 0 {
        return Err("frequency must be positive".to_string());
    }
    let timeout_ms = values
        .remove("--timeout-ms")
        .map(|v| parse_i64(&v, "timeout-ms"))
        .transpose()?
        .unwrap_or(5000);
    if !(1..=60000).contains(&timeout_ms) {
        return Err("timeout-ms must be in 1..60000".to_string());
    }

    let mut opt = |name: &str| -> Result<i32, String> {
        match values.remove(name) {
            Some(v) => i32::try_from(parse_i64(&v, name)?)
                .map_err(|_| format!("{name} is outside i32 range")),
            None => Ok(0),
        }
    };
    let stream_id = opt("--stream-id")?;
    let stream_id_type = opt("--stream-id-type")?;
    let symbol_rate = opt("--symbol-rate")?;
    let modulation = opt("--modulation")?;
    let coderate = opt("--coderate")?;
    let rolloff = opt("--rolloff")?;
    if !values.is_empty() {
        return Err(format!(
            "unknown argument: {}",
            values.keys().next().expect("non-empty map")
        ));
    }

    Ok(Args {
        delivery_system,
        frequency_hz,
        timeout_ms: timeout_ms as u64,
        stream_id,
        stream_id_type,
        symbol_rate,
        modulation,
        coderate,
        rolloff,
    })
}

fn frontend_type(args: &Args) -> FrontendType {
    if args.delivery_system == "ISDBT" {
        FrontendType::ISDBT
    } else {
        FrontendType::ISDBS
    }
}

fn frontend_settings(args: &Args) -> FrontendSettings {
    if args.delivery_system == "ISDBT" {
        FrontendSettings::Isdbt(FrontendIsdbtSettings {
            frequency: args.frequency_hz,
            inversion: FrontendSpectralInversion::UNDEFINED,
            bandwidth: FrontendIsdbtBandwidth::AUTO,
            mode: FrontendIsdbtMode::AUTO,
            guardInterval: FrontendIsdbtGuardInterval::AUTO,
            serviceAreaId: 0,
            partialReceptionFlag: FrontendIsdbtPartialReceptionFlag::UNDEFINED,
            layerSettings: vec![FrontendIsdbtLayerSettings {
                modulation: FrontendIsdbtModulation::AUTO,
                coderate: FrontendIsdbtCoderate::AUTO,
                timeInterleave: FrontendIsdbtTimeInterleaveMode::AUTO,
                numOfSegment: 0,
            }],
            ..Default::default()
        })
    } else {
        FrontendSettings::Isdbs(FrontendIsdbsSettings {
            frequency: args.frequency_hz,
            streamId: args.stream_id,
            streamIdType: FrontendIsdbsStreamIdType(args.stream_id_type),
            symbolRate: args.symbol_rate,
            modulation: FrontendIsdbsModulation(args.modulation),
            coderate: FrontendIsdbsCoderate(args.coderate),
            rolloff: FrontendIsdbsRolloff(args.rolloff),
            ..Default::default()
        })
    }
}

fn section_filter_settings(pid: i32, table_id: i32) -> (DemuxFilterType, DemuxFilterSettings) {
    let ty = DemuxFilterType {
        mainType: DemuxFilterMainType::TS,
        subType: DemuxFilterSubType::TsFilterType(DemuxTsFilterType::SECTION),
    };
    let section = DemuxFilterSectionSettings {
        condition: DemuxFilterSectionSettingsCondition::TableInfo(
            DemuxFilterSectionSettingsConditionTableInfo {
                tableId: table_id,
                version: -1,
            },
        ),
        isCheckCrc: true,
        isRepeat: false,
        isRaw: false,
        bitWidthOfLengthField: 12,
    };
    let settings = DemuxFilterSettings::Ts(DemuxTsFilterSettings {
        tpid: pid,
        filterSettings: DemuxTsFilterSettingsFilterSettings::Section(section),
    });
    (ty, settings)
}

fn wait_for_lock(frontend: &Strong<dyn IFrontend>, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let statuses = frontend
            .getStatus(&[FrontendStatusType::DEMOD_LOCK])
            .map_err(|e| format!("getStatus failed: {e:?}"))?;
        if statuses
            .iter()
            .any(|s| matches!(s, FrontendStatus::IsDemodLocked(true)))
        {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }
    Err("frontend did not reach LOCKED".to_string())
}

fn hex_lower(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(H[(byte >> 4) as usize] as char);
        out.push(H[(byte & 15) as usize] as char);
    }
    out
}

struct DeviceSession {
    args: Args,
    frontend: Strong<dyn IFrontend>,
    demux: Strong<dyn IDemux>,
}

impl DeviceSession {
    fn open(args: Args) -> Result<Self, String> {
        binder::ProcessState::start_thread_pool();
        let tuner: Strong<dyn ITuner> = binder::wait_for_interface(TUNER_SERVICE)
            .map_err(|e| format!("Tuner AIDL service is unavailable: {e:?}"))?;
        let ids = tuner
            .getFrontendIds()
            .map_err(|e| format!("getFrontendIds failed: {e:?}"))?;
        let wanted = frontend_type(&args);
        let frontend_id = ids
            .into_iter()
            .find(|id| {
                tuner
                    .getFrontendInfo(*id)
                    .map(|info| info.r#type == wanted)
                    .unwrap_or(false)
            })
            .ok_or_else(|| "no matching frontend".to_string())?;
        let frontend = tuner
            .openFrontendById(frontend_id)
            .map_err(|e| format!("openFrontendById failed: {e:?}"))?;
        let callback = BnFrontendCallback::new_binder(FrontendCallback, BinderFeatures::default());
        if let Err(error) = frontend.setCallback(&callback) {
            let _ = frontend.close();
            return Err(format!("setCallback failed: {error:?}"));
        }
        if let Err(error) = frontend.tune(&frontend_settings(&args)) {
            let _ = frontend.close();
            return Err(format!("tune failed: {error:?}"));
        }
        if let Err(error) = wait_for_lock(&frontend, Duration::from_millis(args.timeout_ms)) {
            let _ = frontend.stopTune();
            let _ = frontend.close();
            return Err(error);
        }

        let demux_ids = match tuner.getDemuxIds() {
            Ok(ids) => ids,
            Err(error) => {
                let _ = frontend.stopTune();
                let _ = frontend.close();
                return Err(format!("getDemuxIds failed: {error:?}"));
            }
        };
        let demux_id = match demux_ids.first() {
            Some(id) => *id,
            None => {
                let _ = frontend.stopTune();
                let _ = frontend.close();
                return Err("no demux available".to_string());
            }
        };
        let demux = match tuner.openDemuxById(demux_id) {
            Ok(demux) => demux,
            Err(error) => {
                let _ = frontend.stopTune();
                let _ = frontend.close();
                return Err(format!("openDemuxById failed: {error:?}"));
            }
        };
        if let Err(error) = demux.setFrontendDataSource(frontend_id) {
            let _ = demux.close();
            let _ = frontend.stopTune();
            let _ = frontend.close();
            return Err(format!("setFrontendDataSource failed: {error:?}"));
        }

        Ok(Self {
            args,
            frontend,
            demux,
        })
    }

    fn read_section(&self, pid: i32, table_id: i32) -> Result<Vec<u8>, String> {
        if !(0..=0x1fff).contains(&pid) {
            return Err("pid must be in 0..8191".to_string());
        }
        if !(0..=0xff).contains(&table_id) {
            return Err("table_id must be in 0..255".to_string());
        }

        let (filter_type, settings) = section_filter_settings(pid, table_id);
        let callback = BnFilterCallback::new_binder(FilterCallback, BinderFeatures::default());
        let filter = self
            .demux
            .openFilter(&filter_type, FILTER_BUFFER_BYTES, &callback)
            .map_err(|e| format!("openFilter failed: {e:?}"))?;

        let result = (|| {
            filter
                .configure(&settings)
                .map_err(|e| format!("configure section filter failed: {e:?}"))?;
            let mut desc = MQDescriptor::<i8, SynchronizedReadWrite>::default();
            filter
                .getQueueDesc(&mut desc)
                .map_err(|e| format!("getQueueDesc failed: {e:?}"))?;
            let mut queue = FmqReader::import(&desc)?;
            filter
                .start()
                .map_err(|e| format!("start section filter failed: {e:?}"))?;

            let deadline = Instant::now() + Duration::from_millis(self.args.timeout_ms);
            let mut bytes = Vec::new();
            let mut last_data_at: Option<Instant> = None;
            while Instant::now() < deadline {
                if queue.available() != 0 {
                    let chunk = queue.read_available()?;
                    if !chunk.is_empty() {
                        bytes.extend_from_slice(&chunk);
                        last_data_at = Some(Instant::now());
                    }
                } else if last_data_at
                    .map(|instant| instant.elapsed() >= QUIET_AFTER_DATA)
                    .unwrap_or(false)
                {
                    break;
                }
                thread::sleep(POLL_INTERVAL);
            }
            if bytes.is_empty() {
                Err("section filter produced no payload".to_string())
            } else {
                Ok(bytes)
            }
        })();

        let _ = filter.stop();
        let _ = filter.close();
        result
    }
}

impl Drop for DeviceSession {
    fn drop(&mut self) {
        let _ = self.demux.close();
        let _ = self.frontend.stopTune();
        let _ = self.frontend.close();
    }
}

fn write_response(value: Value) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value).map_err(|e| e.to_string())?;
    stdout.write_all(b"\n").map_err(|e| e.to_string())?;
    stdout.flush().map_err(|e| e.to_string())
}

fn handle_request(session: &DeviceSession, request: &Value) -> Result<bool, String> {
    let op = request
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| "request.op is required".to_string())?;
    match op {
        "section" => {
            let pid = request
                .get("pid")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| "section request pid must be an integer".to_string())?;
            let table_id = request
                .get("table_id")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| "section request table_id must be an integer".to_string())?;
            let bytes = session.read_section(pid, table_id)?;
            write_response(json!({
                "status": "ok",
                "pid": pid,
                "table_id": table_id,
                "payload_hex": hex_lower(&bytes),
            }))?;
            Ok(true)
        }
        "close" => {
            write_response(json!({"status": "closed"}))?;
            Ok(false)
        }
        _ => Err(format!("unsupported operation: {op}")),
    }
}

fn run(args: Args) -> Result<(), String> {
    let session = DeviceSession::open(args)?;
    write_response(json!({
        "status": "ready",
        "frequency_hz": session.args.frequency_hz,
    }))?;

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("stdin read failed: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                write_response(json!({
                    "status": "error",
                    "message": format!("malformed request JSON: {error}"),
                }))?;
                continue;
            }
        };
        match handle_request(&session, &request) {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => {
                write_response(json!({"status": "error", "message": error}))?;
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = parse_args().and_then(run) {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}
