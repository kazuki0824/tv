use maleicacid_tuner_hal2_common::WorkerCleanupFailureKind;
use maleicacid_tuner_hal2_common::{PoisonTrackedMutex, RuntimeLockKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerRuntimeOwnerFailure {
    ThreadPanic,
    JoinFailure,
    ResultLockPoison,
    CompletionLockPoison,
    MissingReport,
    ResultAlreadyCollected,
}

impl WorkerRuntimeOwnerFailure {
    pub fn into_terminal_result<T>(self, owner: &'static str) -> WorkerTerminalResult<T> {
        use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, WorkerLockKind};
        let error = match self {
            Self::ThreadPanic | Self::JoinFailure => {
                return WorkerTerminalResult::PanicOrJoinFailure;
            }
            Self::ResultLockPoison => HalError::WorkerLockPoisoned {
                owner,
                lock: WorkerLockKind::Result,
            },
            Self::CompletionLockPoison => HalError::WorkerLockPoisoned {
                owner,
                lock: WorkerLockKind::Completion,
            },
            Self::MissingReport | Self::ResultAlreadyCollected => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{owner}: {self:?}"),
            ),
        };
        WorkerTerminalResult::RuntimeFailure(error)
    }
}

pub enum WorkerRuntimePoll<T, E> {
    Running,
    Completed(Result<T, E>),
    OwnerFailure(WorkerRuntimeOwnerFailure),
}

/// 未完後片付け義務はこの従属保管値に残し、実行権限へ移動しない。
#[derive(Debug)]
pub struct WorkerRuntimeCleanup<T> {
    state: std::sync::Arc<std::sync::Mutex<WorkerCleanupState<T>>>,
}

#[derive(Debug)]
struct WorkerCleanupState<T> {
    attempt: u64,
    phase: WorkerCleanupPhase<T>,
}

#[derive(Debug)]
enum WorkerCleanupPhase<T> {
    Ready(T),
    Executing,
    Quarantined(T),
    Completed,
}

enum WorkerCleanupDisposition {
    Pending,
    Quarantined,
    Completed,
}

#[derive(Debug)]
#[must_use = "後片付け権限は実行するか、管理主体から再発行する必要があります"]
pub struct WorkerCleanupAuthority<T> {
    state: std::sync::Arc<std::sync::Mutex<WorkerCleanupState<T>>>,
    attempt: u64,
}

pub enum WorkerCleanupProgress<R> {
    Pending,
    Quarantined(R),
    Completed(R),
}

pub enum WorkerCleanupRun<T, R> {
    Pending(WorkerCleanupAuthority<T>),
    Completed(R),
}

fn cleanup_authority_error(
    kind: WorkerCleanupFailureKind,
) -> maleicacid_tuner_hal2_common::HalError {
    maleicacid_tuner_hal2_common::HalError::WorkerCleanupFailed { kind }
}

fn cleanup_lock_error<T>(
    error: std::sync::TryLockError<T>,
) -> maleicacid_tuner_hal2_common::HalError {
    cleanup_authority_error(match error {
        std::sync::TryLockError::WouldBlock => WorkerCleanupFailureKind::Executing,
        std::sync::TryLockError::Poisoned(_) => WorkerCleanupFailureKind::StatePoisoned,
    })
}

impl<T> WorkerCleanupState<T> {
    fn ensure_ready(&self) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        match &self.phase {
            WorkerCleanupPhase::Ready(_) => Ok(()),
            WorkerCleanupPhase::Executing => {
                Err(cleanup_authority_error(WorkerCleanupFailureKind::Executing))
            }
            WorkerCleanupPhase::Quarantined(_) => Err(cleanup_authority_error(
                WorkerCleanupFailureKind::Quarantined,
            )),
            WorkerCleanupPhase::Completed => {
                Err(cleanup_authority_error(WorkerCleanupFailureKind::Completed))
            }
        }
    }
}

impl<T> WorkerRuntimeCleanup<T> {
    pub fn is_pending(&self) -> bool {
        match self.state.try_lock() {
            Ok(state) => !matches!(state.phase, WorkerCleanupPhase::Completed),
            Err(_) => true,
        }
    }

    pub fn issue(
        &self,
    ) -> Result<WorkerCleanupAuthority<T>, maleicacid_tuner_hal2_common::HalError> {
        let mut state = self.state.try_lock().map_err(cleanup_lock_error)?;
        state.ensure_ready()?;
        state.attempt = state
            .attempt
            .checked_add(1)
            .ok_or_else(|| cleanup_authority_error(WorkerCleanupFailureKind::AttemptExhausted))?;
        Ok(WorkerCleanupAuthority {
            state: std::sync::Arc::clone(&self.state),
            attempt: state.attempt,
        })
    }
}

impl<T> WorkerCleanupAuthority<T> {
    fn with_value<R>(
        &self,
        execute: impl FnOnce(&mut T) -> (R, WorkerCleanupDisposition),
    ) -> Result<R, maleicacid_tuner_hal2_common::HalError> {
        let mut value = {
            let mut state = self.state.try_lock().map_err(cleanup_lock_error)?;
            if state.attempt != self.attempt {
                return Err(cleanup_authority_error(
                    WorkerCleanupFailureKind::Superseded,
                ));
            }
            state.ensure_ready()?;
            match std::mem::replace(&mut state.phase, WorkerCleanupPhase::Executing) {
                WorkerCleanupPhase::Ready(value) => value,
                phase => {
                    state.phase = phase;
                    return Err(cleanup_authority_error(WorkerCleanupFailureKind::Executing));
                }
            }
        };
        // 実行中の義務と試行番号は正本に残す。外部処理・待機中はロックを保持しない。
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(&mut value)));
        let mut state = self
            .state
            .lock()
            .map_err(|_| cleanup_authority_error(WorkerCleanupFailureKind::StatePoisoned))?;
        if state.attempt != self.attempt || !matches!(state.phase, WorkerCleanupPhase::Executing) {
            return Err(cleanup_authority_error(
                WorkerCleanupFailureKind::Superseded,
            ));
        }
        match outcome {
            Ok((result, disposition)) => {
                state.phase = match disposition {
                    WorkerCleanupDisposition::Pending => WorkerCleanupPhase::Ready(value),
                    WorkerCleanupDisposition::Quarantined => WorkerCleanupPhase::Quarantined(value),
                    WorkerCleanupDisposition::Completed => WorkerCleanupPhase::Completed,
                };
                drop(state);
                Ok(result)
            }
            Err(payload) => {
                state.phase = WorkerCleanupPhase::Quarantined(value);
                drop(state);
                drop(payload);
                Err(cleanup_authority_error(
                    WorkerCleanupFailureKind::Interrupted,
                ))
            }
        }
    }

    pub fn inspect<R>(
        &self,
        inspect: impl FnOnce(&T) -> R,
    ) -> Result<R, maleicacid_tuner_hal2_common::HalError> {
        self.with_value(|value| (inspect(value), WorkerCleanupDisposition::Pending))
    }

    pub fn execute<R>(
        self,
        execute: impl FnOnce(&mut T) -> WorkerCleanupProgress<R>,
    ) -> Result<WorkerCleanupRun<T, R>, maleicacid_tuner_hal2_common::HalError> {
        let progress = self.with_value(|value| {
            let progress = execute(value);
            let disposition = match &progress {
                WorkerCleanupProgress::Pending => WorkerCleanupDisposition::Pending,
                WorkerCleanupProgress::Quarantined(_) => WorkerCleanupDisposition::Quarantined,
                WorkerCleanupProgress::Completed(_) => WorkerCleanupDisposition::Completed,
            };
            (progress, disposition)
        })?;
        match progress {
            WorkerCleanupProgress::Completed(result)
            | WorkerCleanupProgress::Quarantined(result) => Ok(WorkerCleanupRun::Completed(result)),
            WorkerCleanupProgress::Pending => Ok(WorkerCleanupRun::Pending(self)),
        }
    }
}

