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
    pub fn new() -> Self { Self::default() }

    pub fn validate<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where F: FnOnce() -> Result<(), E> { self.run_stage(LifecycleStage::Validate, name, f) }
    pub fn prepare<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where F: FnOnce() -> Result<(), E> { self.run_stage(LifecycleStage::Prepare, name, f) }
    pub fn apply<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where F: FnOnce() -> Result<(), E> { self.run_stage(LifecycleStage::Apply, name, f) }
    pub fn commit<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where F: FnOnce() -> Result<(), E> { self.run_stage(LifecycleStage::Commit, name, f) }
    pub fn rollback<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where F: FnOnce() -> Result<(), E> { self.run_stage(LifecycleStage::Rollback, name, f) }
    pub fn cleanup<F, E>(&mut self, name: &'static str, f: F) -> Result<(), E>
    where F: FnOnce() -> Result<(), E> {
        let result = self.run_stage(LifecycleStage::Cleanup, name, f);
        self.cleanup_outcome = if result.is_ok() { CleanupOutcome::Success } else { CleanupOutcome::Failed };
        result
    }
    pub fn cleanup_value<F, T, E>(&mut self, name: &'static str, f: F) -> Result<T, E>
    where F: FnOnce() -> Result<T, E> {
        let result = self.run_stage_value(LifecycleStage::Cleanup, name, f);
        self.cleanup_outcome = if result.is_ok() { CleanupOutcome::Success } else { CleanupOutcome::Failed };
        result
    }
    pub fn prepare_value<F, T, E>(&mut self, name: &'static str, f: F) -> Result<T, E>
    where F: FnOnce() -> Result<T, E> { self.run_stage_value(LifecycleStage::Prepare, name, f) }
    pub fn apply_value<F, T, E>(&mut self, name: &'static str, f: F) -> Result<T, E>
    where F: FnOnce() -> Result<T, E> { self.run_stage_value(LifecycleStage::Apply, name, f) }
    pub fn commit_value<F, T, E>(&mut self, name: &'static str, f: F) -> Result<T, E>
    where F: FnOnce() -> Result<T, E> { self.run_stage_value(LifecycleStage::Commit, name, f) }

    fn run_stage<F, E>(&mut self, stage: LifecycleStage, name: &'static str, f: F) -> Result<(), E>
    where F: FnOnce() -> Result<(), E> {
        self.steps.push(TxnStep { stage, name });
        match f() {
            Ok(()) => Ok(()),
            Err(e) => { self.record_error_once_at(stage, name); Err(e) }
        }
    }

    fn run_stage_value<F, T, E>(&mut self, stage: LifecycleStage, name: &'static str, f: F) -> Result<T, E>
    where F: FnOnce() -> Result<T, E> {
        self.steps.push(TxnStep { stage, name });
        match f() {
            Ok(value) => Ok(value),
            Err(e) => { self.record_error_once_at(stage, name); Err(e) }
        }
    }
    pub fn record_error_once_at(&mut self, stage: LifecycleStage, step: &'static str) {
        if self.first_error.is_none() { self.first_error = Some(FirstError { stage, step }); }
    }
    #[cfg(test)]
    pub fn first_error(&self) -> Option<&FirstError> { self.first_error.as_ref() }
    #[cfg(test)]
    pub fn steps(&self) -> &[TxnStep] { &self.steps }
    #[cfg(test)]
    pub fn cleanup_outcome(&self) -> CleanupOutcome { self.cleanup_outcome }
}

impl Default for CleanupOutcome { fn default() -> Self { Self::NotRun } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleCleanupCaller { ExternalClose, BestEffortDrop, WorkerFailure }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleCleanupStepResult { Success, SafeNoOp, Failed, Unknown, SkippedDueToWorkerFailureContext }
impl LifecycleCleanupStepResult { pub fn is_complete(self) -> bool { matches!(self, Self::Success | Self::SafeNoOp) } }
impl Default for LifecycleCleanupStepResult { fn default() -> Self { Self::Unknown } }

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CloseCleanupStepResults {
    pub callback_worker: LifecycleCleanupStepResult,
    pub queue_clear: LifecycleCleanupStepResult,
    pub runtime_unregister: LifecycleCleanupStepResult,
    pub queue_stop: LifecycleCleanupStepResult,
    pub demux_unregister: LifecycleCleanupStepResult,
    pub key_release: LifecycleCleanupStepResult,
    pub registry_unregister: LifecycleCleanupStepResult,
}

pub type DvrCleanupStepResults = CloseCleanupStepResults;
pub type FilterCleanupStepResults = CloseCleanupStepResults;

#[derive(Debug)]
pub struct CloseCleanupOutcome<E> {
    pub first_error: Option<E>,
    pub all_cleanup_complete: bool,
    pub step_results: CloseCleanupStepResults,
}
impl<E> CloseCleanupOutcome<E> {
    #[cfg(test)]
    pub fn new(first_error: Option<E>, all_cleanup_complete: bool, step_results: CloseCleanupStepResults) -> Self {
        Self { first_error, all_cleanup_complete, step_results }
    }
}
pub type DvrCleanupOutcome<E> = CloseCleanupOutcome<E>;
pub type FilterCleanupOutcome<E> = CloseCleanupOutcome<E>;

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
        assert!(txn.steps().iter().any(|step| step.stage == LifecycleStage::Rollback && step.name == "rollback"));
    }
}
