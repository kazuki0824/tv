use crate::runtime::{LnbBackendOps, LnbElectricalState, LnbRuntime};
use crate::{LnbFailureKind, LnbFailureRecord, LnbFailureStep};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbApplyStep {
    ValidateState,
    ApplyBackend,
    CommitRegistry,
    CommitOpen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnbApplyOutcome {
    pub steps: Vec<LnbApplyStep>,
    pub result: Result<LnbElectricalState, LnbFailureRecord>,
}

#[derive(Debug, Default)]
pub struct LnbApplyTxn {
    steps: Vec<LnbApplyStep>,
}

impl LnbApplyTxn {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }
    fn record_step(&mut self, step: LnbApplyStep) {
        self.steps.push(step);
    }

    pub fn apply<B: LnbBackendOps>(
        mut self,
        runtime: &mut LnbRuntime,
        backend: &mut B,
        target_state: LnbElectricalState,
    ) -> LnbApplyOutcome {
        self.record_step(LnbApplyStep::ValidateState);
        if let Err(record) = runtime.begin_apply() {
            return LnbApplyOutcome {
                steps: self.steps,
                result: Err(record),
            };
        }

        self.record_step(LnbApplyStep::ApplyBackend);
        if let Err(kind) = backend.apply_lnb_state(runtime.lnb_id(), target_state) {
            let record =
                runtime.record_failure(map_backend_failure(kind), LnbFailureStep::ApplyBackend);
            return LnbApplyOutcome {
                steps: self.steps,
                result: Err(record),
            };
        }
        runtime.note_backend_applied(target_state);

        self.record_step(LnbApplyStep::CommitRegistry);
        if let Err(record) = runtime.commit_registry(target_state, LnbFailureStep::CommitRegistry) {
            return LnbApplyOutcome {
                steps: self.steps,
                result: Err(record),
            };
        }

        self.record_step(LnbApplyStep::CommitOpen);
        runtime.commit_open();
        LnbApplyOutcome {
            steps: self.steps,
            result: Ok(target_state),
        }
    }
}

fn map_backend_failure(kind: LnbFailureKind) -> LnbFailureKind {
    match kind {
        LnbFailureKind::BackendApplyFailed => LnbFailureKind::BackendApplyFailed,
        _ => LnbFailureKind::BackendApplyFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{LnbTone, LnbVoltage};

    struct TestBackend {
        fail: bool,
        applied: Vec<LnbElectricalState>,
    }
    impl TestBackend {
        fn new() -> Self {
            Self {
                fail: false,
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
            if self.fail {
                return Err(LnbFailureKind::BackendApplyFailed);
            }
            self.applied.push(state);
            Ok(())
        }
        fn clear_lnb_callback(&mut self, _lnb_id: i32) -> Result<(), LnbFailureKind> {
            Ok(())
        }
    }

    #[test]
    fn apply_commits_backend_and_registry_together() {
        let mut runtime = LnbRuntime::new(1);
        let mut backend = TestBackend::new();
        let target = LnbElectricalState {
            voltage: LnbVoltage::V13,
            tone: LnbTone::On,
            satellite_position: Some(3),
        };
        let outcome = LnbApplyTxn::new().apply(&mut runtime, &mut backend, target);
        assert_eq!(outcome.result, Ok(target));
        assert_eq!(backend.applied, vec![target]);
        assert_eq!(runtime.registry_state(), target);
        assert_eq!(runtime.backend_committed_state(), target);
    }

    #[test]
    fn registry_commit_failure_is_not_normal_state() {
        let mut runtime = LnbRuntime::new(1);
        let mut backend = TestBackend::new();
        runtime.inject_next_registry_commit_failure(LnbFailureKind::RegistryCommitFailed);
        let target = LnbElectricalState {
            voltage: LnbVoltage::V18,
            tone: LnbTone::Off,
            satellite_position: None,
        };
        let outcome = LnbApplyTxn::new().apply(&mut runtime, &mut backend, target);
        assert_eq!(
            outcome.result.unwrap_err().kind,
            LnbFailureKind::RegistryCommitFailed
        );
        assert_eq!(runtime.state(), crate::runtime::LnbRuntimeState::Failed);
    }
}
