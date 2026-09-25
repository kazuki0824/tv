//! frontend live TS pump の中核。
//!
//! descriptorだけのlive readerモデルを置き換える実装である。pumpはread loopとTS packet再同期を所有する。
//! 明示的なpacket sinkを必須とし、demux bindingなしで完了に見える無処理成功sinkは提供しない。

use super::frontend_worker::FrontendWorkerContext;
use super::reader::{FrontendLiveReaderDescriptor, FrontendLiveReaderDescriptorKind};
use maleicacid_tuner_hal2_control_core::WorkerContext;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_common::{
    HalError, HalErrorDetail, HalInternalKind, TsPacketCompletionBuffer, TS_PACKET_SIZE,
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
    pub malformed_byte_counter_saturated: bool,
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

    fn add_malformed(&mut self, amount: u64, descriptor: &FrontendLiveReaderDescriptor) {
        self.malformed_bytes = self.malformed_bytes.saturating_add(amount);
        if self.malformed_bytes == u64::MAX && !self.malformed_byte_counter_saturated {
            self.malformed_byte_counter_saturated = true;
            eprintln!(
                "diagnostic_counter_saturated counter=malformed_bytes owner=frontend_live_pump frontend={} reader={:?}",
                descriptor.frontend_id, descriptor.kind
            );
        }
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

#[must_use = "準備済みライブTSポンプはactivateまたはjoin_after_stopで消費してください"]
pub struct PreparedFrontendLivePump {
    thread_result: ThreadResultOwner<FrontendLivePumpReport>,
    start_gate: Arc<AtomicBool>,
}

impl core::fmt::Debug for PreparedFrontendLivePump {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PreparedFrontendLivePump")
            .field("thread_result", &self.thread_result)
            .finish()
    }
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

    pub fn prepare(
        descriptor: FrontendLiveReaderDescriptor,
        reader: Box<dyn Read + Send>,
        sink: Box<dyn FrontendLivePacketSink>,
        caller: &FrontendWorkerContext,
    ) -> Result<Option<PreparedFrontendLivePump>, HalError> {
        PreparedFrontendLivePump::start(descriptor, reader, sink, caller)
    }

    pub fn request_stop(&self) {
        self.thread_result.request_stop()
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

impl PreparedFrontendLivePump {
    fn start(
        descriptor: FrontendLiveReaderDescriptor,
        mut reader: Box<dyn Read + Send>,
        mut sink: Box<dyn FrontendLivePacketSink>,
        caller: &FrontendWorkerContext,
    ) -> Result<Option<Self>, HalError> {
        let caller_wake = caller.clone();
        let ready = Arc::new(AtomicBool::new(false));
        let worker_ready = Arc::clone(&ready);
        let start_gate = Arc::new(AtomicBool::new(false));
        let worker_start_gate = Arc::clone(&start_gate);
        let thread_result =
            ThreadResultOwner::start_controlled("maleicacid-frontend-live-pump", move |control| {
                worker_ready.store(true, Ordering::Release);
                caller_wake.wake();
                loop {
                    if control.stop_requested() {
                        return Ok(FrontendLivePumpReport {
                            stopped_by_cancel: true,
                            ..FrontendLivePumpReport::default()
                        });
                    }
                    if worker_start_gate.load(Ordering::Acquire) {
                        break;
                    }
                    control.wait_until(None);
                }
                run_frontend_live_pump(&mut reader, &mut sink, &control, &descriptor)
            })?;

        if caller.cancel_requested() {
            thread_result.request_stop();
            let report = thread_result.join_after_stop()?;
            debug_assert!(report.stopped_by_cancel);
            return Ok(None);
        }

        while !ready.load(Ordering::Acquire) {
            if caller.cancel_requested() {
                thread_result.request_stop();
                let report = thread_result.join_after_stop()?;
                debug_assert!(report.stopped_by_cancel);
                return Ok(None);
            }
            caller.wait_until(None);
        }

        Ok(Some(Self {
            thread_result,
            start_gate,
        }))
    }

    pub fn activate(self) -> FrontendLivePumpOwner {
        self.start_gate.store(true, Ordering::Release);
        self.thread_result.wake();
        FrontendLivePumpOwner {
            thread_result: self.thread_result,
        }
    }

    pub fn join_after_stop(self) -> Result<FrontendLivePumpReport, HalError> {
        self.thread_result.request_stop();
        self.thread_result.join_after_stop()
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
                control.wait_until(Some(deadline));
                continue;
            }
            Err(error) => return Err(io_error_to_hal(descriptor, "read", error)),
        };

        if read_len == 0 {
            report.reached_eof = true;
            break;
        }

        let drain = completion.push(&buf[..read_len]);
        report.add_malformed(
            u64::try_from(drain.malformed_bytes).unwrap_or(u64::MAX),
            descriptor,
        );
        for packet in &drain.packets {
            sink.deliver_ts_packet(packet)?;
        }
        report.add_packets(drain.packets.len())?;
    }

    let boundary = completion.drain_for_boundary();
    report.add_malformed(
        u64::try_from(boundary.malformed_bytes).unwrap_or(u64::MAX),
        descriptor,
    );
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
    use std::sync::mpsc;

    fn prepare_for_test<R, S>(reader: R, sink: S) -> PreparedFrontendLivePump
    where
        R: Read + Send + 'static,
        S: FrontendLivePacketSink + Send + 'static,
    {
        use crate::runtime::frontend_worker::{
            FrontendWorkerKind, FrontendWorkerRegistry, FrontendWorkerStopOutcome,
        };

        let mut registry = FrontendWorkerRegistry::default();
        let (prepared_tx, prepared_rx) = mpsc::channel();
        registry
            .start(1, FrontendWorkerKind::Tune, 1, move |ctx| {
                let prepared = FrontendLivePumpOwner::prepare(
                    descriptor(),
                    Box::new(reader),
                    Box::new(sink),
                    &ctx,
                )?
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "試験用の準備済みライブTSポンプが取消されました",
                    )
                })?;
                prepared_tx.send(prepared).map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "試験用の準備済みライブTSポンプを返却できませんでした",
                    )
                })
            })
            .unwrap();

        let prepared = prepared_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("準備済みライブTSポンプを受信できませんでした");
        for _ in 0..100 {
            if let Some(outcome) = registry.take_completed(1, FrontendWorkerKind::Tune) {
                assert!(matches!(
                    outcome,
                    FrontendWorkerStopOutcome::Completed { result: Ok(()), .. }
                ));
                return prepared;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("準備親ワーカーが終了していません");
    }

    #[test]
    fn caller_cancel_during_prepare_stops_child_and_returns_none() {
        use crate::runtime::frontend_worker::{
            FrontendWorkerCancelReason, FrontendWorkerKind, FrontendWorkerRegistry,
            FrontendWorkerStopOutcome,
        };

        let mut registry = FrontendWorkerRegistry::default();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();

        registry
            .start(9, FrontendWorkerKind::Tune, 1, move |ctx| {
                entered_tx.send(()).map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "試験用の準備開始通知を送信できませんでした",
                    )
                })?;
                resume_rx.recv().map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "試験用の準備再開通知を受信できませんでした",
                    )
                })?;
                let prepared = FrontendLivePumpOwner::prepare(
                    descriptor(),
                    Box::new(Cursor::new(Vec::<u8>::new())),
                    Box::new(VecSink::default()),
                    &ctx,
                )?;
                result_tx.send(prepared.is_none()).map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "試験用の準備結果を送信できませんでした",
                    )
                })
            })
            .unwrap();

        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            registry.request_stop(
                9,
                FrontendWorkerKind::Tune,
                FrontendWorkerCancelReason::StopRequested,
            ),
            FrontendWorkerStopOutcome::CancelRequested { .. }
        ));
        resume_tx.send(()).unwrap();
        assert!(result_rx.recv_timeout(Duration::from_secs(1)).unwrap());

        for _ in 0..100 {
            if let Some(outcome) = registry.take_completed(9, FrontendWorkerKind::Tune) {
                assert!(matches!(
                    outcome,
                    FrontendWorkerStopOutcome::Completed { result: Ok(()), .. }
                ));
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("取消し済み準備ワーカーが終了していません");
    }

    #[test]
    fn malformed_counter_saturation_is_retained_without_failing_the_pump() {
        for amount in [1, 2, u64::MAX] {
            let mut report = FrontendLivePumpReport {
                malformed_bytes: u64::MAX - 1,
                ..FrontendLivePumpReport::default()
            };
            report.add_malformed(0, &descriptor());
            assert!(!report.malformed_byte_counter_saturated);
            report.add_malformed(amount, &descriptor());
            assert_eq!(report.malformed_bytes, u64::MAX);
            assert!(report.malformed_byte_counter_saturated);
            report.add_malformed(1, &descriptor());
            report.add_packets(1).unwrap();
            assert_eq!(report.malformed_bytes, u64::MAX);
            assert_eq!(report.packets_delivered, 1);
            assert!(!report.stopped_by_cancel);
            assert!(!report.reached_eof);
        }
    }

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
        owner.request_stop();
        assert!(owner
            .thread_result
            .wait_until_finished(Some(Instant::now() + Duration::from_secs(1)))
            .unwrap());
        let report = owner.join_after_stop().unwrap();
        assert!(report.stopped_by_cancel);
        assert!(!report.reached_eof);
    }

    #[test]
    fn prepared_pump_does_not_read_until_activated() {
        let (read_tx, read_rx) = mpsc::channel();
        struct ObservedReader {
            read_tx: mpsc::Sender<()>,
        }
        impl Read for ObservedReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                self.read_tx.send(()).unwrap();
                Ok(0)
            }
        }

        let owner = prepare_for_test(ObservedReader { read_tx }, VecSink::default());
        assert!(read_rx.try_recv().is_err());
        let owner = owner.activate();
        read_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let report = owner.join_after_stop().unwrap();
        assert!(report.reached_eof || report.stopped_by_cancel);
    }

    #[test]
    fn prepared_pump_can_be_stopped_before_activation_without_reading() {
        let (read_tx, read_rx) = mpsc::channel();
        struct ObservedReader {
            read_tx: mpsc::Sender<()>,
        }
        impl Read for ObservedReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                self.read_tx.send(()).unwrap();
                Ok(0)
            }
        }

        let owner = prepare_for_test(ObservedReader { read_tx }, VecSink::default());
        let report = owner.join_after_stop().unwrap();
        assert!(report.stopped_by_cancel);
        assert!(read_rx.try_recv().is_err());
    }

    #[test]
    fn prepared_pump_delivers_first_packets_after_activation() {
        struct ChannelSink {
            packet_tx: mpsc::Sender<[u8; TS_PACKET_SIZE]>,
        }
        impl FrontendLivePacketSink for ChannelSink {
            fn deliver_ts_packet(&mut self, packet: &[u8; TS_PACKET_SIZE]) -> Result<(), HalError> {
                self.packet_tx.send(*packet).unwrap();
                Ok(())
            }
        }

        let first = packet(0x11);
        let second = packet(0x22);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&first);
        bytes.extend_from_slice(&second);
        let (packet_tx, packet_rx) = mpsc::channel();
        let owner = prepare_for_test(Cursor::new(bytes), ChannelSink { packet_tx });
        assert!(packet_rx.try_recv().is_err());

        let owner = owner.activate();
        assert_eq!(
            packet_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            first
        );
        assert_eq!(
            packet_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            second
        );
        let report = owner.join_after_stop().unwrap();
        assert!(report.reached_eof || report.stopped_by_cancel);
    }

    #[test]
    fn prepared_pump_start_gates_are_independent() {
        struct ObservedReader {
            read_tx: mpsc::Sender<u8>,
            id: u8,
        }
        impl Read for ObservedReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                self.read_tx.send(self.id).unwrap();
                Ok(0)
            }
        }

        let (read_tx, read_rx) = mpsc::channel();
        let first = prepare_for_test(
            ObservedReader {
                read_tx: read_tx.clone(),
                id: 1,
            },
            VecSink::default(),
        );
        let second = prepare_for_test(ObservedReader { read_tx, id: 2 }, VecSink::default());

        let first = first.activate();
        assert_eq!(read_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert!(read_rx.try_recv().is_err());

        let second = second.activate();
        assert_eq!(read_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 2);

        let first_report = first.join_after_stop().unwrap();
        let second_report = second.join_after_stop().unwrap();
        assert!(first_report.reached_eof || first_report.stopped_by_cancel);
        assert!(second_report.reached_eof || second_report.stopped_by_cancel);
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
