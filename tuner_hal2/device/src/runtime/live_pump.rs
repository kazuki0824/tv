//! frontend live TS pump の中核。
//!
//! descriptorだけのlive readerモデルを置き換える実装である。pumpはread loopとTS packet再同期を所有する。
//! 明示的なpacket sinkを必須とし、demux bindingなしで完了に見える無処理成功sinkは提供しない。

use std::io::{self, Read};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use maleicacid_tuner_hal2_common::{
    retry_after_interrupted_read_with_saturation, HalError, HalErrorDetail, HalInternalKind,
    TS_PACKET_SIZE, TsPacketCompletionBuffer,
};

pub trait FrontendLivePacketSink: Send {
    fn deliver_ts_packet(&mut self, packet: &[u8; TS_PACKET_SIZE]) -> Result<(), HalError>;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FrontendLivePumpReport {
    pub packets_delivered: u64,
    pub malformed_bytes: u64,
    pub read_retries: u64,
    pub read_retry_counter_saturated: bool,
    pub stopped_by_cancel: bool,
    pub reached_eof: bool,
}

impl FrontendLivePumpReport {
    fn add_packets(&mut self, amount: usize) -> Result<(), HalError> {
        let amount = u64::try_from(amount).unwrap_or(u64::MAX);
        self.packets_delivered = self.packets_delivered.checked_add(amount).ok_or_else(|| {
            HalError::cleanup_failed("frontend live pump", "delivered packet counter overflow")
        })?;
        Ok(())
    }

    fn add_malformed(&mut self, amount: u64) {
        self.malformed_bytes = self.malformed_bytes.saturating_add(amount);
    }
}

#[derive(Debug)]
pub enum FrontendLivePumpJoinOutcome {
    Running,
    Completed(Result<FrontendLivePumpReport, HalError>),
}

impl core::fmt::Debug for FrontendLivePumpOwner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrontendLivePumpOwner")
            .field("cancelled", &self.cancel.load(Ordering::SeqCst))
            .field("join_present", &self.join.is_some())
            .finish()
    }
}

pub struct FrontendLivePumpOwner {
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<FrontendLivePumpReport, HalError>>>>,
    join: Option<JoinHandle<()>>,
}

impl FrontendLivePumpOwner {
    pub fn start(
        mut reader: Box<dyn Read + Send>,
        mut sink: Box<dyn FrontendLivePacketSink>,
    ) -> Result<Self, HalError> {
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let worker_cancel = Arc::clone(&cancel);
        let worker_result = Arc::clone(&result);
        let join = thread::Builder::new()
            .name("maleicacid-frontend-live-pump".to_string())
            .spawn(move || {
                let outcome = run_frontend_live_pump(&mut reader, &mut sink, &worker_cancel);
                if let Ok(mut guard) = worker_result.lock() {
                    *guard = Some(outcome);
                }
            })
            .map_err(|error| HalError::cleanup_failed("frontend live pump spawn", error.to_string()))?;
        Ok(Self { cancel, result, join: Some(join) })
    }

    pub fn request_stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn collect_if_finished(&mut self) -> FrontendLivePumpJoinOutcome {
        if self.join.as_ref().map(|handle| handle.is_finished()).unwrap_or(false) {
            if let Some(handle) = self.join.take() {
                if handle.join().is_err() {
                    return FrontendLivePumpJoinOutcome::Completed(Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend live pump thread panicked",
                    )));
                }
            }
        }
        match self.result.lock() {
            Ok(mut guard) => match guard.take() {
                Some(result) => FrontendLivePumpJoinOutcome::Completed(result),
                None => FrontendLivePumpJoinOutcome::Running,
            },
            Err(_) => FrontendLivePumpJoinOutcome::Completed(Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend live pump result lock poisoned",
            ))),
        }
    }

    pub fn join_after_stop(mut self) -> Result<FrontendLivePumpReport, HalError> {
        self.request_stop();
        if let Some(handle) = self.join.take() {
            if handle.join().is_err() {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend live pump thread panicked while stopping",
                ));
            }
        }
        match self.result.lock() {
            Ok(mut guard) => guard.take().unwrap_or_else(|| {
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend live pump finished without report",
                ))
            }),
            Err(_) => Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend live pump result lock poisoned while stopping",
            )),
        }
    }
}

