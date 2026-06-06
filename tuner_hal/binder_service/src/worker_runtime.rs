//! Tuner HAL 内部 worker 制御を集約する。
//!
//! r50dz19 では既存の worker signal 実装をこの module へ移し、
//! lock / wait 失敗を正常停止やtimeoutに丸めない方針を固定する。

use std::sync::{atomic::{AtomicBool, Ordering}, Condvar, LockResult, Mutex, MutexGuard, WaitTimeoutResult};
use std::time::Duration;


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerExit {
    Normal,
    StopRequested,
    RuntimeFailure,
    PanicOrJoinFailure,
}

impl WorkerExit {
    pub fn is_abnormal(self) -> bool {
        matches!(
            self,
            WorkerExit::RuntimeFailure | WorkerExit::PanicOrJoinFailure
        )
    }
}

pub trait IntoWorkerExit {
    fn into_worker_exit(self) -> WorkerExit;
}

impl IntoWorkerExit for () {
    fn into_worker_exit(self) -> WorkerExit {
        WorkerExit::Normal
    }
}

impl IntoWorkerExit for WorkerExit {
    fn into_worker_exit(self) -> WorkerExit {
        self
    }
}

type ThreadWorkerHandleRaw = std::thread::JoinHandle<WorkerExit>;

#[derive(Debug)]
pub struct RuntimeAtomicFlag {
    _inner: AtomicBool,
}

impl Clone for RuntimeAtomicFlag {
    fn clone(&self) -> Self { Self::new(self.load(Ordering::SeqCst)) }
}

impl Default for RuntimeAtomicFlag {
    fn default() -> Self { Self::new(false) }
}

impl RuntimeAtomicFlag {
    pub fn new(value: bool) -> Self { Self { _inner: AtomicBool::new(value) } }
    pub fn load(&self, order: Ordering) -> bool { self._inner.load(order) }
    pub fn store(&self, value: bool, order: Ordering) { self._inner.store(value, order) }
    pub fn swap(&self, value: bool, order: Ordering) -> bool { self._inner.swap(value, order) }
    #[cfg(test)]
    pub fn compare_exchange(&self, current: bool, new: bool, success: Ordering, failure: Ordering) -> Result<bool, bool> {
        self._inner.compare_exchange(current, new, success, failure)
    }
}

#[derive(Debug, Default)]
pub struct RuntimeWaitSignal {
    _inner: Condvar,
}

impl RuntimeWaitSignal {
    pub fn new() -> Self { Self { _inner: Condvar::new() } }
    #[cfg(test)]
    pub fn notify_all(&self) { self._inner.notify_all(); }
    pub fn wait_timeout<'a, T>(&self, guard: MutexGuard<'a, T>, dur: Duration) -> LockResult<(MutexGuard<'a, T>, WaitTimeoutResult)> {
        self._inner.wait_timeout(guard, dur)
    }
}


static WORKER_PANIC_COUNT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static WORKER_ERROR_COUNT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static WORKER_DIAGNOSTIC_COUNTER_SATURATED: AtomicBool = AtomicBool::new(false);

fn increment_worker_diagnostic_counter(
    counter: &std::sync::atomic::AtomicU64,
    name: &'static str,
) -> u64 {
    let mut current = counter.load(Ordering::SeqCst);
    loop {
        let Some(total) = current.checked_add(1) else {
            WORKER_DIAGNOSTIC_COUNTER_SATURATED.store(true, Ordering::SeqCst);
            eprintln!("maleicacid-tuner-hal-worker: diagnostic_counter_saturated name={name}");
            return u64::MAX;
        };
        match counter.compare_exchange(current, total, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return total,
            Err(next_current) => current = next_current,
        }
    }
}

