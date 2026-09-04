//! 正規control-core worker result ownerに対するdevice-domain adapter。

use std::time::Instant;

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_control_core::{
    WorkerHandle, WorkerRuntime, WorkerRuntimeOwnerFailure, WorkerRuntimePoll,
};

fn owner_failure_to_hal(error: WorkerRuntimeOwnerFailure, name: &'static str) -> HalError {
    let detail = match error {
        WorkerRuntimeOwnerFailure::ThreadPanic => "thread panicked",
        WorkerRuntimeOwnerFailure::JoinFailure => "thread join failed",
        WorkerRuntimeOwnerFailure::ResultLockPoison => "thread result lock poisoned",
        WorkerRuntimeOwnerFailure::CompletionLockPoison => "thread completion lock poisoned",
        WorkerRuntimeOwnerFailure::MissingReport => "finished without report",
        WorkerRuntimeOwnerFailure::ResultAlreadyCollected => "thread result already collected",
    };
    HalError::internal(
        HalInternalKind::InvariantViolation,
        format!("{name}: {detail}"),
    )
}

pub(crate) enum ThreadResultPoll<T> {
    Running,
    Completed(Result<T, HalError>),
}

pub(crate) struct ThreadResultOwner<T> {
    owner: WorkerHandle<T, HalError>,
    name: &'static str,
}

impl<T> core::fmt::Debug for ThreadResultOwner<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ThreadResultOwner")
            .field("name", &self.name)
            .finish()
    }
}

impl<T> ThreadResultOwner<T>
where
    T: Send + 'static,
{
    pub(crate) fn start(
        name: &'static str,
        run: impl FnOnce() -> Result<T, HalError> + Send + 'static,
    ) -> Result<Self, HalError> {
        let owner = WorkerRuntime::spawn_handle(name.to_owned(), run).map_err(|error| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{name}: thread spawn failed: {error}"),
            )
        })?;
        Ok(Self { owner, name })
    }

    pub(crate) fn collect_if_finished(&mut self) -> ThreadResultPoll<T> {
        match self.owner.collect_if_finished() {
            WorkerRuntimePoll::Running => ThreadResultPoll::Running,
            WorkerRuntimePoll::Completed(result) => ThreadResultPoll::Completed(result),
            WorkerRuntimePoll::OwnerFailure(error) => {
                ThreadResultPoll::Completed(Err(owner_failure_to_hal(error, self.name)))
            }
        }
    }

    pub(crate) fn join_after_stop(self) -> Result<T, HalError> {
        match self.owner.join_after_stop() {
            Ok(result) => result,
            Err(error) => Err(owner_failure_to_hal(error, self.name)),
        }
    }

    pub(crate) fn wait_until_finished(&self, deadline: Option<Instant>) -> Result<bool, HalError> {
        self.owner
            .wait_until_finished(deadline)
            .map_err(|error| owner_failure_to_hal(error, self.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn adapter_reports_normal_completion() {
        let owner = ThreadResultOwner::start("normal", || Ok(7u32)).unwrap();
        assert_eq!(owner.join_after_stop().unwrap(), 7);
    }

    #[test]
    fn adapter_reports_running_then_completion() {
        let mut owner = ThreadResultOwner::start("running", || {
            std::thread::sleep(Duration::from_millis(20));
            Ok(())
        })
        .unwrap();
        assert!(matches!(
            owner.collect_if_finished(),
            ThreadResultPoll::Running
        ));
        assert!(owner
            .wait_until_finished(Some(Instant::now() + Duration::from_secs(1)))
            .unwrap());
        assert!(matches!(
            owner.collect_if_finished(),
            ThreadResultPoll::Completed(Ok(()))
        ));
    }
}