/// `WorkerRuntime`だけが発行するopaqueな物理result/join権限。
/// 独自generation、retry policy、reaper registry、domain stateは所有しない。
pub struct WorkerHandle<T, E> {
    result: std::sync::Arc<std::sync::Mutex<Option<Result<T, E>>>>,
    owner_failure: std::sync::Arc<std::sync::Mutex<Option<WorkerRuntimeOwnerFailure>>>,
    completion: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    join: Option<std::thread::JoinHandle<()>>,
    collected: bool,
    context: WorkerContext,
}

impl<T, E> WorkerHandle<T, E> {
    fn start(
        name: String,
        run: impl FnOnce(WorkerContext) -> Result<T, E> + Send + 'static,
    ) -> std::io::Result<Self>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        Self::start_with_context(name, run, WorkerContext::new())
    }

    fn start_with_context(
        name: String,
        run: impl FnOnce(WorkerContext) -> Result<T, E> + Send + 'static,
        context: WorkerContext,
    ) -> std::io::Result<Self>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        let thread_context = context.clone();
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let owner_failure = std::sync::Arc::new(std::sync::Mutex::new(None));
        let completion =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let result_for_thread = std::sync::Arc::clone(&result);
        let failure_for_thread = std::sync::Arc::clone(&owner_failure);
        let completion_for_thread = std::sync::Arc::clone(&completion);
        let join = std::thread::Builder::new().name(name).spawn(move || {
            thread_context.wake.bind_current_thread();
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(thread_context)));
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
            context,
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

    pub fn request_stop(&self) {
        if !self
            .context
            .stop
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            self.context.wake.notify();
        }
    }

    pub fn wake(&self) {
        self.context.wake.notify();
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

impl<T, E> Drop for WorkerHandle<T, E> {
    fn drop(&mut self) {
        if self
            .join
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
        {
            self.request_stop();
        }
    }
}

/// 起床要求はatomic値、待機対象は当該workerのthreadに限定する。
#[derive(Clone, Debug)]
struct WorkerWake {
    pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: std::sync::Arc<std::sync::OnceLock<std::thread::Thread>>,
}

impl WorkerWake {
    fn bind_current_thread(&self) {
        self.thread.get_or_init(std::thread::current);
    }

    fn notify(&self) {
        self.pending
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(thread) = self.thread.get() {
            thread.unpark();
        }
    }

    fn wait_until(&self, deadline: Option<std::time::Instant>) {
        while !self
            .pending
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            match deadline {
                Some(deadline) => {
                    let Some(remaining) =
                        deadline.checked_duration_since(std::time::Instant::now())
                    else {
                        return;
                    };
                    std::thread::park_timeout(remaining);
                }
                None => std::thread::park(),
            }
        }
    }
}

/// workerだけへ渡す停止観測・待機権限。停止状態は正規ownerだけが変更する。
#[derive(Clone, Debug)]
pub struct WorkerContext {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    wake: WorkerWake,
}

impl WorkerContext {
    fn new() -> Self {
        Self {
            stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            wake: WorkerWake {
                pending: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                thread: std::sync::Arc::new(std::sync::OnceLock::new()),
            },
        }
    }

    pub fn stop_requested(&self) -> bool {
        self.stop.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn wait_until(&self, deadline: Option<std::time::Instant>) {
        if self.stop_requested() {
            return;
        }
        self.wake.wait_until(deadline)
    }
}

/// device層とservice層が共有するgeneric worker lifecycleの正規owner。
/// 全thread生成と従属reaper/supervisor handleはここから発行する。
pub struct WorkerRuntime<T = ()> {
    owner_id: i64,
    generation: u64,
    handle: Option<WorkerHandle<WorkerTerminalResult<T>, ()>>,
}

impl<T> WorkerRuntime<T> {
    pub fn wake(&self) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        self.handle
            .as_ref()
            .ok_or(maleicacid_tuner_hal2_common::HalError::NotInitialized {
                resource: "worker handle",
            })?
            .wake();
        Ok(())
    }
    pub const fn owner_id(&self) -> i64 {
        self.owner_id
    }
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .map(|handle| handle.is_thread_finished())
            .unwrap_or(true)
    }
    pub fn request_stop(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.request_stop();
        }
    }

    pub fn join(mut self) -> WorkerTerminalResult<T> {
        let Some(handle) = self.handle.take() else {
            return WorkerRuntimeOwnerFailure::ResultAlreadyCollected
                .into_terminal_result("WorkerRuntime");
        };
        match handle.join_after_stop() {
            Ok(Ok(result)) => result,
            Err(failure) => failure.into_terminal_result("WorkerRuntime"),
            Ok(Err(())) => {
                WorkerRuntimeOwnerFailure::MissingReport.into_terminal_result("WorkerRuntime")
            }
        }
    }
}

struct WorkerRuntimeReaperPendingState<K, V> {
    entries: std::collections::BTreeMap<K, V>,
    groups: std::collections::BTreeMap<u64, Vec<K>>,
    next_group_id: u64,
    capacity: usize,
}

pub struct WorkerRuntimeReaperPending<K, V> {
    state: std::sync::Arc<std::sync::Mutex<WorkerRuntimeReaperPendingState<K, V>>>,
    poison: std::sync::Arc<WorkerReaperPoisonState>,
}

#[derive(Default)]
struct WorkerReaperPoisonState {
    pending_count: std::sync::atomic::AtomicU64,
    receiver_count: std::sync::atomic::AtomicU64,
    receiver_poisoned: std::sync::atomic::AtomicBool,
}

impl WorkerReaperPoisonState {
    fn pending_failure(&self) -> maleicacid_tuner_hal2_common::HalError {
        reaper_lock_poison(
            &self.pending_count,
            maleicacid_tuner_hal2_common::WorkerLockKind::ReaperPending,
        )
    }

    fn check_receiver(&self) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        if self
            .receiver_poisoned
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(maleicacid_tuner_hal2_common::HalError::WorkerLockPoisoned {
                owner: "WorkerRuntimeReaperQueue",
                lock: maleicacid_tuner_hal2_common::WorkerLockKind::ReaperReceiver,
            });
        }
        Ok(())
    }
}

fn reaper_lock_poison(
    count: &std::sync::atomic::AtomicU64,
    lock: maleicacid_tuner_hal2_common::WorkerLockKind,
) -> maleicacid_tuner_hal2_common::HalError {
    let count = maleicacid_tuner_hal2_common::increment_atomic_counter_with_saturation(count, None);
    eprintln!(
        "ワーカー回収ロック汚染: ロック={lock:?} 検出回数={count} 飽和={}",
        count == u64::MAX
    );
    maleicacid_tuner_hal2_common::HalError::WorkerLockPoisoned {
        owner: "WorkerRuntimeReaperQueue",
        lock,
    }
}

fn lock_reaper_receiver<'a, J>(
    receiver: &'a std::sync::Mutex<std::sync::mpsc::Receiver<J>>,
    poison: &WorkerReaperPoisonState,
) -> Result<
    std::sync::MutexGuard<'a, std::sync::mpsc::Receiver<J>>,
    maleicacid_tuner_hal2_common::HalError,