pub fn run_frontend_live_pump<R, S>(
    reader: &mut R,
    sink: &mut S,
    cancel: &AtomicBool,
) -> Result<FrontendLivePumpReport, HalError>
where
    R: Read,
    S: FrontendLivePacketSink,
{
    run_frontend_live_pump_limited(reader, sink, cancel, None)
}

pub fn run_frontend_live_pump_limited<R, S>(
    reader: &mut R,
    sink: &mut S,
    cancel: &AtomicBool,
    max_iterations: Option<usize>,
) -> Result<FrontendLivePumpReport, HalError>
where
    R: Read,
    S: FrontendLivePacketSink,
{
    let mut report = FrontendLivePumpReport::default();
    let retry_counter = AtomicU64::new(0);
    let retry_counter_saturated = AtomicBool::new(false);
    let mut completion = TsPacketCompletionBuffer::default();
    let mut buf = [0u8; TS_PACKET_SIZE * 16];
    let mut iterations = 0usize;

    loop {
        if cancel.load(Ordering::SeqCst) {
            report.stopped_by_cancel = true;
            break;
        }
        if let Some(max) = max_iterations {
            if iterations >= max {
                break;
            }
        }
        iterations = iterations.checked_add(1).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend live pump iteration counter overflow",
            )
        })?;

        let read_len = retry_after_interrupted_read_with_saturation(
            "frontend live pump read",
            &retry_counter,
            Some(&retry_counter_saturated),
            || reader.read(&mut buf),
        )
        .map_err(|error| io_error_to_hal("read", error))?;

        if read_len == 0 {
            report.reached_eof = true;
            break;
        }

        let drain = completion.push(&buf[..read_len]);
        report.add_malformed(u64::try_from(drain.malformed_bytes).unwrap_or(u64::MAX));
        for packet in &drain.packets {
            sink.deliver_ts_packet(packet)?;
        }
        report.add_packets(drain.packets.len())?;
    }

    let boundary = completion.drain_for_boundary();
    report.add_malformed(u64::try_from(boundary.malformed_bytes).unwrap_or(u64::MAX));
    for packet in &boundary.packets {
        sink.deliver_ts_packet(packet)?;
    }
    report.add_packets(boundary.packets.len())?;
    report.read_retries = retry_counter.load(Ordering::SeqCst);
    report.read_retry_counter_saturated = retry_counter_saturated.load(Ordering::SeqCst);
    Ok(report)
}

fn io_error_to_hal(operation: &'static str, error: io::Error) -> HalError {
    HalError::Io {
        backend: "frontend-live-pump",
        operation,
        path: None,
        errno: error.raw_os_error(),
        detail: HalErrorDetail::new(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Default)]
    struct VecSink {
        packets: Vec<[u8; TS_PACKET_SIZE]>,
    }

    impl FrontendLivePacketSink for VecSink {
        fn deliver_ts_packet(&mut self, packet: &[u8; TS_PACKET_SIZE]) -> Result<(), HalError> {
            self.packets.push(*packet);
            Ok(())
        }
    }

    fn packet(seed: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [seed; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet
    }

    #[test]
    fn live_pump_delivers_completed_ts_packets() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&packet(1));
        bytes.extend_from_slice(&packet(2));
        let mut reader = Cursor::new(bytes);
        let mut sink = VecSink::default();
        let cancel = AtomicBool::new(false);
        let report = run_frontend_live_pump(&mut reader, &mut sink, &cancel).unwrap();
        assert_eq!(report.packets_delivered, 2);
        assert_eq!(sink.packets.len(), 2);
        assert!(report.reached_eof);
    }

    #[test]
    fn live_pump_reports_sink_failure() {
        struct FailingSink;
        impl FrontendLivePacketSink for FailingSink {
            fn deliver_ts_packet(&mut self, _packet: &[u8; TS_PACKET_SIZE]) -> Result<(), HalError> {
                Err(HalError::cleanup_failed("frontend live pump test sink", "forced failure"))
            }
        }
        let mut reader = Cursor::new(packet(3).to_vec());
        let mut sink = FailingSink;
        let cancel = AtomicBool::new(false);
        assert!(run_frontend_live_pump(&mut reader, &mut sink, &cancel).is_err());
    }

    #[test]
    fn live_pump_owner_collects_report() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&packet(4));
        let owner = FrontendLivePumpOwner::start(Box::new(Cursor::new(bytes)), Box::new(VecSink::default())).unwrap();
        let report = owner.join_after_stop().unwrap();
        assert_eq!(report.packets_delivered, 1);
    }
}
