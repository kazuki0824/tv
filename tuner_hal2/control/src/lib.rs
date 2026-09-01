#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerRuntimeOwnerFailure {
    ThreadPanic,
    JoinFailure,
    ResultLockPoison,
    CompletionLockPoison,
    MissingReport,
    ResultAlreadyCollected,
}

pub enum WorkerRuntimePoll<T, E> {
    Running,
    Completed(Result<T, E>),
    OwnerFailure(WorkerRuntimeOwnerFailure),
}

/// Canonical low-level owner for a spawned worker's JoinHandle, result cell and
/// completion wakeup. Domain layers add stop/cancel semantics but do not create
/// a second thread-result lifecycle owner.
pub struct WorkerRuntimeResultOwner<T, E> {
    result: std::sync::Arc<std::sync::Mutex<Option<Result<T, E>>>>,
    owner_failure: std::sync::Arc<std::sync::Mutex<Option<WorkerRuntimeOwnerFailure>>>,
    completion: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    join: Option<std::thread::JoinHandle<()>>,
    collected: bool,
}

impl<T, E> WorkerRuntimeResultOwner<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
{
    pub fn start(
        name: String,
        run: impl FnOnce() -> Result<T, E> + Send + 'static,
    ) -> std::io::Result<Self> {
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let owner_failure = std::sync::Arc::new(std::sync::Mutex::new(None));
        let completion =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let result_for_thread = std::sync::Arc::clone(&result);
        let failure_for_thread = std::sync::Arc::clone(&owner_failure);
        let completion_for_thread = std::sync::Arc::clone(&completion);
        let join = std::thread::Builder::new().name(name).spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
            match outcome {
                Ok(outcome) => match result_for_thread.lock() {
                    Ok(mut slot) => *slot = Some(outcome),
                    Err(_) => {
                        if let Ok(mut failure) = failure_for_thread.lock() {
                            *failure = Some(WorkerRuntimeOwnerFailure::ResultLockPoison);
                        }
                    }
                },
                Err(_) => {
                    if let Ok(mut failure) = failure_for_thread.lock() {
                        *failure = Some(WorkerRuntimeOwnerFailure::ThreadPanic);
                    }
                }
            }
            let (completed, wake) = &*completion_for_thread;
            match completed.lock() {
                Ok(mut completed) => {
                    *completed = true;
                    wake.notify_all();
                }
                Err(_) => {
                    if let Ok(mut failure) = failure_for_thread.lock() {
                        *failure = Some(WorkerRuntimeOwnerFailure::CompletionLockPoison);
                    }
                    wake.notify_all();
                }
            }
        })?;
        Ok(Self {
            result,
            owner_failure,
            completion,
            join: Some(join),
            collected: false,
        })
    }

    pub fn collect_if_finished(&mut self) -> WorkerRuntimePoll<T, E> {
        if self.collected {
            return WorkerRuntimePoll::OwnerFailure(
                WorkerRuntimeOwnerFailure::ResultAlreadyCollected,
            );
        }
        if self
            .join
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
        {
            return WorkerRuntimePoll::Running;
        }
        if let Some(handle) = self.join.take() {
            if handle.join().is_err() {
                self.collected = true;
                return WorkerRuntimePoll::OwnerFailure(WorkerRuntimeOwnerFailure::JoinFailure);
            }
        }
        self.collected = true;
        match self.take_result() {
            Ok(result) => WorkerRuntimePoll::Completed(result),
            Err(error) => WorkerRuntimePoll::OwnerFailure(error),
        }
    }

    pub fn join_after_stop(mut self) -> Result<Result<T, E>, WorkerRuntimeOwnerFailure> {
        if self.collected {
            return Err(WorkerRuntimeOwnerFailure::ResultAlreadyCollected);
        }
        self.collected = true;
        if let Some(handle) = self.join.take() {
            if handle.join().is_err() {
                return Err(WorkerRuntimeOwnerFailure::JoinFailure);
            }
        }
        self.take_result()
    }

    pub fn is_thread_finished(&self) -> bool {
        self.join
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(true)
    }

    pub fn unpark(&self) {
        if let Some(handle) = self.join.as_ref() {
            handle.thread().unpark();
        }
    }

    pub fn wait_until_finished(
        &self,
        deadline: Option<std::time::Instant>,
    ) -> Result<bool, WorkerRuntimeOwnerFailure> {
        let (completed, wake) = &*self.completion;
        let mut completed = completed
            .lock()
            .map_err(|_| WorkerRuntimeOwnerFailure::CompletionLockPoison)?;
        loop {
            if *completed {
                return Ok(true);
            }
            match deadline {
                Some(deadline) => {
                    let now = std::time::Instant::now();
                    if now >= deadline {
                        return Ok(false);
                    }
                    let (next, timeout) = wake
                        .wait_timeout(completed, deadline.saturating_duration_since(now))
                        .map_err(|_| WorkerRuntimeOwnerFailure::CompletionLockPoison)?;
                    completed = next;
                    if timeout.timed_out() && !*completed {
                        return Ok(false);
                    }
                }
                None => {
                    completed = wake
                        .wait(completed)
                        .map_err(|_| WorkerRuntimeOwnerFailure::CompletionLockPoison)?;
                }
            }
        }
    }

    fn take_result(&mut self) -> Result<Result<T, E>, WorkerRuntimeOwnerFailure> {
        if let Some(failure) = self
            .owner_failure
            .lock()
            .map_err(|_| WorkerRuntimeOwnerFailure::ResultLockPoison)?
            .take()
        {
            return Err(failure);
        }
        self.result
            .lock()
            .map_err(|_| WorkerRuntimeOwnerFailure::ResultLockPoison)?
            .take()
            .ok_or(WorkerRuntimeOwnerFailure::MissingReport)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerStopReason {
    ExplicitClose,
    Reconfigure,
    OwnerLoss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerRuntimeFailureKind {
    SignalPoisoned,
    BackendFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerFailureDomain {
    Signal,
    Backend,
}

impl WorkerFailureDomain {
    pub const fn runtime_failure_kind(self) -> WorkerRuntimeFailureKind {
        match self {
            WorkerFailureDomain::Signal => WorkerRuntimeFailureKind::SignalPoisoned,
            WorkerFailureDomain::Backend => WorkerRuntimeFailureKind::BackendFailed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerExit {
    Normal,
    StopRequested(WorkerStopReason),
    RuntimeFailure(WorkerRuntimeFailureKind),
    PanicOrJoinFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FmqObjectKind {
    Filter,
    DvrRecord,
    DvrPlayback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FmqDeliveryPhase {
    CapacityCheck,
    Write,
    Wake,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FmqFailureKind {
    WriteFailed,
    ShortWrite,
    EventFlagWakeFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FmqDeliveryAction {
    Continue,
    WakePending,
    Overflow,
    RuntimeFailed(FmqFailureKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FmqDeliveryResult {
    pub object_kind: FmqObjectKind,
    pub phase: FmqDeliveryPhase,
    pub bytes: usize,
    pub action: FmqDeliveryAction,
}

pub struct FmqDeliveryTxn {
    object_kind: FmqObjectKind,
    phase: FmqDeliveryPhase,
}

impl FmqDeliveryTxn {
    pub fn new(object_kind: FmqObjectKind) -> Self {
        Self {
            object_kind,
            phase: FmqDeliveryPhase::CapacityCheck,
        }
    }

    pub fn commit_payload(
        self,
        expected_bytes: usize,
        write_result: Result<usize, FmqFailureKind>,
        wake_result: Result<(), FmqFailureKind>,
    ) -> FmqDeliveryResult {
        let written_bytes = match write_result {
            Ok(written_bytes) if written_bytes == expected_bytes => written_bytes,
            Ok(_) => {
                return FmqDeliveryResult {
                    object_kind: self.object_kind,
                    phase: FmqDeliveryPhase::Write,
                    bytes: 0,
                    action: FmqDeliveryAction::RuntimeFailed(FmqFailureKind::ShortWrite),
                };
            }
            Err(err) => {
                return FmqDeliveryResult {
                    object_kind: self.object_kind,
                    phase: FmqDeliveryPhase::Write,
                    bytes: 0,
                    action: FmqDeliveryAction::RuntimeFailed(err),
                };
            }
        };

        match wake_result {
            Ok(()) => FmqDeliveryResult {
                object_kind: self.object_kind,
                phase: FmqDeliveryPhase::Wake,
                bytes: written_bytes,
                action: FmqDeliveryAction::Continue,
            },
            Err(err) => FmqDeliveryResult {
                object_kind: self.object_kind,
                phase: FmqDeliveryPhase::Wake,
                bytes: written_bytes,
                action: if err == FmqFailureKind::EventFlagWakeFailed {
                    FmqDeliveryAction::WakePending
                } else {
                    FmqDeliveryAction::RuntimeFailed(err)
                },
            },
        }
    }

    pub fn overflow(self) -> FmqDeliveryResult {
        FmqDeliveryResult {
            object_kind: self.object_kind,
            phase: self.phase,
            bytes: 0,
            action: FmqDeliveryAction::Overflow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_failure_domain_maps_to_runtime_failure_kind() {
        assert_eq!(
            WorkerFailureDomain::Signal.runtime_failure_kind(),
            WorkerRuntimeFailureKind::SignalPoisoned
        );
        assert_eq!(
            WorkerFailureDomain::Backend.runtime_failure_kind(),
            WorkerRuntimeFailureKind::BackendFailed
        );
    }

    #[test]
    fn fmq_wake_failure_preserves_committed_payload_for_retry() {
        let result = FmqDeliveryTxn::new(FmqObjectKind::Filter).commit_payload(
            188,
            Ok(188),
            Err(FmqFailureKind::EventFlagWakeFailed),
        );
        assert_eq!(result.action, FmqDeliveryAction::WakePending);
        assert_eq!(result.bytes, 188);
    }

    #[test]
    fn fmq_short_write_fails_before_wake_commit() {
        let result =
            FmqDeliveryTxn::new(FmqObjectKind::Filter).commit_payload(188, Ok(187), Ok(()));
        assert_eq!(result.phase, FmqDeliveryPhase::Write);
        assert_eq!(
            result.action,
            FmqDeliveryAction::RuntimeFailed(FmqFailureKind::ShortWrite)
        );
    }
}