fn spawn_worker_with_exit_hook<F, R, H>(
    name: &'static str,
    body: F,
    hook: H,
) -> std::io::Result<ThreadWorkerHandleRaw>
where
    F: FnOnce() -> R + Send + 'static,
    R: IntoWorkerExit + Send + 'static,
    H: FnOnce(WorkerExit) + Send + 'static,
{
    std::thread::Builder::new().name(name.to_string()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
        let exit = match result {
            Ok(value) => value.into_worker_exit(),
            Err(_) => {
                let total = increment_worker_diagnostic_counter(
                    &WORKER_PANIC_COUNT,
                    "worker_panic_count",
                );
                eprintln!(
                    "maleicacid-tuner-hal-worker: panic stop fail-closed: worker={} worker_panic_count={}",
                    name, total
                );
                WorkerExit::PanicOrJoinFailure
            }
        };
        if matches!(exit, WorkerExit::RuntimeFailure) {
            let total = increment_worker_diagnostic_counter(
                &WORKER_ERROR_COUNT,
                "worker_error_count",
            );
            eprintln!(
                "maleicacid-tuner-hal-worker: error stop fail-closed: worker={} worker_error_count={}",
                name, total
            );
        }
        hook(exit);
        exit
    })
}

fn join_worker_with_diagnostics(handle: ThreadWorkerHandleRaw, name: &'static str) -> WorkerExit {
    match handle.join() {
        Ok(exit) => {
            if exit.is_abnormal() {
                eprintln!(
                    "maleicacid-tuner-hal-worker: observed abnormal worker stop during join: worker={} exit={:?}",
                    name, exit
                );
            }
            exit
        }
        Err(_) => {
            let total = increment_worker_diagnostic_counter(
                &WORKER_PANIC_COUNT,
                "worker_panic_count",
            );
            eprintln!(
                "maleicacid-tuner-hal-worker: observed uncaught panic stop during join: worker={} worker_panic_count={}",
                name, total
            );
            WorkerExit::PanicOrJoinFailure
        }
    }
}

pub type ConcreteWorkerSignal = WorkerSignal<WorkerExit>;

pub trait WorkerSignalRuntimeExit {
    fn runtime_failure_exit() -> Self;
}

impl WorkerSignalRuntimeExit for WorkerExit {
    fn runtime_failure_exit() -> Self { WorkerExit::RuntimeFailure }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WorkerExitReason {
    NotStarted,
    Normal,
    StopRequested,
    RuntimeFailure,
    PanicOrJoinFailure,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WorkerJoinOutcome {
    Joined(WorkerExitReason),
    SkippedSelfJoin,
    JoinUnavailable,
}

#[derive(Debug)]
struct WorkerSignalState<E> {
    stop_requested: bool,
    work_generation: u64,
    active: bool,
    exit_reason: Option<E>,
}

impl<E> Default for WorkerSignalState<E> {
    fn default() -> Self {
        Self {
            stop_requested: false,
            work_generation: 0,
            active: false,
            exit_reason: None,
        }
    }
}

#[derive(Debug)]
pub struct WorkerSignal<E> {
    state: Mutex<WorkerSignalState<E>>,
    cv: Condvar,
    runtime_failure: AtomicBool,
}

impl<E: WorkerSignalRuntimeExit> WorkerSignal<E> {
    pub fn new(active: bool) -> Self {
        Self {
            state: Mutex::new(WorkerSignalState {
                active,
                ..WorkerSignalState::default()
            }),
            cv: Condvar::new(),
            runtime_failure: AtomicBool::new(false),
        }
    }

    fn mark_runtime_failure(&self, reason: &str) {
        self.runtime_failure.store(true, Ordering::SeqCst);
        eprintln!("maleicacid-tuner-hal-worker: worker signal failure: {reason}");
        self.cv.notify_all();
    }

    fn mark_runtime_failure_locked(state: &mut WorkerSignalState<E>, reason: &str) {
        state.stop_requested = true;
        state.active = false;
        if state.exit_reason.is_none() {
            state.exit_reason = Some(E::runtime_failure_exit());
        }
        eprintln!("maleicacid-tuner-hal-worker: worker signal failure: {reason}");
    }

    fn advance_work_generation_locked(state: &mut WorkerSignalState<E>, reason: &str) -> bool {
        match state.work_generation.checked_add(1) {
            Some(next_generation) => {
                state.work_generation = next_generation;
                true
            }
            None => {
                Self::mark_runtime_failure_locked(state, reason);
                false
            }
        }
    }

    #[cfg(test)]

    pub fn clear_for_start(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                state.stop_requested = false;
                state.active = true;
                state.exit_reason = None;
                if Self::advance_work_generation_locked(&mut state, "generation exhausted during start") {
                    self.runtime_failure.store(false, Ordering::SeqCst);
                } else {
                    self.runtime_failure.store(true, Ordering::SeqCst);
                }
                self.cv.notify_all();
            }
            Err(_) => self.mark_runtime_failure("poisoned during start"),
        }
    }

