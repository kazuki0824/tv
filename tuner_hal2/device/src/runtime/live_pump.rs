//! frontend live TS pump の中核。
//!
//! descriptorだけのlive readerモデルを置き換える実装である。pumpはread loopとTS packet再同期を所有する。
//! 明示的なpacket sinkを必須とし、demux bindingなしで完了に見える無処理成功sinkは提供しない。

use super::reader::{FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind};
use maleicacid_tuner_hal2_control_core::WorkerContext;
use std::io::{self, Read};
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalErrorDetail, HalInternalKind,
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
            .field("thread_result", &self.thread_result)
            .finish()
    }
}

pub struct FrontendLivePumpOwner {
    thread_result: ThreadResultOwner<FrontendLivePumpReport>,
}

impl FrontendLivePumpOwner {
    pub fn start(
        descriptor: FrontendLiveReaderDescriptor,
        mut reader: Box<dyn Read + Send>,
        mut sink: Box<dyn FrontendLivePacketSink>,
    ) -> Result<Self, HalError> {
        let thread_result =
            ThreadResultOwner::start_controlled("maleicacid-frontend-live-pump", move |control| {
                run_frontend_live_pump(&mut reader, &mut sink, &control, &descriptor)
            })?;
        Ok(Self { thread_result })
    }

    pub fn request_stop(&self) -> Result<(), HalError> {
        self.thread_result.request_stop_and_wake()
    }

    pub fn collect_if_finished(&mut self) -> FrontendLivePumpJoinOutcome {
        match self.thread_result.collect_if_finished() {
            ThreadResultPoll::Running => FrontendLivePumpJoinOutcome::Running,
            ThreadResultPoll::Completed(result) => FrontendLivePumpJoinOutcome::Completed(result),
        }
    }

    pub fn join_after_stop(self) -> Result<FrontendLivePumpReport, HalError> {
        let stop = self.request_stop();
        let result = self.thread_result.join_after_stop();
        match (stop, result) {
            (Ok(()), result) => result,
            (Err(error), Ok(_)) => Err(error),
            (Err(primary), Err(cleanup)) => Err(compose_primary_cleanup_failure(
                "live pump stop and join failed",
                primary,
                cleanup,
            )),
        }
    }
}

fn run_frontend_live_pump<R, S>(
    reader: &mut R,
    sink: &mut S,
    control: &WorkerContext,
    descriptor: &FrontendLiveReaderDescriptor,
) -> Result<FrontendLivePumpReport, HalError>
where
    R: Read,
    S: FrontendLivePacketSink + ?Sized,
{
    let mut report = FrontendLivePumpReport::default();
    let mut completion = TsPacketCompletionBuffer::default();
    let mut buf = [0u8; TS_PACKET_SIZE * 16];

    loop {
        if control.stop_requested() {
            report.stopped_by_cancel = true;
            break;
        }
        let read_len = match reader.read(&mut buf) {
            Ok(length) => length,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                match report.read_retries.checked_add(1) {
                    Some(count) => report.read_retries = count,
                    None if !report.read_retry_counter_saturated => {
                        report.read_retry_counter_saturated = true;
                        eprintln!(
                            "live read EINTR counter saturated: frontend={} reader={:?}",
                            descriptor.frontend_id, descriptor.kind
                        );
                    }
                    None => {}
                }
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let deadline = Instant::now()
                    .checked_add(Duration::from_millis(20))
                    .ok_or_else(|| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "live read retry deadline overflow",
                        )
                    })?;
                control.wait_until(Some(deadline))?;
                continue;
            }
            Err(error) => return Err(io_error_to_hal(descriptor, "read", error)),
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
    if !report.stopped_by_cancel {
        for packet in &boundary.packets {
            sink.deliver_ts_packet(packet)?;
        }
        report.add_packets(boundary.packets.len())?;
    }
    Ok(report)
}

