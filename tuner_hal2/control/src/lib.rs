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

/// Opaque physical result/join authority issued only by `WorkerRuntime`.
/// It owns no independent generation, retry policy, reaper registry, or domain state.
pub struct WorkerHandle<T, E> {
    result: std::sync::Arc<std::sync::Mutex<Option<Result<T, E>>>>,
    owner_failure: std::sync::Arc<std::sync::Mutex<Option<WorkerRuntimeOwnerFailure>>>,
    completion: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    join: Option<std::thread::JoinHandle<()>>,
    collected: bool,
}

impl<T, E> WorkerHandle<T, E> {
    fn start(
        name: String,
        run: impl FnOnce() -> Result<T, E> + Send + 'static,
    ) -> std::io::Result<Self>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkerTerminalResult<T> {
    Normal(T),
    StopRequested,
    RuntimeFailure(maleicacid_tuner_hal2_common::HalError),
    PanicOrJoinFailure,
}

/// Canonical generic worker lifecycle owner shared by device and service layers.
/// All thread creation and all subordinate reaper/supervisor handles originate here.
pub struct WorkerRuntime<T = ()> {
    owner_id: i64,
    generation: u64,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    stop_signalled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    wake_signalled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<WorkerHandle<WorkerTerminalResult<T>, ()>>,
}

impl<T> WorkerRuntime<T> {
    pub const fn owner_id(&self) -> i64 {
        self.owner_id
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn stop_signal(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        std::sync::Arc::clone(&self.stop)
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(std::sync::atomic::Ordering::Acquire)
            || self
                .handle
                .as_ref()
                .map(|handle| handle.is_thread_finished())
                .unwrap_or(true)
    }

    pub fn request_stop_and_wake(&self) {
        if self
            .stop_signalled
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
        {
            self.stop.store(true, std::sync::atomic::Ordering::Release);
        }
        if self
            .wake_signalled
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
        {
            if let Some(handle) = self.handle.as_ref() {
                handle.unpark();
            }
        }
    }

    pub fn join(mut self) -> WorkerTerminalResult<T> {
        let Some(handle) = self.handle.take() else {
            return WorkerTerminalResult::PanicOrJoinFailure;
        };
        match handle.join_after_stop() {
            Ok(Ok(result)) => result,
            Ok(Err(())) | Err(_) => WorkerTerminalResult::PanicOrJoinFailure,
        }
    }
}

impl<T> Drop for WorkerRuntime<T> {
    fn drop(&mut self) {
        if !self.is_finished() {
            self.request_stop_and_wake();
        }
    }
}

type WorkerReaperRunner<K, V, J> = dyn Fn(J, std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>)
    + Send
    + Sync
    + 'static;

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
        F: FnOnce(
                std::sync::Arc<std::sync::atomic::AtomicBool>,
            ) -> Result<T, maleicacid_tuner_hal2_common::HalError>
            + Send
            + 'static,
        C: FnOnce() + Send + 'static,
    {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = std::sync::Arc::clone(&stop);
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_finished = std::sync::Arc::clone(&finished);
        let handle = Self::spawn_handle(thread_name, move || {
            let terminal = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                worker(std::sync::Arc::clone(&thread_stop))
            })) {
                Ok(Ok(_result)) if thread_stop.load(std::sync::atomic::Ordering::Acquire) => {
                    WorkerTerminalResult::StopRequested
                }
                Ok(Ok(result)) => WorkerTerminalResult::Normal(result),
                Ok(Err(error)) => WorkerTerminalResult::RuntimeFailure(error),
                Err(_) => WorkerTerminalResult::PanicOrJoinFailure,
            };
            thread_finished.store(true, std::sync::atomic::Ordering::Release);
            completion_signal();
            Ok::<_, ()>(terminal)
        })?;
        Ok(WorkerRuntime {
            owner_id,
            generation,
            stop,
            stop_signalled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            wake_signalled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            finished,
            handle: Some(handle),
        })
    }

    pub fn spawn_handle<T, E>(
        name: String,
        run: impl FnOnce() -> Result<T, E> + Send + 'static,
    ) -> std::io::Result<WorkerHandle<T, E>>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        WorkerHandle::start(name, run)
    }

    pub fn start_reaper_queue<K, V, J>(
        capacity: usize,
        thread_prefix: &'static str,
        runner: std::sync::Arc<WorkerReaperRunner<K, V, J>>,
    ) -> Result<WorkerRuntimeReaperQueue<K, V, J>, maleicacid_tuner_hal2_common::HalError>
    where
        K: Ord + Clone + Send + 'static,
        V: Clone + Send + 'static,
        J: Send + 'static,
    {
        WorkerRuntimeReaperQueue::start(capacity, thread_prefix, runner)
    }

    pub fn supervisor<K, A, R>(
        capacity: usize,
        deadline: std::time::Duration,
    ) -> WorkerRuntimeSupervisor<K, A, R> {
        WorkerRuntimeSupervisor::new(capacity, deadline)
    }

    pub fn checked_next_generation(current: u64) -> Option<u64> {
        current.checked_add(1)
    }
}

