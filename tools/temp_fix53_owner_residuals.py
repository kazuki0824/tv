from pathlib import Path
import re

ROOT = Path("tuner_hal2")


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, got {count}: {old[:120]!r}")
    path.write_text(text.replace(old, new, 1))


def regex_once(path: Path, pattern: str, repl: str, flags: int = 0) -> None:
    text = path.read_text()
    new, count = re.subn(pattern, repl, text, count=1, flags=flags)
    if count != 1:
        raise SystemExit(f"{path}: regex anchor count={count}: {pattern[:120]!r}")
    path.write_text(new)


# S-04: the lower dependency layer contains the single generic WorkerRuntime
# implementation. WorkerHandle is constructor-private and is issued only by
# WorkerRuntime, so device adapters cannot create an independent JoinHandle owner.
control = ROOT / "control/src/lib.rs"
text = control.read_text()
marker = "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub enum WorkerStopReason"
if marker not in text:
    raise SystemExit("control worker marker missing")
tail = text[text.index(marker):]
worker_prefix = r'''#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
            self.stop
                .store(true, std::sync::atomic::Ordering::Release);
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
        F: FnOnce(std::sync::Arc<std::sync::atomic::AtomicBool>)
                -> Result<T, maleicacid_tuner_hal2_common::HalError>
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
        runner: std::sync::Arc<
            dyn Fn(
                    J,
                    std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,
                ) + Send
                + Sync
                + 'static,
        >,
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
        runner: std::sync::Arc<
            dyn Fn(
                    J,
                    std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,
                ) + Send
                + Sync
                + 'static,
        >,
    ) -> Result<Self, maleicacid_tuner_hal2_common::HalError> {
        let capacity = capacity.max(1);
        let (sender, receiver) = std::sync::mpsc::sync_channel(capacity);
        let pending = std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::BTreeMap::new(),
        ));
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

'''
control.write_text(worker_prefix + tail)

# The service layer no longer owns a second physical worker runtime. It re-exports
# the canonical control-core owner and adds only service failure classification.
worker_runtime = ROOT / "service_runtime/src/worker_runtime.rs"
worker_runtime.write_text(r'''pub use maleicacid_tuner_hal2_control_core::{
    WorkerHandle, WorkerRuntime, WorkerRuntimeReaperQueue, WorkerRuntimeSupervisor,
    WorkerTerminalResult,
};

pub const CLEANUP_RETRY_SCHEDULE_MS: &[u64] = &[0, 10, 100, 1_000];
pub const CLEANUP_TERMINAL_DEADLINE_MS: u64 = 30_000;
pub const WORKER_IO_DEADLINE_MS: u64 = 2_000;
pub const WORKER_REAPER_DEADLINE_MS: u64 = 10_000;

pub fn join_worker_classified<T>(
    worker: WorkerRuntime<T>,
) -> crate::worker_failure_classifier::ClassifiedWorkerTerminalResult<T> {
    crate::worker_failure_classifier::WorkerFailureClassifier::classify_terminal(
        worker.join(),
        "worker panicked or could not be joined",
    )
}
''')

thread_owner = ROOT / "device/src/runtime/thread_result_owner.rs"
replace_once(
    thread_owner,
    "use maleicacid_tuner_hal2_control_core::{\n    WorkerRuntimeOwnerFailure, WorkerRuntimePoll, WorkerRuntimeResultOwner,\n};",
    "use maleicacid_tuner_hal2_control_core::{\n    WorkerHandle, WorkerRuntime, WorkerRuntimeOwnerFailure, WorkerRuntimePoll,\n};",
)
replace_once(
    thread_owner,
    "    owner: WorkerRuntimeResultOwner<T, HalError>,",
    "    owner: WorkerHandle<T, HalError>,",
)
replace_once(
    thread_owner,
    "        let owner = WorkerRuntimeResultOwner::start(name.to_owned(), run).map_err(|error| {",
    "        let owner = WorkerRuntime::spawn_handle(name.to_owned(), run).map_err(|error| {",
)

