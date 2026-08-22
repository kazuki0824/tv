use crate::runtime::{
    LnbBackendApplyOutcome, LnbBackendOps, LnbElectricalState, LnbRuntime, LnbRuntimeState,
};
use crate::{LnbFailureKind, LnbFailureRecord, LnbFailureStep};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbApplyStep {
    ValidateState,
    AdvanceGeneration,
    ApplyBackend,
    CommitRegistry,
    CommitOpen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnbApplyOutcome {
    pub steps: Vec<LnbApplyStep>,
    pub result: Result<LnbElectricalState, LnbFailureRecord>,
}

#[must_use = "a prepared LNB state change must consume exactly one backend result"]
#[derive(Debug, Eq, PartialEq)]
pub struct PreparedLnbStateApply {
    lnb_id: i32,
    expected_generation: u64,
    next_generation: u64,
    target_state: LnbElectricalState,
}

impl PreparedLnbStateApply {
    fn begin_with_generation(
        runtime: &mut LnbRuntime,
        target_state: LnbElectricalState,
        next_generation: u64,
    ) -> Result<Self, LnbFailureRecord> {
        runtime.begin_apply()?;
        if next_generation <= runtime.generation() {
            return Err(runtime.quarantine_generation_overflow());
        }
        Ok(Self {
            lnb_id: runtime.lnb_id(),
            expected_generation: runtime.generation(),
            next_generation,
            target_state,
        })
    }

    pub const fn lnb_id(&self) -> i32 {
        self.lnb_id
    }

    pub const fn target_state(&self) -> LnbElectricalState {
        self.target_state
    }
}

pub fn prepare_lnb_state_apply(
    runtime: &mut LnbRuntime,
    target_state: LnbElectricalState,
) -> Result<PreparedLnbStateApply, LnbFailureRecord> {
    let next_generation = match runtime.checked_next_generation() {
        Ok(next) => next,
        Err(_) => return Err(runtime.quarantine_generation_overflow()),
    };
    PreparedLnbStateApply::begin_with_generation(runtime, target_state, next_generation)
}

pub fn finish_lnb_state_apply(
    runtime: &mut LnbRuntime,
    prepared: PreparedLnbStateApply,
    backend_result: LnbBackendApplyOutcome,
) -> Result<LnbElectricalState, LnbFailureRecord> {
    if runtime.lnb_id() != prepared.lnb_id
        || runtime.generation() != prepared.expected_generation
        || runtime.state() != LnbRuntimeState::Applying
    {
        return Err(
            runtime.record_failure(LnbFailureKind::InvalidState, LnbFailureStep::ValidateState)
        );
    }
    match backend_result {
        LnbBackendApplyOutcome::Applied => {}
        LnbBackendApplyOutcome::Rejected(kind) => {
            return Err(runtime.abort_rejected_apply(kind, LnbFailureStep::ApplyBackend));
        }
        LnbBackendApplyOutcome::Indeterminate(kind) => {
            return Err(
                runtime.quarantine_indeterminate_backend(kind, LnbFailureStep::ApplyBackend)
            );
        }
    }
    runtime.commit_successful_apply(prepared.target_state, prepared.next_generation);
    Ok(prepared.target_state)
}

#[derive(Debug, Default)]
pub(crate) struct LnbApplyTxn {
    steps: Vec<LnbApplyStep>,
}

impl LnbApplyTxn {
    pub(crate) fn new() -> Self {
        Self { steps: Vec::new() }
    }
    fn record_step(&mut self, step: LnbApplyStep) {
        self.steps.push(step);
    }

    pub(crate) fn apply<B: LnbBackendOps>(
        self,
        runtime: &mut LnbRuntime,
        backend: &mut B,
        target_state: LnbElectricalState,
    ) -> LnbApplyOutcome {
        let next_generation = match runtime.checked_next_generation() {
            Ok(next) => next,
            Err(_) => {
                let record = runtime.quarantine_generation_overflow();
                return LnbApplyOutcome {
                    steps: vec![LnbApplyStep::AdvanceGeneration],
                    result: Err(record),
                };
            }
        };
        self.apply_with_generation(runtime, backend, target_state, next_generation)
    }

    pub(crate) fn apply_with_generation<B: LnbBackendOps>(
        mut self,
        runtime: &mut LnbRuntime,
        backend: &mut B,
        target_state: LnbElectricalState,
        next_generation: u64,
    ) -> LnbApplyOutcome {
        self.record_step(LnbApplyStep::ValidateState);
        let prepared = match PreparedLnbStateApply::begin_with_generation(
            runtime,
            target_state,
            next_generation,
        ) {
            Ok(prepared) => prepared,
            Err(record) => {
                return LnbApplyOutcome {
                    steps: self.steps,
                    result: Err(record),
                };
            }
        };
        self.record_step(LnbApplyStep::AdvanceGeneration);

        self.record_step(LnbApplyStep::ApplyBackend);
        let backend_result = backend.apply_lnb_state(runtime.lnb_id(), target_state);
        if backend_result != LnbBackendApplyOutcome::Applied {
            return LnbApplyOutcome {
                steps: self.steps,
                result: finish_lnb_state_apply(runtime, prepared, backend_result),
            };
        }

        self.record_step(LnbApplyStep::CommitRegistry);
        if let Err(record) = finish_lnb_state_apply(runtime, prepared, backend_result) {
            return LnbApplyOutcome {
                steps: self.steps,
                result: Err(record),
            };
        }

        self.record_step(LnbApplyStep::CommitOpen);
        LnbApplyOutcome {
            steps: self.steps,
            result: Ok(target_state),
        }
    }
}

pub fn apply_lnb_state_with_txn<B: LnbBackendOps>(
    runtime: &mut LnbRuntime,
    backend: &mut B,
    target_state: LnbElectricalState,
) -> LnbApplyOutcome {
    LnbApplyTxn::new().apply(runtime, backend, target_state)
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
        ) -> LnbBackendApplyOutcome {
            if self.fail {
                return LnbBackendApplyOutcome::Indeterminate(LnbFailureKind::BackendApplyFailed);
            }
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
    fn apply_commits_backend_and_registry_together() {
        let mut runtime = LnbRuntime::new(1);
        let mut backend = TestBackend::new();
        let target = LnbElectricalState {
            voltage: LnbVoltage::Voltage11V,
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
    fn backend_success_has_an_infallible_final_commit() {
        let mut runtime = LnbRuntime::new(1);
        let mut backend = TestBackend::new();
        let target = LnbElectricalState {
            voltage: LnbVoltage::Voltage15V,
            tone: LnbTone::Off,
            satellite_position: None,
        };
        let outcome = LnbApplyTxn::new().apply(&mut runtime, &mut backend, target);
        assert_eq!(outcome.result, Ok(target));
        assert_eq!(runtime.state(), crate::runtime::LnbRuntimeState::Open);
        assert_eq!(runtime.registry_state(), target);
        assert_eq!(runtime.backend_committed_state(), target);
        assert_eq!(runtime.generation(), 1);
    }

    #[test]
    fn explicit_backend_rejection_restores_open_with_registry_unchanged() {
        let mut runtime = LnbRuntime::new(1);
        let target = LnbElectricalState {
            voltage: LnbVoltage::Voltage15V,
            tone: LnbTone::Off,
            satellite_position: None,
        };
        let prepared = prepare_lnb_state_apply(&mut runtime, target).unwrap();

        let result = finish_lnb_state_apply(
            &mut runtime,
            prepared,
            LnbBackendApplyOutcome::Rejected(LnbFailureKind::BackendApplyFailed),
        );

        assert!(result.is_err());
        assert_eq!(runtime.state(), LnbRuntimeState::Open);
        assert_eq!(runtime.registry_state(), LnbElectricalState::safe());
        assert_eq!(
            runtime.backend_committed_state(),
            LnbElectricalState::safe()
        );
        assert_eq!(runtime.generation(), 0);
    }

    #[test]
    fn indeterminate_backend_result_quarantines_without_committing_candidate() {
        let mut runtime = LnbRuntime::new(1);
        let target = LnbElectricalState {
            voltage: LnbVoltage::Voltage15V,
            tone: LnbTone::Off,
            satellite_position: None,
        };
        let prepared = prepare_lnb_state_apply(&mut runtime, target).unwrap();

        let result = finish_lnb_state_apply(
            &mut runtime,
            prepared,
            LnbBackendApplyOutcome::Indeterminate(LnbFailureKind::BackendApplyFailed),
        );

        assert!(result.is_err());
        assert_eq!(runtime.state(), LnbRuntimeState::Quarantined);
        assert_eq!(runtime.registry_state(), LnbElectricalState::safe());
        assert_eq!(runtime.generation(), 0);
    }
}
