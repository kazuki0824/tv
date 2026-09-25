//! 正規control-core worker result ownerに対するdevice-domain adapter。

use std::time::Instant;

use maleicacid_tuner_hal2_common::{HalError, HalErrorDetail, HalInternalKind, WorkerLockKind};
use maleicacid_tuner_hal2_control_core::{
    WorkerContext, WorkerHandle, WorkerRuntime, WorkerRuntimeOwnerFailure, WorkerRuntimePoll,
    WorkerTerminalResult,
};

fn owner_failure_to_hal(error: WorkerRuntimeOwnerFailure, name: &'static str) -> HalError {
    let detail = match error {
        WorkerRuntimeOwnerFailure::ThreadPanic => "thread panicked",
        WorkerRuntimeOwnerFailure::JoinFailure => "thread join failed",
        WorkerRuntimeOwnerFailure::ResultLockPoison => {
            return HalError::WorkerLockPoisoned {
                owner: name,
                lock: WorkerLockKind::Result,
            };
        }
        WorkerRuntimeOwnerFailure::CompletionLockPoison => {
            return HalError::WorkerLockPoisoned {
                owner: name,
                lock: WorkerLockKind::Completion,
            };
        }
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
        Self::start_controlled(name, move |_| run())
    }

    pub(crate) fn start_controlled(
        name: &'static str,
        run: impl FnOnce(WorkerContext) -> Result<T, HalError> + Send + 'static,
    ) -> Result<Self, HalError> {
        let owner =
            WorkerRuntime::spawn_controlled_handle(name.to_owned(), run).map_err(|error| {
                HalError::Io {
                    backend: name,
                    operation: "スレッド生成",
                    path: None,
                    errno: error.raw_os_error(),
                    detail: HalErrorDetail::new(error.to_string()),
                }
            })?;
        Ok(Self { owner, name })
    }

    pub(crate) fn request_stop(&self) {
        self.owner.request_stop()
    }

    pub(crate) fn wake(&self) {
        self.owner.wake()
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

    pub(crate) fn collect_terminal_if_finished(&mut self) -> Option<WorkerTerminalResult<T>> {
        match self.owner.collect_if_finished() {
            WorkerRuntimePoll::Running => None,
            WorkerRuntimePoll::Completed(Ok(result)) => Some(WorkerTerminalResult::Normal(result)),
            WorkerRuntimePoll::Completed(Err(error)) => {
                Some(WorkerTerminalResult::RuntimeFailure(error))
            }
            WorkerRuntimePoll::OwnerFailure(error) => Some(error.into_terminal_result(self.name)),
        }
    }

    pub(crate) fn join_terminal_after_stop(self) -> WorkerTerminalResult<T> {
        match self.owner.join_after_stop() {
            Ok(Ok(result)) => WorkerTerminalResult::Normal(result),
            Ok(Err(error)) => WorkerTerminalResult::RuntimeFailure(error),
            Err(error) => error.into_terminal_result(self.name),
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
    fn owner_poison_preserves_lock_identity() {
        for (failure, lock) in [
            (
                WorkerRuntimeOwnerFailure::ResultLockPoison,
                WorkerLockKind::Result,
            ),
            (
                WorkerRuntimeOwnerFailure::CompletionLockPoison,
                WorkerLockKind::Completion,
            ),
        ] {
            assert_eq!(
                owner_failure_to_hal(failure, "frontend"),
                HalError::WorkerLockPoisoned {
                    owner: "frontend",
                    lock,
                }
            );
        }
    }

    #[test]
    fn adapter_reports_normal_completion() {
        let owner = ThreadResultOwner::start("normal", || Ok(7u32)).unwrap();
        assert_eq!(owner.join_after_stop().unwrap(), 7);
    }

    #[test]
    fn collected_result_does_not_become_a_panic() {
        let mut owner = ThreadResultOwner::start("collected", || Ok(7u32)).unwrap();
        assert!(owner.wait_until_finished(None).unwrap());
        assert_eq!(
            owner.collect_terminal_if_finished(),
            Some(WorkerTerminalResult::Normal(7))
        );
        assert!(matches!(
            owner.join_terminal_after_stop(),
            WorkerTerminalResult::RuntimeFailure(_)
        ));
    }

    #[test]
    fn panic_keeps_its_terminal_category() {
        let owner = ThreadResultOwner::<()>::start("panic", || panic!("injected")).unwrap();
        assert_eq!(
            owner.join_terminal_after_stop(),
            WorkerTerminalResult::PanicOrJoinFailure
        );
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
