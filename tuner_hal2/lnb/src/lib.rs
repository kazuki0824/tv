mod apply_txn;
mod lifecycle_txn;
mod runtime;

pub use apply_txn::{
    apply_lnb_state_with_txn, finish_lnb_state_apply, prepare_lnb_state_apply, LnbApplyOutcome,
    LnbApplyStep, PreparedLnbStateApply,
};
pub use lifecycle_txn::{
    close_lnb_lifecycle, finish_lnb_close, prepare_lnb_close, record_lnb_drop_leak_lifecycle,
    LnbLifecycleOutcome, LnbLifecycleOutcomeReason, LnbLifecycleReason, LnbLifecycleStep,
    PreparedLnbClose,
};
pub use runtime::{
    LnbBackendApplyOutcome, LnbBackendOps, LnbDiseqcMessage, LnbElectricalState, LnbRuntime,
    LnbRuntimeState, LnbTone, LnbVoltage,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbFailureKind {
    InvalidState,
    BackendApplyFailed,
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