# S-03: reaper/supervisor handles are no longer independently constructible.
cleanup = ROOT / "aidl_service/src/cleanup_reaper.rs"
replace_once(
    cleanup,
    "    let owner = maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue::start(\n",
    "    let owner = maleicacid_tuner_hal2_service_runtime::WorkerRuntime::start_reaper_queue(\n",
)

frontend_worker = ROOT / "service_runtime/src/frontend_worker_txn.rs"
replace_once(
    frontend_worker,
    "use crate::worker_runtime::{WorkerRuntimeReaperQueue, WorkerTerminalResult};",
    "use crate::worker_runtime::{WorkerRuntime, WorkerRuntimeReaperQueue, WorkerTerminalResult};",
)
replace_once(
    frontend_worker,
    "            runtime: WorkerRuntimeReaperQueue::start(\n",
    "            runtime: WorkerRuntime::start_reaper_queue(\n",
)

dvr = ROOT / "aidl_service/src/dvr_callback_delivery.rs"
replace_once(
    dvr,
    "    DvrStatusNotifierCleanupDiagnosticRecord, DvrStatusPollSnapshot, WorkerRuntime,\n    WorkerRuntimeSupervisor,\n",
    "    join_worker_classified, DvrStatusNotifierCleanupDiagnosticRecord, DvrStatusPollSnapshot,\n    WorkerRuntime, WorkerRuntimeSupervisor,\n",
)
replace_once(
    dvr,
    "    match notifier.worker.join_classified() {",
    "    match join_worker_classified(notifier.worker) {",
)
replace_once(
    dvr,
    "            runtime: WorkerRuntimeSupervisor::new(\n",
    "            runtime: WorkerRuntime::supervisor(\n",
)
replace_once(dvr, "        if state\n            .active\n            .get(&key)", "        if state\n            .active()\n            .get(&key)")
replace_once(dvr, "            let next_wait = state\n                .reaping\n                .values()", "            let next_wait = state\n                .reaping()\n                .values()")
replace_once(
    dvr,
    "                supervisor.wake.notify_one();",
    "                supervisor.runtime.wake().notify_one();",
)

# Keep service-runtime Android dependency graph honest: the worker_runtime module now
# directly re-exports control-core types. This dependency was also already required
# by the previous physical-owner implementation but had not been declared in Soong.
android_bp = ROOT / "Android.bp"
text = android_bp.read_text()
service_anchor = '''        "libmaleicacid_tuner_hal2_common",\n        "libmaleicacid_tuner_hal2_binder_adapter",'''
if text.count(service_anchor) < 1:
    raise SystemExit("service rustlibs anchor missing")
# Restrict replacement to the service_runtime module by locating its block.
start = text.index('rust_library {\n    name: "libmaleicacid_tuner_hal2_service_runtime"')
end = text.index('\n}\n\n\nrust_library {\n    name: "libmaleicacid_tuner_hal2_domain_request"', start)
block = text[start:end]
if '"libmaleicacid_tuner_hal2_control_core"' not in block:
    block = block.replace(
        '        "libmaleicacid_tuner_hal2_common",\n',
        '        "libmaleicacid_tuner_hal2_common",\n        "libmaleicacid_tuner_hal2_control_core",\n',
        1,
    )
    text = text[:start] + block + text[end:]
android_bp.write_text(text)

service_lib = ROOT / "service_runtime/src/lib.rs"
replace_once(
    service_lib,
    "pub use worker_runtime::{\n    WorkerHandle, WorkerRuntime, WorkerRuntimeReaperQueue, WorkerRuntimeSupervisor,\n    CLEANUP_RETRY_SCHEDULE_MS,\n",
    "pub use worker_runtime::{\n    join_worker_classified, WorkerHandle, WorkerRuntime, WorkerRuntimeReaperQueue,\n    WorkerRuntimeSupervisor, WorkerTerminalResult, CLEANUP_RETRY_SCHEDULE_MS,\n",
)

