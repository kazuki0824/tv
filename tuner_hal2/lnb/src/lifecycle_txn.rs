use crate::runtime::{LnbBackendOps, LnbElectricalState, LnbRuntime};
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
    DropLeak,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnbLifecycleOutcome {
    pub reason: LnbLifecycleReason,
    pub steps: Vec<LnbLifecycleStep>,
    pub result: Result<(), LnbFailureRecord>,
}

#[derive(Debug, Default)]
pub struct LnbLifecycleTxn {
    steps: Vec<LnbLifecycleStep>,
}

impl LnbLifecycleTxn {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }
    fn record_step(&mut self, step: LnbLifecycleStep) {
        self.steps.push(step);
    }

    pub fn close<B: LnbBackendOps>(
        mut self,
        runtime: &mut LnbRuntime,
        backend: &mut B,
        reason: LnbLifecycleReason,
    ) -> LnbLifecycleOutcome {
        if reason == LnbLifecycleReason::DropLeak {
            self.record_step(LnbLifecycleStep::RecordDropLeak);
            let record = runtime.record_unclosed_drop();
            return LnbLifecycleOutcome {
                reason,
                steps: self.steps,
                result: Err(record),
            };
        }

        self.record_step(LnbLifecycleStep::MarkClosing);
        if let Err(record) = runtime.begin_close() {
            return LnbLifecycleOutcome {
                reason,
                steps: self.steps,
                result: Err(record),
            };
        }
        if runtime.state() == crate::runtime::LnbRuntimeState::Closed {
            return LnbLifecycleOutcome {
                reason,
                steps: self.steps,
                result: Ok(()),
            };
        }

        self.record_step(LnbLifecycleStep::BuildSafeState);
        let safe = LnbElectricalState::safe();

        self.record_step(LnbLifecycleStep::ApplySafeState);
        if let Err(_kind) = backend.apply_lnb_state(runtime.lnb_id(), safe) {
            let record = runtime.record_failure(
                LnbFailureKind::BackendApplyFailed,
                LnbFailureStep::ApplyBackend,
            );
            return LnbLifecycleOutcome {
                reason,
                steps: self.steps,
                result: Err(record),
            };
        }
        runtime.note_backend_applied(safe);

        self.record_step(LnbLifecycleStep::CommitRegistry);
        if let Err(record) = runtime.commit_registry(safe, LnbFailureStep::CommitRegistry) {
            return LnbLifecycleOutcome {
                reason,
                steps: self.steps,
                result: Err(record),
            };
        }

        self.record_step(LnbLifecycleStep::ClearRuntimeCallbackState);
        runtime.clear_callback();

        self.record_step(LnbLifecycleStep::CommitClosed);
        runtime.commit_closed();
        LnbLifecycleOutcome {
            reason,
            steps: self.steps,
            result: Ok(()),
        }
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
        ) -> Result<(), LnbFailureKind> {
            self.applied.push(state);
            Ok(())
        }
        fn send_diseqc_message(
            &mut self,
            _lnb_id: i32,
            _message: &crate::runtime::LnbDiseqcMessage,
        ) -> Result<(), LnbFailureKind> {
            Err(LnbFailureKind::DiseqcUnsupported)
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
        let mut backend = TestBackend::new();
        let outcome =
            LnbLifecycleTxn::new().close(&mut runtime, &mut backend, LnbLifecycleReason::DropLeak);
        assert!(outcome.result.is_err());
        assert_eq!(backend.applied.len(), 0);
        assert_eq!(
            runtime.state(),
            crate::runtime::LnbRuntimeState::Quarantined
        );
    }
}
