//! Shared worker thread result ownership.
//!
//! This module owns only the low-level thread-result cell and JoinHandle
//! contract. Cancel signals and domain-specific worker exit meanings stay with
//! the caller.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThreadResultFailure {
    ThreadPanic,
    JoinFailure,
    ResultLockPoison,
    MissingReport,
    ResultAlreadyCollected,
}

impl ThreadResultFailure {
    fn into_hal_error(self, name: &'static str) -> HalError {
        let detail = match self {
            ThreadResultFailure::ThreadPanic => "thread panicked",
            ThreadResultFailure::JoinFailure => "thread join failed",
            ThreadResultFailure::ResultLockPoison => "thread result lock poisoned",
            ThreadResultFailure::MissingReport => "finished without report",
            ThreadResultFailure::ResultAlreadyCollected => "thread result already collected",
        };
        HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("{name}: {detail}"),
        )
    }
}

pub(crate) enum ThreadResultPoll<T> {
    Running,
    Completed(Result<T, HalError>),
}

struct ThreadResultProducer<T> {
    result: Arc<Mutex<Option<Result<T, HalError>>>>,
    producer_failure: Arc<Mutex<Option<HalError>>>,
}

impl<T> ThreadResultProducer<T> {
    fn new(
        result: Arc<Mutex<Option<Result<T, HalError>>>>,
        producer_failure: Arc<Mutex<Option<HalError>>>,
    ) -> Self {
        Self {
            result,
            producer_failure,
        }
    }

    fn record_or_capture_failure(self, result: Result<T, HalError>, name: &'static str) {
        match self.result.lock() {
            Ok(mut guard) => {
                *guard = Some(result);
            }
            Err(_) => {
                let error = ThreadResultFailure::ResultLockPoison.into_hal_error(name);
                if let Ok(mut guard) = self.producer_failure.lock() {
                    *guard = Some(error);
                }
            }
        }
    }
}

pub(crate) struct ThreadResultOwner<T> {
    result: Arc<Mutex<Option<Result<T, HalError>>>>,
    producer_failure: Arc<Mutex<Option<HalError>>>,
    join: Option<JoinHandle<()>>,
    name: &'static str,
    join_failure: Option<HalError>,
    collected: bool,
}

