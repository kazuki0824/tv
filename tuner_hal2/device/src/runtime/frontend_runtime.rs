use std::collections::BTreeMap;
use std::fmt;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard,
};

use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, FrontendTuneRequest, HalError, HalInternalKind,
};

use super::scan_session::{
    FrontendScanPhase, FrontendScanSession, FrontendScanTerminalReason,
};
use super::{
    FrontendLivePumpReport, FrontendLiveReaderDescriptor, FrontendWorkerCancelReason,
    FrontendWorkerKind,
};

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
struct FrontendRuntimeSnapshot {
    state: FrontendRuntimeState,
    generation: u64,
    live_reader_descriptor: Option<FrontendLiveReaderDescriptor>,
    terminal_event_min_generation: u64,
    scan_session: Option<FrontendScanSession>,
    active_tune_request: Option<FrontendTuneRequest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FrontendRuntimeRollbackExpectedPostState {
    Tune {
        generation: u64,
        live_reader_descriptor: FrontendLiveReaderDescriptor,
        terminal_event_min_generation: u64,
        active_tune_request: FrontendTuneRequest,
    },
    Scan {
        generation: u64,
        live_reader_descriptor: FrontendLiveReaderDescriptor,
        terminal_event_min_generation: u64,
        scan_fingerprint: String,
        active_tune_request: Option<FrontendTuneRequest>,
    },
}

impl FrontendRuntimeRollbackExpectedPostState {
    fn matches(&self, runtime: &FrontendRuntime) -> bool {
        match self {
            Self::Tune {
                generation,
                live_reader_descriptor,
                terminal_event_min_generation,
                active_tune_request,
            } => {
                runtime.generation == *generation
                    && matches!(
                        runtime.state,
                        FrontendRuntimeState::Tuning { generation: current }
                            if current == *generation
                    )
                    && runtime.live_reader_descriptor.as_ref()
                        == Some(live_reader_descriptor)
                    && runtime.terminal_event_min_generation
                        == *terminal_event_min_generation
                    && runtime.scan_session.is_none()
                    && runtime.active_tune_request.as_ref() == Some(active_tune_request)
            }
            Self::Scan {
                generation,
                live_reader_descriptor,
                terminal_event_min_generation,
                scan_fingerprint,
                active_tune_request,
            } => {
                runtime.generation == *generation
                    && matches!(
                        runtime.state,
                        FrontendRuntimeState::Scanning { generation: current }
                            if current == *generation
                    )
                    && runtime.live_reader_descriptor.as_ref()
                        == Some(live_reader_descriptor)
                    && runtime.terminal_event_min_generation
                        == *terminal_event_min_generation
                    && runtime.active_tune_request == *active_tune_request
                    && runtime.scan_session.as_ref().is_some_and(|session| {
                        session.generation() == *generation
                            && session.fingerprint() == scan_fingerprint
                    })
            }
        }
    }
}

#[derive(Debug, Default)]
struct FrontendRuntimeRollbackLedger {
    next_token_id: u64,
    snapshots: BTreeMap<u64, FrontendRuntimeSnapshot>,
    expected_post_states: BTreeMap<u64, FrontendRuntimeRollbackExpectedPostState>,
    active_token_id: Option<u64>,
}

fn lock_rollback_ledger(
    ledger: &Arc<Mutex<FrontendRuntimeRollbackLedger>>,
) -> Result<MutexGuard<'_, FrontendRuntimeRollbackLedger>, HalError> {
    ledger.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend rollback authority ledger lock poisoned",
        )
    })
}

/// frontend worker transaction が所有する不透明な one-shot rollback token。
///
/// snapshot 本体は frontend runtime 内部 ledger に残し、token は復元 authority を表す ID と
/// ledger identity だけを保持する。token が復元に使われず破棄された場合も、ledger entry を
/// 除去する。
pub struct FrontendRuntimeRollbackToken {
    frontend_id: i32,
    token_id: u64,
    ledger: Arc<Mutex<FrontendRuntimeRollbackLedger>>,
    ledger_failure_count: Arc<AtomicU64>,
    armed: bool,
}

impl fmt::Debug for FrontendRuntimeRollbackToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrontendRuntimeRollbackToken")
            .finish_non_exhaustive()
    }
}

