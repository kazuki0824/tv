use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, FrontendTuneRequest, HalError, HalInternalKind,
};

use super::{FrontendLiveReaderDescriptor, FrontendScanSession, FrontendWorkerCancelReason};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendRuntimeState {
    Idle,
    Tuning { generation: u64 },
    Scanning { generation: u64 },
    Closing,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendSignalState {
    Unknown,
    NoSignal,
    SignalDetected,
    Locked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendTerminalEventKind {
    ScanEnd,
    ScanCancelled,
    TuneFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendTerminalEventReason {
    End,
    StopRequested,
    SupersededByNewRequest,
    FrontendClosing,
    CallbackFailure,
    BackendFailure,
    PanicOrJoinFailure,
}

impl From<FrontendWorkerCancelReason> for FrontendTerminalEventReason {
    fn from(reason: FrontendWorkerCancelReason) -> Self {
        match reason {
            FrontendWorkerCancelReason::StopRequested => Self::StopRequested,
            FrontendWorkerCancelReason::SupersededByNewRequest => Self::SupersededByNewRequest,
            FrontendWorkerCancelReason::FrontendClosing => Self::FrontendClosing,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendTerminalEvent {
    pub generation: u64,
    pub kind: FrontendTerminalEventKind,
    pub reason: FrontendTerminalEventReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendRuntimeSnapshot {
    pub state: FrontendRuntimeState,
    pub generation: u64,
    pub live_reader_descriptor: Option<FrontendLiveReaderDescriptor>,
    pub terminal_event_min_generation: u64,
    pub terminal_events: Vec<FrontendTerminalEvent>,
    pub scan_session: Option<FrontendScanSession>,
    pub last_error: Option<HalError>,
    pub active_tune_request: Option<FrontendTuneRequest>,
    pub signal_state: FrontendSignalState,
}

#[derive(Debug)]
pub struct FrontendRuntime {
    frontend_id: i32,
    backend_kind: FrontendBackendKind,
    state: FrontendRuntimeState,
    generation: u64,
    live_reader_descriptor: Option<FrontendLiveReaderDescriptor>,
    terminal_event_min_generation: u64,
    terminal_events: Vec<FrontendTerminalEvent>,
    scan_session: Option<FrontendScanSession>,
    last_error: Option<HalError>,
    active_tune_request: Option<FrontendTuneRequest>,
    signal_state: FrontendSignalState,
}

impl FrontendRuntime {
    pub fn new(frontend_id: i32, backend_kind: FrontendBackendKind) -> Self {
        Self {
            frontend_id,
            backend_kind,
            state: FrontendRuntimeState::Idle,
            generation: 0,
            live_reader_descriptor: None,
            terminal_event_min_generation: 0,
            terminal_events: Vec::new(),
            scan_session: None,
            last_error: None,
            active_tune_request: None,
            signal_state: FrontendSignalState::Unknown,
        }
    }

    pub fn frontend_id(&self) -> i32 {
        self.frontend_id
    }
    pub fn backend_kind(&self) -> FrontendBackendKind {
        self.backend_kind
    }
    pub fn state(&self) -> FrontendRuntimeState {
        self.state
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn checked_next_generation(&self) -> Result<u64, HalError> {
        self.generation
            .checked_add(1)
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend operation generation exhausted",
                )
            })
    }

    pub fn commit_generation(&mut self, generation: u64) -> Result<(), HalError> {
        if generation <= self.generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend operation generation must monotonically advance",
            ));
        }
        self.generation = generation;
        Ok(())
    }

    pub fn live_reader_descriptor(&self) -> Option<&FrontendLiveReaderDescriptor> {
        self.live_reader_descriptor.as_ref()
    }
    pub fn terminal_event_min_generation(&self) -> u64 {
        self.terminal_event_min_generation
    }
    pub fn should_accept_terminal_event(&self, generation: u64) -> bool {
        generation >= self.terminal_event_min_generation && generation == self.generation
    }
    pub fn terminal_events(&self) -> &[FrontendTerminalEvent] {
        &self.terminal_events
    }
    pub fn active_scan_session(&self) -> Option<&FrontendScanSession> {
        self.scan_session.as_ref()
    }
    pub fn active_tune_request(&self) -> Option<&FrontendTuneRequest> {
        self.active_tune_request.as_ref()
    }
    pub fn signal_state(&self) -> FrontendSignalState {
        self.signal_state
    }
    pub fn same_active_tune(&self, request: &FrontendTuneRequest) -> bool {
        self.active_tune_request.as_ref() == Some(request)
    }

    pub fn snapshot(&self) -> FrontendRuntimeSnapshot {
        FrontendRuntimeSnapshot {
            state: self.state,
            generation: self.generation,
            live_reader_descriptor: self.live_reader_descriptor.clone(),
            terminal_event_min_generation: self.terminal_event_min_generation,
            terminal_events: self.terminal_events.clone(),
            scan_session: self.scan_session.clone(),
            last_error: self.last_error.clone(),
            active_tune_request: self.active_tune_request.clone(),
            signal_state: self.signal_state,
        }
    }

    pub fn restore_snapshot(&mut self, snapshot: FrontendRuntimeSnapshot) {
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.live_reader_descriptor = snapshot.live_reader_descriptor;
        self.terminal_event_min_generation = snapshot.terminal_event_min_generation;
        self.terminal_events = snapshot.terminal_events;
        self.scan_session = snapshot.scan_session;
        self.last_error = snapshot.last_error;
        self.active_tune_request = snapshot.active_tune_request;
        self.signal_state = snapshot.signal_state;
    }

    pub fn record_signal_state(
        &mut self,
        generation: u64,
        signal_state: FrontendSignalState,
    ) -> Result<(), HalError> {
        if generation != self.generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend signal state generation must match frontend runtime generation",
            ));
        }
        self.signal_state = signal_state;
        Ok(())
    }

    pub fn commit_active_tune_request(
        &mut self,
        generation: u64,
        request: FrontendTuneRequest,
    ) -> Result<(), HalError> {
        if generation != self.generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "active tune request generation must match frontend runtime generation",
            ));
        }
        self.active_tune_request = Some(request);
        Ok(())
    }

    pub fn begin_scan_session(
        &mut self,
        generation: u64,
        fingerprint: impl Into<String>,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        if generation != self.generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan session generation must match frontend runtime generation",
            ));
        }
        let session = FrontendScanSession::start(generation, fingerprint, candidates)?;
        self.mark_scanning(generation);
        self.scan_session = Some(session);
        Ok(())
    }

    pub fn cancel_scan_session(
        &mut self,
        generation: u64,
        reason: FrontendWorkerCancelReason,
    ) -> Result<(), HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale scan session cancellation generation cannot be recorded",
            ));
        }
        let Some(session) = self.scan_session.as_mut() else {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "active scan session is missing",
            ));
        };
        if session.generation() != generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "active scan session generation mismatch",
            ));
        }
        session.cancel(reason);
        self.terminal_events.push(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::ScanCancelled,
            reason: reason.into(),
        });
        self.state = FrontendRuntimeState::Idle;
        Ok(())
    }

    pub fn mark_scan_session_backend_failed(&mut self, generation: u64) -> Result<(), HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale scan backend failure generation cannot be recorded",
            ));
        }
        let Some(session) = self.scan_session.as_mut() else {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "active scan session is missing",
            ));
        };
        if session.generation() != generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "active scan session generation mismatch",
            ));
        }
        session.fail_backend();
        self.last_error = Some(HalError::internal(
            HalInternalKind::InvariantViolation,
            "scan backend failure",
        ));
        self.state = FrontendRuntimeState::Failed;
        Ok(())
    }
    pub fn advance_scan_session_after_candidate(
        &mut self,
        generation: u64,
    ) -> Result<bool, HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale scan session candidate generation cannot be advanced",
            ));
        }
        let Some(session) = self.scan_session.as_mut() else {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "active scan session is missing",
            ));
        };
        if session.generation() != generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "active scan session generation mismatch",
            ));
        }
        let has_next = session.advance_after_candidate()?.is_some();
        if !has_next {
            self.terminal_events.push(FrontendTerminalEvent {
                generation,
                kind: FrontendTerminalEventKind::ScanEnd,
                reason: FrontendTerminalEventReason::End,
            });
            self.state = FrontendRuntimeState::Idle;
        }
        Ok(has_next)
    }

    pub fn mark_tune_worker_failed(
        &mut self,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale tune failure generation cannot be recorded",
            ));
        }
        if !matches!(self.state, FrontendRuntimeState::Tuning { generation: current } if current == generation)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "tune failure can only be recorded for the active tune generation",
            ));
        }
        self.terminal_events.push(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::TuneFailed,
            reason: FrontendTerminalEventReason::BackendFailure,
        });
        self.mark_failed(error);
        Ok(())
    }

    pub fn mark_scan_session_callback_failed(&mut self, generation: u64) -> Result<(), HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale scan callback failure generation cannot be recorded",
            ));
        }
        let Some(session) = self.scan_session.as_mut() else {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "active scan session is missing",
            ));
        };
        if session.generation() != generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "active scan session generation mismatch",
            ));
        }
        session.fail_callback();
        self.terminal_events.push(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::ScanEnd,
            reason: FrontendTerminalEventReason::CallbackFailure,
        });
        self.last_error = Some(HalError::callback_failed(
            "IFrontendCallback.onScanMessage(END)",
            "scan terminal delivery failed",
        ));
        self.state = FrontendRuntimeState::Idle;
        Ok(())
    }

    pub fn last_terminal_event(&self) -> Option<FrontendTerminalEvent> {
        self.terminal_events.last().copied()
    }
    pub fn last_error(&self) -> Option<&HalError> {
        self.last_error.as_ref()
    }

    pub fn record_scan_cancelled(
        &mut self,
        generation: u64,
        reason: FrontendWorkerCancelReason,
    ) -> Result<(), HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale scan cancellation generation cannot be recorded",
            ));
        }
        if !matches!(self.state, FrontendRuntimeState::Scanning { generation: current } if current == generation)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan cancellation can only be recorded for the active scan generation",
            ));
        }
        self.terminal_events.push(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::ScanCancelled,
            reason: reason.into(),
        });
        Ok(())
    }

    pub fn next_generation(&mut self) -> Result<u64, HalError> {
        let next = self.checked_next_generation()?;
        self.generation = next;
        Ok(next)
    }

    pub fn mark_tuning(&mut self, generation: u64) {
        self.terminal_event_min_generation = generation;
        self.state = FrontendRuntimeState::Tuning { generation };
    }
    pub fn mark_scanning(&mut self, generation: u64) {
        self.terminal_event_min_generation = generation;
        self.state = FrontendRuntimeState::Scanning { generation };
    }
    pub fn mark_idle(&mut self) {
        self.state = FrontendRuntimeState::Idle;
    }
    pub fn mark_closing(&mut self) {
        self.state = FrontendRuntimeState::Closing;
    }
    pub fn set_live_reader_descriptor(&mut self, reader: FrontendLiveReaderDescriptor) {
        self.live_reader_descriptor = Some(reader);
    }
    pub fn clear_live_reader_descriptor(&mut self) {
        self.live_reader_descriptor = None;
    }
    pub fn mark_failed(&mut self, error: HalError) {
        self.last_error = Some(error);
        self.state = FrontendRuntimeState::Failed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::FrontendLiveReaderDescriptor;
    use crate::{FrontendScanPhase, FrontendScanTerminalReason};
    use maleicacid_tuner_hal2_common::FrontendDevicePath;

    #[test]
    fn runtime_owns_backend_kind_generation_and_reader() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        assert_eq!(runtime.backend_kind(), FrontendBackendKind::Px4CharDevice);
        let generation = runtime.next_generation().unwrap();
        runtime.mark_tuning(generation);
        runtime.set_live_reader_descriptor(FrontendLiveReaderDescriptor::px4_from_control_fd(
            7,
            FrontendDevicePath::new("/dev/px4video0"),
        ));
        assert_eq!(runtime.state(), FrontendRuntimeState::Tuning { generation });
        assert!(runtime.live_reader_descriptor().is_some());
    }

    #[test]
    fn frontend_generation_overflow_is_error_not_saturated() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.generation = u64::MAX;
        assert!(runtime.next_generation().is_err());
        assert_eq!(runtime.generation(), u64::MAX);
        assert_eq!(runtime.state(), FrontendRuntimeState::Idle);
    }

    #[test]
    fn checked_next_generation_does_not_commit_state() {
        let runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        assert_eq!(runtime.checked_next_generation().unwrap(), 1);
        assert_eq!(runtime.generation(), 0);
        assert_eq!(runtime.state(), FrontendRuntimeState::Idle);
    }
    #[test]
    fn new_generation_invalidates_old_terminal_events() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        runtime.mark_scanning(1);
        assert!(runtime.should_accept_terminal_event(1));
        runtime.commit_generation(2).unwrap();
        runtime.mark_scanning(2);
        assert!(!runtime.should_accept_terminal_event(1));
        assert!(runtime.should_accept_terminal_event(2));
    }

    #[test]
    fn active_scan_cancel_records_terminal_diagnostic() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        runtime.mark_scanning(1);
        runtime
            .record_scan_cancelled(1, FrontendWorkerCancelReason::StopRequested)
            .unwrap();
        assert_eq!(
            runtime.last_terminal_event(),
            Some(FrontendTerminalEvent {
                generation: 1,
                kind: FrontendTerminalEventKind::ScanCancelled,
                reason: FrontendTerminalEventReason::StopRequested,
            }),
        );
    }

    #[test]
    fn stale_scan_cancel_is_rejected() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        runtime.mark_scanning(1);
        runtime.commit_generation(2).unwrap();
        runtime.mark_scanning(2);
        assert!(runtime
            .record_scan_cancelled(1, FrontendWorkerCancelReason::StopRequested)
            .is_err());
    }

    #[test]
    fn scan_session_requires_matching_generation() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        let request = maleicacid_tuner_hal2_common::FrontendTuneRequest {
            system: maleicacid_tuner_hal2_common::FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        assert!(runtime.begin_scan_session(2, "bad", vec![request]).is_err());
    }

    #[test]
    fn active_tune_failure_records_terminal_and_failed_state() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        runtime.mark_tuning(1);
        runtime
            .mark_tune_worker_failed(
                1,
                HalError::internal(HalInternalKind::InvariantViolation, "backend failed"),
            )
            .unwrap();
        assert_eq!(runtime.state(), FrontendRuntimeState::Failed);
        assert_eq!(
            runtime.last_terminal_event(),
            Some(FrontendTerminalEvent {
                generation: 1,
                kind: FrontendTerminalEventKind::TuneFailed,
                reason: FrontendTerminalEventReason::BackendFailure,
            }),
        );
    }

    #[test]
    fn scan_session_cancel_records_terminal_and_idles() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        let request = maleicacid_tuner_hal2_common::FrontendTuneRequest {
            system: maleicacid_tuner_hal2_common::FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        runtime
            .begin_scan_session(1, "scan", vec![request])
            .unwrap();
        runtime
            .cancel_scan_session(1, FrontendWorkerCancelReason::StopRequested)
            .unwrap();
        assert_eq!(runtime.state(), FrontendRuntimeState::Idle);
        assert_eq!(
            runtime.active_scan_session().unwrap().phase(),
            FrontendScanPhase::Cancelled
        );
        assert_eq!(
            runtime.active_scan_session().unwrap().terminal_reason(),
            Some(FrontendScanTerminalReason::StopRequested)
        );
        assert_eq!(runtime.last_terminal_event().unwrap().generation, 1);
    }
}
