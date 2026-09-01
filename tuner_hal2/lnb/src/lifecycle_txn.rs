use crate::runtime::{
    LnbBackendApplyOutcome, LnbBackendOps, LnbElectricalState, LnbRuntime, LnbRuntimeState,
};
use crate::{LnbFailureKind, LnbFailureRecord, LnbFailureStep};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbLifecycleStep {
    MarkClosing,
    BuildSafeState,
    ApplySafeState,
    CommitRegistry,
    ClearRuntimeCallbackState,
    CommitClosed,
    RecordDropLeak,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbLifecycleReason {
    PublicClose,
    OwnerLoss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbLifecycleOutcomeReason {
    PublicClose,
    OwnerLoss,
    DropLeak,
}

impl From<LnbLifecycleReason> for LnbLifecycleOutcomeReason {
    fn from(reason: LnbLifecycleReason) -> Self {
        match reason {
            LnbLifecycleReason::PublicClose => Self::PublicClose,
            LnbLifecycleReason::OwnerLoss => Self::OwnerLoss,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnbLifecycleOutcome {
    pub reason: LnbLifecycleOutcomeReason,
    pub steps: Vec<LnbLifecycleStep>,
    pub result: Result<(), LnbFailureRecord>,
}

#[must_use = "準備済みLNB closeはbackend結果を正確に1件消費する必要があります"]
#[derive(Debug, Eq, PartialEq)]
pub struct PreparedLnbClose {
    lnb_id: i32,
    expected_generation: u64,
    already_closed: bool,
}

impl PreparedLnbClose {
    pub const fn lnb_id(&self) -> i32 {
        self.lnb_id
    }

    pub const fn requires_backend_io(&self) -> bool {
        !self.already_closed
    }
}

pub fn prepare_lnb_close(runtime: &mut LnbRuntime) -> Result<PreparedLnbClose, LnbFailureRecord> {
    runtime.begin_close()?;
    Ok(PreparedLnbClose {
        lnb_id: runtime.lnb_id(),
        expected_generation: runtime.generation(),
        already_closed: runtime.state() == LnbRuntimeState::Closed,
    })
}

pub fn finish_lnb_close(
    runtime: &mut LnbRuntime,
    prepared: PreparedLnbClose,
    backend_result: LnbBackendApplyOutcome,
) -> Result<(), LnbFailureRecord> {
    if runtime.lnb_id() != prepared.lnb_id
        || runtime.generation() != prepared.expected_generation
        || (prepared.already_closed && runtime.state() != LnbRuntimeState::Closed)
        || (!prepared.already_closed && runtime.state() != LnbRuntimeState::Closing)
    {
        return Err(
            runtime.record_failure(LnbFailureKind::InvalidState, LnbFailureStep::ValidateState)
        );
    }
    if prepared.already_closed {
        return Ok(());
    }
    match backend_result {
        LnbBackendApplyOutcome::Applied => {}
        LnbBackendApplyOutcome::Rejected(kind) => {
            return Err(runtime.record_failure(kind, LnbFailureStep::ApplyBackend));
        }
        LnbBackendApplyOutcome::Indeterminate(kind) => {
            return Err(
                runtime.quarantine_indeterminate_backend(kind, LnbFailureStep::ApplyBackend)
            );
        }
    }
    let safe = LnbElectricalState::safe();
    runtime.commit_successful_close(safe);
    Ok(())
}

#[derive(Debug, Default)]
pub struct LnbLifecycleTxn {
    steps: Vec<LnbLifecycleStep>,
}

impl LnbLifecycleTxn {
    fn new() -> Self {
        Self { steps: Vec::new() }
    }
    fn record_step(&mut self, step: LnbLifecycleStep) {
        self.steps.push(step);
    }

    fn close<B: LnbBackendOps>(
        mut self,
        runtime: &mut LnbRuntime,
        backend: &mut B,
        reason: LnbLifecycleReason,
    ) -> LnbLifecycleOutcome {
        let outcome_reason = LnbLifecycleOutcomeReason::from(reason);

        self.record_step(LnbLifecycleStep::MarkClosing);
        let prepared = match prepare_lnb_close(runtime) {
            Ok(prepared) => prepared,
            Err(record) => {
                return LnbLifecycleOutcome {
                    reason: outcome_reason,
                    steps: self.steps,
                    result: Err(record),
                };
            }
        };
        if !prepared.requires_backend_io() {
            return LnbLifecycleOutcome {
                reason: outcome_reason,
                steps: self.steps,
                result: Ok(()),
            };
        }

        self.record_step(LnbLifecycleStep::BuildSafeState);
        let safe = LnbElectricalState::safe();

        self.record_step(LnbLifecycleStep::ApplySafeState);
        let backend_result = backend.apply_lnb_state(runtime.lnb_id(), safe);
        if backend_result != LnbBackendApplyOutcome::Applied {
            return LnbLifecycleOutcome {
                reason: outcome_reason,
                steps: self.steps,
                result: finish_lnb_close(runtime, prepared, backend_result),
            };
        }

        self.record_step(LnbLifecycleStep::CommitRegistry);
        if let Err(record) = finish_lnb_close(runtime, prepared, backend_result) {
            return LnbLifecycleOutcome {
                reason: outcome_reason,
                steps: self.steps,
                result: Err(record),
            };
        }

        self.record_step(LnbLifecycleStep::ClearRuntimeCallbackState);
        self.record_step(LnbLifecycleStep::CommitClosed);
        LnbLifecycleOutcome {
            reason: outcome_reason,
            steps: self.steps,
            result: Ok(()),
        }
    }
}

pub fn close_lnb_lifecycle<B: LnbBackendOps>(
    runtime: &mut LnbRuntime,
    backend: &mut B,
    reason: LnbLifecycleReason,
) -> LnbLifecycleOutcome {
    LnbLifecycleTxn::new().close(runtime, backend, reason)
}

pub fn record_lnb_drop_leak_lifecycle(runtime: &mut LnbRuntime) -> LnbLifecycleOutcome {
    let mut txn = LnbLifecycleTxn::new();
    txn.record_step(LnbLifecycleStep::RecordDropLeak);
    let record = runtime.record_unclosed_drop();
    LnbLifecycleOutcome {
        reason: LnbLifecycleOutcomeReason::DropLeak,
        steps: txn.steps,
        result: Err(record),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{LnbTone, LnbVoltage};

    struct TestBackend {
        applied: Vec<LnbElectricalState>,
    }
    impl TestBackend {
        fn new() -> Self {
            Self {
                applied: Vec::new(),
            }
        }
    }
    impl LnbBackendOps for TestBackend {
        fn apply_lnb_state(
            &mut self,
            _lnb_id: i32,
            state: LnbElectricalState,
        ) -> LnbBackendApplyOutcome {
            self.applied.push(state);
            LnbBackendApplyOutcome::Applied
        }
        fn send_diseqc_message(
            &mut self,
            _lnb_id: i32,
            _message: &crate::runtime::LnbDiseqcMessage,
        ) -> LnbBackendApplyOutcome {
            LnbBackendApplyOutcome::Rejected(LnbFailureKind::DiseqcUnsupported)
        }
    }

    #[test]
    fn close_applies_safe_state_and_closes() {
        let mut runtime = LnbRuntime::new(2);
        let mut backend = TestBackend::new();
        let target = LnbElectricalState {
            voltage: LnbVoltage::Voltage15V,
            tone: LnbTone::On,
            satellite_position: Some(1),
        };
        assert!(crate::apply_txn::LnbApplyTxn::new()
            .apply(&mut runtime, &mut backend, target)
            .result
            .is_ok());
        runtime.set_callback_registered(true);
        assert!(runtime.callback_registered());
        let outcome = LnbLifecycleTxn::new().close(
            &mut runtime,
            &mut backend,
            LnbLifecycleReason::PublicClose,
        );
        assert!(outcome.result.is_ok());
        assert_eq!(runtime.state(), crate::runtime::LnbRuntimeState::Closed);
        assert_eq!(runtime.registry_state(), LnbElectricalState::safe());
        assert_eq!(
            runtime.backend_committed_state(),
            LnbElectricalState::safe()
        );
        assert!(!runtime.callback_registered());
    }

    #[test]
    fn drop_path_does_not_apply_backend_cleanup() {
        let mut runtime = LnbRuntime::new(3);
        let backend = TestBackend::new();
        let outcome = record_lnb_drop_leak_lifecycle(&mut runtime);
        assert!(outcome.result.is_err());
        assert_eq!(backend.applied.len(), 0);
        assert_eq!(
            runtime.state(),
            crate::runtime::LnbRuntimeState::Quarantined
        );
    }
}