# S-12: fixed-power orchestration/mutation authority belongs to FrontendTuneScanTxn.
# Remove the second mutation surface from FrontendTxn.
frontend_txn = ROOT / "service_runtime/src/boot/frontend_txn.rs"
regex_once(
    frontend_txn,
    r"impl<'a> FrontendTxn<'a> \{\n    pub\(crate\) fn retain_fixed_power_lease\(.*?\n    pub\(crate\) fn is_stable_locked_tune_reentry\(",
    "impl<'a> FrontendTxn<'a> {\n    pub(crate) fn is_stable_locked_tune_reentry(",
    flags=re.S,
)

lnb_ops = ROOT / "service_runtime/src/lnb_ops.rs"
regex_once(
    lnb_ops,
    r"#\[derive\(Debug, Eq, PartialEq\)\]\n#\[must_use = \"frontend fixed-power preparation must be completed or rolled back by value\"\]\npub\(crate\) struct FrontendFixedPowerPreparation \{.*?\n\}\n\nimpl TunerServiceRuntime \{",
    "impl TunerServiceRuntime {",
    flags=re.S,
)
regex_once(
    lnb_ops,
    r"fn restore_fixed_power_lease_after_failure\(.*?\nfn live_lnb_io_authority\(",
    "fn live_lnb_io_authority(",
    flags=re.S,
)
# These imports were owned only by the removed fixed-power orchestration.
replace_once(
    lnb_ops,
    "use crate::registry::{\n    FrontendRuntimeId, LnbPhysicalIoAuthority, LnbRuntimeId, SatellitePowerTopology,\n};",
    "use crate::registry::{LnbPhysicalIoAuthority, LnbRuntimeId};",
)
replace_once(
    lnb_ops,
    "use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};",
    "use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};",
)

frontend_ops = ROOT / "service_runtime/src/frontend_ops.rs"
replace_once(
    frontend_ops,
    "use crate::registry::FrontendRuntimeId;",
    "use crate::registry::{FrontendRuntimeId, LnbRuntimeId, SatellitePowerTopology};",
)
replace_once(
    frontend_ops,
    "    compose_primary_cleanup_failure, FrontendScanMode, FrontendTuneRequest, HalError,\n    HalInternalKind,\n",
    "    compose_primary_cleanup_failure, FrontendScanMode, FrontendTuneRequest, HalError,\n    HalInternalKind, LnbVoltageRequest,\n",
)
replace_once(
    frontend_ops,
    "pub struct FrontendTuneScanTxn;\n\nimpl FrontendTuneScanTxn {",
    '''pub struct FrontendTuneScanTxn;\n\n#[derive(Debug, Eq, PartialEq)]\n#[must_use = "frontend fixed-power preparation must be completed or rolled back by value"]\nstruct FrontendFixedPowerPreparation {\n    frontend_id: FrontendRuntimeId,\n    newly_retained: bool,\n}\n\nimpl FrontendFixedPowerPreparation {\n    const fn frontend_id(&self) -> FrontendRuntimeId {\n        self.frontend_id\n    }\n\n    const fn newly_retained(&self) -> bool {\n        self.newly_retained\n    }\n}\n\nimpl FrontendTuneScanTxn {''',
)
replace_once(
    frontend_ops,
    "        let fixed_power = crate::lnb_ops::ensure_frontend_fixed_power_for_object(\n",
    "        let fixed_power = Self::ensure_frontend_fixed_power_for_object(\n",
)
replace_once(
    frontend_ops,
    "        let fixed_power = crate::lnb_ops::ensure_frontend_fixed_power_for_object(\n",
    "        let fixed_power = Self::ensure_frontend_fixed_power_for_object(\n",
)
replace_once(
    frontend_ops,
    "        preparation: crate::lnb_ops::FrontendFixedPowerPreparation,",
    "        preparation: FrontendFixedPowerPreparation,",
)
replace_once(
    frontend_ops,
    "        match crate::lnb_ops::release_frontend_fixed_power_after_operation(\n            runtime,\n            preparation.frontend_id(),\n        ) {",
    "        match Self::release_frontend_fixed_power_after_operation(\n            runtime,\n            preparation.frontend_id(),\n        ) {",
)
replace_once(
    frontend_ops,
    "            crate::lnb_ops::release_frontend_fixed_power_after_operation(runtime, frontend_id)?;",
    "            Self::release_frontend_fixed_power_after_operation(runtime, frontend_id)?;",
)