> {
    receiver.lock().map_err(|_| {
        let error = reaper_lock_poison(
            &poison.receiver_count,
            maleicacid_tuner_hal2_common::WorkerLockKind::ReaperReceiver,
        );
        poison
            .receiver_poisoned
            .store(true, std::sync::atomic::Ordering::Release);
        error
    })
}

impl<K, V> Clone for WorkerRuntimeReaperPending<K, V> {
    fn clone(&self) -> Self {
        Self {
            state: std::sync::Arc::clone(&self.state),
            poison: std::sync::Arc::clone(&self.poison),
        }
    }
}

impl<K, V> WorkerRuntimeReaperPending<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    fn new(capacity: usize) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(WorkerRuntimeReaperPendingState {
                entries: std::collections::BTreeMap::new(),
                groups: std::collections::BTreeMap::new(),
                next_group_id: 1,
                capacity,
            })),
            poison: std::sync::Arc::new(WorkerReaperPoisonState::default()),
        }
    }

    fn lock_state(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, WorkerRuntimeReaperPendingState<K, V>>,
        maleicacid_tuner_hal2_common::HalError,
    > {
        self.poison.check_receiver()?;
        self.state.lock().map_err(|_| self.poison.pending_failure())
    }

    fn reserve_group(
        &self,
        reservations: impl IntoIterator<Item = (K, V)>,
    ) -> Result<u64, maleicacid_tuner_hal2_common::HalError> {
        let reservations: Vec<_> = reservations.into_iter().collect();
        if reservations.is_empty() {
            return Err(maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "ワーカー回収予約グループには1つ以上のキーが必要です",
            ));
        }
        let mut state = self.lock_state()?;
        if state.groups.len() >= state.capacity {
            return Err(maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "ワーカー回収予約の容量が不足しています",
            ));
        }
        if reservations
            .iter()
            .any(|(key, _)| state.entries.contains_key(key))
        {
            return Err(maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "ワーカー回収で重複したendpoint leaseを受け取りました",
            ));
        }
        let group_id = state.next_group_id;
        state.next_group_id = state.next_group_id.checked_add(1).ok_or_else(|| {
            maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "ワーカー回収予約グループIDを発行できません",
            )
        })?;
        let mut keys = Vec::with_capacity(reservations.len());
        for (key, value) in reservations {
            state.entries.insert(key.clone(), value);
            keys.push(key);
        }
        state.groups.insert(group_id, keys);
        Ok(group_id)
    }

    fn release_group(&self, group_id: u64) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        let mut state = self.lock_state()?;
        let keys = state.groups.remove(&group_id).ok_or_else(|| {
            maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "ワーカー回収予約グループは保留中ではありません",
            )
        })?;
        for key in keys {
            state.entries.remove(&key);
        }
        Ok(())
    }

    fn same_owner(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.state, &other.state)
    }

    pub fn pending_value(
        &self,
        key: &K,
    ) -> Result<Option<V>, maleicacid_tuner_hal2_common::HalError> {
        self.lock_state()
            .map(|state| state.entries.get(key).cloned())
    }

    pub fn update_value(
        &self,
        key: &K,
        value: V,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        let mut state = self.lock_state()?;
        let slot = state.entries.get_mut(key).ok_or_else(|| {
            maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "ワーカー回収の保留キーは予約されていません",
            )
        })?;
        *slot = value;
        Ok(())
    }

    #[cfg(test)]
    fn pending_group_count(&self) -> Result<usize, maleicacid_tuner_hal2_common::HalError> {
        self.lock_state().map(|state| state.groups.len())
    }
}

type WorkerReaperRunner<K, V, J> =
    dyn Fn(J, WorkerRuntimeReaperPending<K, V>, WorkerContext) + Send + Sync + 'static;

impl WorkerRuntime<()> {
    pub fn retain_cleanup<T>(value: T) -> WorkerRuntimeCleanup<T> {
        WorkerRuntimeCleanup {
            state: std::sync::Arc::new(std::sync::Mutex::new(WorkerCleanupState {
                attempt: 0,
                phase: WorkerCleanupPhase::Ready(value),
            })),
        }
    }

    pub fn spawn<T, F, C>(
        thread_name: String,
        owner_id: i64,
        generation: u64,
        worker: F,
        completion_signal: C,
    ) -> std::io::Result<WorkerRuntime<T>>
    where
        T: Send + 'static,
        F: FnOnce(WorkerContext) -> Result<T, maleicacid_tuner_hal2_common::HalError>
            + Send
            + 'static,
        C: FnOnce() + Send + 'static,
    {
        Self::spawn_with_context(
            thread_name,
            owner_id,
            generation,
            WorkerContext::new(),
            worker,
            move |_| completion_signal(),
        )
    }

    fn spawn_with_context<T, F, C>(
        thread_name: String,
        owner_id: i64,
        generation: u64,
        context: WorkerContext,
        worker: F,
        terminal_observer: C,
    ) -> std::io::Result<WorkerRuntime<T>>
    where
        T: Send + 'static,
        F: FnOnce(WorkerContext) -> Result<T, maleicacid_tuner_hal2_common::HalError>
            + Send
            + 'static,
        C: FnOnce(&WorkerTerminalResult<T>) + Send + 'static,
    {
        let handle = WorkerHandle::start_with_context(
            thread_name,
            move |context| {
                let terminal = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker(context.clone())
                })) {
                    Ok(Ok(_result)) if context.stop_requested() => {
                        WorkerTerminalResult::StopRequested
                    }
                    Ok(Ok(result)) => WorkerTerminalResult::Normal(result),
                    Ok(Err(error)) => WorkerTerminalResult::RuntimeFailure(error),
                    Err(_) => WorkerTerminalResult::PanicOrJoinFailure,
                };
                terminal_observer(&terminal);
                Ok::<_, ()>(terminal)
            },
            context,
        )?;
        Ok(WorkerRuntime {
            owner_id,
            generation,
            handle: Some(handle),
        })
    }

    pub fn spawn_controlled_handle<T, E>(
        name: String,
        run: impl FnOnce(WorkerContext) -> Result<T, E> + Send + 'static,
    ) -> std::io::Result<WorkerHandle<T, E>>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        WorkerHandle::start(name, run)
    }

    pub fn spawn_handle<T, E>(
        name: String,
        run: impl FnOnce() -> Result<T, E> + Send + 'static,
    ) -> std::io::Result<WorkerHandle<T, E>>
    where
        T: Send + 'static,
        E: Send + 'static,
    {
        Self::spawn_controlled_handle(name, move |_| run())
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

/// `WorkerRuntime`が発行するopaqueなbounded reaper handle。
pub struct WorkerRuntimeReaperQueue<K, V, J> {
    lanes: std::sync::Arc<Vec<WorkerHandle<(), maleicacid_tuner_hal2_common::HalError>>>,
    sender: std::sync::mpsc::SyncSender<WorkerRuntimeReaperQueuedJob<J>>,
    pending: WorkerRuntimeReaperPending<K, V>,
}

struct WorkerRuntimeReaperQueuedJob<J> {
    job: J,
    group_id: u64,
}

#[must_use = "reaper pending reservation must be released or transferred to the reaper queue"]
pub struct WorkerRuntimeReaperReservation<K, V> {
    pending: WorkerRuntimeReaperPending<K, V>,
    group_id: u64,
}

impl<K, V> WorkerRuntimeReaperReservation<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    fn release(self) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        self.pending.release_group(self.group_id)
    }

    fn transfer(self) {
        // canonical pending group と queued job が以後の所有者になる。
    }
}