impl<T> core::fmt::Debug for ThreadResultOwner<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ThreadResultOwner")
            .field("name", &self.name)
            .field("join_present", &self.join.is_some())
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
        let result = Arc::new(Mutex::new(None));
        let producer_failure = Arc::new(Mutex::new(None));
        let producer =
            ThreadResultProducer::new(Arc::clone(&result), Arc::clone(&producer_failure));
        let join = thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let outcome = match catch_unwind(AssertUnwindSafe(run)) {
                    Ok(result) => result,
                    Err(_) => Err(ThreadResultFailure::ThreadPanic.into_hal_error(name)),
                };
                producer.record_or_capture_failure(outcome, name);
            })
            .map_err(|error| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    format!("{name}: thread spawn failed: {error}"),
                )
            })?;
        Ok(Self {
            result,
            producer_failure,
            join: Some(join),
            name,
            join_failure: None,
            collected: false,
        })
    }

    pub(crate) fn collect_if_finished(&mut self) -> ThreadResultPoll<T> {
        if self.collected {
            return ThreadResultPoll::Completed(Err(
                ThreadResultFailure::ResultAlreadyCollected.into_hal_error(self.name)
            ));
        }
        if self
            .join
            .as_ref()
            .map(|handle| !handle.is_finished())
            .unwrap_or(false)
        {
            return ThreadResultPoll::Running;
        }
        self.join_if_finished();
        self.collected = true;
        if let Some(error) = self.join_failure.take() {
            return ThreadResultPoll::Completed(Err(error));
        }
        match self.producer_failure.lock() {
            Ok(mut guard) => {
                if let Some(error) = guard.take() {
                    return ThreadResultPoll::Completed(Err(error));
                }
            }
            Err(_) => {
                return ThreadResultPoll::Completed(Err(
                    ThreadResultFailure::ResultLockPoison.into_hal_error(self.name)
                ));
            }
        }
        match self.result.lock() {
            Ok(mut guard) => match guard.take() {
                Some(result) => ThreadResultPoll::Completed(result),
                None => ThreadResultPoll::Completed(Err(
                    ThreadResultFailure::MissingReport.into_hal_error(self.name)
                )),
            },
            Err(_) => ThreadResultPoll::Completed(Err(
                ThreadResultFailure::ResultLockPoison.into_hal_error(self.name)
            )),
        }
    }

    pub(crate) fn join_after_stop(mut self) -> Result<T, HalError> {
        if self.collected {
            return Err(ThreadResultFailure::ResultAlreadyCollected.into_hal_error(self.name));
        }
        self.collected = true;
        if let Some(handle) = self.join.take() {
            if handle.join().is_err() {
                return Err(ThreadResultFailure::JoinFailure.into_hal_error(self.name));
            }
        }
        match self.producer_failure.lock() {
            Ok(mut guard) => {
                if let Some(error) = guard.take() {
                    return Err(error);
                }
            }
            Err(_) => return Err(ThreadResultFailure::ResultLockPoison.into_hal_error(self.name)),
        }
        match self.result.lock() {
            Ok(mut guard) => match guard.take() {
                Some(result) => result,
                None => Err(ThreadResultFailure::MissingReport.into_hal_error(self.name)),
            },
            Err(_) => Err(ThreadResultFailure::ResultLockPoison.into_hal_error(self.name)),
        }
    }

    pub(crate) fn is_thread_finished(&self) -> bool {
        self.join
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(false)
    }

    fn join_if_finished(&mut self) {
        if self.is_thread_finished() {
            if let Some(handle) = self.join.take() {
                if handle.join().is_err() {
                    self.join_failure =
                        Some(ThreadResultFailure::JoinFailure.into_hal_error(self.name));
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        name: &'static str,
        result: Arc<Mutex<Option<Result<T, HalError>>>>,
        join: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            result,
            producer_failure: Arc::new(Mutex::new(None)),
            join,
            name,
            join_failure: None,
            collected: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn thread_result_owner_normal_completion() {
        let owner = ThreadResultOwner::start("normal", || Ok(7u32)).unwrap();
        assert_eq!(owner.join_after_stop().unwrap(), 7);
    }

    #[test]
    fn thread_result_owner_join_panic() {
        let owner = ThreadResultOwner::<u32>::start("panic", || panic!("boom")).unwrap();
        assert!(owner.join_after_stop().is_err());
    }

    #[test]
    fn thread_result_owner_collect_running() {
        let mut owner = ThreadResultOwner::start("running", || {
            std::thread::sleep(Duration::from_millis(50));
            Ok(())
        })
        .unwrap();
        assert!(matches!(
            owner.collect_if_finished(),
            ThreadResultPoll::Running
        ));
        let _ = owner.join_after_stop();
    }

    #[test]
    fn thread_result_owner_collect_completed() {
        let mut owner = ThreadResultOwner::start("completed", || Ok(())).unwrap();
        for _ in 0..100 {
            if matches!(
                owner.collect_if_finished(),
                ThreadResultPoll::Completed(Ok(()))
            ) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("thread did not complete");
    }

    #[test]
    fn thread_result_owner_second_collect_is_not_missing_report() {
        let mut owner = ThreadResultOwner::start("completed_once", || Ok(())).unwrap();
        for _ in 0..100 {
            if matches!(
                owner.collect_if_finished(),
                ThreadResultPoll::Completed(Ok(()))
            ) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        match owner.collect_if_finished() {
            ThreadResultPoll::Completed(Err(error)) => {
                assert!(format!("{error:?}").contains("already collected"));
            }
            _ => panic!("second collect must report already-collected failure"),
        }
    }

    #[test]
    fn thread_result_owner_missing_report_failure() {
        let result = Arc::new(Mutex::new(None));
        let join = thread::spawn(|| {});
        let owner = ThreadResultOwner::<u32> {
            result,
            producer_failure: Arc::new(Mutex::new(None)),
            join: Some(join),
            name: "missing",
            join_failure: None,
            collected: false,
        };
        assert!(owner.join_after_stop().is_err());
    }

    #[test]
    fn thread_result_owner_result_lock_poison() {
        let result = Arc::new(Mutex::new(None));
        let poisoned = Arc::clone(&result);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poisoned.lock().unwrap();
            panic!("poison");
        }));
        let join = thread::spawn(|| {});
        let owner = ThreadResultOwner::<u32> {
            result,
            producer_failure: Arc::new(Mutex::new(None)),
            join: Some(join),
            name: "poison",
            join_failure: None,
            collected: false,
        };
        assert!(owner.join_after_stop().is_err());
    }
}
