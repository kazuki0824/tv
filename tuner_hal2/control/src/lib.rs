use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerOwnerKind {
    FrontendTune,
    FrontendScan,
    FilterCallback,
    DvrCallback,
    DvrPlayback,
    LivePump,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerOwnerId {
    pub kind: WorkerOwnerKind,
    pub id: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerStopReason {
    ExplicitClose,
    Reconfigure,
    StreamBoundary,
    OwnerLoss,
    RuntimeFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerTerminalState {
    Running,
    StopRequested(WorkerStopReason),
    RuntimeFailure(WorkerRuntimeFailureKind),
    Joined(WorkerExit),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerRuntimeFailureKind {
    SignalPoisoned,
    WakeFailed,
    JoinFailed,
    BackendFailed,
    CallbackFailed,
    DeliveryFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerFailureDomain {
    Signal,
    Wake,
    Join,
    Backend,
    Callback,
    Delivery,
    PanicOrJoin,
}

impl WorkerFailureDomain {
    pub const fn runtime_failure_kind(self) -> WorkerRuntimeFailureKind {
        match self {
            WorkerFailureDomain::Signal => WorkerRuntimeFailureKind::SignalPoisoned,
            WorkerFailureDomain::Wake => WorkerRuntimeFailureKind::WakeFailed,
            WorkerFailureDomain::Join | WorkerFailureDomain::PanicOrJoin => {
                WorkerRuntimeFailureKind::JoinFailed
            }
            WorkerFailureDomain::Backend => WorkerRuntimeFailureKind::BackendFailed,
            WorkerFailureDomain::Callback => WorkerRuntimeFailureKind::CallbackFailed,
            WorkerFailureDomain::Delivery => WorkerRuntimeFailureKind::DeliveryFailed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerExit {
    Normal,
    StopRequested(WorkerStopReason),
    RuntimeFailure(WorkerRuntimeFailureKind),
    PanicOrJoinFailure,
}

#[derive(Debug)]
pub struct WorkerSignal {
    owner: WorkerOwnerId,
    state: Mutex<WorkerTerminalState>,
    condvar: Condvar,
}

impl WorkerSignal {
    pub fn new(owner: WorkerOwnerId) -> Arc<Self> {
        Arc::new(Self {
            owner,
            state: Mutex::new(WorkerTerminalState::Running),
            condvar: Condvar::new(),
        })
    }

    pub fn owner(&self) -> WorkerOwnerId {
        self.owner
    }

    pub fn request_stop(&self, reason: WorkerStopReason) -> Result<(), WorkerRuntimeFailureKind> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| WorkerRuntimeFailureKind::SignalPoisoned)?;
        match *state {
            WorkerTerminalState::RuntimeFailure(_) | WorkerTerminalState::Joined(_) => {}
            _ => *state = WorkerTerminalState::StopRequested(reason),
        }
        self.condvar.notify_all();
        Ok(())
    }

    pub fn mark_runtime_failure(
        &self,
        failure: WorkerRuntimeFailureKind,
    ) -> Result<(), WorkerRuntimeFailureKind> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| WorkerRuntimeFailureKind::SignalPoisoned)?;
        *state = WorkerTerminalState::RuntimeFailure(failure);
        self.condvar.notify_all();
        Ok(())
    }

    pub fn wait_until_stop_or_timeout(
        &self,
        timeout: Duration,
    ) -> Result<WorkerTerminalState, WorkerRuntimeFailureKind> {
        let state = self
            .state
            .lock()
            .map_err(|_| WorkerRuntimeFailureKind::SignalPoisoned)?;
        let (state, _) = self
            .condvar
            .wait_timeout_while(state, timeout, |s| {
                matches!(s, WorkerTerminalState::Running)
            })
            .map_err(|_| WorkerRuntimeFailureKind::SignalPoisoned)?;
        Ok(*state)
    }

    pub fn finish_join(&self, exit: WorkerExit) -> Result<(), WorkerRuntimeFailureKind> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| WorkerRuntimeFailureKind::SignalPoisoned)?;
        *state = WorkerTerminalState::Joined(exit);
        self.condvar.notify_all();
        Ok(())
    }

    pub fn snapshot(&self) -> Result<WorkerTerminalState, WorkerRuntimeFailureKind> {
        Ok(*self
            .state
            .lock()
            .map_err(|_| WorkerRuntimeFailureKind::SignalPoisoned)?)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleKind {
    Open,
    Configure,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleStage {
    Validate,
    Reserve,
    Prepare,
    Apply,
    Commit,
    Rollback,
    Cleanup,
    Quarantine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleStepKind {
    LedgerReserve,
    RuntimeRegister,
    WorkerSpawn,
    QueueCreate,
    BackendApply,
    StreamBoundaryReset,
    CallbackDelivery,
    QueueClear,
    RuntimeUnregister,
    LedgerCommit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleFailureKind {
    InvalidState,
    InvalidArgument,
    BackendFailure,
    WorkerFailure,
    DeliveryFailure,
    LedgerFailure,
    QueueFailure,
    RollbackFailure,
    CleanupFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleStepOutcome {
    pub stage: LifecycleStage,
    pub step: LifecycleStepKind,
    pub result: Result<(), LifecycleFailureKind>,
}

#[derive(Debug)]
pub struct LifecycleTxn {
    kind: LifecycleKind,
    outcomes: Vec<LifecycleStepOutcome>,
    first_error: Option<LifecycleStepOutcome>,
}

impl LifecycleTxn {
    pub fn new(kind: LifecycleKind) -> Self {
        Self {
            kind,
            outcomes: Vec::new(),
            first_error: None,
        }
    }

    pub fn kind(&self) -> LifecycleKind {
        self.kind
    }

    pub fn record(
        &mut self,
        stage: LifecycleStage,
        step: LifecycleStepKind,
        result: Result<(), LifecycleFailureKind>,
    ) {
        let outcome = LifecycleStepOutcome {
            stage,
            step,
            result,
        };
        if outcome.result.is_err() && self.first_error.is_none() {
            self.first_error = Some(outcome);
        }
        self.outcomes.push(outcome);
    }

    pub fn first_error(&self) -> Option<LifecycleStepOutcome> {
        self.first_error
    }
    pub fn outcomes(&self) -> &[LifecycleStepOutcome] {
        &self.outcomes
    }
    pub fn is_success(&self) -> bool {
        self.first_error.is_none()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FmqObjectKind {
    Filter,
    DvrRecord,
    DvrPlayback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FmqDeliveryPhase {
    CapacityCheck,
    Write,
    Wake,
    Clear,
    DescriptorExport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FmqFailureKind {
    DescriptorStructural,
    DescriptorTransient,
    WriteFailed,
    ShortWrite,
    EventFlagWakeFailed,
    ClearFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FmqDeliveryAction {
    Continue,
    Overflow,
    RuntimeFailed(FmqFailureKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FmqDeliveryResult {
    pub object_kind: FmqObjectKind,
    pub phase: FmqDeliveryPhase,
    pub bytes: usize,
    pub action: FmqDeliveryAction,
}

pub struct FmqDeliveryTxn {
    object_kind: FmqObjectKind,
    phase: FmqDeliveryPhase,
}

impl FmqDeliveryTxn {
    pub fn new(object_kind: FmqObjectKind) -> Self {
        Self {
            object_kind,
            phase: FmqDeliveryPhase::CapacityCheck,
        }
    }

    pub fn commit_payload(
        self,
        expected_bytes: usize,
        write_result: Result<usize, FmqFailureKind>,
        wake_result: Result<(), FmqFailureKind>,
    ) -> FmqDeliveryResult {
        let written_bytes = match write_result {
            Ok(written_bytes) if written_bytes == expected_bytes => written_bytes,
            Ok(_) => {
                return FmqDeliveryResult {
                    object_kind: self.object_kind,
                    phase: FmqDeliveryPhase::Write,
                    bytes: 0,
                    action: FmqDeliveryAction::RuntimeFailed(FmqFailureKind::ShortWrite),
                };
            }
            Err(err) => {
                return FmqDeliveryResult {
                    object_kind: self.object_kind,
                    phase: FmqDeliveryPhase::Write,
                    bytes: 0,
                    action: FmqDeliveryAction::RuntimeFailed(err),
                };
            }
        };

        match wake_result {
            Ok(()) => FmqDeliveryResult {
                object_kind: self.object_kind,
                phase: FmqDeliveryPhase::Wake,
                bytes: written_bytes,
                action: FmqDeliveryAction::Continue,
            },
            Err(err) => FmqDeliveryResult {
                object_kind: self.object_kind,
                phase: FmqDeliveryPhase::Wake,
                bytes: written_bytes,
                action: FmqDeliveryAction::RuntimeFailed(err),
            },
        }
    }

    pub fn commit_write_and_wake(
        self,
        write_result: Result<usize, FmqFailureKind>,
        wake_result: Result<(), FmqFailureKind>,
    ) -> FmqDeliveryResult {
        match write_result {
            Ok(bytes) => self.commit_payload(bytes, Ok(bytes), wake_result),
            Err(err) => self.commit_payload(0, Err(err), wake_result),
        }
    }

    pub fn overflow(self) -> FmqDeliveryResult {
        FmqDeliveryResult {
            object_kind: self.object_kind,
            phase: self.phase,
            bytes: 0,
            action: FmqDeliveryAction::Overflow,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum StreamBoundaryReason {
    Tune,
    Scan,
    FrontendClose,
    DemuxClose,
    SourceFilterChange,
    Flush,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum StreamBoundaryStep {
    StopLivePump,
    FlushRuntimeIo,
    ClearFmq,
    DiscardAvPayloads,
    ResetDvrPlayback,
    ResetAssemblers,
    InvalidateGeneration,
    QuarantineOnFailure,
}

impl StreamBoundaryStep {
    pub const ORDERED: [Self; 8] = [
        Self::StopLivePump,
        Self::FlushRuntimeIo,
        Self::ClearFmq,
        Self::DiscardAvPayloads,
        Self::ResetDvrPlayback,
        Self::ResetAssemblers,
        Self::InvalidateGeneration,
        Self::QuarantineOnFailure,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamBoundaryFailureKind {
    StepFailed(StreamBoundaryStep),
    QuarantineFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamBoundaryRecord {
    pub demux_id: i32,
    pub reason: StreamBoundaryReason,
    pub attempted: Vec<StreamBoundaryStep>,
    pub failure: Option<StreamBoundaryFailureKind>,
}

pub trait StreamBoundaryResource {
    fn apply_step(
        &mut self,
        demux_id: i32,
        reason: StreamBoundaryReason,
        step: StreamBoundaryStep,
    ) -> Result<(), StreamBoundaryFailureKind>;
}

pub struct StreamBoundaryTxn {
    reason: StreamBoundaryReason,
    records: BTreeMap<i32, StreamBoundaryRecord>,
}

impl StreamBoundaryTxn {
    pub fn new(reason: StreamBoundaryReason) -> Self {
        Self {
            reason,
            records: BTreeMap::new(),
        }
    }

    pub fn reset_demux<R: StreamBoundaryResource>(
        &mut self,
        demux_id: i32,
        resources: &mut R,
    ) -> StreamBoundaryRecord {
        let mut record = StreamBoundaryRecord {
            demux_id,
            reason: self.reason,
            attempted: Vec::new(),
            failure: None,
        };
        for step in StreamBoundaryStep::ORDERED {
            record.attempted.push(step);
            if let Err(err) = resources.apply_step(demux_id, self.reason, step) {
                record.failure = Some(err);
                if !matches!(step, StreamBoundaryStep::QuarantineOnFailure) {
                    record
                        .attempted
                        .push(StreamBoundaryStep::QuarantineOnFailure);
                    if resources
                        .apply_step(
                            demux_id,
                            self.reason,
                            StreamBoundaryStep::QuarantineOnFailure,
                        )
                        .is_err()
                    {
                        record.failure = Some(StreamBoundaryFailureKind::QuarantineFailed);
                    }
                }
                break;
            }
        }
        self.records.insert(demux_id, record.clone());
        record
    }

    pub fn records(&self) -> impl Iterator<Item = &StreamBoundaryRecord> {
        self.records.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_stop_reason_is_preserved() {
        let signal = WorkerSignal::new(WorkerOwnerId {
            kind: WorkerOwnerKind::FrontendTune,
            id: 10,
        });
        signal
            .request_stop(WorkerStopReason::StreamBoundary)
            .unwrap();
        assert_eq!(
            signal.snapshot().unwrap(),
            WorkerTerminalState::StopRequested(WorkerStopReason::StreamBoundary)
        );
    }

    #[test]
    fn lifecycle_records_typed_first_error() {
        let mut txn = LifecycleTxn::new(LifecycleKind::Close);
        txn.record(
            LifecycleStage::Apply,
            LifecycleStepKind::QueueClear,
            Err(LifecycleFailureKind::QueueFailure),
        );
        txn.record(
            LifecycleStage::Cleanup,
            LifecycleStepKind::RuntimeUnregister,
            Err(LifecycleFailureKind::CleanupFailure),
        );
        assert_eq!(
            txn.first_error().unwrap().step,
            LifecycleStepKind::QueueClear
        );
        assert!(!txn.is_success());
    }

    #[test]
    fn fmq_wake_failure_is_typed_runtime_failure() {
        let result = FmqDeliveryTxn::new(FmqObjectKind::Filter).commit_payload(
            188,
            Ok(188),
            Err(FmqFailureKind::EventFlagWakeFailed),
        );
        assert_eq!(
            result.action,
            FmqDeliveryAction::RuntimeFailed(FmqFailureKind::EventFlagWakeFailed)
        );
        assert_eq!(result.bytes, 188);
    }

    #[test]
    fn fmq_short_write_fails_before_wake_commit() {
        let result =
            FmqDeliveryTxn::new(FmqObjectKind::Filter).commit_payload(188, Ok(187), Ok(()));
        assert_eq!(result.phase, FmqDeliveryPhase::Write);
        assert_eq!(
            result.action,
            FmqDeliveryAction::RuntimeFailed(FmqFailureKind::ShortWrite)
        );
    }

    struct FailingResources {
        fail_step: StreamBoundaryStep,
    }

    impl StreamBoundaryResource for FailingResources {
        fn apply_step(
            &mut self,
            _demux_id: i32,
            _reason: StreamBoundaryReason,
            step: StreamBoundaryStep,
        ) -> Result<(), StreamBoundaryFailureKind> {
            if step == self.fail_step {
                return Err(StreamBoundaryFailureKind::StepFailed(step));
            }
            Ok(())
        }
    }

    #[test]
    fn stream_boundary_quarantines_after_step_failure() {
        let mut txn = StreamBoundaryTxn::new(StreamBoundaryReason::Tune);
        let mut res = FailingResources {
            fail_step: StreamBoundaryStep::ClearFmq,
        };
        let record = txn.reset_demux(7, &mut res);
        assert!(record.attempted.contains(&StreamBoundaryStep::ClearFmq));
        assert!(record
            .attempted
            .contains(&StreamBoundaryStep::QuarantineOnFailure));
        assert_eq!(
            record.failure,
            Some(StreamBoundaryFailureKind::StepFailed(
                StreamBoundaryStep::ClearFmq
            ))
        );
    }
}