impl<K, V, J> Clone for WorkerRuntimeReaperQueue<K, V, J> {
    fn clone(&self) -> Self {
        Self {
            lanes: std::sync::Arc::clone(&self.lanes),
            sender: self.sender.clone(),
            pending: self.pending.clone(),
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
        let pending = WorkerRuntimeReaperPending::new(capacity);
        let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));
        let mut lanes = Vec::with_capacity(capacity);
        for lane in 0..capacity {
            let receiver = std::sync::Arc::clone(&receiver);
            let runner = std::sync::Arc::clone(&runner);
            let pending_for_lane = pending.clone();
            let lane = WorkerRuntime::spawn_controlled_handle(
                format!("{thread_prefix}-{lane}"),
                move |context| loop {
                    let queued = lock_reaper_receiver(&receiver, &pending_for_lane.poison)?.recv();
                    let WorkerRuntimeReaperQueuedJob { job, group_id } = match queued {
                        Ok(queued) => queued,
                        Err(_) => return Ok(()),
                    };
                    runner(job, pending_for_lane.clone(), context.clone());
                    pending_for_lane.release_group(group_id)?;
                },
            )
            .map_err(|error| maleicacid_tuner_hal2_common::HalError::Io {
                backend: thread_prefix,
                operation: "スレッド生成",
                path: None,
                errno: error.raw_os_error(),
                detail: maleicacid_tuner_hal2_common::HalErrorDetail::new(error.to_string()),
            })?;
            lanes.push(lane);
        }
        Ok(Self {
            lanes: std::sync::Arc::new(lanes),
            sender,
            pending,
        })
    }

    pub fn reserve_pending(
        &self,
        reservations: impl IntoIterator<Item = (K, V)>,
    ) -> Result<WorkerRuntimeReaperReservation<K, V>, maleicacid_tuner_hal2_common::HalError> {
        let group_id = self.pending.reserve_group(reservations)?;
        Ok(WorkerRuntimeReaperReservation {
            pending: self.pending.clone(),
            group_id,
        })
    }

    pub fn release_reservation(
        &self,
        reservation: WorkerRuntimeReaperReservation<K, V>,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        if !self.pending.same_owner(&reservation.pending) {
            let mismatch = maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "ワーカー回収予約は別のキューに属しています",
            );
            return match reservation.release() {
                Ok(()) => Err(mismatch),
                Err(release_error) => Err(
                    maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                        "ワーカー回収予約のキュー不一致と元予約の解放がともに失敗しました",
                        mismatch,
                        release_error,
                    ),
                ),
            };
        }
        reservation.release()
    }

    pub fn enqueue_with_reservation(
        &self,
        job: J,
        reservation: WorkerRuntimeReaperReservation<K, V>,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        if !self.pending.same_owner(&reservation.pending) {
            drop(job);
            let mismatch = maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "ワーカー回収予約は別のキューに属しています",
            );
            return match reservation.release() {
                Ok(()) => Err(mismatch),
                Err(release_error) => Err(
                    maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                        "ワーカー回収投入のキュー不一致と元予約の解放がともに失敗しました",
                        mismatch,
                        release_error,
                    ),
                ),
            };
        }
        let group_id = reservation.group_id;
        match self
            .sender
            .try_send(WorkerRuntimeReaperQueuedJob { job, group_id })
        {
            Ok(()) => {
                reservation.transfer();
                Ok(())
            }
            Err(error) => {
                let send_error = match error {
                    std::sync::mpsc::TrySendError::Full(queued) => {
                        drop(queued.job);
                        maleicacid_tuner_hal2_common::HalError::internal(
                            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                            "ワーカー回収の容量が不足しています",
                        )
                    }
                    std::sync::mpsc::TrySendError::Disconnected(queued) => {
                        drop(queued.job);
                        maleicacid_tuner_hal2_common::HalError::internal(
                            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                            "ワーカー回収機構を利用できません",
                        )
                    }
                };
                match reservation.release() {
                    Ok(()) => Err(send_error),
                    Err(release_error) => Err(
                        maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                            "ワーカー回収への投入と予約解放がともに失敗しました",
                            send_error,
                            release_error,
                        ),
                    ),
                }
            }
        }
    }

    pub fn enqueue_reserved(
        &self,
        job: J,
        reservations: impl IntoIterator<Item = (K, V)>,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        let reservation = match self.reserve_pending(reservations) {
            Ok(reservation) => reservation,
            Err(error) => {
                drop(job);
                return Err(error);
            }
        };
        self.enqueue_with_reservation(job, reservation)
    }

    pub fn pending_value(
        &self,
        key: &K,
    ) -> Result<Option<V>, maleicacid_tuner_hal2_common::HalError> {
        self.pending.pending_value(key)
    }
}

/// `WorkerRuntime`が発行するactive/reaping registryの正本所有者。
pub struct WorkerRuntimeSupervisor<K, A, R> {
    capacity: usize,
    deadline: std::time::Duration,
    state: PoisonTrackedMutex<WorkerRuntimeSupervisorMaps<K, A, R>>,
    worker: std::sync::Mutex<SupervisorWorkerState>,
    worker_context: WorkerContext,
}

enum SupervisorWorkerState {
    NotStarted,
    Running(WorkerRuntime<()>),
    Finished(WorkerTerminalResult<()>),
}

struct WorkerRuntimeSupervisorMaps<K, A, R> {
    active: std::collections::BTreeMap<K, A>,
    reaping: std::collections::BTreeMap<K, R>,
    reserved_start: std::collections::BTreeSet<K>,
}

impl<K, A, R> Default for WorkerRuntimeSupervisorMaps<K, A, R> {
    fn default() -> Self {
        Self {
            active: std::collections::BTreeMap::new(),
            reaping: std::collections::BTreeMap::new(),
            reserved_start: std::collections::BTreeSet::new(),
        }
    }
}

impl<K: Ord, A, R> WorkerRuntimeSupervisorMaps<K, A, R> {
    fn total_len(&self) -> usize {
        self.active
            .len()
            .saturating_add(self.reaping.len())
            .saturating_add(self.reserved_start.len())
    }
}

pub trait WorkerRuntimeSupervisorActiveEntry {
    fn supervisor_is_finished(&self) -> bool;
    fn supervisor_request_stop(&self);
}

pub trait WorkerRuntimeSupervisorReapingEntry<K, A>: Sized {
    type DeadlineTarget: Copy;

    fn from_terminal(key: K, active: A) -> Self;
    fn from_stop(key: K, active: A) -> Self;
    fn from_reset(key: K, active: A) -> Self;
    fn supervisor_is_finished(&self) -> bool;
    fn supervisor_set_restart_requested(&mut self, requested: bool);
    fn supervisor_deadline_reported(&self) -> bool;
    fn supervisor_mark_deadline_reported(&mut self);
    fn supervisor_transferred_at(&self) -> std::time::Instant;
    fn supervisor_deadline_target(&self) -> Self::DeadlineTarget;
}

#[must_use]
pub struct WorkerRuntimeSupervisorStartPermit<K> {
    key: K,
}

