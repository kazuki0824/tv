use maleicacid_tuner_hal2_common::{FrontendTuneRequest, HalError, HalInternalKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendTuneStep {
    CapturePreviousState,
    StopPreviousTune,
    ApplySystemMode,
    ApplyChannel,
    StartStreaming,
    ReadInitialStatus,
    Commit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendTuneRollbackStep {
    RollbackStopStreaming,
    RollbackRestorePreviousState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendTuneRollbackFailure {
    pub step: BackendTuneRollbackStep,
    pub error: HalError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendTuneRollbackReport {
    attempted_steps: Vec<BackendTuneRollbackStep>,
    failure: Option<BackendTuneRollbackFailure>,
}

impl BackendTuneRollbackReport {
    pub fn not_required() -> Self {
        Self {
            attempted_steps: Vec::new(),
            failure: None,
        }
    }

    pub fn attempted_steps(&self) -> &[BackendTuneRollbackStep] {
        &self.attempted_steps
    }
    pub fn failure(&self) -> Option<&BackendTuneRollbackFailure> {
        self.failure.as_ref()
    }
    pub fn succeeded(&self) -> bool {
        self.failure.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendTuneCommit {
    pub frontend_id: i32,
    pub generation: u64,
    pub request: FrontendTuneRequest,
    completed_steps: Vec<BackendTuneStep>,
}

impl BackendTuneCommit {
    pub fn completed_steps(&self) -> &[BackendTuneStep] {
        &self.completed_steps
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendTuneOutcome {
    Committed {
        commit: BackendTuneCommit,
    },
    Failed {
        step: BackendTuneStep,
        error: HalError,
        rollback: BackendTuneRollbackReport,
    },
    RollbackFailed {
        step: BackendTuneStep,
        error: HalError,
        rollback: BackendTuneRollbackReport,
    },
}

impl BackendTuneOutcome {
    pub fn is_committed(&self) -> bool {
        matches!(self, Self::Committed { .. })
    }
}

pub trait BackendTuneOps {
    type Snapshot: Clone + core::fmt::Debug + Eq + PartialEq;

    fn capture_previous_state(&mut self) -> Result<Self::Snapshot, HalError>;
    fn stop_previous_tune(&mut self) -> Result<(), HalError>;
    fn apply_system_mode(&mut self, request: &FrontendTuneRequest) -> Result<(), HalError>;
    fn apply_channel(&mut self, request: &FrontendTuneRequest) -> Result<(), HalError>;
    fn start_streaming(&mut self) -> Result<(), HalError>;
    fn read_initial_status(&mut self) -> Result<(), HalError>;
    fn rollback_stop_streaming(&mut self) -> Result<(), HalError>;
    fn rollback_restore_previous_state(
        &mut self,
        snapshot: &Self::Snapshot,
    ) -> Result<(), HalError>;
}

pub trait TuneWorkerStart {
    fn start_tune_worker(&mut self, frontend_id: i32, generation: u64) -> Result<(), HalError>;
}

#[derive(Debug)]
pub struct BackendTuneTxn {
    frontend_id: i32,
    generation: u64,
    request: FrontendTuneRequest,
    completed_steps: Vec<BackendTuneStep>,
}

impl BackendTuneTxn {
    pub fn new(frontend_id: i32, generation: u64, request: FrontendTuneRequest) -> Self {
        Self {
            frontend_id,
            generation,
            request,
            completed_steps: Vec::new(),
        }
    }

    pub fn frontend_id(&self) -> i32 {
        self.frontend_id
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn request(&self) -> &FrontendTuneRequest {
        &self.request
    }
    pub fn completed_steps(&self) -> &[BackendTuneStep] {
        &self.completed_steps
    }

    fn record_step(&mut self, step: BackendTuneStep) {
        self.completed_steps.push(step);
    }

    fn fail_without_rollback(&self, step: BackendTuneStep, error: HalError) -> BackendTuneOutcome {
        BackendTuneOutcome::Failed {
            step,
            error,
            rollback: BackendTuneRollbackReport::not_required(),
        }
    }

    fn fail_after_rollback<B: BackendTuneOps>(
        &self,
        backend: &mut B,
        snapshot: &B::Snapshot,
        step: BackendTuneStep,
        error: HalError,
    ) -> BackendTuneOutcome {
        let rollback = rollback_backend_tune(backend, snapshot);
        if rollback.succeeded() {
            BackendTuneOutcome::Failed {
                step,
                error,
                rollback,
            }
        } else {
            BackendTuneOutcome::RollbackFailed {
                step,
                error,
                rollback,
            }
        }
    }

    pub fn apply<B: BackendTuneOps>(&mut self, backend: &mut B) -> BackendTuneOutcome {
        let snapshot = match backend.capture_previous_state() {
            Ok(snapshot) => {
                self.record_step(BackendTuneStep::CapturePreviousState);
                snapshot
            }
            Err(error) => {
                return self.fail_without_rollback(BackendTuneStep::CapturePreviousState, error)
            }
        };

        if let Err(error) = backend.stop_previous_tune() {
            return self.fail_after_rollback(
                backend,
                &snapshot,
                BackendTuneStep::StopPreviousTune,
                error,
            );
        }
        self.record_step(BackendTuneStep::StopPreviousTune);

        if let Err(error) = backend.apply_system_mode(&self.request) {
            return self.fail_after_rollback(
                backend,
                &snapshot,
                BackendTuneStep::ApplySystemMode,
                error,
            );
        }
        self.record_step(BackendTuneStep::ApplySystemMode);

        if let Err(error) = backend.apply_channel(&self.request) {
            return self.fail_after_rollback(
                backend,
                &snapshot,
                BackendTuneStep::ApplyChannel,
                error,
            );
        }
        self.record_step(BackendTuneStep::ApplyChannel);

        if let Err(error) = backend.start_streaming() {
            return self.fail_after_rollback(
                backend,
                &snapshot,
                BackendTuneStep::StartStreaming,
                error,
            );
        }
        self.record_step(BackendTuneStep::StartStreaming);

        if let Err(error) = backend.read_initial_status() {
            return self.fail_after_rollback(
                backend,
                &snapshot,
                BackendTuneStep::ReadInitialStatus,
                error,
            );
        }
        self.record_step(BackendTuneStep::ReadInitialStatus);

        self.record_step(BackendTuneStep::Commit);
        BackendTuneOutcome::Committed {
            commit: BackendTuneCommit {
                frontend_id: self.frontend_id,
                generation: self.generation,
                request: self.request.clone(),
                completed_steps: self.completed_steps.clone(),
            },
        }
    }

    pub fn apply_with_worker<B: BackendTuneOps, W: TuneWorkerStart>(
        &mut self,
        backend: &mut B,
        worker: &mut W,
    ) -> FrontendTuneOutcome {
        let snapshot = match backend.capture_previous_state() {
            Ok(snapshot) => {
                self.record_step(BackendTuneStep::CapturePreviousState);
                snapshot
            }
            Err(error) => {
                return FrontendTuneOutcome::BackendFailed {
                    outcome: self
                        .fail_without_rollback(BackendTuneStep::CapturePreviousState, error),
                };
            }
        };

        macro_rules! step {
            ($step:expr, $call:expr) => {
                if let Err(error) = $call {
                    return FrontendTuneOutcome::BackendFailed {
                        outcome: self.fail_after_rollback(backend, &snapshot, $step, error),
                    };
                }
                self.record_step($step);
            };
        }

        step!(
            BackendTuneStep::StopPreviousTune,
            backend.stop_previous_tune()
        );
        step!(
            BackendTuneStep::ApplySystemMode,
            backend.apply_system_mode(&self.request)
        );
        step!(
            BackendTuneStep::ApplyChannel,
            backend.apply_channel(&self.request)
        );
        step!(BackendTuneStep::StartStreaming, backend.start_streaming());
        step!(
            BackendTuneStep::ReadInitialStatus,
            backend.read_initial_status()
        );

        if let Err(error) = worker.start_tune_worker(self.frontend_id, self.generation) {
            let rollback = rollback_backend_tune(backend, &snapshot);
            return FrontendTuneOutcome::WorkerStartFailed { error, rollback };
        }

        self.record_step(BackendTuneStep::Commit);
        FrontendTuneOutcome::Committed {
            commit: BackendTuneCommit {
                frontend_id: self.frontend_id,
                generation: self.generation,
                request: self.request.clone(),
                completed_steps: self.completed_steps.clone(),
            },
        }
    }
}

fn rollback_backend_tune<B: BackendTuneOps>(
    backend: &mut B,
    snapshot: &B::Snapshot,
) -> BackendTuneRollbackReport {
    let mut attempted_steps = Vec::new();
    attempted_steps.push(BackendTuneRollbackStep::RollbackStopStreaming);
    if let Err(error) = backend.rollback_stop_streaming() {
        return BackendTuneRollbackReport {
            attempted_steps,
            failure: Some(BackendTuneRollbackFailure {
                step: BackendTuneRollbackStep::RollbackStopStreaming,
                error,
            }),
        };
    }

    attempted_steps.push(BackendTuneRollbackStep::RollbackRestorePreviousState);
    if let Err(error) = backend.rollback_restore_previous_state(snapshot) {
        return BackendTuneRollbackReport {
            attempted_steps,
            failure: Some(BackendTuneRollbackFailure {
                step: BackendTuneRollbackStep::RollbackRestorePreviousState,
                error,
            }),
        };
    }

    BackendTuneRollbackReport {
        attempted_steps,
        failure: None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendTuneOutcome {
    Committed {
        commit: BackendTuneCommit,
    },
    BackendFailed {
        outcome: BackendTuneOutcome,
    },
    WorkerStartFailed {
        error: HalError,
        rollback: BackendTuneRollbackReport,
    },
}

#[derive(Debug)]
pub struct FrontendTuneTxn {
    backend: BackendTuneTxn,
}

impl FrontendTuneTxn {
    pub fn new(frontend_id: i32, generation: u64, request: FrontendTuneRequest) -> Self {
        Self {
            backend: BackendTuneTxn::new(frontend_id, generation, request),
        }
    }

    pub fn apply<B: BackendTuneOps, W: TuneWorkerStart>(
        &mut self,
        backend: &mut B,
        worker: &mut W,
    ) -> FrontendTuneOutcome {
        self.backend.apply_with_worker(backend, worker)
    }
}

pub fn invariant_error(detail: impl Into<String>) -> HalError {
    HalError::internal(HalInternalKind::InvariantViolation, detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{FrontendSystem, HalInvalidArgumentKind};

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Snapshot {
        tuned: bool,
    }

    #[derive(Default)]
    struct FakeBackend {
        tuned: bool,
        fail_step: Option<BackendTuneStep>,
        fail_rollback: Option<BackendTuneRollbackStep>,
        calls: Vec<&'static str>,
    }

    impl FakeBackend {
        fn maybe_fail(&self, step: BackendTuneStep) -> Result<(), HalError> {
            if self.fail_step == Some(step) {
                Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "fake step failure",
                ))
            } else {
                Ok(())
            }
        }
    }

    impl BackendTuneOps for FakeBackend {
        type Snapshot = Snapshot;

        fn capture_previous_state(&mut self) -> Result<Self::Snapshot, HalError> {
            self.calls.push("capture");
            self.maybe_fail(BackendTuneStep::CapturePreviousState)?;
            Ok(Snapshot { tuned: self.tuned })
        }
        fn stop_previous_tune(&mut self) -> Result<(), HalError> {
            self.calls.push("stop_previous");
            self.maybe_fail(BackendTuneStep::StopPreviousTune)
        }
        fn apply_system_mode(&mut self, _request: &FrontendTuneRequest) -> Result<(), HalError> {
            self.calls.push("apply_system");
            self.maybe_fail(BackendTuneStep::ApplySystemMode)
        }
        fn apply_channel(&mut self, _request: &FrontendTuneRequest) -> Result<(), HalError> {
            self.calls.push("apply_channel");
            self.maybe_fail(BackendTuneStep::ApplyChannel)
        }
        fn start_streaming(&mut self) -> Result<(), HalError> {
            self.calls.push("start_streaming");
            self.maybe_fail(BackendTuneStep::StartStreaming)?;
            self.tuned = true;
            Ok(())
        }
        fn read_initial_status(&mut self) -> Result<(), HalError> {
            self.calls.push("read_status");
            self.maybe_fail(BackendTuneStep::ReadInitialStatus)
        }
        fn rollback_stop_streaming(&mut self) -> Result<(), HalError> {
            self.calls.push("rollback_stop");
            if self.fail_rollback == Some(BackendTuneRollbackStep::RollbackStopStreaming) {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "rollback stop failed",
                ));
            }
            self.tuned = false;
            Ok(())
        }
        fn rollback_restore_previous_state(
            &mut self,
            snapshot: &Self::Snapshot,
        ) -> Result<(), HalError> {
            self.calls.push("rollback_restore");
            if self.fail_rollback == Some(BackendTuneRollbackStep::RollbackRestorePreviousState) {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "rollback restore failed",
                ));
            }
            self.tuned = snapshot.tuned;
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeWorker {
        fail: bool,
        started: bool,
    }

    impl TuneWorkerStart for FakeWorker {
        fn start_tune_worker(
            &mut self,
            _frontend_id: i32,
            _generation: u64,
        ) -> Result<(), HalError> {
            if self.fail {
                Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "worker start failed",
                ))
            } else {
                self.started = true;
                Ok(())
            }
        }
    }

    fn request() -> FrontendTuneRequest {
        FrontendTuneRequest {
            system: FrontendSystem::IsdbT,
            frequency: 473_142_857,
            end_frequency: None,
            stream_id: None,
            stream_id_kind: None,
            bandwidth_hz: None,
            symbol_rate: None,
        }
    }

    #[test]
    fn backend_tune_commit_records_typed_steps() {
        let mut backend = FakeBackend::default();
        let mut txn = BackendTuneTxn::new(10, 1, request());
        let outcome = txn.apply(&mut backend);
        match outcome {
            BackendTuneOutcome::Committed { commit } => {
                assert_eq!(
                    commit.completed_steps(),
                    &[
                        BackendTuneStep::CapturePreviousState,
                        BackendTuneStep::StopPreviousTune,
                        BackendTuneStep::ApplySystemMode,
                        BackendTuneStep::ApplyChannel,
                        BackendTuneStep::StartStreaming,
                        BackendTuneStep::ReadInitialStatus,
                        BackendTuneStep::Commit,
                    ]
                );
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn apply_failure_rolls_back_with_typed_steps() {
        let mut backend = FakeBackend {
            fail_step: Some(BackendTuneStep::ApplyChannel),
            ..Default::default()
        };
        let mut txn = BackendTuneTxn::new(10, 1, request());
        let outcome = txn.apply(&mut backend);
        match outcome {
            BackendTuneOutcome::Failed { step, rollback, .. } => {
                assert_eq!(step, BackendTuneStep::ApplyChannel);
                assert_eq!(
                    rollback.attempted_steps(),
                    &[
                        BackendTuneRollbackStep::RollbackStopStreaming,
                        BackendTuneRollbackStep::RollbackRestorePreviousState,
                    ]
                );
                assert!(rollback.succeeded());
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn rollback_failure_is_reported_by_typed_step() {
        let mut backend = FakeBackend {
            fail_step: Some(BackendTuneStep::ApplyChannel),
            fail_rollback: Some(BackendTuneRollbackStep::RollbackRestorePreviousState),
            ..Default::default()
        };
        let mut txn = BackendTuneTxn::new(10, 1, request());
        let outcome = txn.apply(&mut backend);
        match outcome {
            BackendTuneOutcome::RollbackFailed { rollback, .. } => {
                assert_eq!(
                    rollback.failure().unwrap().step,
                    BackendTuneRollbackStep::RollbackRestorePreviousState
                );
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn worker_start_failure_rolls_back_backend_tune() {
        let mut backend = FakeBackend::default();
        let mut worker = FakeWorker {
            fail: true,
            started: false,
        };
        let mut txn = FrontendTuneTxn::new(10, 1, request());
        let outcome = txn.apply(&mut backend, &mut worker);
        match outcome {
            FrontendTuneOutcome::WorkerStartFailed { rollback, .. } => {
                assert!(rollback.succeeded());
                assert!(!backend.tuned);
                assert!(!worker.started);
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
}
