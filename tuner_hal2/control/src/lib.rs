use maleicacid_tuner_hal2_common::WorkerCleanupFailureKind;

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
            return WorkerTerminalResult::PanicOrJoinFailure;
        };
        match handle.join_after_stop() {
            Ok(Ok(result)) => result,
            Err(WorkerRuntimeOwnerFailure::ResultLockPoison) => {
                WorkerTerminalResult::RuntimeFailure(
                    maleicacid_tuner_hal2_common::HalError::WorkerLockPoisoned {
                        owner: "WorkerRuntime",
                        lock: maleicacid_tuner_hal2_common::WorkerLockKind::Result,
                    },
                )
            }
            Err(WorkerRuntimeOwnerFailure::CompletionLockPoison) => {
                WorkerTerminalResult::RuntimeFailure(
                    maleicacid_tuner_hal2_common::HalError::WorkerLockPoisoned {
                        owner: "WorkerRuntime",
                        lock: maleicacid_tuner_hal2_common::WorkerLockKind::Completion,
                    },
                )
            }
            Ok(Err(())) | Err(_) => WorkerTerminalResult::PanicOrJoinFailure,
        }
    }
}

type WorkerReaperRunner<K, V, J> = dyn Fn(J, std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>, WorkerContext)
    + Send
    + Sync
    + 'static;

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
    sender: std::sync::mpsc::SyncSender<J>,
    pending: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,
}

#[must_use = "reaper pending reservation must be released or transferred to the reaper queue"]
pub struct WorkerRuntimeReaperReservation<K: Ord, V> {
    pending: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,
    keys: Vec<K>,
}

impl<K: Ord, V> WorkerRuntimeReaperReservation<K, V> {
    fn release(self) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        let mut pending = self.pending.lock().map_err(|_| {
            maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "worker reaper pending registry lock poisoned while releasing reservation",
            )
        })?;
        for key in self.keys {
            pending.remove(&key);
        }
        Ok(())
    }
}

impl<K, V, J> Clone for WorkerRuntimeReaperQueue<K, V, J> {
    fn clone(&self) -> Self {
        Self {
            lanes: std::sync::Arc::clone(&self.lanes),
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
        let mut lanes = Vec::with_capacity(capacity);
        for lane in 0..capacity {
            let receiver = std::sync::Arc::clone(&receiver);
            let runner = std::sync::Arc::clone(&runner);
            let pending_for_lane = std::sync::Arc::clone(&pending);
            let lane =
                WorkerRuntime::spawn_controlled_handle(
                    format!("{thread_prefix}-{lane}"),
                    move |context| loop {
                        let job = match receiver.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => return Err(maleicacid_tuner_hal2_common::HalError::internal(
                            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                            "worker reaper receiver lock poisoned",
                        )),
                    };
                        match job {
                            Ok(job) => runner(
                                job,
                                std::sync::Arc::clone(&pending_for_lane),
                                context.clone(),
                            ),
                            Err(_) => return Ok(()),
                        }
                    },
                )
                .map_err(|error| maleicacid_tuner_hal2_common::HalError::Io {
                    backend: thread_prefix,
                    operation: "thread spawn",
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
        let reservations: Vec<_> = reservations.into_iter().collect();
        let mut pending = self.pending.lock().map_err(|_| {
            maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "worker reaper pending registry lock poisoned",
            )
        })?;
        if reservations
            .iter()
            .any(|(key, _)| pending.contains_key(key))
        {
            return Err(maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "worker reaper received a duplicate endpoint lease",
            ));
        }
        let mut keys = Vec::with_capacity(reservations.len());
        for (key, value) in reservations {
            pending.insert(key.clone(), value);
            keys.push(key);
        }
        drop(pending);
        Ok(WorkerRuntimeReaperReservation {
            pending: std::sync::Arc::clone(&self.pending),
            keys,
        })
    }

    pub fn release_reservation(
        &self,
        reservation: WorkerRuntimeReaperReservation<K, V>,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        if !std::sync::Arc::ptr_eq(&self.pending, &reservation.pending) {
            return Err(maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "worker reaper reservation belongs to a different queue",
            ));
        }
        reservation.release()
    }