pub enum WorkerRuntimeSupervisorStartPreparation<K> {
    Active,
    ReapingPending,
    Vacant(WorkerRuntimeSupervisorStartPermit<K>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerRuntimeSupervisorStopDisposition {
    Complete,
    ReapingPending,
}

pub enum WorkerRuntimeSupervisorAction<R, T> {
    Completed(R),
    Deadline(T),
}

impl<K, A, R> WorkerRuntimeSupervisor<K, A, R> {
    fn new(capacity: usize, deadline: std::time::Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            deadline,
            state: PoisonTrackedMutex::new(
                WorkerRuntimeSupervisorMaps::default(),
                RuntimeLockKind::SupervisorState,
            ),
            worker: std::sync::Mutex::new(SupervisorWorkerState::NotStarted),
            worker_context: WorkerContext::new(),
        }
    }

    fn lock_supervisor_state(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, WorkerRuntimeSupervisorMaps<K, A, R>>,
        maleicacid_tuner_hal2_common::HalError,
    > {
        self.state
            .lock()
            .map_err(maleicacid_tuner_hal2_common::HalError::LockPoisoned)
    }

    pub fn prepare_start(
        &self,
        key: K,
    ) -> Result<WorkerRuntimeSupervisorStartPreparation<K>, maleicacid_tuner_hal2_common::HalError>
    where
        K: Ord + Copy,
        A: WorkerRuntimeSupervisorActiveEntry,
        R: WorkerRuntimeSupervisorReapingEntry<K, A>,
    {
        use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
        let mut state = self.lock_supervisor_state()?;
        if state
            .active
            .get(&key)
            .is_some_and(WorkerRuntimeSupervisorActiveEntry::supervisor_is_finished)
        {
            let active = state.active.remove(&key).ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "完了済み監督ワーカーが回収移管前に消失しました",
                )
            })?;
            let mut reaping = R::from_terminal(key, active);
            reaping.supervisor_set_restart_requested(true);
            state.reaping.insert(key, reaping);
            drop(state);
            self.worker_context.wake.notify();
            return Ok(WorkerRuntimeSupervisorStartPreparation::ReapingPending);
        }
        if state.active.contains_key(&key) || state.reserved_start.contains(&key) {
            return Ok(WorkerRuntimeSupervisorStartPreparation::Active);
        }
        if let Some(reaping) = state.reaping.get_mut(&key) {
            reaping.supervisor_set_restart_requested(true);
            drop(state);
            self.worker_context.wake.notify();
            return Ok(WorkerRuntimeSupervisorStartPreparation::ReapingPending);
        }
        if state.total_len() >= self.capacity {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "監督ワーカーの容量を使い切っています",
            ));
        }
        state.reserved_start.insert(key);
        Ok(WorkerRuntimeSupervisorStartPreparation::Vacant(
            WorkerRuntimeSupervisorStartPermit { key },
        ))
    }

    pub fn commit_start(
        &self,
        permit: WorkerRuntimeSupervisorStartPermit<K>,
        active: A,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError>
    where
        K: Ord + Copy,
    {
        use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
        let mut state = self.lock_supervisor_state()?;
        if !state.reserved_start.remove(&permit.key)
            || state.active.contains_key(&permit.key)
            || state.reaping.contains_key(&permit.key)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "監督ワーカー開始予約の確定条件が崩れています",
            ));
        }
        state.active.insert(permit.key, active);
        drop(state);
        self.worker_context.wake.notify();
        Ok(())
    }

    pub fn abort_start(
        &self,
        permit: WorkerRuntimeSupervisorStartPermit<K>,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError>
    where
        K: Ord,
    {
        use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
        let mut state = self.lock_supervisor_state()?;
        if !state.reserved_start.remove(&permit.key) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "監督ワーカー開始予約の取消し対象がありません",
            ));
        }
        Ok(())
    }

    pub fn request_supervised_stop(
        &self,
        key: K,
    ) -> Result<WorkerRuntimeSupervisorStopDisposition, maleicacid_tuner_hal2_common::HalError>
    where
        K: Ord + Copy,
        A: WorkerRuntimeSupervisorActiveEntry,
        R: WorkerRuntimeSupervisorReapingEntry<K, A>,
    {
        let mut state = self.lock_supervisor_state()?;
        state.reserved_start.remove(&key);
        if let Some(reaping) = state.reaping.get_mut(&key) {
            reaping.supervisor_set_restart_requested(false);
            drop(state);
            self.worker_context.wake.notify();
            return Ok(WorkerRuntimeSupervisorStopDisposition::ReapingPending);
        }
        let Some(active) = state.active.remove(&key) else {
            return Ok(WorkerRuntimeSupervisorStopDisposition::Complete);
        };
        active.supervisor_request_stop();
        state.reaping.insert(key, R::from_stop(key, active));
        drop(state);
        self.worker_context.wake.notify();
        Ok(WorkerRuntimeSupervisorStopDisposition::ReapingPending)
    }

    pub fn request_supervised_reset(
        &self,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError>
    where
        K: Ord + Copy,
        A: WorkerRuntimeSupervisorActiveEntry,
        R: WorkerRuntimeSupervisorReapingEntry<K, A>,
    {
        let mut state = self.lock_supervisor_state()?;
        state.reserved_start.clear();
        for reaping in state.reaping.values_mut() {
            reaping.supervisor_set_restart_requested(false);
        }
        let active = core::mem::take(&mut state.active);
        for (key, worker) in active {
            worker.supervisor_request_stop();
            state.reaping.insert(key, R::from_reset(key, worker));
        }
        drop(state);
        self.worker_context.wake.notify();
        Ok(())
    }

    pub fn take_supervisor_action(
        &self,
    ) -> Result<
        (
            Option<WorkerRuntimeSupervisorAction<R, R::DeadlineTarget>>,
            Option<std::time::Instant>,
        ),
        maleicacid_tuner_hal2_common::HalError,
    >
    where
        K: Ord + Copy,
        A: WorkerRuntimeSupervisorActiveEntry,
        R: WorkerRuntimeSupervisorReapingEntry<K, A>,
    {
        let mut state = self.lock_supervisor_state()?;
        loop {
            if let Some(key) = state
                .active
                .iter()
                .find_map(|(key, active)| active.supervisor_is_finished().then_some(*key))
            {
                let Some(active) = state.active.remove(&key) else {
                    continue;
                };
                state.reaping.insert(key, R::from_terminal(key, active));
                continue;
            }
            if let Some(key) = state
                .reaping
                .iter()
                .find_map(|(key, reaping)| reaping.supervisor_is_finished().then_some(*key))
            {
                let Some(reaping) = state.reaping.remove(&key) else {
                    continue;
                };
                return Ok((
                    Some(WorkerRuntimeSupervisorAction::Completed(reaping)),
                    None,
                ));
            }
            if let Some(target) = state.reaping.values_mut().find_map(|reaping| {
                if !reaping.supervisor_deadline_reported()
                    && reaping.supervisor_transferred_at().elapsed() >= self.deadline
                {
                    reaping.supervisor_mark_deadline_reported();
                    Some(reaping.supervisor_deadline_target())
                } else {
                    None
                }
            }) {
                return Ok((Some(WorkerRuntimeSupervisorAction::Deadline(target)), None));
            }
            let next_wait = state
                .reaping
                .values()
                .filter(|reaping| !reaping.supervisor_deadline_reported())
                .map(|reaping| {
                    self.deadline
                        .saturating_sub(reaping.supervisor_transferred_at().elapsed())
                })
                .min();
            return Ok((
                None,
                next_wait.and_then(|wait| std::time::Instant::now().checked_add(wait)),
            ));
        }
    }

    pub fn start_worker(
        &self,
        name: &'static str,
        run: impl FnOnce(WorkerContext) -> Result<(), maleicacid_tuner_hal2_common::HalError>
            + Send
            + 'static,
        terminal_observer: impl FnOnce(&WorkerTerminalResult<()>) + Send + 'static,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        use maleicacid_tuner_hal2_common::{
            HalError, HalErrorDetail, HalInvalidStateKind, WorkerLockKind,
        };
        let mut slot = self
            .worker
            .lock()
            .map_err(|_| HalError::WorkerLockPoisoned {
                owner: "WorkerRuntimeSupervisor",
                lock: WorkerLockKind::SupervisorWorker,
            })?;
        if !matches!(*slot, SupervisorWorkerState::NotStarted) {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "監督ワーカーは既に登録されています",
            ));
        }
        let worker = WorkerRuntime::spawn_with_context(
            name.to_owned(),
            0,
            1,
            self.worker_context.clone(),
            run,
            terminal_observer,
        )
        .map_err(|error| HalError::Io {
            backend: name,
            operation: "スレッド生成",
            path: None,
            errno: error.raw_os_error(),
            detail: HalErrorDetail::new(error.to_string()),
        })?;
        *slot = SupervisorWorkerState::Running(worker);
        Ok(())
    }

    pub fn worker_terminal_result(
        &self,
    ) -> Result<Option<WorkerTerminalResult<()>>, maleicacid_tuner_hal2_common::HalError> {
        use maleicacid_tuner_hal2_common::{HalError, WorkerLockKind};
        let mut slot = self
            .worker
            .lock()
            .map_err(|_| HalError::WorkerLockPoisoned {
                owner: "WorkerRuntimeSupervisor",
                lock: WorkerLockKind::SupervisorWorker,
            })?;
        if matches!(&*slot, SupervisorWorkerState::Running(worker) if worker.is_finished()) {
            if let SupervisorWorkerState::Running(worker) =
                std::mem::replace(&mut *slot, SupervisorWorkerState::NotStarted)
            {
                *slot = SupervisorWorkerState::Finished(worker.join());
            }
        }
        match &*slot {
            SupervisorWorkerState::Finished(result) => Ok(Some(result.clone())),
            SupervisorWorkerState::Running(_) => Ok(None),
            SupervisorWorkerState::NotStarted => Err(HalError::NotInitialized {
                resource: "監督ワーカー",
            }),
        }
    }
}