/// Opaque bounded reaper handle issued by `WorkerRuntime`.
pub struct WorkerRuntimeReaperQueue<K, V, J> {
    sender: std::sync::mpsc::SyncSender<J>,
    pending: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,
}

impl<K, V, J> Clone for WorkerRuntimeReaperQueue<K, V, J> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            pending: std::sync::Arc::clone(&self.pending),
        }
    }
}

impl<K, V, J> WorkerRuntimeReaperQueue<K, V, J>
where
    K: Ord + Clone + Send + 'static,
    V: Clone + Send + 'static,
    J: Send + 'static,
{
    fn start(
        capacity: usize,
        thread_prefix: &'static str,
        runner: std::sync::Arc<WorkerReaperRunner<K, V, J>>,
    ) -> Result<Self, maleicacid_tuner_hal2_common::HalError> {
        let capacity = capacity.max(1);
        let (sender, receiver) = std::sync::mpsc::sync_channel(capacity);
        let pending = std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
        let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));
        for lane in 0..capacity {
            let receiver = std::sync::Arc::clone(&receiver);
            let runner = std::sync::Arc::clone(&runner);
            let pending_for_lane = std::sync::Arc::clone(&pending);
            std::thread::Builder::new()
                .name(format!("{thread_prefix}-{lane}"))
                .spawn(move || loop {
                    let job = match receiver.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => return,
                    };
                    match job {
                        Ok(job) => runner(job, std::sync::Arc::clone(&pending_for_lane)),
                        Err(_) => return,
                    }
                })
                .map_err(|error| {
                    maleicacid_tuner_hal2_common::HalError::internal(
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
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        let reservations: Vec<_> = reservations.into_iter().collect();
        let mut pending = match self.pending.lock() {
            Ok(pending) => pending,
            Err(_) => {
                core::mem::forget(job);
                return Err(maleicacid_tuner_hal2_common::HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "worker reaper pending registry lock poisoned",
                ));
            }
        };
        if reservations
            .iter()
            .any(|(key, _)| pending.contains_key(key))
        {
            core::mem::forget(job);
            return Err(maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "worker reaper received a duplicate endpoint lease",
            ));
        }
        for (key, value) in reservations {
            pending.insert(key, value);
        }
        drop(pending);
        self.sender.try_send(job).map_err(|error| match error {
            std::sync::mpsc::TrySendError::Full(job) => {
                core::mem::forget(job);
                maleicacid_tuner_hal2_common::HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "worker reaper capacity exhausted",
                )
            }
            std::sync::mpsc::TrySendError::Disconnected(job) => {
                core::mem::forget(job);
                maleicacid_tuner_hal2_common::HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "worker reaper is unavailable",
                )
            }
        })
    }

    pub fn pending_value(
        &self,
        key: &K,
    ) -> Result<Option<V>, maleicacid_tuner_hal2_common::HalError> {
        self.pending
            .lock()
            .map(|pending| pending.get(key).cloned())
            .map_err(|_| {
                maleicacid_tuner_hal2_common::HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "worker reaper pending registry lock poisoned",
                )
            })
    }
}

/// Opaque active/reaping registry handle issued by `WorkerRuntime`.
pub struct WorkerRuntimeSupervisor<K, A, R> {
    capacity: usize,
    deadline: std::time::Duration,
    state: std::sync::Mutex<WorkerRuntimeSupervisorMaps<K, A, R>>,
    wake: std::sync::Condvar,
}

pub struct WorkerRuntimeSupervisorMaps<K, A, R> {
    active: std::collections::BTreeMap<K, A>,
    reaping: std::collections::BTreeMap<K, R>,
}

impl<K, A, R> Default for WorkerRuntimeSupervisorMaps<K, A, R> {
    fn default() -> Self {
        Self {
            active: std::collections::BTreeMap::new(),
            reaping: std::collections::BTreeMap::new(),
        }
    }
}

impl<K: Ord, A, R> WorkerRuntimeSupervisorMaps<K, A, R> {
    pub fn active(&self) -> &std::collections::BTreeMap<K, A> {
        &self.active
    }
    pub fn reaping(&self) -> &std::collections::BTreeMap<K, R> {
        &self.reaping
    }
    pub fn active_mut(&mut self) -> &mut std::collections::BTreeMap<K, A> {
        &mut self.active
    }
    pub fn reaping_mut(&mut self) -> &mut std::collections::BTreeMap<K, R> {
        &mut self.reaping
    }
    pub fn total_len(&self) -> usize {
        self.active.len().saturating_add(self.reaping.len())
    }
}

impl<K, A, R> WorkerRuntimeSupervisor<K, A, R> {
    fn new(capacity: usize, deadline: std::time::Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            deadline,
            state: std::sync::Mutex::new(WorkerRuntimeSupervisorMaps::default()),
            wake: std::sync::Condvar::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn deadline(&self) -> std::time::Duration {
        self.deadline
    }
    pub fn state(&self) -> &std::sync::Mutex<WorkerRuntimeSupervisorMaps<K, A, R>> {
        &self.state
    }
    pub fn wake(&self) -> &std::sync::Condvar {
        &self.wake
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
