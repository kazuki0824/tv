use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, FrontendTuneRequest, HalError, HalInternalKind,
};

use super::{
    FrontendLivePumpReport, FrontendLiveReaderDescriptor, FrontendScanPhase, FrontendScanSession,
    FrontendWorkerCancelReason, FrontendWorkerKind,
};

const FRONTEND_RUNTIME_DIAGNOSTIC_CAPACITY: usize = 64;

fn push_bounded<T>(records: &mut Vec<T>, dropped_count: &mut u64, value: T) {
    if records.len() >= FRONTEND_RUNTIME_DIAGNOSTIC_CAPACITY {
        records.remove(0);
        *dropped_count = dropped_count.saturating_add(1);
    }
    records.push(value);
}

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
    TuneNoSignal,
    TuneFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendTerminalEventReason {
    End,
    NoSignal,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendLivePumpTerminalReason {
    Eof,
    Cancelled,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendLivePumpJoinResult {
    Joined,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendLivePumpDiagnostic {
    pub generation: u64,
    pub packets_delivered: u64,
    pub malformed_bytes: u64,
    pub stopped_by_cancel: bool,
    pub reached_eof: bool,
    pub cancel_reason: Option<FrontendWorkerCancelReason>,
    pub terminal_reason: FrontendLivePumpTerminalReason,
    pub join_result: FrontendLivePumpJoinResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendDiagnosticWriteFailure {
    pub generation: u64,
    pub detail: String,
}

impl FrontendLivePumpDiagnostic {
    pub fn from_report(
        generation: u64,
        report: FrontendLivePumpReport,
        cancel_reason: Option<FrontendWorkerCancelReason>,
    ) -> Self {
        let terminal_reason = if report.reached_eof {
            FrontendLivePumpTerminalReason::Eof
        } else if report.stopped_by_cancel {
            FrontendLivePumpTerminalReason::Cancelled
        } else {
            FrontendLivePumpTerminalReason::Stopped
        };
        Self {
            generation,
            packets_delivered: report.packets_delivered,
            malformed_bytes: report.malformed_bytes,
            stopped_by_cancel: report.stopped_by_cancel,
            reached_eof: report.reached_eof,
            cancel_reason,
            terminal_reason,
            join_result: FrontendLivePumpJoinResult::Joined,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendRuntimeSnapshot {
    pub state: FrontendRuntimeState,
    pub generation: u64,
    pub live_reader_descriptor: Option<FrontendLiveReaderDescriptor>,
    pub terminal_event_min_generation: u64,
    pub terminal_events: Vec<FrontendTerminalEvent>,
    pub terminal_events_dropped_count: u64,
    pub live_pump_reports: Vec<FrontendLivePumpDiagnostic>,
    pub live_pump_reports_dropped_count: u64,
    pub diagnostic_write_failures: Vec<FrontendDiagnosticWriteFailure>,
    pub diagnostic_write_failures_dropped_count: u64,
    pub scan_session: Option<FrontendScanSession>,
    pub last_error: Option<HalError>,
    pub active_tune_request: Option<FrontendTuneRequest>,
    pub tune_request_sequence: u64,
    pub signal_state: FrontendSignalState,
    pub tune_lock_qualified: bool,
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
    terminal_events_dropped_count: u64,
    live_pump_reports: Vec<FrontendLivePumpDiagnostic>,
    live_pump_reports_dropped_count: u64,
    diagnostic_write_failures: Vec<FrontendDiagnosticWriteFailure>,
    diagnostic_write_failures_dropped_count: u64,
    scan_session: Option<FrontendScanSession>,
    last_error: Option<HalError>,
    active_tune_request: Option<FrontendTuneRequest>,
    tune_request_sequence: u64,
    signal_state: FrontendSignalState,
    tune_lock_qualified: bool,
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
            terminal_events_dropped_count: 0,
            live_pump_reports: Vec::new(),
            live_pump_reports_dropped_count: 0,
            diagnostic_write_failures: Vec::new(),
            diagnostic_write_failures_dropped_count: 0,
            scan_session: None,
            last_error: None,
            active_tune_request: None,
            tune_request_sequence: 0,
            signal_state: FrontendSignalState::Unknown,
            tune_lock_qualified: false,
        }
    }

    pub fn frontend_id(&self) -> i32 {
        self.frontend_id
    }
    pub fn backend_kind(&self) -> FrontendBackendKind {
        self.backend_kind
    }
    #[cfg(test)]
    pub(crate) fn state(&self) -> FrontendRuntimeState {
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

    pub(crate) fn commit_generation(&mut self, generation: u64) -> Result<(), HalError> {
        if generation <= self.generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend operation generation must monotonically advance",
            ));
        }
        self.generation = generation;
        self.tune_lock_qualified = false;
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
    pub fn terminal_events_dropped_count(&self) -> u64 {
        self.terminal_events_dropped_count
    }
    pub fn live_pump_reports(&self) -> &[FrontendLivePumpDiagnostic] {
        &self.live_pump_reports
    }
    pub fn live_pump_reports_dropped_count(&self) -> u64 {
        self.live_pump_reports_dropped_count
    }
    pub fn diagnostic_write_failures(&self) -> &[FrontendDiagnosticWriteFailure] {
        &self.diagnostic_write_failures
    }
    pub fn diagnostic_write_failures_dropped_count(&self) -> u64 {
        self.diagnostic_write_failures_dropped_count
    }
    #[cfg(test)]
    pub(crate) fn active_scan_session(&self) -> Option<&FrontendScanSession> {
        self.scan_session.as_ref()
    }
    pub fn snapshot(&self) -> FrontendRuntimeSnapshot {
        FrontendRuntimeSnapshot {
            state: self.state,
            generation: self.generation,
            live_reader_descriptor: self.live_reader_descriptor.clone(),
            terminal_event_min_generation: self.terminal_event_min_generation,
            terminal_events: self.terminal_events.clone(),
            terminal_events_dropped_count: self.terminal_events_dropped_count,
            live_pump_reports: self.live_pump_reports.clone(),
            live_pump_reports_dropped_count: self.live_pump_reports_dropped_count,
            diagnostic_write_failures: self.diagnostic_write_failures.clone(),
            diagnostic_write_failures_dropped_count: self.diagnostic_write_failures_dropped_count,
            scan_session: self.scan_session.clone(),
            last_error: self.last_error.clone(),
            active_tune_request: self.active_tune_request.clone(),
            tune_request_sequence: self.tune_request_sequence,
            signal_state: self.signal_state,
            tune_lock_qualified: self.tune_lock_qualified,
        }
    }

    pub fn restore_from_rollback_snapshot(&mut self, snapshot: FrontendRuntimeSnapshot) {
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.live_reader_descriptor = snapshot.live_reader_descriptor;
        self.terminal_event_min_generation = snapshot.terminal_event_min_generation;
        self.terminal_events = snapshot.terminal_events;
        self.terminal_events_dropped_count = snapshot.terminal_events_dropped_count;
        self.live_pump_reports = snapshot.live_pump_reports;
        self.live_pump_reports_dropped_count = snapshot.live_pump_reports_dropped_count;
        self.diagnostic_write_failures = snapshot.diagnostic_write_failures;
        self.diagnostic_write_failures_dropped_count =
            snapshot.diagnostic_write_failures_dropped_count;
        self.scan_session = snapshot.scan_session;
        self.last_error = snapshot.last_error;
        self.active_tune_request = snapshot.active_tune_request;
        self.tune_request_sequence = snapshot.tune_request_sequence;
        self.signal_state = snapshot.signal_state;
        self.tune_lock_qualified = snapshot.tune_lock_qualified;
    }

    fn record_terminal_event(&mut self, event: FrontendTerminalEvent) {
        push_bounded(
            &mut self.terminal_events,
            &mut self.terminal_events_dropped_count,
            event,
        );
    }

    pub fn install_live_reader_for_worker_generation(
        &mut self,
        generation: u64,
        reader: FrontendLiveReaderDescriptor,
        kind: FrontendWorkerKind,
    ) -> Result<(), HalError> {
        self.commit_generation(generation)?;
        self.set_live_reader_descriptor(reader);
        match kind {
            FrontendWorkerKind::Tune => self.mark_tuning(generation),
            FrontendWorkerKind::Scan => self.mark_scanning(generation),
        }
        Ok(())
    }

    pub fn fence_for_worker_replacement(&mut self, generation: u64) -> Result<(), HalError> {
        self.commit_generation(generation)?;
        self.terminal_event_min_generation = generation;
        self.clear_live_reader_descriptor();
        self.active_tune_request = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.mark_idle();
        Ok(())
    }

    pub fn mark_worker_stop_pending_failure(
        &mut self,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        if generation != self.generation || self.live_reader_descriptor.is_some() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "worker-stop failure requires the matching fenced frontend generation",
            ));
        }
        self.active_tune_request = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.mark_failed(error);
        Ok(())
    }

    pub fn install_live_reader_for_fenced_worker_generation(
        &mut self,
        generation: u64,
        reader: FrontendLiveReaderDescriptor,
        kind: FrontendWorkerKind,
    ) -> Result<(), HalError> {
        if generation != self.generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "fenced frontend generation changed before worker installation",
            ));
        }
        self.set_live_reader_descriptor(reader);
        match kind {
            FrontendWorkerKind::Tune => self.mark_tuning(generation),
            FrontendWorkerKind::Scan => self.mark_scanning(generation),
        }
        Ok(())
    }

    pub fn commit_tune_after_fence(
        &mut self,
        generation: u64,
        reader: FrontendLiveReaderDescriptor,
        request: FrontendTuneRequest,
    ) -> Result<(), HalError> {
        if generation != self.generation
            || self.live_reader_descriptor.is_some()
            || self.state != FrontendRuntimeState::Idle
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "tune commit requires the matching fenced frontend generation",
            ));
        }
        self.live_reader_descriptor = Some(reader);
        self.active_tune_request = Some(request);
        self.scan_session = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.last_error = None;
        self.mark_tuning(generation);
        Ok(())
    }

    pub fn commit_scan_after_fence(
        &mut self,
        generation: u64,
        reader: FrontendLiveReaderDescriptor,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        if generation != self.generation
            || self.live_reader_descriptor.is_some()
            || self.state != FrontendRuntimeState::Idle
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan commit requires the matching fenced frontend generation",
            ));
        }
        let session = FrontendScanSession::start(generation, fingerprint, candidates)?;
        self.live_reader_descriptor = Some(reader);
        self.active_tune_request = None;
        self.scan_session = Some(session);
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.last_error = None;
        self.mark_scanning(generation);
        Ok(())
    }

    pub fn record_backend_request_failure_after_fence(
        &mut self,
        generation: u64,
        error: HalError,
        backend_stopped: bool,
    ) -> Result<(), HalError> {
        if generation != self.generation || self.live_reader_descriptor.is_some() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "backend request failure requires the matching fenced frontend generation",
            ));
        }
        self.active_tune_request = None;
        self.scan_session = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        if backend_stopped {
            self.last_error = Some(error);
            self.mark_idle();
        } else {
            self.mark_failed(error);
        }
        Ok(())
    }

    pub fn record_backend_activation_failure_after_commit(
        &mut self,
        generation: u64,
        error: HalError,
        backend_stopped: bool,
    ) -> Result<(), HalError> {
        if generation != self.generation
            || !matches!(
                self.state,
                FrontendRuntimeState::Tuning {
                    generation: current
                } | FrontendRuntimeState::Scanning {
                    generation: current
                } if current == generation
            )
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "backend activation failure requires the committed frontend generation",
            ));
        }
        self.live_reader_descriptor = None;
        self.active_tune_request = None;
        self.scan_session = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        if backend_stopped {
            self.last_error = Some(error);
            self.mark_idle();
        } else {
            self.mark_failed(error);
        }
        Ok(())
    }

    pub fn clear_live_reader_and_mark_idle(&mut self) {
        self.clear_live_reader_descriptor();
        self.active_tune_request = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.mark_idle();
    }

    pub fn clear_live_reader_and_mark_closing(&mut self) {
        self.clear_live_reader_descriptor();
        self.tune_lock_qualified = false;
        self.mark_closing();
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
        if signal_state != FrontendSignalState::Locked {
            self.tune_lock_qualified = false;
        }
        Ok(())
    }

    pub fn record_tune_lock_qualified(&mut self, generation: u64) -> Result<(), HalError> {
        if generation != self.generation
            || !matches!(
                self.state,
                FrontendRuntimeState::Tuning {
                    generation: current
                } if current == generation
            )
            || self.signal_state != FrontendSignalState::Locked
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "tune lock qualification requires the current locked tune generation",
            ));
        }
        self.tune_lock_qualified = true;
        Ok(())
    }

    pub fn record_live_pump_report(
        &mut self,
        generation: u64,
        report: FrontendLivePumpReport,
        cancel_reason: Option<FrontendWorkerCancelReason>,
    ) -> Result<(), HalError> {
        if generation != self.generation {
            let detail = format!(
                "DiagnosticWriteFailed: live pump report generation mismatch: report={} runtime={}",
                generation, self.generation
            );
            push_bounded(
                &mut self.diagnostic_write_failures,
                &mut self.diagnostic_write_failures_dropped_count,
                FrontendDiagnosticWriteFailure {
                    generation,
                    detail: detail.clone(),
                },
            );
            let error = HalError::internal(HalInternalKind::InvariantViolation, detail);
            self.last_error = Some(error.clone());
            return Err(error);
        }
        push_bounded(
            &mut self.live_pump_reports,
            &mut self.live_pump_reports_dropped_count,
            FrontendLivePumpDiagnostic::from_report(generation, report, cancel_reason),
        );
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
        let next_sequence = self.tune_request_sequence.checked_add(1).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend tune request sequence exhausted",
            )
        })?;
        self.active_tune_request = Some(request);
        self.tune_request_sequence = next_sequence;
        Ok(())
    }

    pub fn commit_stable_tune_reentry(
        &mut self,
        generation: u64,
        request: &FrontendTuneRequest,
    ) -> Result<u64, HalError> {
        if !matches!(
            self.state,
            FrontendRuntimeState::Tuning {
                generation: current
            } if current == generation
        ) || self.signal_state != FrontendSignalState::Locked
            || !self.tune_lock_qualified
            || self.active_tune_request.as_ref() != Some(request)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stable tune re-entry snapshot changed before sequence commit",
            ));
        }
        let next_sequence = self.tune_request_sequence.checked_add(1).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend tune request sequence exhausted",
            )
        })?;
        self.tune_request_sequence = next_sequence;
        Ok(next_sequence)
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
        self.record_terminal_event(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::ScanCancelled,
            reason: reason.into(),
        });
        self.live_reader_descriptor = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
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
        self.live_reader_descriptor = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.last_error = Some(HalError::internal(
            HalInternalKind::InvariantViolation,
            "scan backend failure",
        ));
        self.state = FrontendRuntimeState::Failed;
        Ok(())
    }

    pub fn mark_scan_submit_rejected_after_boundary(
        &mut self,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale scan submission failure generation cannot be recorded",
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
        self.record_terminal_event(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::ScanEnd,
            reason: FrontendTerminalEventReason::BackendFailure,
        });
        self.live_reader_descriptor = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.last_error = Some(error);
        self.state = FrontendRuntimeState::Idle;
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
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        if !has_next {
            self.record_terminal_event(FrontendTerminalEvent {
                generation,
                kind: FrontendTerminalEventKind::ScanEnd,
                reason: FrontendTerminalEventReason::End,
            });
            self.state = FrontendRuntimeState::Idle;
        }
        Ok(has_next)
    }

    pub fn mark_scan_session_locked_reported(&mut self, generation: u64) -> Result<(), HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale scan lock generation cannot be recorded",
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
        session.mark_locked_reported()?;
        self.live_reader_descriptor = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.state = FrontendRuntimeState::Idle;
        Ok(())
    }

    pub fn complete_locked_scan_continuation(
        &mut self,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        let previous_generation = self.generation;
        let is_matching_continuation = self.scan_session.as_ref().is_some_and(|session| {
            session.generation() == previous_generation
                && session.phase() == FrontendScanPhase::LockedReported
                && session.fingerprint() == fingerprint
        });
        if !is_matching_continuation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan continuation requires a matching locked-report session",
            ));
        }
        self.cancel_scan_session(
            previous_generation,
            FrontendWorkerCancelReason::SupersededByNewRequest,
        )?;
        self.commit_generation(generation)?;
        self.begin_scan_session(generation, fingerprint, candidates)?;
        let session = self.scan_session.as_mut().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "new scan continuation session is missing",
            )
        })?;
        session.complete();
        self.record_terminal_event(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::ScanEnd,
            reason: FrontendTerminalEventReason::End,
        });
        self.live_reader_descriptor = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.state = FrontendRuntimeState::Idle;
        Ok(())
    }

    pub fn complete_locked_scan_continuation_after_fence(
        &mut self,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        if generation != self.generation
            || !self.scan_session.as_ref().is_some_and(|session| {
                session.phase() == FrontendScanPhase::LockedReported
                    && session.fingerprint() == fingerprint
            })
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "fenced scan continuation requires the matching locked-report session",
            ));
        }
        let mut session = FrontendScanSession::start(generation, fingerprint, candidates)?;
        session.complete();
        self.scan_session = Some(session);
        self.record_terminal_event(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::ScanEnd,
            reason: FrontendTerminalEventReason::End,
        });
        self.live_reader_descriptor = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.state = FrontendRuntimeState::Idle;
        Ok(())
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
        if !matches!(
            self.state,
            FrontendRuntimeState::Tuning {
                generation: current
            } if current == generation
        ) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "tune failure can only be recorded for the active tune generation",
            ));
        }
        self.record_terminal_event(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::TuneFailed,
            reason: FrontendTerminalEventReason::BackendFailure,
        });
        self.live_reader_descriptor = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.mark_failed(error);
        Ok(())
    }

    pub fn mark_tune_submit_rejected_after_boundary(
        &mut self,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale tune submission failure generation cannot be recorded",
            ));
        }
        if !matches!(
            self.state,
            FrontendRuntimeState::Tuning {
                generation: current
            } if current == generation
        ) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "tune submission failure requires the active tune generation",
            ));
        }
        self.record_terminal_event(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::TuneFailed,
            reason: FrontendTerminalEventReason::BackendFailure,
        });
        self.live_reader_descriptor = None;
        self.active_tune_request = None;
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
        self.last_error = Some(error);
        self.state = FrontendRuntimeState::Idle;
        Ok(())
    }

    pub fn mark_tune_no_signal(&mut self, generation: u64) -> Result<(), HalError> {
        if !self.should_accept_terminal_event(generation) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "stale tune no-signal generation cannot be recorded",
            ));
        }
        if !matches!(
            self.state,
            FrontendRuntimeState::Tuning {
                generation: current
            } if current == generation
        ) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "tune no-signal requires the active tune generation",
            ));
        }
        self.record_terminal_event(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::TuneNoSignal,
            reason: FrontendTerminalEventReason::NoSignal,
        });
        self.live_reader_descriptor = None;
        self.active_tune_request = None;
        self.signal_state = FrontendSignalState::NoSignal;
        self.tune_lock_qualified = false;
        self.state = FrontendRuntimeState::Idle;
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
        self.record_terminal_event(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::ScanEnd,
            reason: FrontendTerminalEventReason::CallbackFailure,
        });
        self.last_error = Some(HalError::callback_failed(
            "IFrontendCallback.onScanMessage",
            "scan callback delivery failed",
        ));
        self.signal_state = FrontendSignalState::Unknown;
        self.tune_lock_qualified = false;
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
        if !matches!(
            self.state,
            FrontendRuntimeState::Scanning {
                generation: current
            } if current == generation
        ) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan cancellation can only be recorded for the active scan generation",
            ));
        }
        self.record_terminal_event(FrontendTerminalEvent {
            generation,
            kind: FrontendTerminalEventKind::ScanCancelled,
            reason: reason.into(),
        });
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn next_generation(&mut self) -> Result<u64, HalError> {
        let next = self.checked_next_generation()?;
        self.generation = next;
        Ok(next)
    }

    pub(crate) fn mark_tuning(&mut self, generation: u64) {
        self.terminal_event_min_generation = generation;
        self.state = FrontendRuntimeState::Tuning { generation };
    }
    pub(crate) fn mark_scanning(&mut self, generation: u64) {
        self.terminal_event_min_generation = generation;
        self.state = FrontendRuntimeState::Scanning { generation };
    }
    pub(crate) fn mark_idle(&mut self) {
        self.state = FrontendRuntimeState::Idle;
    }
    pub(crate) fn mark_closing(&mut self) {
        self.state = FrontendRuntimeState::Closing;
    }
    pub(crate) fn set_live_reader_descriptor(&mut self, reader: FrontendLiveReaderDescriptor) {
        self.live_reader_descriptor = Some(reader);
    }
    pub(crate) fn clear_live_reader_descriptor(&mut self) {
        self.live_reader_descriptor = None;
    }
    pub(crate) fn mark_failed(&mut self, error: HalError) {
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
    fn live_pump_report_is_recorded_as_frontend_diagnostic() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        runtime.mark_tuning(1);
        runtime
            .record_live_pump_report(
                1,
                FrontendLivePumpReport {
                    packets_delivered: 3,
                    malformed_bytes: 2,
                    read_retries: 0,
                    read_retry_counter_saturated: false,
                    stopped_by_cancel: false,
                    reached_eof: true,
                },
                None,
            )
            .unwrap();
        let report = &runtime.live_pump_reports()[0];
        assert_eq!(report.packets_delivered, 3);
        assert_eq!(report.malformed_bytes, 2);
        assert_eq!(report.cancel_reason, None);
        assert_eq!(report.terminal_reason, FrontendLivePumpTerminalReason::Eof);
        assert_eq!(report.join_result, FrontendLivePumpJoinResult::Joined);
    }

    #[test]
    fn live_pump_report_records_cancel_reason_and_malformed_count() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(3).unwrap();
        runtime.mark_tuning(3);
        runtime
            .record_live_pump_report(
                3,
                FrontendLivePumpReport {
                    packets_delivered: 5,
                    malformed_bytes: 9,
                    read_retries: 0,
                    read_retry_counter_saturated: false,
                    stopped_by_cancel: true,
                    reached_eof: false,
                },
                Some(FrontendWorkerCancelReason::SupersededByNewRequest),
            )
            .unwrap();
        let report = &runtime.live_pump_reports()[0];
        assert_eq!(report.packets_delivered, 5);
        assert_eq!(report.malformed_bytes, 9);
        assert_eq!(
            report.cancel_reason,
            Some(FrontendWorkerCancelReason::SupersededByNewRequest)
        );
        assert_eq!(
            report.terminal_reason,
            FrontendLivePumpTerminalReason::Cancelled
        );
    }

    #[test]
    fn live_pump_report_write_failure_is_diagnostic_not_silent() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(2).unwrap();
        runtime.mark_tuning(2);
        let result = runtime.record_live_pump_report(
            1,
            FrontendLivePumpReport {
                packets_delivered: 0,
                malformed_bytes: 4,
                read_retries: 0,
                read_retry_counter_saturated: false,
                stopped_by_cancel: true,
                reached_eof: false,
            },
            Some(FrontendWorkerCancelReason::StopRequested),
        );
        assert!(result.is_err());
        assert_eq!(runtime.diagnostic_write_failures().len(), 1);
        assert!(matches!(
            runtime.last_error(),
            Some(HalError::Internal { .. })
        ));
    }

    #[test]
    fn frontend_runtime_diagnostic_histories_are_bounded() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        runtime.mark_tuning(1);
        for _ in 0..(FRONTEND_RUNTIME_DIAGNOSTIC_CAPACITY + 3) {
            runtime
                .record_live_pump_report(
                    1,
                    FrontendLivePumpReport {
                        packets_delivered: 1,
                        malformed_bytes: 0,
                        read_retries: 0,
                        read_retry_counter_saturated: false,
                        stopped_by_cancel: false,
                        reached_eof: true,
                    },
                    None,
                )
                .unwrap();
        }
        assert_eq!(
            runtime.live_pump_reports().len(),
            FRONTEND_RUNTIME_DIAGNOSTIC_CAPACITY
        );
        assert_eq!(runtime.live_pump_reports_dropped_count(), 3);
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
    fn new_generation_rejects_stale_signal_readback() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        runtime.mark_tuning(1);
        runtime.commit_generation(2).unwrap();
        runtime.mark_tuning(2);

        assert!(runtime
            .record_signal_state(1, FrontendSignalState::Locked)
            .is_err());
        assert_eq!(
            runtime.snapshot().signal_state,
            FrontendSignalState::Unknown
        );
    }

    #[test]
    fn tune_lock_qualification_is_generation_bound_and_cleared_on_lock_loss() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        let request = FrontendTuneRequest {
            system: maleicacid_tuner_hal2_common::FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Required(
                    true,
                ),
        };
        runtime.commit_generation(1).unwrap();
        runtime.mark_tuning(1);
        runtime
            .commit_active_tune_request(1, request.clone())
            .unwrap();
        runtime
            .record_signal_state(1, FrontendSignalState::Locked)
            .unwrap();
        assert!(!runtime.snapshot().tune_lock_qualified);
        assert!(runtime.commit_stable_tune_reentry(1, &request).is_err());

        runtime.record_tune_lock_qualified(1).unwrap();
        assert!(runtime.snapshot().tune_lock_qualified);
        assert_eq!(runtime.commit_stable_tune_reentry(1, &request), Ok(2));

        runtime
            .record_signal_state(1, FrontendSignalState::NoSignal)
            .unwrap();
        assert!(!runtime.snapshot().tune_lock_qualified);

        runtime.commit_generation(2).unwrap();
        runtime.mark_tuning(2);
        runtime
            .record_signal_state(2, FrontendSignalState::Locked)
            .unwrap();
        assert!(runtime.record_tune_lock_qualified(1).is_err());
        assert!(!runtime.snapshot().tune_lock_qualified);
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
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
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
            partial_reception:
                maleicacid_tuner_hal2_common::FrontendIsdbtPartialReceptionRequirement::Unspecified,
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