fixed_power_methods = r'''
    fn restore_fixed_power_lease_after_failure(
        runtime: &mut TunerServiceRuntime,
        frontend_id: FrontendRuntimeId,
        lnb_id: LnbRuntimeId,
        primary: HalError,
    ) -> HalError {
        match runtime
            .registry_mut()
            .retain_frontend_fixed_power_lease(frontend_id, lnb_id)
        {
            Ok(_) => primary,
            Err(cleanup) => compose_primary_cleanup_failure(
                "fixed LNB power failure and rail lease restoration both failed",
                primary,
                cleanup,
            ),
        }
    }

    fn rollback_new_fixed_power_lease(
        runtime: &mut TunerServiceRuntime,
        frontend_id: FrontendRuntimeId,
        newly_retained: bool,
        primary: HalError,
    ) -> HalError {
        if !newly_retained {
            return primary;
        }
        match runtime
            .registry_mut()
            .release_frontend_fixed_power_lease(frontend_id)
        {
            Ok(Some(_)) => primary,
            Ok(None) => compose_primary_cleanup_failure(
                "fixed LNB power preparation failed after its rail lease disappeared",
                primary,
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "new fixed-power lease was missing during rollback",
                ),
            ),
            Err(cleanup) => compose_primary_cleanup_failure(
                "fixed LNB power preparation and rail lease rollback both failed",
                primary,
                cleanup,
            ),
        }
    }

    fn ensure_frontend_fixed_power_for_object(
        runtime: &SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
    ) -> Result<FrontendFixedPowerPreparation, HalError> {
        let (frontend_id, lnb_id, authority) = {
            let guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned during fixed-power preflight",
                )
            })?;
            let frontend = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
            let frontend_id = frontend.id;
            if frontend.satellite_power_topology != SatellitePowerTopology::InternalFixed15V {
                return Ok(FrontendFixedPowerPreparation {
                    frontend_id,
                    newly_retained: false,
                });
            }
            let lnb_id = guard
                .registry()
                .lnb_for_frontend(frontend_id)
                .map(|entry| entry.id)
                .ok_or(HalError::Unsupported(
                    "internal fixed-15V frontend has no registered LNB rail",
                ))?;
            let authority = guard
                .registry()
                .lnb_physical_io_authority(lnb_id)
                .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
            (frontend_id, lnb_id, authority)
        };

        authority.execute(|permit| {
            let (prepared, newly_retained) = {
                let mut guard = runtime.lock().map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned during fixed-power preparation",
                    )
                })?;
                let current = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
                if current.id != frontend_id
                    || current.satellite_power_topology != SatellitePowerTopology::InternalFixed15V
                {
                    return Err(HalError::invalid_state(
                        maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                        "frontend fixed-power topology changed before rail preparation",
                    ));
                }
                let newly_retained = guard
                    .registry_mut()
                    .retain_frontend_fixed_power_lease(frontend_id, lnb_id)?;
                let already_applied = guard.registry().lnb_runtime(lnb_id).is_some_and(|lnb| {
                    lnb.state() == maleicacid_tuner_hal2_lnb::LnbRuntimeState::Open
                        && lnb.registry_state().voltage
                            == maleicacid_tuner_hal2_lnb::LnbVoltage::Voltage15V
                });
                if already_applied {
                    return Ok(FrontendFixedPowerPreparation {
                        frontend_id,
                        newly_retained,
                    });
                }
                if guard.registry().lnb_runtime(lnb_id).is_some_and(|lnb| {
                    lnb.state() == maleicacid_tuner_hal2_lnb::LnbRuntimeState::Closed
                }) {
                    if let Err(error) = guard
                        .registry_mut()
                        .reopen_lnb(lnb_id)
                        .map_err(crate::boot::lnb_txn::map_lnb_failure)
                    {
                        return Err(Self::rollback_new_fixed_power_lease(
                            &mut guard,
                            frontend_id,
                            newly_retained,
                            error,
                        ));
                    }
                }
                let prepared = match guard.lnb_control_txn().prepare_internal_fixed_15v(lnb_id.0) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        return Err(Self::rollback_new_fixed_power_lease(
                            &mut guard,
                            frontend_id,
                            newly_retained,
                            error,
                        ));
                    }
                };
                (prepared, newly_retained)
            };

            let completed = prepared.execute(&permit);
            let backend_result = completed.backend_result();
            let finish_result = runtime
                .lock()
                .map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while finishing fixed power",
                    )
                })?
                .lnb_control_txn()
                .finish(completed);
            match finish_result {
                Ok(()) => Ok(FrontendFixedPowerPreparation {
                    frontend_id,
                    newly_retained,
                }),
                Err(error)
                    if matches!(
                        backend_result,
                        maleicacid_tuner_hal2_lnb::LnbBackendApplyOutcome::Rejected(_)
                    ) =>
                {
                    let mut guard = runtime.lock().map_err(|_| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "service runtime lock poisoned while rolling back fixed power",
                        )
                    })?;
                    Err(Self::rollback_new_fixed_power_lease(
                        &mut guard,
                        frontend_id,
                        newly_retained,
                        error,
                    ))
                }
                Err(error) => Err(error),
            }
        })
    }

    fn release_frontend_fixed_power_after_operation(
        runtime: &SharedFrontendRuntime,
        frontend_id: FrontendRuntimeId,
    ) -> Result<(), HalError> {
        let (lnb_id, authority) = {
            let guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned during fixed-power release preflight",
                )
            })?;
            let Some(lnb_id) = guard.registry().frontend_fixed_power_lnb(frontend_id) else {
                return Ok(());
            };
            let authority = guard
                .registry()
                .lnb_physical_io_authority(lnb_id)
                .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
            (lnb_id, authority)
        };

        authority.execute(|permit| {
            let prepared = {
                let mut guard = runtime.lock().map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned during fixed-power release",
                    )
                })?;
                if guard.registry().frontend_fixed_power_lnb(frontend_id) != Some(lnb_id) {
                    return Ok(());
                }
                let operation_is_terminal = guard
                    .registry()
                    .frontend_runtime(frontend_id)
                    .map(|frontend| {
                        matches!(
                            frontend.snapshot().state,
                            FrontendRuntimeState::Idle
                                | FrontendRuntimeState::Closing
                                | FrontendRuntimeState::Failed
                        )
                    })
                    .unwrap_or(true);
                if !operation_is_terminal {
                    return Ok(());
                }
                let state_is_safe = guard.registry().lnb_runtime(lnb_id).is_some_and(|lnb| {
                    lnb.registry_state() == maleicacid_tuner_hal2_lnb::LnbElectricalState::safe()
                });
                let remaining = match guard
                    .registry_mut()
                    .release_frontend_fixed_power_lease(frontend_id)?
                {
                    Some((released_lnb_id, remaining)) if released_lnb_id == lnb_id => remaining,
                    Some(_) => {
                        return Err(HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "fixed-power release changed physical LNB identity",
                        ));
                    }
                    None => return Ok(()),
                };
                if remaining != 0 || state_is_safe {
                    return Ok(());
                }
                match guard
                    .lnb_control_txn()
                    .prepare_voltage(lnb_id.0, LnbVoltageRequest::None)
                {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        return Err(Self::restore_fixed_power_lease_after_failure(
                            &mut guard,
                            frontend_id,
                            lnb_id,
                            error,
                        ));
                    }
                }
            };

            let completed = prepared.execute(&permit);
            match runtime
                .lock()
                .map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while finishing fixed-power release",
                    )
                })?
                .lnb_control_txn()
                .finish(completed)
            {
                Ok(()) => Ok(()),
                Err(error) => {
                    let mut guard = runtime.lock().map_err(|_| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "service runtime lock poisoned while restoring fixed-power lease",
                        )
                    })?;
                    Err(Self::restore_fixed_power_lease_after_failure(
                        &mut guard,
                        frontend_id,
                        lnb_id,
                        error,
                    ))
                }
            }
        })
    }

'''
replace_once(
    frontend_ops,
    "    fn preflight_begin(\n",
    fixed_power_methods + "    fn preflight_begin(\n",
)