impl<K, A, R> Drop for WorkerRuntimeSupervisor<K, A, R> {
    fn drop(&mut self) {
        self.worker_context
            .stop
            .store(true, std::sync::atomic::Ordering::Release);
        self.worker_context.wake.notify();
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

use maleicacid_tuner_hal2_common::FmqFailureKind;

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

    #[test]
    fn supervisor_state_poison_is_not_worker_slot_poison() {
        let supervisor =
            super::WorkerRuntime::supervisor::<u8, (), ()>(4, std::time::Duration::from_secs(1));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = supervisor.lock_supervisor_state().unwrap();
            panic!("汚染を注入");
        }));
        for count in [1, 2] {
            assert!(matches!(supervisor.lock_supervisor_state(),
                Err(maleicacid_tuner_hal2_common::HalError::LockPoisoned(poison))
                if poison.lock == maleicacid_tuner_hal2_common::RuntimeLockKind::SupervisorState && poison.poison_count == count
            ));
        }
    }

    #[test]
    fn owner_failure_keeps_missing_results_distinct_from_panic_and_join() {
        use super::{WorkerRuntimeOwnerFailure, WorkerTerminalResult};
        for failure in [
            WorkerRuntimeOwnerFailure::MissingReport,
            WorkerRuntimeOwnerFailure::ResultAlreadyCollected,
        ] {
            assert!(matches!(
                failure.into_terminal_result::<()>("test"),
                WorkerTerminalResult::RuntimeFailure(_)
            ));
        }
        for failure in [
            WorkerRuntimeOwnerFailure::ThreadPanic,
            WorkerRuntimeOwnerFailure::JoinFailure,
        ] {
            assert_eq!(
                failure.into_terminal_result::<()>("test"),
                WorkerTerminalResult::PanicOrJoinFailure
            );
        }
    }

    #[test]
    fn supervisor_worker_uses_canonical_stop_and_terminal_collection() {
        use super::{WorkerRuntime, WorkerTerminalResult};
        use std::sync::{mpsc, Arc};
        use std::time::{Duration, Instant};
        let supervisor = Arc::new(WorkerRuntime::supervisor::<u32, (), ()>(
            1,
            Duration::from_secs(1),
        ));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (terminal_tx, terminal_rx) = mpsc::channel();
        supervisor
            .start_worker(
                "supervisor-stop-test",
                move |control| {
                    ready_tx.send(()).unwrap();
                    while !control.stop_requested() {
                        control.wait_until(None);
                    }
                    Ok(())
                },
                move |terminal| terminal_tx.send(terminal.clone()).unwrap(),
            )
            .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(supervisor
            .start_worker("duplicate", |_| Ok(()), |_| {})
            .is_err());
        supervisor
            .worker_context
            .stop
            .store(true, std::sync::atomic::Ordering::Release);
        supervisor.notify_worker();
        assert_eq!(
            terminal_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            WorkerTerminalResult::StopRequested
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(terminal) = supervisor.worker_terminal_result().unwrap() {
                assert_eq!(terminal, WorkerTerminalResult::StopRequested);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            supervisor.worker_terminal_result().unwrap(),
            Some(WorkerTerminalResult::StopRequested)
        );
    }

    #[test]
    fn dropping_supervisor_wakes_its_parked_worker_without_a_strong_cycle() {
        use super::WorkerRuntime;
        use std::sync::mpsc;
        use std::time::Duration;
        let supervisor = WorkerRuntime::supervisor::<u32, (), ()>(1, Duration::from_secs(1));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stopped_tx, stopped_rx) = mpsc::channel();
        supervisor
            .start_worker(
                "supervisor-drop-test",
                move |control| {
                    ready_tx.send(()).unwrap();
                    while !control.stop_requested() {
                        control.wait_until(None);
                    }
                    stopped_tx.send(()).unwrap();
                    Ok(())
                },
                |_| {},
            )
            .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(supervisor);
        stopped_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn supervisor_preserves_panic_as_a_terminal_result() {
        use super::{WorkerRuntime, WorkerTerminalResult};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        let supervisor = WorkerRuntime::supervisor::<u32, (), ()>(1, Duration::from_secs(1));
        let (terminal_tx, terminal_rx) = mpsc::channel();
        supervisor
            .start_worker(
                "supervisor-panic-test",
                |_| panic!("panicを注入"),
                move |terminal| terminal_tx.send(terminal.clone()).unwrap(),
            )
            .unwrap();
        assert_eq!(
            terminal_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            WorkerTerminalResult::PanicOrJoinFailure
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while supervisor.worker_terminal_result().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            supervisor.worker_terminal_result().unwrap(),
            Some(WorkerTerminalResult::PanicOrJoinFailure)
        );
    }

    #[test]
    fn reaper_wait_is_cancelled_only_after_the_last_queue_owner_is_dropped() {
        use std::sync::{mpsc, Arc};
        use std::time::{Duration, Instant};
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let runner = Arc::new(
            move |(),
                  _pending: super::WorkerRuntimeReaperPending<u32, u32>,
                  context: super::WorkerContext| {
                started_tx.send(()).unwrap();
                context.wait_until(Instant::now().checked_add(Duration::from_secs(3_600)));
                finished_tx.send(context.stop_requested()).unwrap();
            },
        );
        let queue =
            super::WorkerRuntime::start_reaper_queue(1, "test-reaper-cancel", runner).unwrap();
        let remaining_owner = queue.clone();
        queue.enqueue_reserved((), [(1, 1)]).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(queue);
        assert_eq!(finished_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
        drop(remaining_owner);
        assert!(finished_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    }

    #[test]
    fn wake_before_wait_is_retained_by_the_worker() {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (continue_tx, continue_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = super::WorkerRuntime::spawn(
            "wake-before-wait".into(),
            1,
            1,
            move |context| {
                ready_tx.send(()).unwrap();
                continue_rx.recv().unwrap();
                context.wait_until(Some(
                    std::time::Instant::now() + std::time::Duration::from_secs(10),
                ));
                done_tx.send(()).unwrap();
                Ok(())
            },
            || {},
        )
        .unwrap();
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        worker.wake().unwrap();
        continue_tx.send(()).unwrap();
        done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(matches!(
            worker.join(),
            super::WorkerTerminalResult::Normal(())
        ));
    }

    #[test]
    fn stop_wakes_an_indefinitely_waiting_worker() {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = super::WorkerRuntime::spawn(
            "stop-parked".into(),
            2,
            1,
            move |context| {
                ready_tx.send(()).unwrap();
                while !context.stop_requested() {
                    context.wait_until(None);
                }
                done_tx.send(()).unwrap();
                Ok(())
            },
            || {},
        )
        .unwrap();
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        worker.request_stop();
        done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(matches!(
            worker.join(),
            super::WorkerTerminalResult::StopRequested
        ));
    }
    use super::*;

    #[test]
    fn lost_cleanup_authority_keeps_the_obligation_reissuable() {
        for forget in [false, true] {
            let owner = WorkerRuntime::retain_cleanup(7);
            let first = owner.issue().unwrap();
            if forget {
                std::mem::forget(first);
            } else {
                drop(first);
            }
            assert!(owner.is_pending());
            let replacement = owner.issue().unwrap();
            assert!(matches!(
                replacement.execute(|value| WorkerCleanupProgress::Completed(*value)),
                Ok(WorkerCleanupRun::Completed(7))
            ));
            assert!(!owner.is_pending());
            assert!(owner.issue().is_err());
        }
    }

    #[test]
    fn superseded_cleanup_authority_cannot_execute_a_side_effect() {
        let owner = WorkerRuntime::retain_cleanup(0);
        let first = owner.issue().unwrap();
        let replacement = owner.issue().unwrap();
        assert!(matches!(
            first.execute::<()>(|_| panic!("失効した権限が実行されました")),
            Err(
                maleicacid_tuner_hal2_common::HalError::WorkerCleanupFailed {
                    kind: WorkerCleanupFailureKind::Superseded
                }
            )
        ));
        assert!(matches!(
            replacement.execute(|value| {
                *value += 1;
                WorkerCleanupProgress::Completed(*value)
            }),
            Ok(WorkerCleanupRun::Completed(1))
        ));
    }

    #[test]
    fn executing_cleanup_rejects_reissue_without_waiting() {
        let owner = WorkerRuntime::retain_cleanup(());
        let authority = owner.issue().unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            authority.execute(|_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                WorkerCleanupProgress::Completed(())
            })
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(owner.state.try_lock().is_ok());
        assert!(owner.issue().is_err());
        assert!(owner.is_pending());
        release_tx.send(()).unwrap();
        assert!(matches!(
            worker.join().unwrap(),
            Ok(WorkerCleanupRun::Completed(()))
        ));
        assert!(!owner.is_pending());
    }

    #[test]
    fn exhausted_cleanup_attempt_does_not_reuse_an_identity() {
        let owner = WorkerRuntime::retain_cleanup(());
        owner.state.lock().unwrap().attempt = u64::MAX;
        assert!(matches!(
            owner.issue(),
            Err(
                maleicacid_tuner_hal2_common::HalError::WorkerCleanupFailed {
                    kind: WorkerCleanupFailureKind::AttemptExhausted
                }
            )
        ));
        assert!(owner.is_pending());
    }

    #[test]
    fn interrupted_cleanup_keeps_the_obligation_and_rejects_blind_retry() {
        let owner = WorkerRuntime::retain_cleanup(0);
        let authority = owner.issue().unwrap();
        assert!(matches!(
            authority.execute::<()>(|value| {
                *value = 1;
                panic!("副作用の後で中断しました");
            }),
            Err(
                maleicacid_tuner_hal2_common::HalError::WorkerCleanupFailed {
                    kind: WorkerCleanupFailureKind::Interrupted,
                }
            )
        ));
        assert!(owner.is_pending());
        assert!(matches!(
            owner.issue(),
            Err(
                maleicacid_tuner_hal2_common::HalError::WorkerCleanupFailed {
                    kind: WorkerCleanupFailureKind::Quarantined
                }
            )
        ));
    }

    #[test]
    fn failed_cleanup_retains_its_result_and_rejects_retry() {
        let owner = WorkerRuntime::retain_cleanup(None);
        let authority = owner.issue().unwrap();
        assert!(matches!(
            authority.execute(|value| {
                *value = Some("停止失敗");
                WorkerCleanupProgress::Quarantined("停止失敗")
            }),
            Ok(WorkerCleanupRun::Completed("停止失敗"))
        ));
        assert!(owner.is_pending());
        assert!(matches!(
            &owner.state.lock().unwrap().phase,
            WorkerCleanupPhase::Quarantined(Some("停止失敗"))
        ));
        assert!(matches!(
            owner.issue(),
            Err(
                maleicacid_tuner_hal2_common::HalError::WorkerCleanupFailed {
                    kind: WorkerCleanupFailureKind::Quarantined,
                }
            )
        ));
    }

    #[test]
    fn cleanup_wait_releases_the_state_lock_and_keeps_the_attempt_exclusive() {
        let owner = WorkerRuntime::retain_cleanup(7);
        let authority = owner.issue().unwrap();
        let observed = authority
            .inspect(|value| {
                assert!(owner.state.try_lock().is_ok());
                assert!(matches!(
                    owner.issue(),
                    Err(
                        maleicacid_tuner_hal2_common::HalError::WorkerCleanupFailed {
                            kind: WorkerCleanupFailureKind::Executing,
                        }
                    )
                ));
                *value
            })
            .unwrap();
        assert_eq!(observed, 7);
        assert!(matches!(
            authority.execute(|value| WorkerCleanupProgress::Completed(*value)),
            Ok(WorkerCleanupRun::Completed(7))
        ));
    }

    fn reaper_queue_for_test(
        capacity: usize,
    ) -> (
        WorkerRuntimeReaperQueue<u32, u32, ()>,
        std::sync::mpsc::Receiver<WorkerRuntimeReaperQueuedJob<()>>,
    ) {
        let (sender, receiver) =
            std::sync::mpsc::sync_channel::<WorkerRuntimeReaperQueuedJob<()>>(capacity.max(1));
        (
            WorkerRuntimeReaperQueue {
                lanes: std::sync::Arc::new(Vec::new()),
                sender,
                pending: WorkerRuntimeReaperPending::new(capacity.max(1)),
            },
            receiver,
        )
    }

    #[test]
    fn reaper_reservation_blocks_duplicates_until_explicit_release() {
        let (queue, _receiver) = reaper_queue_for_test(2);

        let reservation = queue.reserve_pending([(7, 11)]).unwrap();
        assert_eq!(queue.pending_value(&7).unwrap(), Some(11));
        assert!(queue.reserve_pending([(7, 12)]).is_err());

        queue.release_reservation(reservation).unwrap();
        assert_eq!(queue.pending_value(&7).unwrap(), None);
        assert!(queue.reserve_pending([(7, 13)]).is_ok());
    }

    #[test]
    fn reaper_capacity_counts_reservation_groups_not_keys() {
        let (queue, _receiver) = reaper_queue_for_test(1);

        let reservation = queue.reserve_pending([(1, 11), (2, 12)]).unwrap();
        assert_eq!(queue.pending.pending_group_count().unwrap(), 1);
        assert_eq!(queue.pending_value(&1).unwrap(), Some(11));
        assert_eq!(queue.pending_value(&2).unwrap(), Some(12));
        assert!(queue.reserve_pending([(3, 13)]).is_err());

        queue.release_reservation(reservation).unwrap();
        assert_eq!(queue.pending.pending_group_count().unwrap(), 0);
        assert!(queue.reserve_pending([(3, 13)]).is_ok());
    }

    #[test]
    fn cross_queue_release_releases_the_original_reservation_before_returning_error() {
        let (queue_a, _receiver_a) = reaper_queue_for_test(1);
        let (queue_b, _receiver_b) = reaper_queue_for_test(1);

        let reservation = queue_a.reserve_pending([(7, 11)]).unwrap();
        assert_eq!(queue_a.pending_value(&7).unwrap(), Some(11));
        assert!(queue_b.release_reservation(reservation).is_err());
        assert_eq!(queue_a.pending_value(&7).unwrap(), None);
        assert!(queue_a.reserve_pending([(7, 12)]).is_ok());
    }

    #[test]
    fn cross_queue_enqueue_releases_the_original_reservation_before_returning_error() {
        let (queue_a, _receiver_a) = reaper_queue_for_test(1);
        let (queue_b, _receiver_b) = reaper_queue_for_test(1);

        let reservation = queue_a.reserve_pending([(8, 21)]).unwrap();
        assert_eq!(queue_a.pending_value(&8).unwrap(), Some(21));
        assert!(queue_b.enqueue_with_reservation((), reservation).is_err());
        assert_eq!(queue_a.pending_value(&8).unwrap(), None);
        assert!(queue_a.reserve_pending([(8, 22)]).is_ok());
    }

    #[test]
    fn disconnected_reaper_send_releases_only_its_own_reservation_group() {
        let (queue, receiver) = reaper_queue_for_test(2);
        let retained = queue.reserve_pending([(1, 9)]).unwrap();
        drop(receiver);

        assert!(queue.enqueue_reserved((), [(2, 8)]).is_err());
        assert_eq!(queue.pending_value(&1).unwrap(), Some(9));
        assert_eq!(queue.pending_value(&2).unwrap(), None);
        assert_eq!(queue.pending.pending_group_count().unwrap(), 1);

        queue.release_reservation(retained).unwrap();
        assert_eq!(queue.pending.pending_group_count().unwrap(), 0);
    }

    #[test]
    fn reaper_pending_poison_preserves_identity_count_and_failed_state() {
        use maleicacid_tuner_hal2_common::{HalError, WorkerLockKind};
        use std::sync::atomic::Ordering;

        let pending = WorkerRuntimeReaperPending::new(2);
        let group = pending.reserve_group([(1, 9)]).unwrap();
        let clone = pending.clone();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = pending.state.lock().unwrap();
            panic!("保留レジストリを汚染");
        }))
        .is_err());
        let expected = HalError::WorkerLockPoisoned {
            owner: "WorkerRuntimeReaperQueue",
            lock: WorkerLockKind::ReaperPending,
        };
        assert_eq!(pending.reserve_group([(2, 8)]).unwrap_err(), expected);
        assert_eq!(pending.release_group(group).unwrap_err(), expected);
        assert_eq!(clone.pending_value(&1).unwrap_err(), expected);
        assert_eq!(clone.update_value(&1, 7).unwrap_err(), expected);
        assert_eq!(pending.poison.pending_count.load(Ordering::Acquire), 4);
        assert!(pending.state.is_poisoned());
        pending
            .poison
            .pending_count
            .store(u64::MAX, Ordering::Release);
        assert_eq!(pending.pending_value(&1).unwrap_err(), expected);
        assert_eq!(
            pending.poison.pending_count.load(Ordering::Acquire),
            u64::MAX
        );
    }

    #[test]
    fn reaper_receiver_poison_is_visible_to_pending_callers_without_releasing_ownership() {
        use maleicacid_tuner_hal2_common::{HalError, WorkerLockKind};
        use std::sync::atomic::Ordering;

        let pending = WorkerRuntimeReaperPending::new(2);
        pending.reserve_group([(1, 9)]).unwrap();
        let clone = pending.clone();
        let (_sender, receiver) = std::sync::mpsc::channel::<()>();
        let receiver = std::sync::Mutex::new(receiver);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = receiver.lock().unwrap();
            panic!("回収受信側を汚染");
        }))
        .is_err());
        let expected = HalError::WorkerLockPoisoned {
            owner: "WorkerRuntimeReaperQueue",
            lock: WorkerLockKind::ReaperReceiver,
        };
        assert_eq!(
            lock_reaper_receiver(&receiver, &pending.poison).unwrap_err(),
            expected
        );
        assert_eq!(clone.pending_value(&1).unwrap_err(), expected);
        assert_eq!(clone.reserve_group([(2, 8)]).unwrap_err(), expected);
        assert_eq!(pending.poison.receiver_count.load(Ordering::Acquire), 1);
        assert_eq!(pending.state.lock().unwrap().entries.get(&1), Some(&9));
        assert!(receiver.is_poisoned());
    }

    #[test]
    fn join_preserves_result_and_completion_poison_identity() {
        use maleicacid_tuner_hal2_common::{HalError, WorkerLockKind};

        for lock in [WorkerLockKind::Result, WorkerLockKind::Completion] {
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let worker = WorkerRuntime::spawn(
                "poisoned-owner".into(),
                3,
                1,
                move |_| {
                    release_rx.recv().unwrap();
                    Ok(())
                },
                || {},
            )
            .unwrap();
            let handle = worker.handle.as_ref().unwrap();
            let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match lock {
                WorkerLockKind::Result => {
                    let _guard = handle.result.lock().unwrap();
                    panic!("結果ロックを汚染");
                }
                WorkerLockKind::Completion => {
                    let _guard = handle.completion.0.lock().unwrap();
                    panic!("完了ロックを汚染");
                }
                WorkerLockKind::SupervisorWorker
                | WorkerLockKind::ReaperPending
                | WorkerLockKind::ReaperReceiver => panic!("ワーカー結果ロックではありません"),
            }));
            assert!(poisoned.is_err());
            release_tx.send(()).unwrap();
            assert_eq!(
                worker.join(),
                WorkerTerminalResult::RuntimeFailure(HalError::WorkerLockPoisoned {
                    owner: "WorkerRuntime",
                    lock,
                })
            );
        }
    }

    #[test]
    fn dropping_handle_wakes_worker_without_a_fallible_cleanup() {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = WorkerRuntime::spawn(
            "drop-stop".into(),
            4,
            1,
            move |context| {
                ready_tx.send(()).unwrap();
                while !context.stop_requested() {
                    context.wait_until(None);
                }
                done_tx.send(()).unwrap();
                Ok(())
            },
            || {},
        )
        .unwrap();
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        drop(worker);
        done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
    }

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
