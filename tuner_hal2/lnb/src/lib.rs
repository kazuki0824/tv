mod apply_txn;
mod lifecycle_txn;
mod runtime;

pub use apply_txn::{apply_lnb_state_with_txn, LnbApplyOutcome, LnbApplyStep};
pub use lifecycle_txn::{
    close_lnb_lifecycle, record_lnb_drop_leak_lifecycle, LnbLifecycleOutcome,
    LnbLifecycleOutcomeReason, LnbLifecycleReason, LnbLifecycleStep,
};
pub use runtime::{
    LnbBackendOps, LnbDiseqcMessage, LnbElectricalState, LnbRuntime, LnbRuntimeState, LnbTone,
    LnbVoltage,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbFailureKind {
    InvalidState,
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