# WorkerRuntime design anchor: one physical generic owner in control-core; service
# worker_runtime is only the service failure-classification/re-export boundary.
design = ROOT / "DESIGN_JA.md"
text = design.read_text()
old_row = next(
    line for line in text.splitlines() if line.startswith("| `WorkerRuntime` |")
)
new_row = "| `WorkerRuntime` | `control/src/lib.rs::{WorkerRuntime, WorkerHandle, WorkerRuntimeReaperQueue, WorkerRuntimeSupervisor}` がgeneric worker生成・停止・wake・join/result-completionと、同ownerが発行するbounded reaper/pending/supervisor従属handleの唯一の物理canonical A state owner。`service_runtime/src/worker_runtime.rs`は同型のre-export、service failure-classification接続、product定数だけを持ちpersistent generic stateを所有しない。`WorkerHandle` / reaper / supervisorはconstructorを公開せず`WorkerRuntime`のtyped factoryからのみ発行し、独自generation/retry/reaper正本を持たない。domain固有stop ticket群のpoll/wait、domain completion/deadline actionなど、1件のWorkerRuntime-managed job実行中だけ存在して外部呼出し越しの別registry/queue/retry正本を形成しないcall-local進行状態はdomain typed jobに保持してよい | 各domain worker ownerの`WorkerRuntime`正規入口。必要な場合に同ownerが発行・管理するopaque従属handleを使用する | 従属handleによる独立したgeneration / retry / reaper state所有、別generic lifecycle owner、domain start/stop ownerの吸収 |"
text = text.replace(old_row, new_row, 1)
design.write_text(text)

