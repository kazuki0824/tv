use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use maleicacid_tuner_hal2_common::HalError;

pub const CLEANUP_RETRY_SCHEDULE_MS: &[u64] = &[0, 10, 100, 1_000];
pub const CLEANUP_TERMINAL_DEADLINE_MS: u64 = 30_000;
pub const WORKER_IO_DEADLINE_MS: u64 = 2_000;
pub const WORKER_REAPER_DEADLINE_MS: u64 = 10_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkerTerminalResult<T> {
    Normal(T),
    StopRequested,
    RuntimeFailure(HalError),
    PanicOrJoinFailure,
}

pub struct WorkerHandle<T> {
    owner_id: i64,
    generation: u64,
    stop: Arc<AtomicBool>,
    stop_signalled: Arc<AtomicBool>,
    wake_signalled: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    join: Option<JoinHandle<WorkerTerminalResult<T>>>,
}

impl<T> WorkerHandle<T> {
    pub const fn owner_id(&self) -> i64 {
        self.owner_id
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn stop_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    pub fn request_stop_and_wake(&self) {
        if self
            .stop_signalled
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.stop.store(true, Ordering::Release);
        }
        if self
            .wake_signalled
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            if let Some(join) = self.join.as_ref() {
                join.thread().unpark();
            }
        }
    }

    pub fn join(mut self) -> WorkerTerminalResult<T> {
        let Some(join) = self.join.take() else {
            return WorkerTerminalResult::PanicOrJoinFailure;
        };
        match join.join() {
            Ok(result) => result,
            Err(_) => WorkerTerminalResult::PanicOrJoinFailure,
        }
    }
}

impl<T> Drop for WorkerHandle<T> {
    fn drop(&mut self) {
        if self.join.is_some() && !self.is_finished() {
            self.request_stop_and_wake();
        }
    }
}

pub struct WorkerRuntime;

impl WorkerRuntime {
    pub fn spawn<T, F, C>(
        thread_name: String,
        owner_id: i64,
        generation: u64,
        worker: F,
        completion_signal: C,
    ) -> std::io::Result<WorkerHandle<T>>
    where
        T: Send + 'static,
        F: FnOnce(Arc<AtomicBool>) -> Result<T, HalError> + Send + 'static,
        C: FnOnce() + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let finished = Arc::new(AtomicBool::new(false));
        let thread_finished = Arc::clone(&finished);
        let join = thread::Builder::new().name(thread_name).spawn(move || {
            let terminal = match catch_unwind(AssertUnwindSafe(|| worker(Arc::clone(&thread_stop)))) {
                Ok(Ok(_result)) if thread_stop.load(Ordering::Acquire) => {
                    WorkerTerminalResult::StopRequested
                }
                Ok(Ok(result)) => WorkerTerminalResult::Normal(result),
                Ok(Err(error)) => WorkerTerminalResult::RuntimeFailure(error),
                Err(_) => WorkerTerminalResult::PanicOrJoinFailure,
            };
            thread_finished.store(true, Ordering::Release);
            completion_signal();
            terminal
        })?;
        Ok(WorkerHandle {
            owner_id,
            generation,
            stop,
            stop_signalled: Arc::new(AtomicBool::new(false)),
            wake_signalled: Arc::new(AtomicBool::new(false)),
            finished,
            join: Some(join),
        })
    }

    pub fn checked_next_generation(current: u64) -> Option<u64> {
        current.checked_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_common::{HalInternalKind, HalError};

    #[test]
    fn worker_runtime_reports_normal_runtime_failure_and_panic() {
        let normal = WorkerRuntime::spawn(
            "worker-normal".to_owned(),
            1,
            1,
            |_| Ok(7_u8),
            || {},
        )
        .unwrap();
        assert_eq!(normal.join(), WorkerTerminalResult::Normal(7));

        let failure = WorkerRuntime::spawn(
            "worker-failure".to_owned(),
            1,
            2,
            |_| {
                Err::<u8, _>(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "worker failed",
                ))
            },
            || {},
        )
        .unwrap();
        assert!(matches!(
            failure.join(),
            WorkerTerminalResult::RuntimeFailure(_)
        ));

        let panic = WorkerRuntime::spawn(
            "worker-panic".to_owned(),
            1,
            3,
            |_| -> Result<(), HalError> { panic!("worker panic") },
            || {},
        )
        .unwrap();
        assert_eq!(panic.join(), WorkerTerminalResult::PanicOrJoinFailure);
    }

    #[test]
    fn worker_runtime_reports_stop_requested() {
        let worker = WorkerRuntime::spawn(
            "worker-stop".to_owned(),
            2,
            1,
            |stop| {
                while !stop.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                Ok(())
            },
            || {},
        )
        .unwrap();
        worker.request_stop_and_wake();
        assert_eq!(worker.join(), WorkerTerminalResult::StopRequested);
    }

    #[test]
    fn generation_never_wraps() {
        assert_eq!(WorkerRuntime::checked_next_generation(7), Some(8));
        assert_eq!(WorkerRuntime::checked_next_generation(u64::MAX), None);
    }
}