impl FrontendRuntimeRollbackToken {
    pub fn discard_without_runtime(mut self) -> Result<(), HalError> {
        let mut ledger = lock_rollback_ledger(&self.ledger)?;
        if ledger.active_token_id != Some(self.token_id)
            || !ledger.snapshots.contains_key(&self.token_id)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token is not the active authority",
            ));
        }
        ledger.snapshots.remove(&self.token_id);
        ledger.expected_post_states.remove(&self.token_id);
        ledger.active_token_id = None;
        drop(ledger);
        self.armed = false;
        Ok(())
    }
}

impl Drop for FrontendRuntimeRollbackToken {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        match lock_rollback_ledger(&self.ledger) {
            Ok(mut ledger) => {
                ledger.snapshots.remove(&self.token_id);
                ledger.expected_post_states.remove(&self.token_id);
                if ledger.active_token_id == Some(self.token_id) {
                    ledger.active_token_id = None;
                }
            }
            Err(_) => {
                let _ = self.ledger_failure_count.fetch_update(
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                    |value| Some(value.saturating_add(1)),
                );
            }
        }
        self.armed = false;
    }
}

/// rollback token と worker replacement に必要な直前 request だけを束ねた取得結果。
///
/// token 本体から phase/state 情報を読み出す accessor は持たせない。
#[derive(Debug)]
pub struct FrontendRuntimeRollbackCapture {
    token: FrontendRuntimeRollbackToken,
    previous_active_tune_request: Option<FrontendTuneRequest>,
}

impl FrontendRuntimeRollbackCapture {
    pub fn into_token(self) -> FrontendRuntimeRollbackToken {
        self.token
    }

    pub fn into_replacement_parts(
        self,
    ) -> (FrontendRuntimeRollbackToken, Option<FrontendTuneRequest>) {
        (self.token, self.previous_active_tune_request)
    }
}

/// public query façade が使う目的限定の read-only DTO。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendRuntimeStatusSnapshot {
    state: FrontendRuntimeState,
    generation: u64,
    signal_state: FrontendSignalState,
}

impl FrontendRuntimeStatusSnapshot {
    pub const fn state(self) -> FrontendRuntimeState {
        self.state
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn signal_state(self) -> FrontendSignalState {
        self.signal_state
    }
}

/// frontend worker diagnostic query 用の read-only DTO。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendRuntimeDiagnosticSnapshot {
    terminal_events: Vec<FrontendTerminalEvent>,
    live_pump_reports: Vec<FrontendLivePumpDiagnostic>,
    diagnostic_write_failures: Vec<FrontendDiagnosticWriteFailure>,
    last_error: Option<HalError>,
    rollback_ledger_failures: u64,
}

impl FrontendRuntimeDiagnosticSnapshot {
    pub fn terminal_events(&self) -> &[FrontendTerminalEvent] {
        &self.terminal_events
    }

    pub fn live_pump_reports(&self) -> &[FrontendLivePumpDiagnostic] {
        &self.live_pump_reports
    }

    pub fn diagnostic_write_failures(&self) -> &[FrontendDiagnosticWriteFailure] {
        &self.diagnostic_write_failures
    }

    pub fn last_error(&self) -> Option<&HalError> {
        self.last_error.as_ref()
    }

    pub const fn rollback_ledger_failures(&self) -> u64 {
        self.rollback_ledger_failures
    }
}

#[derive(Debug)]
pub struct FrontendWorkerInstallRequest {
    generation: u64,
    reader: FrontendLiveReaderDescriptor,
    kind: FrontendWorkerKind,
}