# Structural assertions for the three residual comments.
control_text = control.read_text()
if "pub struct WorkerRuntimeResultOwner" in control_text:
    raise SystemExit("S-04 old result owner remains")
if "pub fn start(" in control_text.split("pub struct WorkerHandle", 1)[1].split("pub enum WorkerTerminalResult", 1)[0]:
    raise SystemExit("S-04 WorkerHandle still exposes a constructor")
if "WorkerRuntime::spawn_handle" not in thread_owner.read_text():
    raise SystemExit("S-04 device adapter does not use canonical WorkerRuntime")
if "WorkerRuntimeReaperQueue::start" in cleanup.read_text():
    raise SystemExit("S-03 cleanup reaper bypass remains")
if "WorkerRuntimeReaperQueue::start" in frontend_worker.read_text():
    raise SystemExit("S-03 frontend reaper bypass remains")
if "pub(crate) fn retain_fixed_power_lease" in frontend_txn.read_text():
    raise SystemExit("S-12 FrontendTxn fixed-power mutation entry remains")
if "frontend_txn().retain_fixed_power_lease" in lnb_ops.read_text() or "frontend_txn().release_fixed_power_lease" in lnb_ops.read_text():
    raise SystemExit("S-12 lnb_ops still mutates through second owner")
if "fn ensure_frontend_fixed_power_for_object" not in frontend_ops.read_text():
    raise SystemExit("S-12 canonical fixed-power implementation missing")

print("PR53 owner residual repair applied")
