pub mod apply_txn;
pub mod lifecycle_txn;
pub mod operation_guard;
pub mod runtime;

pub use apply_txn::{LnbApplyOutcome, LnbApplyStep, LnbApplyTxn};
pub use lifecycle_txn::{
    LnbLifecycleOutcome, LnbLifecycleReason, LnbLifecycleStep, LnbLifecycleTxn,
};
pub use operation_guard::{
    LnbOperationFailureRecord, LnbOperationGuard, LnbOperationKind, LnbOperationLedger,
};
pub use runtime::{
    LnbBackendOps, LnbDiseqcMessage, LnbElectricalState, LnbRuntime, LnbRuntimeState, LnbTone,
    LnbVoltage,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbFailureKind {
    InvalidState,
    OperationAlreadyActive,
    OperationLockFailed,
    BackendApplyFailed,
    RegistryCommitFailed,
    DiseqcInvalidMessage,
    DiseqcUnsupported,
    GenerationOverflow,
    DropWithoutClose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbFailureStep {
    ValidateState,
    MarkClosing,
    BuildSafeState,
    AdvanceGeneration,
    ApplyBackend,
    CommitRegistry,
    ClearRuntimeCallbackState,
    SendDiseqc,
    CommitClosed,
    DropLeakRecord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnbFailureRecord {
    pub lnb_id: i32,
    pub kind: LnbFailureKind,
    pub step: LnbFailureStep,
}
