//! open / close / configure の部分成功を防ぐ共通 transaction。
//!
//! WP-03 では、public API 主経路の validate / prepare / apply / commit /
//! rollback / cleanup をこの型に集約する。単なる step 記録ではなく、
//! 失敗段階、rollback 結果、cleanup 結果を保持する。

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum LifecycleStage {
    Validate,
    Prepare,
    Apply,
    Commit,
    Rollback,
    Cleanup,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TxnStep {
    pub stage: LifecycleStage,
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CleanupOutcome {
    NotRun,
    Success,
    Failed,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FirstError {
    pub stage: LifecycleStage,
    pub step: &'static str,
}

#[derive(Debug, Default)]
pub struct LifecycleTxn {
    steps: Vec<TxnStep>,
    first_error: Option<FirstError>,
    cleanup_outcome: CleanupOutcome,
}

impl LifecycleTxn {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn validate<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where
        F: FnOnce() -> Result<(), E>,
    {
        self.run_stage(LifecycleStage::Validate, name, f)
    }
    pub fn prepare<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where
        F: FnOnce() -> Result<(), E>,
    {
        self.run_stage(LifecycleStage::Prepare, name, f)
    }
    pub fn apply<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where
        F: FnOnce() -> Result<(), E>,
    {
        self.run_stage(LifecycleStage::Apply, name, f)
    }
    pub fn commit<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where
        F: FnOnce() -> Result<(), E>,
    {
        self.run_stage(LifecycleStage::Commit, name, f)
    }
    pub fn rollback<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where
        F: FnOnce() -> Result<(), E>,
    {
        self.run_stage(LifecycleStage::Rollback, name, f)
    }
    pub fn cleanup<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where
        F: FnOnce() -> Result<(), E>,
    {
        let result = self.run_stage(LifecycleStage::Cleanup, name, f);
        self.cleanup_outcome = if result.is_ok() {
            CleanupOutcome::Success
        } else {
            CleanupOutcome::Failed
        };
        result
    }
    pub fn cleanup_value<F, T, E>(&mut self, name: &'static str, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let result = self.run_stage_value(LifecycleStage::Cleanup, name, f);
        self.cleanup_outcome = if result.is_ok() {
            CleanupOutcome::Success
        } else {
            CleanupOutcome::Failed
        };
        result
    }
    pub fn prepare_value<F, T, E>(&mut self, name: &'static str, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        self.run_stage_value(LifecycleStage::Prepare, name, f)
    }
    pub fn apply_value<F, T, E>(&mut self, name: &'static str, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        self.run_stage_value(LifecycleStage::Apply, name, f)
    }
    pub fn commit_value<F, T, E>(&mut self, name: &'static str, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        self.run_stage_value(LifecycleStage::Commit, name, f)
    }

    fn run_stage<F, E>(&mut self, stage: LifecycleStage, name: &'static str, f: F) -> Result<(), E>
    where
        F: FnOnce() -> Result<(), E>,
    {
        self.steps.push(TxnStep { stage, name });
        match f() {
            Ok(()) => Ok(()),
            Err(e) => {
                self.record_error_once_at(stage, name);
                Err(e)
            }
        }
    }

    fn run_stage_value<F, T, E>(
        &mut self,
        stage: LifecycleStage,
        name: &'static str,
        f: F,
    ) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        self.steps.push(TxnStep { stage, name });
        match f() {
            Ok(value) => Ok(value),
            Err(e) => {
                self.record_error_once_at(stage, name);
                Err(e)
            }
        }
    }
    pub fn record_error_once_at(&mut self, stage: LifecycleStage, step: &'static str) {
        if self.first_error.is_none() {
            self.first_error = Some(FirstError { stage, step });
        }
    }
    #[cfg(test)]
    pub fn first_error(&self) -> Option<&FirstError> {
        self.first_error.as_ref()
    }
    #[cfg(test)]
    pub fn steps(&self) -> &[TxnStep] {
        &self.steps
    }
    #[cfg(test)]
    pub fn cleanup_outcome(&self) -> CleanupOutcome {
        self.cleanup_outcome
    }
}

impl Default for CleanupOutcome {
    fn default() -> Self {
        Self::NotRun
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleCleanupStepResult {
    Success,
    SafeNoOp,
    Failed,
    Unknown,
}
impl LifecycleCleanupStepResult {
    pub fn is_complete(self) -> bool {
        matches!(self, Self::Success | Self::SafeNoOp)
    }
}
impl Default for LifecycleCleanupStepResult {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Common close-step runner for resumable resource cleanup.
///
/// The concrete owner still supplies the actual cleanup operation and the
/// persistent step marker, but this type owns the common order:
/// run current step, advance marker, and route failures through a single
/// failure recorder.
#[derive(Debug, Clone, Copy)]
pub struct CloseStepTxn<Step> {
    current: Step,
}

impl<Step> CloseStepTxn<Step>
where
    Step: Copy + Ord,
{
    pub fn new(current: Step) -> Self {
        Self { current }
    }
    pub fn current_step(&self) -> Step {
        self.current
    }

    pub fn run_required<F, M, R, E>(
        &mut self,
        step: Step,
        next: Step,
        operation: F,
        mark_next: M,
        record_failure: R,
    ) -> Result<(), E>
    where
        F: FnOnce() -> Result<(), E>,
        M: FnOnce(Step) -> Result<(), E>,
        R: FnOnce(Step, E) -> Result<(), E>,
    {
        if self.current > step {
            return Ok(());
        }
        if let Err(error) = operation() {
            return record_failure(step, error);
        }
        self.current = next;
        if let Err(error) = mark_next(next) {
            return record_failure(next, error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_txn_keeps_first_error_stage() {
        let mut txn = LifecycleTxn::new();
        let _: Result<(), ()> = txn.prepare("prepare", || Err(()));
        let _: Result<(), ()> = txn.cleanup("cleanup", || Err(()));
        let err = txn.first_error().unwrap();
        assert_eq!(err.stage, LifecycleStage::Prepare);
        assert_eq!(err.step, "prepare");
        assert_eq!(txn.cleanup_outcome(), CleanupOutcome::Failed);
    }

    #[test]
    fn lifecycle_txn_value_stages_and_rollback_are_recorded() {
        let mut txn = LifecycleTxn::new();
        let prepared: Result<i32, ()> = txn.prepare_value("prepare_value", || Ok(10));
        assert_eq!(prepared, Ok(10));
        let applied: Result<(), ()> = txn.apply("apply", || Err(()));
        assert_eq!(applied, Err(()));
        let rollback: Result<(), ()> = txn.rollback("rollback", || Ok(()));
        assert_eq!(rollback, Ok(()));
        assert_eq!(txn.first_error().unwrap().stage, LifecycleStage::Apply);
        assert_eq!(txn.first_error().unwrap().step, "apply");
        assert!(txn
            .steps()
            .iter()
            .any(|step| step.stage == LifecycleStage::Rollback && step.name == "rollback"));
    }
}