impl FrontendWorkerInstallRequest {
    pub fn new(
        generation: u64,
        reader: FrontendLiveReaderDescriptor,
        kind: FrontendWorkerKind,
    ) -> Self {
        Self {
            generation,
            reader,
            kind,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendLiveDataCompletion {
    Idle,
    Closing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendLiveDataCompletionRequest {
    Worker {
        generation: u64,
        kind: FrontendWorkerKind,
        completion: FrontendLiveDataCompletion,
    },
    NoWorker {
        generation: u64,
        expected_state: FrontendRuntimeState,
        completion: FrontendLiveDataCompletion,
    },
}

impl FrontendLiveDataCompletionRequest {
    pub const fn worker(
        generation: u64,
        kind: FrontendWorkerKind,
        completion: FrontendLiveDataCompletion,
    ) -> Self {
        Self::Worker {
            generation,
            kind,
            completion,
        }
    }

    pub const fn no_worker(
        generation: u64,
        expected_state: FrontendRuntimeState,
        completion: FrontendLiveDataCompletion,
    ) -> Self {
        Self::NoWorker {
            generation,
            expected_state,
            completion,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendSignalRecordRequest {
    generation: u64,
    signal_state: FrontendSignalState,
}

impl FrontendSignalRecordRequest {
    pub const fn new(generation: u64, signal_state: FrontendSignalState) -> Self {
        Self {
            generation,
            signal_state,
        }
    }
}

#[derive(Debug)]
pub struct FrontendLivePumpCompletionRequest {
    generation: u64,
    report: FrontendLivePumpReport,
    cancel_reason: Option<FrontendWorkerCancelReason>,
}

impl FrontendLivePumpCompletionRequest {
    pub fn new(
        generation: u64,
        report: FrontendLivePumpReport,
        cancel_reason: Option<FrontendWorkerCancelReason>,
    ) -> Self {
        Self {
            generation,
            report,
            cancel_reason,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendTuneCommitRequest {
    generation: u64,
    request: FrontendTuneRequest,
}

impl FrontendTuneCommitRequest {
    pub fn new(generation: u64, request: FrontendTuneRequest) -> Self {
        Self {
            generation,
            request,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendScanStartRequest {
    generation: u64,
    fingerprint: String,
    candidates: Vec<FrontendTuneRequest>,
}

impl FrontendScanStartRequest {
    pub fn new(
        generation: u64,
        fingerprint: impl Into<String>,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Self {
        Self {
            generation,
            fingerprint: fingerprint.into(),
            candidates,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FrontendScanTransitionKind {
    Cancel(FrontendWorkerCancelReason),
    BackendFailed,
    AdvanceAfterCandidate,
    CallbackFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendScanTransitionRequest {
    generation: u64,
    kind: FrontendScanTransitionKind,
}

impl FrontendScanTransitionRequest {
    pub const fn cancel(generation: u64, reason: FrontendWorkerCancelReason) -> Self {
        Self {
            generation,
            kind: FrontendScanTransitionKind::Cancel(reason),
        }
    }

    pub const fn backend_failed(generation: u64) -> Self {
        Self {
            generation,
            kind: FrontendScanTransitionKind::BackendFailed,
        }
    }

    pub const fn advance_after_candidate(generation: u64) -> Self {
        Self {
            generation,
            kind: FrontendScanTransitionKind::AdvanceAfterCandidate,
        }
    }

    pub const fn callback_failed(generation: u64) -> Self {
        Self {
            generation,
            kind: FrontendScanTransitionKind::CallbackFailed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendScanTransitionOutcome {
    Applied,
    CandidateAdvanced { has_next: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendTuneWorkerFailureRequest {
    generation: u64,
    error: HalError,
}

#[derive(Clone, Debug)]
pub struct FrontendRollbackFailureRequest {
    error: HalError,
}

impl FrontendRollbackFailureRequest {
    pub fn new(error: HalError) -> Self {
        Self { error }
    }
}

impl FrontendTuneWorkerFailureRequest {
    pub fn new(generation: u64, error: HalError) -> Self {
        Self { generation, error }
    }
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
    live_pump_reports: Vec<FrontendLivePumpDiagnostic>,
    diagnostic_write_failures: Vec<FrontendDiagnosticWriteFailure>,
    scan_session: Option<FrontendScanSession>,
    last_error: Option<HalError>,
    active_tune_request: Option<FrontendTuneRequest>,
    signal_state: FrontendSignalState,
    rollback_ledger: Arc<Mutex<FrontendRuntimeRollbackLedger>>,
    rollback_ledger_failure_count: Arc<AtomicU64>,
}

pub struct FrontendRuntimeQuery<'a> {
    runtime: &'a FrontendRuntime,
}

impl<'a> FrontendRuntimeQuery<'a> {
    pub fn status_snapshot(&self) -> FrontendRuntimeStatusSnapshot {
        self.runtime.status_snapshot()
    }

    pub fn diagnostic_snapshot(&self) -> FrontendRuntimeDiagnosticSnapshot {
        self.runtime.diagnostic_snapshot()
    }

    pub fn live_reader_descriptor_for_live_pump(
        &self,
    ) -> Option<FrontendLiveReaderDescriptor> {
        self.runtime.live_reader_descriptor_for_live_pump()
    }

    pub fn matches_rollback_token(
        &self,
        token: &FrontendRuntimeRollbackToken,
    ) -> Result<bool, HalError> {
        self.runtime.matches_rollback_token(token)
    }
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
            live_pump_reports: Vec::new(),
            diagnostic_write_failures: Vec::new(),
            scan_session: None,
            last_error: None,
            active_tune_request: None,
            signal_state: FrontendSignalState::Unknown,
            rollback_ledger: Arc::new(Mutex::new(FrontendRuntimeRollbackLedger::default())),
            rollback_ledger_failure_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn query(&self) -> FrontendRuntimeQuery<'_> {
        FrontendRuntimeQuery { runtime: self }
    }


    pub fn frontend_id(&self) -> i32 {
        self.frontend_id
    }
    pub fn backend_kind(&self) -> FrontendBackendKind {
        self.backend_kind
    }
    #[cfg(test)]
    fn state(&self) -> FrontendRuntimeState {
        self.state
    }
    fn generation(&self) -> u64 {
        self.generation
    }

    pub fn checked_next_worker_generation(&self) -> Result<u64, HalError> {
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

    fn commit_generation(&mut self, generation: u64) -> Result<(), HalError> {
        if generation <= self.generation {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend operation generation must monotonically advance",
            ));
        }
        self.generation = generation;
        Ok(())
    }

    fn status_snapshot(&self) -> FrontendRuntimeStatusSnapshot {
        FrontendRuntimeStatusSnapshot {
            state: self.state,
            generation: self.generation,
            signal_state: self.signal_state,
        }
    }

    fn diagnostic_snapshot(&self) -> FrontendRuntimeDiagnosticSnapshot {
        FrontendRuntimeDiagnosticSnapshot {
            terminal_events: self.terminal_events.clone(),
            live_pump_reports: self.live_pump_reports.clone(),
            diagnostic_write_failures: self.diagnostic_write_failures.clone(),
            last_error: self.last_error.clone(),
            rollback_ledger_failures: self
                .rollback_ledger_failure_count
                .load(Ordering::Relaxed),
        }
    }

    fn live_reader_descriptor_for_live_pump(&self) -> Option<FrontendLiveReaderDescriptor> {
        self.live_reader_descriptor.clone()
    }

    fn should_accept_terminal_event(&self, generation: u64) -> bool {
        generation >= self.terminal_event_min_generation && generation == self.generation
    }

    #[cfg(test)]
    fn active_scan_session(&self) -> Option<&FrontendScanSession> {
        self.scan_session.as_ref()
    }

    fn snapshot(&self) -> FrontendRuntimeSnapshot {
        FrontendRuntimeSnapshot {
            state: self.state,
            generation: self.generation,
            live_reader_descriptor: self.live_reader_descriptor.clone(),
            terminal_event_min_generation: self.terminal_event_min_generation,
            scan_session: self.scan_session.clone(),
            active_tune_request: self.active_tune_request.clone(),
        }
    }

    pub fn prepare_worker_rollback(&mut self) -> Result<FrontendRuntimeRollbackCapture, HalError> {
        let snapshot = self.snapshot();
        let previous_active_tune_request = self.active_tune_request.clone();
        let ledger = Arc::clone(&self.rollback_ledger);
        let token_id = {
            let mut ledger_state = lock_rollback_ledger(&ledger)?;
            if ledger_state.active_token_id.is_some() {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend rollback authority is already active",
                ));
            }
            let token_id = ledger_state
                .next_token_id
                .checked_add(1)
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend rollback token id exhausted",
                    )
                })?;
            ledger_state.next_token_id = token_id;
            ledger_state.snapshots.insert(token_id, snapshot);
            ledger_state.active_token_id = Some(token_id);
            token_id
        };
        Ok(FrontendRuntimeRollbackCapture {
            token: FrontendRuntimeRollbackToken {
                frontend_id: self.frontend_id,
                token_id,
                ledger,
                ledger_failure_count: Arc::clone(&self.rollback_ledger_failure_count),
                armed: true,
            },
            previous_active_tune_request,
        })
    }

    fn matches_rollback_token(
        &self,
        token: &FrontendRuntimeRollbackToken,
    ) -> Result<bool, HalError> {
        if token.frontend_id != self.frontend_id
            || !Arc::ptr_eq(&token.ledger, &self.rollback_ledger)
        {
            return Ok(false);
        }
        let ledger = lock_rollback_ledger(&self.rollback_ledger)?;
        if ledger.active_token_id != Some(token.token_id) {
            return Ok(false);
        }
        let Some(snapshot) = ledger.snapshots.get(&token.token_id) else {
            return Ok(false);
        };
        Ok(self.generation == snapshot.generation
            && self.state == snapshot.state
            && self.live_reader_descriptor == snapshot.live_reader_descriptor
            && self.terminal_event_min_generation == snapshot.terminal_event_min_generation
            && self.scan_session == snapshot.scan_session
            && self.active_tune_request == snapshot.active_tune_request)
    }

    pub fn commit_tune_worker_rollback_expected_post_state(
        &mut self,
        token: &FrontendRuntimeRollbackToken,
        generation: u64,
        live_reader_descriptor: FrontendLiveReaderDescriptor,
        active_tune_request: FrontendTuneRequest,
    ) -> Result<(), HalError> {
        if token.frontend_id != self.frontend_id
            || !Arc::ptr_eq(&token.ledger, &self.rollback_ledger)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token does not belong to runtime",
            ));
        }
        if generation != self.checked_next_worker_generation()? || self.scan_session.is_some() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "tune rollback expected post state does not match the next tune operation",
            ));
        }
        let expected = FrontendRuntimeRollbackExpectedPostState::Tune {
            generation,
            live_reader_descriptor: live_reader_descriptor.clone(),
            terminal_event_min_generation: Some(generation),
            active_tune_request: active_tune_request.clone(),
        };
        let ledger_handle = Arc::clone(&self.rollback_ledger);
        let mut ledger = lock_rollback_ledger(&ledger_handle)?;
        if ledger.active_token_id != Some(token.token_id)
            || !ledger.snapshots.contains_key(&token.token_id)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token is not the active authority",
            ));
        }
        // Generation, reader, lifecycle, active request and rollback matcher become visible as
        // one mutation while the rollback ledger is locked. Every fallible validation precedes
        // the first runtime field update.
        self.generation = generation;
        self.live_reader_descriptor = Some(live_reader_descriptor);
        self.mark_tuning(generation);
        self.active_tune_request = Some(active_tune_request);
        ledger.expected_post_states.insert(token.token_id, expected);
        Ok(())
    }

    pub fn begin_scan_worker_rollback_expected_post_state(
        &mut self,
        token: &FrontendRuntimeRollbackToken,
        generation: u64,
        live_reader_descriptor: FrontendLiveReaderDescriptor,
        scan_fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        if token.frontend_id != self.frontend_id
            || !Arc::ptr_eq(&token.ledger, &self.rollback_ledger)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token does not belong to runtime",
            ));
        }
        if generation != self.checked_next_worker_generation()? {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "scan rollback expected post state does not match the next scan operation",
            ));
        }
        let session = FrontendScanSession::start(
            generation,
            scan_fingerprint.clone(),
            candidates,
        )?;
        let expected = FrontendRuntimeRollbackExpectedPostState::Scan {
            generation,
            live_reader_descriptor: live_reader_descriptor.clone(),
            terminal_event_min_generation: Some(generation),
            scan_fingerprint,
            active_tune_request: self.active_tune_request.clone(),
        };
        let ledger_handle = Arc::clone(&self.rollback_ledger);
        let mut ledger = lock_rollback_ledger(&ledger_handle)?;
        if ledger.active_token_id != Some(token.token_id)
            || !ledger.snapshots.contains_key(&token.token_id)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token is not the active authority",
            ));
        }
        // Generation, reader, lifecycle, session and rollback matcher are committed together.
        self.generation = generation;
        self.live_reader_descriptor = Some(live_reader_descriptor);
        self.mark_scanning(generation);
        self.scan_session = Some(session);
        ledger.expected_post_states.insert(token.token_id, expected);
        Ok(())
    }

    pub fn restore_worker_rollback(
        &mut self,
        token: &mut FrontendRuntimeRollbackToken,
    ) -> Result<(), HalError> {
        if token.frontend_id != self.frontend_id
            || !Arc::ptr_eq(&token.ledger, &self.rollback_ledger)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token does not belong to runtime",
            ));
        }
        let ledger_handle = Arc::clone(&self.rollback_ledger);
        let mut ledger = lock_rollback_ledger(&ledger_handle)?;
        if ledger.active_token_id != Some(token.token_id) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token is not the active authority",
            ));
        }
        let snapshot = ledger.snapshots.get(&token.token_id).cloned().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token was already consumed or discarded",
            )
        })?;
        let is_pre_operation_state = self.generation == snapshot.generation
            && self.state == snapshot.state
            && self.live_reader_descriptor == snapshot.live_reader_descriptor
            && self.terminal_event_min_generation == snapshot.terminal_event_min_generation
            && self.scan_session == snapshot.scan_session
            && self.active_tune_request == snapshot.active_tune_request;
        let is_expected_post_operation_state = ledger
            .expected_post_states
            .get(&token.token_id)
            .is_some_and(|expected| expected.matches(self));
        if !is_pre_operation_state && !is_expected_post_operation_state {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token is stale for the current runtime state",
            ));
        }
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.live_reader_descriptor = snapshot.live_reader_descriptor;
        self.terminal_event_min_generation = snapshot.terminal_event_min_generation;
        self.scan_session = snapshot.scan_session;
        self.active_tune_request = snapshot.active_tune_request;
        if ledger.snapshots.remove(&token.token_id).is_none() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback authority changed during restore",
            ));
        }
        ledger.expected_post_states.remove(&token.token_id);
        ledger.active_token_id = None;
        token.armed = false;
        Ok(())
    }

    pub fn discard_worker_rollback(
        &mut self,
        token: &mut FrontendRuntimeRollbackToken,
    ) -> Result<(), HalError> {
        if token.frontend_id != self.frontend_id
            || !Arc::ptr_eq(&token.ledger, &self.rollback_ledger)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token does not belong to runtime",
            ));
        }
        let mut ledger = lock_rollback_ledger(&self.rollback_ledger)?;
        if ledger.active_token_id != Some(token.token_id)
            || ledger.snapshots.remove(&token.token_id).is_none()
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend rollback token is not the active authority",
            ));
        }
        ledger.expected_post_states.remove(&token.token_id);
        ledger.active_token_id = None;
        token.armed = false;
        Ok(())
    }

    pub fn quarantine_rollback_failure(
        &mut self,
        request: FrontendRollbackFailureRequest,
    ) {
        self.state = FrontendRuntimeState::Failed;
        self.live_reader_descriptor = None;
        self.scan_session = None;
        self.active_tune_request = None;
        self.last_error = Some(request.error);
    }

    pub fn install_worker(
        &mut self,
        request: FrontendWorkerInstallRequest,
    ) -> Result<(), HalError> {
        self.install_live_reader_for_worker_generation(
            request.generation,
            request.reader,
            request.kind,
        )
    }

    pub fn complete_live_data(
        &mut self,
        request: FrontendLiveDataCompletionRequest,
    ) -> Result<(), HalError> {
        let completion = match request {
            FrontendLiveDataCompletionRequest::Worker {
                generation,
                kind,
                completion,
            } => {
                let state_matches = match (kind, self.state) {
                    (
                        FrontendWorkerKind::Tune,
                        FrontendRuntimeState::Tuning { generation: state_generation },
                    )
                    | (
                        FrontendWorkerKind::Scan,
                        FrontendRuntimeState::Scanning { generation: state_generation },
                    ) => state_generation == generation,
                    _ => false,
                };
                let session_matches = match kind {
                    FrontendWorkerKind::Tune => self.scan_session.is_none(),
                    FrontendWorkerKind::Scan => self
                        .scan_session
                        .as_ref()
                        .is_some_and(|session| session.generation == generation),
                };
                if self.generation != generation
                    || !state_matches
                    || !session_matches
                    || self.live_reader_descriptor.is_none()
                {
                    return Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend live-data completion no longer matches worker generation/lifecycle",
                    ));
                }
                completion
            }
            FrontendLiveDataCompletionRequest::NoWorker {
                generation,
                expected_state,
                completion,
            } => {
                if self.generation != generation
                    || self.state != expected_state
                    || self.live_reader_descriptor.is_some()
                    || matches!(
                        self.state,
                        FrontendRuntimeState::Tuning { .. }
                            | FrontendRuntimeState::Scanning { .. }
                    )
                {
                    return Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend no-worker completion no longer matches generation/lifecycle",
                    ));
                }
                completion
            }
        };
        match completion {
            FrontendLiveDataCompletion::Idle => self.clear_live_reader_and_mark_idle(),
            FrontendLiveDataCompletion::Closing => self.clear_live_reader_and_mark_closing(),
        }
        Ok(())
    }

    pub fn record_signal(
        &mut self,
        request: FrontendSignalRecordRequest,
    ) -> Result<(), HalError> {
        self.record_signal_state(request.generation, request.signal_state)
    }

    pub fn record_live_pump_completion(
        &mut self,
        request: FrontendLivePumpCompletionRequest,
    ) -> Result<(), HalError> {
        self.record_live_pump_report(
            request.generation,
            request.report,
            request.cancel_reason,
        )
    }

    pub fn commit_tune(&mut self, request: FrontendTuneCommitRequest) -> Result<(), HalError> {
        self.commit_active_tune_request(request.generation, request.request)
    }

    pub fn start_scan(&mut self, request: FrontendScanStartRequest) -> Result<(), HalError> {
        self.begin_scan_session(
            request.generation,
            request.fingerprint,
            request.candidates,
        )
    }

    pub fn apply_scan_transition(
        &mut self,
        request: FrontendScanTransitionRequest,
    ) -> Result<FrontendScanTransitionOutcome, HalError> {
        match request.kind {
            FrontendScanTransitionKind::Cancel(reason) => {
                self.cancel_scan_session(request.generation, reason)?;
                Ok(FrontendScanTransitionOutcome::Applied)
            }
            FrontendScanTransitionKind::BackendFailed => {
                self.mark_scan_session_backend_failed(request.generation)?;
                Ok(FrontendScanTransitionOutcome::Applied)
            }
            FrontendScanTransitionKind::AdvanceAfterCandidate => {
                let has_next = self.advance_scan_session_after_candidate(request.generation)?;
                Ok(FrontendScanTransitionOutcome::CandidateAdvanced { has_next })
            }
            FrontendScanTransitionKind::CallbackFailed => {
                self.mark_scan_session_callback_failed(request.generation)?;
                Ok(FrontendScanTransitionOutcome::Applied)
            }
        }
    }

    pub fn record_tune_worker_failure(
        &mut self,
        request: FrontendTuneWorkerFailureRequest,
    ) -> Result<(), HalError> {
        self.mark_tune_worker_failed(request.generation, request.error)
    }

    fn install_live_reader_for_worker_generation(
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

    fn clear_live_reader_and_mark_idle(&mut self) {
        self.clear_live_reader_descriptor();
        self.mark_idle();
    }

    fn clear_live_reader_and_mark_closing(&mut self) {
        self.clear_live_reader_descriptor();
        self.mark_closing();
    }

    fn record_signal_state(
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

    fn record_live_pump_report(
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
            self.diagnostic_write_failures
                .push(FrontendDiagnosticWriteFailure {
                    generation,
                    detail: detail.clone(),
                });
            let error = HalError::internal(HalInternalKind::InvariantViolation, detail);
            self.last_error = Some(error.clone());
            return Err(error);
        }
        self.live_pump_reports
            .push(FrontendLivePumpDiagnostic::from_report(
                generation,
                report,
                cancel_reason,
            ));
        Ok(())
    }

    fn commit_active_tune_request(
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

    fn begin_scan_session(
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

    fn cancel_scan_session(
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

    fn mark_scan_session_backend_failed(&mut self, generation: u64) -> Result<(), HalError> {
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
    fn advance_scan_session_after_candidate(
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

    fn mark_tune_worker_failed(
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

    fn mark_scan_session_callback_failed(&mut self, generation: u64) -> Result<(), HalError> {
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

    fn last_terminal_event(&self) -> Option<FrontendTerminalEvent> {
        self.terminal_events.last().copied()
    }
    fn last_error(&self) -> Option<&HalError> {
        self.last_error.as_ref()
    }

    fn record_scan_cancelled(
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

    #[cfg(test)]
    fn next_generation(&mut self) -> Result<u64, HalError> {
        let next = self.checked_next_worker_generation()?;
        self.generation = next;
        Ok(next)
    }

    fn mark_tuning(&mut self, generation: u64) {
        self.terminal_event_min_generation = generation;
        self.state = FrontendRuntimeState::Tuning { generation };
    }
    fn mark_scanning(&mut self, generation: u64) {
        self.terminal_event_min_generation = generation;
        self.state = FrontendRuntimeState::Scanning { generation };
    }
    fn mark_idle(&mut self) {
        self.state = FrontendRuntimeState::Idle;
    }
    fn mark_closing(&mut self) {
        self.state = FrontendRuntimeState::Closing;
    }
    fn set_live_reader_descriptor(&mut self, reader: FrontendLiveReaderDescriptor) {
        self.live_reader_descriptor = Some(reader);
    }
    fn clear_live_reader_descriptor(&mut self) {
        self.live_reader_descriptor = None;
    }
    fn mark_failed(&mut self, error: HalError) {
        self.last_error = Some(error);
        self.state = FrontendRuntimeState::Failed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::FrontendLiveReaderDescriptor;
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
        assert_eq!(runtime.checked_next_worker_generation().unwrap(), 1);
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
    #[test]
    fn rollback_token_restores_runtime_once_from_internal_ledger() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        runtime.mark_tuning(1);
        let mut token = runtime
            .prepare_worker_rollback()
            .unwrap()
            .into_token();
        assert!(runtime.query().matches_rollback_token(&token).unwrap());

        let reader = FrontendLiveReaderDescriptor::px4_from_control_fd(
            7,
            FrontendDevicePath::new("/dev/px4video0"),
        );
        let request = FrontendTuneRequest {
            system: maleicacid_tuner_hal2_common::FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        runtime
            .commit_tune_worker_rollback_expected_post_state(&token, 2, reader, request)
            .unwrap();
        assert!(!runtime.query().matches_rollback_token(&token).unwrap());
        runtime
            .restore_worker_rollback(&mut token)
            .unwrap();

        assert_eq!(runtime.generation(), 1);
        assert_eq!(runtime.state(), FrontendRuntimeState::Tuning { generation: 1 });
        assert!(lock_rollback_ledger(&runtime.rollback_ledger)
            .unwrap()
            .snapshots
            .is_empty());
    }

    #[test]
    fn rollback_restore_preserves_diagnostics_recorded_after_capture() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        runtime.commit_generation(1).unwrap();
        runtime.mark_tuning(1);
        let mut token = runtime
            .prepare_worker_rollback()
            .unwrap()
            .into_token();

        runtime
            .record_signal_state(1, FrontendSignalState::Locked)
            .unwrap();
        runtime
            .record_live_pump_report(
                1,
                FrontendLivePumpReport {
                    packets_delivered: 4,
                    malformed_bytes: 1,
                    read_retries: 0,
                    read_retry_counter_saturated: false,
                    stopped_by_cancel: false,
                    reached_eof: true,
                },
                None,
            )
            .unwrap();
        runtime.terminal_events.push(FrontendTerminalEvent {
            generation: 1,
            kind: FrontendTerminalEventKind::TuneFailed,
            reason: FrontendTerminalEventReason::BackendFailure,
        });
        runtime.last_error = Some(HalError::internal(
            HalInternalKind::InvariantViolation,
            "diagnostic recorded after rollback capture",
        ));

        let reader = FrontendLiveReaderDescriptor::px4_from_control_fd(
            7,
            FrontendDevicePath::new("/dev/px4video0"),
        );
        let request = FrontendTuneRequest {
            system: maleicacid_tuner_hal2_common::FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: Some(6_000_000),
            symbol_rate: None,
        };
        runtime
            .commit_tune_worker_rollback_expected_post_state(&token, 2, reader, request)
            .unwrap();
        runtime.restore_worker_rollback(&mut token).unwrap();

        assert_eq!(runtime.signal_state, FrontendSignalState::Locked);
        assert_eq!(runtime.live_pump_reports.len(), 1);
        assert_eq!(runtime.terminal_events.len(), 1);
        assert!(runtime.last_error.is_some());
    }

    #[test]
    fn unused_rollback_token_drop_discards_internal_snapshot() {
        let mut runtime = FrontendRuntime::new(7, FrontendBackendKind::Px4CharDevice);
        let token = runtime
            .prepare_worker_rollback()
            .unwrap()
            .into_token();
        assert_eq!(
            lock_rollback_ledger(&runtime.rollback_ledger)
                .unwrap()
                .snapshots
                .len(),
            1
        );
        drop(token);
        assert!(lock_rollback_ledger(&runtime.rollback_ledger)
            .unwrap()
            .snapshots
            .is_empty());
    }

}
