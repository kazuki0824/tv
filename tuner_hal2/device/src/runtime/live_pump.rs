//! frontend live TS pump の中核。
//!
//! descriptorだけのlive readerモデルを置き換える実装である。pumpはread loopとTS packet再同期を所有する。
//! 明示的なpacket sinkを必須とし、demux bindingなしで完了に見える無処理成功sinkは提供しない。

use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use maleicacid_tuner_hal2_common::{
    retry_after_interrupted_read_with_saturation, HalError, HalErrorDetail, HalInternalKind,
    TsPacketCompletionBuffer, TS_PACKET_SIZE,
};

use crate::runtime::thread_result_owner::{ThreadResultOwner, ThreadResultPoll};

pub trait FrontendLivePacketSink: Send {
    fn deliver_ts_packet(&mut self, packet: &[u8; TS_PACKET_SIZE]) -> Result<(), HalError>;
}

impl<T> FrontendLivePacketSink for Box<T>
where
    T: FrontendLivePacketSink + ?Sized,
{
    fn deliver_ts_packet(&mut self, packet: &[u8; TS_PACKET_SIZE]) -> Result<(), HalError> {
        (**self).deliver_ts_packet(packet)
    }
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
            .field("thread_result", &self.thread_result)
            .finish()
    }
}

pub struct FrontendLivePumpOwner {
    cancel: Arc<AtomicBool>,
    thread_result: ThreadResultOwner<FrontendLivePumpReport>,
}

impl FrontendLivePumpOwner {
    pub fn start(
        mut reader: Box<dyn Read + Send>,
        mut sink: Box<dyn FrontendLivePacketSink>,
    ) -> Result<Self, HalError> {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let thread_result = ThreadResultOwner::start("maleicacid-frontend-live-pump", move || {
            run_frontend_live_pump(&mut reader, &mut sink, &worker_cancel)
        })
        .map_err(|error| {
            HalError::cleanup_failed("frontend live pump spawn", format!("{error:?}"))
        })?;
        Ok(Self {
            cancel,
            thread_result,
        })
    }

    pub fn request_stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn collect_if_finished(&mut self) -> FrontendLivePumpJoinOutcome {
        match self.thread_result.collect_if_finished() {
            ThreadResultPoll::Running => FrontendLivePumpJoinOutcome::Running,
            ThreadResultPoll::Completed(result) => FrontendLivePumpJoinOutcome::Completed(result),
        }
    }

    pub fn join_after_stop(self) -> Result<FrontendLivePumpReport, HalError> {
        self.request_stop();
        self.thread_result.join_after_stop()
    }
}

pub fn run_frontend_live_pump<R, S>(
    reader: &mut R,
    sink: &mut S,
    cancel: &AtomicBool,
) -> Result<FrontendLivePumpReport, HalError>
where
    R: Read,
    S: FrontendLivePacketSink + ?Sized,
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
    S: FrontendLivePacketSink + ?Sized,
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

        let read_len = match retry_after_interrupted_read_with_saturation(
            "frontend live pump read",
            &retry_counter,
            Some(&retry_counter_saturated),
            || reader.read(&mut buf),
        ) {
            Ok(read_len) => read_len,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
            Err(error) => return Err(io_error_to_hal("read", error)),
        };

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
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

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
            fn deliver_ts_packet(
                &mut self,
                _packet: &[u8; TS_PACKET_SIZE],
            ) -> Result<(), HalError> {
                Err(HalError::cleanup_failed(
                    "frontend live pump test sink",
                    "forced failure",
                ))
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
        let mut owner = FrontendLivePumpOwner::start(
            Box::new(Cursor::new(bytes)),
            Box::new(VecSink::default()),
        )
        .unwrap();
        let mut completed_report = None;
        for _ in 0..100 {
            if let FrontendLivePumpJoinOutcome::Completed(Ok(report)) = owner.collect_if_finished()
            {
                completed_report = Some(report);
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            completed_report.map(|report| report.packets_delivered),
            Some(1)
        );
    }

    #[test]
    fn live_pump_owner_reports_reader_failure() {
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "forced reader failure",
                ))
            }
        }
        let mut owner =
            FrontendLivePumpOwner::start(Box::new(FailingReader), Box::new(VecSink::default()))
                .unwrap();
        let mut completed = false;
        for _ in 0..100 {
            if let FrontendLivePumpJoinOutcome::Completed(result) = owner.collect_if_finished() {
                assert!(result.is_err());
                completed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(completed);
    }

    #[test]
    fn live_pump_owner_missing_report_is_error() {
        let result: Arc<Mutex<Option<Result<FrontendLivePumpReport, HalError>>>> =
            Arc::new(Mutex::new(None));
        let join = std::thread::spawn(|| {});
        let owner = FrontendLivePumpOwner {
            cancel: Arc::new(AtomicBool::new(false)),
            thread_result: ThreadResultOwner::new_for_test(
                "live-pump-missing-test",
                result,
                Some(join),
            ),
        };
        assert!(owner.join_after_stop().is_err());
    }

    #[test]
    fn live_pump_owner_missing_report_is_error_after_join() {
        let result: Arc<Mutex<Option<Result<FrontendLivePumpReport, HalError>>>> =
            Arc::new(Mutex::new(None));
        let join = std::thread::spawn(|| {});
        let owner = FrontendLivePumpOwner {
            cancel: Arc::new(AtomicBool::new(false)),
            thread_result: ThreadResultOwner::new_for_test(
                "live-pump-missing-after-join-test",
                result,
                Some(join),
            ),
        };
        assert!(owner.join_after_stop().is_err());
    }
}