    pub fn request_stop(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                state.stop_requested = true;
                if !Self::advance_work_generation_locked(&mut state, "generation exhausted during stop request") {
                    self.runtime_failure.store(true, Ordering::SeqCst);
                }
                self.cv.notify_all();
            }
            Err(_) => self.mark_runtime_failure("poisoned during stop request"),
        }
    }

    pub fn notify_work(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                if !Self::advance_work_generation_locked(&mut state, "generation exhausted during work notification") {
                    self.runtime_failure.store(true, Ordering::SeqCst);
                }
                self.cv.notify_all();
            }
            Err(_) => self.mark_runtime_failure("poisoned during work notification"),
        }
    }

    pub fn is_runtime_failure(&self) -> bool {
        self.runtime_failure.load(Ordering::SeqCst)
    }

    pub fn is_stop_requested(&self) -> bool {
        if self.is_runtime_failure() {
            return true;
        }
        match self.state.lock() {
            Ok(state) => state.stop_requested,
            Err(_) => {
                self.mark_runtime_failure("poisoned during stop check");
                true
            }
        }
    }

    #[cfg(test)]

    pub fn wait_until_work_or_stop(&self, observed_generation: &mut u64, timeout: Duration) -> bool {
        if self.is_runtime_failure() {
            return true;
        }
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.mark_runtime_failure("poisoned before wait");
                return true;
            }
        };
        loop {
            if self.is_runtime_failure() || state.stop_requested {
                return true;
            }
            if state.work_generation != *observed_generation {
                *observed_generation = state.work_generation;
                return false;
            }
            match self.cv.wait_timeout(state, timeout) {
                Ok((next_state, wait_result)) => {
                    state = next_state;
                    if wait_result.timed_out() {
                        return false;
                    }
                }
                Err(_) => {
                    self.mark_runtime_failure("poisoned during wait");
                    return true;
                }
            }
        }
    }

    pub fn wait_timeout_or_stop(&self, timeout: Duration) -> bool {
        if self.is_runtime_failure() {
            return true;
        }
        let state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.mark_runtime_failure("poisoned before timed wait");
                return true;
            }
        };
        if state.stop_requested {
            return true;
        }
        match self.cv.wait_timeout(state, timeout) {
            Ok((next_state, _)) => next_state.stop_requested || self.is_runtime_failure(),
            Err(_) => {
                self.mark_runtime_failure("poisoned during timed wait");
                true
            }
        }
    }

    pub fn set_exit_reason(&self, exit: E) {
        match self.state.lock() {
            Ok(mut state) => {
                if self.is_runtime_failure() {
                    if state.exit_reason.is_none() {
                        state.exit_reason = Some(E::runtime_failure_exit());
                    }
                } else {
                    state.exit_reason = Some(exit);
                }
                state.active = false;
                self.cv.notify_all();
            }
            Err(_) => self.mark_runtime_failure("poisoned while recording exit reason"),
        }
    }
}