    pub fn enqueue_with_reservation(
        &self,
        job: J,
        reservation: WorkerRuntimeReaperReservation<K, V>,
    ) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        if !std::sync::Arc::ptr_eq(&self.pending, &reservation.pending) {
            drop(job);
            return Err(maleicacid_tuner_hal2_common::HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "worker reaper reservation belongs to a different queue",
            ));
        }
        match self.sender.try_send(job) {
            Ok(()) => Ok(()),
            Err(error) => {
                let send_error = match error {
                    std::sync::mpsc::TrySendError::Full(job) => {
                        drop(job);
                        maleicacid_tuner_hal2_common::HalError::internal(
                            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                            "worker reaper capacity exhausted",
                        )
                    }
                    std::sync::mpsc::TrySendError::Disconnected(job) => {
                        drop(job);
                        maleicacid_tuner_hal2_common::HalError::internal(
                            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                            "worker reaper is unavailable",
                        )
                    }
                };
                match reservation.release() {
                    Ok(()) => Err(send_error),
                    Err(release_error) => Err(
                        maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                            "worker reaper enqueue and reservation release both failed",
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

/// `WorkerRuntime`が発行するopaqueなactive/reaping registry handle。
pub struct WorkerRuntimeSupervisor<K, A, R> {
    capacity: usize,
    deadline: std::time::Duration,
    state: std::sync::Mutex<WorkerRuntimeSupervisorMaps<K, A, R>>,
    worker: std::sync::Mutex<SupervisorWorkerState>,
    worker_context: WorkerContext,
}

enum SupervisorWorkerState {
    NotStarted,
    Running(WorkerRuntime<()>),
    Finished(WorkerTerminalResult<()>),
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
            worker: std::sync::Mutex::new(SupervisorWorkerState::NotStarted),
            worker_context: WorkerContext::new(),
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
    pub fn notify_worker(&self) {
        self.worker_context.wake.notify();
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
                "supervisor worker is already installed",
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
            operation: "thread spawn",
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
                resource: "supervisor worker",
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

pub use maleicacid_tuner_hal2_common::FmqFailureKind;

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
                |_| panic!("injected panic"),
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
        use std::sync::{mpsc, Arc, Mutex};
        use std::time::{Duration, Instant};
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let runner = Arc::new(
            move |(),
                  _pending: Arc<Mutex<std::collections::BTreeMap<u32, u32>>>,
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
            first.execute::<()>(|_| panic!("stale authority executed")),
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
                panic!("interrupted after a side effect");
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
                *value = Some("stop failed");
                WorkerCleanupProgress::Quarantined("stop failed")
            }),
            Ok(WorkerCleanupRun::Completed("stop failed"))
        ));
        assert!(owner.is_pending());
        assert!(matches!(
            &owner.state.lock().unwrap().phase,
            WorkerCleanupPhase::Quarantined(Some("stop failed"))
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

    #[test]
    fn reaper_reservation_blocks_duplicates_until_explicit_release() {
        let (sender, _receiver) = std::sync::mpsc::sync_channel::<()>(1);
        let queue = WorkerRuntimeReaperQueue {
            lanes: std::sync::Arc::new(Vec::new()),
            sender,
            pending: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::BTreeMap::new(),
            )),
        };

        let reservation = queue.reserve_pending([(7, 11)]).unwrap();
        assert_eq!(queue.pending_value(&7).unwrap(), Some(11));
        assert!(queue.reserve_pending([(7, 12)]).is_err());

        queue.release_reservation(reservation).unwrap();
        assert_eq!(queue.pending_value(&7).unwrap(), None);
        assert!(queue.reserve_pending([(7, 13)]).is_ok());
    }

    #[test]
    fn rejected_reaper_send_releases_only_its_own_reservations() {
        // 受信側の消滅と満杯を決定的に発生させ、予約の取消しを正規入口で確認する。
        for disconnected in [false, true] {
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let mut receiver = Some(receiver);
            if disconnected {
                drop(receiver.take());
            } else {
                sender.try_send(()).unwrap();
            }
            let queue = WorkerRuntimeReaperQueue {
                lanes: std::sync::Arc::new(Vec::new()),
                sender,
                pending: std::sync::Arc::new(std::sync::Mutex::new(
                    std::collections::BTreeMap::from([(1, 9)]),
                )),
            };
            assert!(queue.enqueue_reserved((), [(2, 8)]).is_err());
            assert_eq!(queue.pending_value(&1).unwrap(), Some(9));
            assert_eq!(queue.pending_value(&2).unwrap(), None);
            drop(receiver);
        }
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
                    panic!("poison result");
                }
                WorkerLockKind::Completion => {
                    let _guard = handle.completion.0.lock().unwrap();
                    panic!("poison completion");
                }
                WorkerLockKind::SupervisorWorker => panic!("not a worker result lock"),
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
