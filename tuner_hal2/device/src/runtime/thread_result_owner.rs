//! 共有worker threadの結果所有。
//!
//! このmoduleは低レベルの結果cell、完了通知、JoinHandle契約だけを所有する。
//! 取消signalとdomain固有の終了意味は呼び出し元が所有する。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThreadResultFailure {
    ThreadPanic,
    JoinFailure,
    ResultLockPoison,
    CompletionLockPoison,
    MissingReport,
    ResultAlreadyCollected,
}

impl ThreadResultFailure {
    fn into_hal_error(self, name: &'static str) -> HalError {
        let detail = match self {
            ThreadResultFailure::ThreadPanic => "thread panicked",
            ThreadResultFailure::JoinFailure => "thread join failed",
            ThreadResultFailure::ResultLockPoison => "thread result lock poisoned",
            ThreadResultFailure::CompletionLockPoison => {
                "thread completion lock poisoned"
            }
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
    completion: Arc<(Mutex<bool>, Condvar)>,
}

impl<T> ThreadResultProducer<T> {
    fn new(
        result: Arc<Mutex<Option<Result<T, HalError>>>>,
        producer_failure: Arc<Mutex<Option<HalError>>>,
        completion: Arc<(Mutex<bool>, Condvar)>,
    ) -> Self {
        Self {
            result,
            producer_failure,
            completion,
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
        let (completed, wake) = &*self.completion;
        match completed.lock() {
            Ok(mut completed) => {
                *completed = true;
                wake.notify_all();
            }
            Err(_) => {
                let error = ThreadResultFailure::CompletionLockPoison.into_hal_error(name);
                if let Ok(mut guard) = self.producer_failure.lock() {
                    *guard = Some(error);
                }
                wake.notify_all();
            }
        }
    }
}

pub(crate) struct ThreadResultOwner<T> {
    result: Arc<Mutex<Option<Result<T, HalError>>>>,
    producer_failure: Arc<Mutex<Option<HalError>>>,
    completion: Arc<(Mutex<bool>, Condvar)>,
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
        let completion = Arc::new((Mutex::new(false), Condvar::new()));
        let producer = ThreadResultProducer::new(
            Arc::clone(&result),
            Arc::clone(&producer_failure),
            Arc::clone(&completion),
        );
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
            completion,
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
        if let Some(error) = self.join_failure.take() {
            return Err(error);
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

    pub(crate) fn wait_until_finished(
        &self,
        deadline: Option<Instant>,
    ) -> Result<bool, HalError> {
        let (completed, wake) = &*self.completion;
        let mut completed = completed.lock().map_err(|_| {
            ThreadResultFailure::CompletionLockPoison.into_hal_error(self.name)
        })?;
        loop {
            if *completed {
                return Ok(true);
            }
            match deadline {
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Ok(false);
                    }
                    let (next, timeout) = wake
                        .wait_timeout(completed, deadline.saturating_duration_since(now))
                        .map_err(|_| {
                            ThreadResultFailure::CompletionLockPoison.into_hal_error(self.name)
                        })?;
                    completed = next;
                    if timeout.timed_out() && !*completed {
                        return Ok(false);
                    }
                }
                None => {
                    completed = wake.wait(completed).map_err(|_| {
                        ThreadResultFailure::CompletionLockPoison.into_hal_error(self.name)
                    })?;
                }
            }
        }
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
            completion: Arc::new((Mutex::new(true), Condvar::new())),
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
    fn thread_result_owner_recorded_join_failure_is_error() {
        let owner = ThreadResultOwner::<u32> {
            result: Arc::new(Mutex::new(Some(Ok(1)))),
            producer_failure: Arc::new(Mutex::new(None)),
            completion: Arc::new((Mutex::new(true), Condvar::new())),
            join: None,
            name: "recorded_join_failure",
            join_failure: Some(
                ThreadResultFailure::JoinFailure.into_hal_error("recorded_join_failure"),
            ),
            collected: false,
        };
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
            completion: Arc::new((Mutex::new(true), Condvar::new())),
            join: Some(join),
            name: "missing",
            join_failure: None,
            collected: false,
        };
        assert!(owner.join_after_stop().is_err());
    }

    #[test]
    fn thread_result_owner_producer_failure_is_reported() {
        let join = thread::spawn(|| {});
        let owner = ThreadResultOwner::<u32> {
            result: Arc::new(Mutex::new(None)),
            producer_failure: Arc::new(Mutex::new(Some(
                ThreadResultFailure::ResultLockPoison.into_hal_error("producer_failure"),
            ))),
            completion: Arc::new((Mutex::new(true), Condvar::new())),
            join: Some(join),
            name: "producer_failure",
            join_failure: None,
            collected: false,
        };
        assert!(owner.join_after_stop().is_err());
    }
}
