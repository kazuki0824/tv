use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use maleicacid_tuner_hal2_common::HalError;

pub const CLEANUP_RETRY_SCHEDULE_MS: &[u64] = &[0, 10, 100, 1_000];
pub const CLEANUP_TERMINAL_DEADLINE_MS: u64 = 30_000;
pub const WORKER_IO_DEADLINE_MS: u64 = 2_000;
pub const WORKER_REAPER_DEADLINE_MS: u64 = 10_000;


/// Canonical generic bounded reaper queue owned by the WorkerRuntime subsystem.
/// Domain code supplies only typed jobs and completion semantics; it does not own
/// the channel, pending-key registry, or worker-lane lifecycle.
pub struct WorkerRuntimeReaperQueue<K, V, J> {
    sender: SyncSender<J>,
    pending: Arc<Mutex<BTreeMap<K, V>>>,
}

impl<K, V, J> Clone for WorkerRuntimeReaperQueue<K, V, J> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            pending: Arc::clone(&self.pending),
        }
    }
}

impl<K, V, J> WorkerRuntimeReaperQueue<K, V, J>
where
    K: Ord + Clone + Send + 'static,
    V: Clone + Send + 'static,
    J: Send + 'static,
{
    pub fn start(
        capacity: usize,
        thread_prefix: &'static str,
        runner: Arc<dyn Fn(J, Arc<Mutex<BTreeMap<K, V>>>) + Send + Sync + 'static>,
    ) -> Result<Self, HalError> {
        let capacity = capacity.max(1);
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let receiver = Arc::new(Mutex::new(receiver));
        for lane in 0..capacity {
            let receiver = Arc::clone(&receiver);
            let runner = Arc::clone(&runner);
            let pending_for_lane = Arc::clone(&pending);
            thread::Builder::new()
                .name(format!("{thread_prefix}-{lane}"))
                .spawn(move || loop {
                    let job = match receiver.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => return,
                    };
                    match job {
                        Ok(job) => runner(job, Arc::clone(&pending_for_lane)),
                        Err(_) => return,
                    }
                })
                .map_err(|error| {
                    HalError::internal(
                        maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                        format!("worker reaper lane spawn failed: {error}"),
                    )
                })?;
        }
        Ok(Self { sender, pending })
    }

    pub fn enqueue_reserved(
        &self,
        job: J,
        reservations: impl IntoIterator<Item = (K, V)>,
    ) -> Result<(), HalError> {
        let reservations: Vec<_> = reservations.into_iter().collect();
        let mut pending = match self.pending.lock() {
            Ok(pending) => pending,
            Err(_) => {
                core::mem::forget(job);
                return Err(HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "worker reaper pending registry lock poisoned",
                ));
            }
        };
        if reservations.iter().any(|(key, _)| pending.contains_key(key)) {
            core::mem::forget(job);
            return Err(HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "worker reaper received a duplicate endpoint lease",
            ));
        }
        for (key, value) in reservations {
            pending.insert(key, value);
        }
        drop(pending);
        self.sender.try_send(job).map_err(|error| match error {
            TrySendError::Full(job) => {
                core::mem::forget(job);
                HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "worker reaper capacity exhausted",
                )
            }
            TrySendError::Disconnected(job) => {
                core::mem::forget(job);
                HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "worker reaper is unavailable",
                )
            }
        })
    }

    pub fn pending_value(&self, key: &K) -> Result<Option<V>, HalError> {
        self.pending
            .lock()
            .map(|pending| pending.get(key).cloned())
            .map_err(|_| {
                HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "worker reaper pending registry lock poisoned",
                )
            })
    }
}

/// Canonical generic active/reaping registry, capacity, deadline and wake state
/// for supervisor-style workers. Domain modules retain only typed job semantics.
pub struct WorkerRuntimeSupervisor<K, A, R> {
    capacity: usize,
    deadline: Duration,
    state: Mutex<WorkerRuntimeSupervisorMaps<K, A, R>>,
    wake: Condvar,
}

pub struct WorkerRuntimeSupervisorMaps<K, A, R> {
    active: BTreeMap<K, A>,
    reaping: BTreeMap<K, R>,
}

impl<K, A, R> Default for WorkerRuntimeSupervisorMaps<K, A, R> {
    fn default() -> Self {
        Self {
            active: BTreeMap::new(),
            reaping: BTreeMap::new(),
        }
    }
}

impl<K: Ord, A, R> WorkerRuntimeSupervisorMaps<K, A, R> {
    pub fn active_mut(&mut self) -> &mut BTreeMap<K, A> { &mut self.active }
    pub fn reaping_mut(&mut self) -> &mut BTreeMap<K, R> { &mut self.reaping }
    pub fn total_len(&self) -> usize { self.active.len().saturating_add(self.reaping.len()) }
}

impl<K, A, R> WorkerRuntimeSupervisor<K, A, R> {
    pub fn new(capacity: usize, deadline: Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            deadline,
            state: Mutex::new(WorkerRuntimeSupervisorMaps::default()),
            wake: Condvar::new(),
        }
    }

    pub fn capacity(&self) -> usize { self.capacity }
    pub fn deadline(&self) -> Duration { self.deadline }
    pub fn state(&self) -> &Mutex<WorkerRuntimeSupervisorMaps<K, A, R>> { &self.state }
    pub fn wake(&self) -> &Condvar { &self.wake }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkerTerminalResult<T> {
    Normal(T),
    StopRequested,
    RuntimeFailure(HalError),
    PanicOrJoinFailure,
}

/// Canonical owner for one generic worker lifecycle.
pub struct WorkerRuntime<T = ()> {
    owner_id: i64,
    generation: u64,
    stop: Arc<AtomicBool>,
    stop_signalled: Arc<AtomicBool>,
    wake_signalled: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    handle: WorkerHandle<T>,
}

impl<T> WorkerRuntime<T> {
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
            if let Some(join) = self.handle.join.as_ref() {
                join.thread().unpark();
            }
        }
    }

    pub(crate) fn join(mut self) -> WorkerTerminalResult<T> {
        let Some(join) = self.handle.join.take() else {
            return WorkerTerminalResult::PanicOrJoinFailure;
        };
        match join.join() {
            Ok(result) => result,
            Err(_) => WorkerTerminalResult::PanicOrJoinFailure,
        }
    }
}

impl<T> Drop for WorkerRuntime<T> {
    fn drop(&mut self) {
        if self.handle.join.is_some() && !self.is_finished() {
            self.request_stop_and_wake();
        }
    }
}

/// Physical join element subordinate to its `WorkerRuntime` owner.
pub struct WorkerHandle<T> {
    join: Option<JoinHandle<WorkerTerminalResult<T>>>,
}

impl WorkerRuntime<()> {
    pub fn spawn<T, F, C>(
        thread_name: String,
        owner_id: i64,
        generation: u64,
        worker: F,
        completion_signal: C,
    ) -> std::io::Result<WorkerRuntime<T>>
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
        Ok(WorkerRuntime {
            owner_id,
            generation,
            stop,
            stop_signalled: Arc::new(AtomicBool::new(false)),
            wake_signalled: Arc::new(AtomicBool::new(false)),
            finished,
            handle: WorkerHandle { join: Some(join) },
        })
    }

    pub fn checked_next_generation(current: u64) -> Option<u64> {
        current.checked_add(1)
    }

    pub fn join_classified(self) -> crate::worker_failure_classifier::ClassifiedWorkerTerminalResult<T> {
        crate::worker_failure_classifier::WorkerFailureClassifier::classify_terminal(
            self.join(),
            "worker panicked or could not be joined",
        )
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