impl<E: WorkerSignalRuntimeExit> Default for WorkerSignal<E> {
    fn default() -> Self {
        Self::new(false)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WorkerRuntimeError { SpawnFailed, WakeFailed, JoinFailed, SelfJoin, ExitReasonRecordFailed }

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WorkerOwnerId(pub &'static str, pub i32);

pub struct WorkerHandle {
    _owner_id: WorkerOwnerId,
    name: &'static str,
    signal: std::sync::Arc<ConcreteWorkerSignal>,
    handle: Option<ThreadWorkerHandleRaw>,
    exit_reason: WorkerExitReason,
}

impl WorkerHandle {
    #[cfg(test)]
    fn from_raw(owner_id: WorkerOwnerId, name: &'static str, handle: ThreadWorkerHandleRaw) -> Self {
        Self {
            _owner_id: owner_id,
            name,
            signal: std::sync::Arc::new(ConcreteWorkerSignal::new(true)),
            handle: Some(handle),
            exit_reason: WorkerExitReason::NotStarted,
        }
    }

    #[cfg(test)]

    fn take_raw(mut self) -> Option<ThreadWorkerHandleRaw> { self.handle.take() }

    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().map(|handle| handle.is_finished()).unwrap_or(true)
    }

    fn request_stop(&self, _reason: WorkerExitReason) -> Result<(), WorkerRuntimeError> {
        self.signal.request_stop();
        if self.signal.is_runtime_failure() { Err(WorkerRuntimeError::WakeFailed) } else { Ok(()) }
    }
    fn wake(&self) -> Result<(), WorkerRuntimeError> {
        self.signal.notify_work();
        if self.signal.is_runtime_failure() { Err(WorkerRuntimeError::WakeFailed) } else { Ok(()) }
    }
    fn join_from_owner(&mut self) -> Result<WorkerJoinOutcome, WorkerRuntimeError> {
        let Some(handle) = self.handle.take() else { return Ok(WorkerJoinOutcome::JoinUnavailable); };
        let exit = join_worker_with_diagnostics(handle, self.name);
        let reason = WorkerExitReason::from(exit);
        self.signal.set_exit_reason(exit);
        self.exit_reason = reason;
        Ok(WorkerJoinOutcome::Joined(reason))
    }
    #[cfg(test)]
    pub fn exit_reason(&self) -> Option<WorkerExitReason> {
        if self.exit_reason == WorkerExitReason::NotStarted { None } else { Some(self.exit_reason) }
    }
    #[cfg(test)]
    pub fn owner_id(&self) -> &WorkerOwnerId { &self._owner_id }
}

#[derive(Debug, Default)]
struct WorkerRuntime;

impl WorkerRuntime {
    fn spawn_owned<F, R>(owner_id: WorkerOwnerId, name: &'static str, body: F) -> Result<WorkerHandle, WorkerRuntimeError>
    where F: FnOnce(std::sync::Arc<ConcreteWorkerSignal>) -> R + Send + 'static, R: IntoWorkerExit + Send + 'static {
        let signal = std::sync::Arc::new(ConcreteWorkerSignal::new(true));
        let thread_signal = std::sync::Arc::clone(&signal);
        let handle = spawn_worker_with_exit_hook(name, move || body(thread_signal), |_| {})
            .map_err(|_| WorkerRuntimeError::SpawnFailed)?;
        Ok(WorkerHandle { _owner_id: owner_id, name, signal, handle: Some(handle), exit_reason: WorkerExitReason::NotStarted })
    }

    fn spawn_owned_with_exit_hook<F, R, H>(
        owner_id: WorkerOwnerId,
        name: &'static str,
        body: F,
        hook: H,
    ) -> Result<WorkerHandle, WorkerRuntimeError>
    where
        F: FnOnce(std::sync::Arc<ConcreteWorkerSignal>) -> R + Send + 'static,
        R: IntoWorkerExit + Send + 'static,
        H: FnOnce(WorkerExit) + Send + 'static,
    {
        let signal = std::sync::Arc::new(ConcreteWorkerSignal::new(true));
        let signal_for_body = std::sync::Arc::clone(&signal);
        let signal_for_hook = std::sync::Arc::clone(&signal);
        let handle = spawn_worker_with_exit_hook(
            name,
            move || body(signal_for_body),
            move |exit| {
                signal_for_hook.set_exit_reason(exit);
                hook(exit);
            },
        )
        .map_err(|_| WorkerRuntimeError::SpawnFailed)?;
        Ok(WorkerHandle {
            _owner_id: owner_id,
            name,
            signal,
            handle: Some(handle),
            exit_reason: WorkerExitReason::NotStarted,
        })
    }

}



#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WorkerStopJoinError {
    StopRequestFailed(WorkerRuntimeError),
    WakeFailed(WorkerRuntimeError),
    JoinFailed(WorkerRuntimeError),
}

/// Worker の所有権遷移を API 個別実装から切り離すための共通取引。
///
/// この型は worker slot の mutex そのものは所有しない。slot の取得順序は
/// 呼び出し側の資源 lifetime と一緒に決める必要があるためである。
/// 代わりに、spawn / stop request / wake / join / abnormal exit 判定を一箇所に集約する。
pub struct WorkerLifecycleTxn;

impl WorkerLifecycleTxn {
    pub fn spawn_with_exit_hook<F, R, H>(
        owner_id: WorkerOwnerId,
        name: &'static str,
        body: F,
        hook: H,
    ) -> Result<WorkerHandle, WorkerRuntimeError>
    where
        F: FnOnce(std::sync::Arc<ConcreteWorkerSignal>) -> R + Send + 'static,
        R: IntoWorkerExit + Send + 'static,
        H: FnOnce(WorkerExit) + Send + 'static,
    {
        WorkerRuntime::spawn_owned_with_exit_hook(owner_id, name, body, hook)
    }

    pub fn spawn<F, R>(
        owner_id: WorkerOwnerId,
        name: &'static str,
        body: F,
    ) -> Result<WorkerHandle, WorkerRuntimeError>
    where
        F: FnOnce(std::sync::Arc<ConcreteWorkerSignal>) -> R + Send + 'static,
        R: IntoWorkerExit + Send + 'static,
    {
        WorkerRuntime::spawn_owned(owner_id, name, body)
    }

    pub fn wake(handle: &WorkerHandle) -> Result<(), WorkerRuntimeError> {
        handle.wake()
    }

    pub fn request_stop(handle: &WorkerHandle, reason: WorkerExitReason) -> Result<(), WorkerRuntimeError> {
        handle.request_stop(reason)
    }

    pub fn join(handle: WorkerHandle) -> Result<WorkerJoinOutcome, WorkerRuntimeError> {
        let mut handle = handle;
        handle.join_from_owner()
    }

    pub fn join_mut(handle: &mut WorkerHandle) -> Result<WorkerJoinOutcome, WorkerRuntimeError> {
        handle.join_from_owner()
    }

    pub fn request_stop_and_join(
        mut handle: WorkerHandle,
        reason: WorkerExitReason,
    ) -> Result<WorkerJoinOutcome, WorkerStopJoinError> {
        handle
            .request_stop(reason)
            .map_err(WorkerStopJoinError::StopRequestFailed)?;
        handle
            .join_from_owner()
            .map_err(WorkerStopJoinError::JoinFailed)
    }

    pub fn request_stop_wake_join_mut(
        handle: &mut WorkerHandle,
        reason: WorkerExitReason,
    ) -> Result<WorkerJoinOutcome, WorkerStopJoinError> {
        handle
            .request_stop(reason)
            .map_err(WorkerStopJoinError::StopRequestFailed)?;
        handle
            .wake()
            .map_err(WorkerStopJoinError::WakeFailed)?;
        handle
            .join_from_owner()
            .map_err(WorkerStopJoinError::JoinFailed)
    }



    pub fn request_stop_join_slot(
        slot: &mut Option<WorkerHandle>,
        reason: WorkerExitReason,
    ) -> Result<Option<WorkerJoinOutcome>, WorkerStopJoinError> {
        let Some(worker) = slot.as_mut() else {
            return Ok(None);
        };
        worker
            .request_stop(reason)
            .map_err(WorkerStopJoinError::StopRequestFailed)?;
        let mut handle = slot.take().expect("worker slot checked as Some");
        handle
            .join_from_owner()
            .map(Some)
            .map_err(WorkerStopJoinError::JoinFailed)
    }

    pub fn request_stop_wake_join_slot(
        slot: &mut Option<WorkerHandle>,
        reason: WorkerExitReason,
    ) -> Result<Option<WorkerJoinOutcome>, WorkerStopJoinError> {
        let Some(worker) = slot.as_mut() else {
            return Ok(None);
        };
        worker
            .request_stop(reason)
            .map_err(WorkerStopJoinError::StopRequestFailed)?;
        worker
            .wake()
            .map_err(WorkerStopJoinError::WakeFailed)?;
        let mut handle = slot.take().expect("worker slot checked as Some");
        handle
            .join_from_owner()
            .map(Some)
            .map_err(WorkerStopJoinError::JoinFailed)
    }

    pub fn exit_from_join_outcome(outcome: WorkerJoinOutcome) -> WorkerExit {
        match outcome {
            WorkerJoinOutcome::Joined(WorkerExitReason::Normal) => WorkerExit::Normal,
            WorkerJoinOutcome::Joined(WorkerExitReason::StopRequested) => WorkerExit::StopRequested,
            WorkerJoinOutcome::Joined(WorkerExitReason::RuntimeFailure) => WorkerExit::RuntimeFailure,
            WorkerJoinOutcome::Joined(WorkerExitReason::PanicOrJoinFailure) => WorkerExit::PanicOrJoinFailure,
            WorkerJoinOutcome::Joined(WorkerExitReason::NotStarted) => WorkerExit::RuntimeFailure,
            WorkerJoinOutcome::SkippedSelfJoin => WorkerExit::RuntimeFailure,
            WorkerJoinOutcome::JoinUnavailable => WorkerExit::Normal,
        }
    }

    pub fn join_exit(handle: WorkerHandle, _name: &'static str) -> WorkerExit {
        match Self::join(handle) {
            Ok(outcome) => Self::exit_from_join_outcome(outcome),
            Err(_) => WorkerExit::RuntimeFailure,
        }
    }

    pub fn abnormal(outcome: WorkerJoinOutcome) -> bool {
        Self::exit_from_join_outcome(outcome).is_abnormal()
    }
}

impl From<WorkerExit> for WorkerExitReason {
    fn from(exit: WorkerExit) -> Self {
        match exit { WorkerExit::Normal => WorkerExitReason::Normal, WorkerExit::StopRequested => WorkerExitReason::StopRequested, WorkerExit::RuntimeFailure => WorkerExitReason::RuntimeFailure, WorkerExit::PanicOrJoinFailure => WorkerExitReason::PanicOrJoinFailure }
    }
}

impl Default for WorkerExitReason {
    fn default() -> Self {
        WorkerExitReason::NotStarted
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn worker_lifecycle_txn_stop_wake_join_mut_returns_stop_requested() {
        let mut handle = WorkerLifecycleTxn::spawn_with_exit_hook(
            WorkerOwnerId("test", 8),
            "worker_lifecycle_txn_stop_wake_join_mut_returns_stop_requested",
            |signal| {
                let mut observed_generation = 0_u64;
                let stopped = signal.wait_until_work_or_stop(
                    &mut observed_generation,
                    Duration::from_millis(100),
                );
                if stopped { WorkerExit::StopRequested } else { WorkerExit::RuntimeFailure }
            },
            |_| {},
        ).expect("worker spawn");
        let outcome = WorkerLifecycleTxn::request_stop_wake_join_mut(
            &mut handle,
            WorkerExitReason::StopRequested,
        ).expect("worker lifecycle txn stop+wake+join");
        assert_eq!(outcome, WorkerJoinOutcome::Joined(WorkerExitReason::StopRequested));
    }



    #[test]
    fn worker_lifecycle_txn_slot_helper_clears_joined_worker() {
        let handle = WorkerLifecycleTxn::spawn_with_exit_hook(
            WorkerOwnerId("test", 9),
            "worker_lifecycle_txn_slot_helper_clears_joined_worker",
            |signal| {
                let mut observed_generation = 0_u64;
                let stopped = signal.wait_until_work_or_stop(
                    &mut observed_generation,
                    Duration::from_millis(100),
                );
                if stopped { WorkerExit::StopRequested } else { WorkerExit::RuntimeFailure }
            },
            |_| {},
        ).expect("worker spawn");
        let mut slot = Some(handle);
        let outcome = WorkerLifecycleTxn::request_stop_wake_join_slot(
            &mut slot,
            WorkerExitReason::StopRequested,
        ).expect("worker lifecycle txn slot stop+wake+join");
        assert_eq!(outcome, Some(WorkerJoinOutcome::Joined(WorkerExitReason::StopRequested)));
        assert!(slot.is_none());
    }

    #[test]
    fn owned_worker_body_observes_owner_stop_signal() {
        let handle = WorkerLifecycleTxn::spawn_with_exit_hook(
            WorkerOwnerId("test", 7),
            "owned_worker_body_observes_owner_stop_signal",
            |signal| {
                assert!(!signal.is_stop_requested());
                signal.request_stop();
                if signal.is_stop_requested() { WorkerExit::StopRequested } else { WorkerExit::RuntimeFailure }
            },
            |_| {},
        ).expect("worker spawn");
        let outcome = WorkerLifecycleTxn::join(handle).expect("worker join");
        assert_eq!(outcome, WorkerJoinOutcome::Joined(WorkerExitReason::StopRequested));
    }
}