fn io_error_to_hal(
    descriptor: &FrontendLiveReaderDescriptor,
    operation: &'static str,
    error: io::Error,
) -> HalError {
    let (backend, path) = match &descriptor.kind {
        FrontendLiveReaderDescriptorKind::Px4DuplicatedControlFd { control_path } => {
            ("px4", control_path)
        }
        FrontendLiveReaderDescriptorKind::DvbDvrDevice { dvr_path } => ("dvb", dvr_path),
    };
    HalError::Io {
        backend,
        operation,
        path: Some(path.as_path().to_path_buf()),
        errno: error.raw_os_error(),
        detail: HalErrorDetail::new(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn descriptor() -> FrontendLiveReaderDescriptor {
        FrontendLiveReaderDescriptor::dvb_dvr_device(
            7,
            maleicacid_tuner_hal2_common::FrontendDevicePath::new("/dev/dvb/adapter0/dvr0"),
        )
    }

    fn run<R: Read + Send + 'static, S: FrontendLivePacketSink + Send + 'static>(
        mut reader: R,
        mut sink: S,
    ) -> (Result<FrontendLivePumpReport, HalError>, S) {
        ThreadResultOwner::start_controlled("live-pump-test", move |control| {
            let result = run_frontend_live_pump(&mut reader, &mut sink, &control, &descriptor());
            Ok((result, sink))
        })
        .unwrap()
        .join_after_stop()
        .unwrap()
    }

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
        let (result, sink) = run(Cursor::new(bytes), VecSink::default());
        let report = result.unwrap();
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
        assert!(run(Cursor::new(packet(3).to_vec()), FailingSink).0.is_err());
    }

    #[test]
    fn interrupted_and_temporarily_empty_reads_preserve_stream_progress() {
        struct IntermittentReader {
            errors: std::collections::VecDeque<io::ErrorKind>,
            bytes: Cursor<Vec<u8>>,
        }
        impl Read for IntermittentReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                match self.errors.pop_front() {
                    Some(kind) => Err(io::Error::from(kind)),
                    None => self.bytes.read(buffer),
                }
            }
        }
        let reader = IntermittentReader {
            errors: [io::ErrorKind::Interrupted, io::ErrorKind::WouldBlock].into(),
            bytes: Cursor::new(packet(9).to_vec()),
        };
        let (report, sink) = run(reader, VecSink::default());
        let report = report.unwrap();
        assert_eq!(report.read_retries, 1);
        assert!(report.reached_eof);
        assert_eq!(sink.packets, vec![packet(9)]);
    }

    #[test]
    fn stop_finishes_a_pump_with_no_input_on_a_nonblocking_fd() {
        let (reader, _writer) = std::os::unix::net::UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        struct ObservedReader {
            reader: std::os::unix::net::UnixStream,
            ready: Option<std::sync::mpsc::Sender<()>>,
        }
        impl Read for ObservedReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let result = self.reader.read(buffer);
                if let Some(ready) = self.ready.take() {
                    ready.send(()).unwrap();
                }
                result
            }
        }
        let owner = FrontendLivePumpOwner::start(
            descriptor(),
            Box::new(ObservedReader {
                reader,
                ready: Some(ready_tx),
            }),
            Box::new(VecSink::default()),
        )
        .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        owner.request_stop().unwrap();
        assert!(owner
            .thread_result
            .wait_until_finished(Some(Instant::now() + Duration::from_secs(1)))
            .unwrap());
        let report = owner.join_after_stop().unwrap();
        assert!(report.stopped_by_cancel);
        assert!(!report.reached_eof);
    }

    #[test]
    fn permanent_read_failure_retains_each_backend_and_device_path() {
        struct FailedReader;
        impl Read for FailedReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from_raw_os_error(5))
            }
        }
        for (descriptor, expected_backend, expected_path) in [
            (descriptor(), "dvb", "/dev/dvb/adapter0/dvr0"),
            (
                FrontendLiveReaderDescriptor::px4_from_control_fd(
                    8,
                    maleicacid_tuner_hal2_common::FrontendDevicePath::new("/dev/px4video0"),
                ),
                "px4",
                "/dev/px4video0",
            ),
        ] {
            let error = ThreadResultOwner::start_controlled("live-error-origin", move |control| {
                run_frontend_live_pump(
                    &mut FailedReader,
                    &mut VecSink::default(),
                    &control,
                    &descriptor,
                )
            })
            .unwrap()
            .join_after_stop()
            .unwrap_err();
            match error {
                HalError::Io {
                    backend,
                    path,
                    errno,
                    ..
                } => {
                    assert_eq!(backend, expected_backend);
                    assert_eq!(path.unwrap(), std::path::PathBuf::from(expected_path));
                    assert_eq!(errno, Some(5));
                }
                other => panic!("unexpected read failure: {other:?}"),
            }
        }
    }

    #[test]
    fn live_pump_owner_collects_report() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&packet(4));
        let mut owner = FrontendLivePumpOwner::start(
            descriptor(),
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
        let mut owner = FrontendLivePumpOwner::start(
            descriptor(),
            Box::new(FailingReader),
            Box::new(VecSink::default()),
        )
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
        let owner = FrontendLivePumpOwner {
            thread_result: ThreadResultOwner::start(
                "live-pump-owner-failure-test",
                || -> Result<FrontendLivePumpReport, HalError> {
                    panic!("forced worker owner failure")
                },
            )
            .unwrap(),
        };
        assert!(owner.join_after_stop().is_err());
    }

    #[test]
    fn live_pump_owner_missing_report_is_error_after_join() {
        let owner = FrontendLivePumpOwner {
            thread_result: ThreadResultOwner::start(
                "live-pump-owner-failure-test",
                || -> Result<FrontendLivePumpReport, HalError> {
                    panic!("forced worker owner failure")
                },
            )
            .unwrap(),
        };
        assert!(owner.join_after_stop().is_err());
    }
}
