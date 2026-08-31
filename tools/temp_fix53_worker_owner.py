from pathlib import Path


def replace_once(path, old, new):
    p = Path(path)
    s = p.read_text()
    count = s.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, got {count}: {old[:100]!r}")
    p.write_text(s.replace(old, new, 1))

# Generic worker lifecycle state belongs to worker_runtime, not frontend/DVR domains.
p = Path("tuner_hal2/service_runtime/src/worker_runtime.rs")
s = p.read_text()
s = s.replace(
    "use std::panic::{catch_unwind, AssertUnwindSafe};\n",
    "use std::collections::BTreeMap;\nuse std::panic::{catch_unwind, AssertUnwindSafe};\nuse std::sync::mpsc::{self, SyncSender, TrySendError};\nuse std::sync::{Condvar, Mutex, MutexGuard};\nuse std::time::Duration;\n",
    1,
)
anchor = "pub const WORKER_REAPER_DEADLINE_MS: u64 = 10_000;\n"
addition = anchor + r'''

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
'''
if s.count(anchor) != 1:
    raise SystemExit("worker runtime insertion anchor mismatch")
s = s.replace(anchor, addition, 1)
p.write_text(s)

# Export canonical supervisor primitives for the AIDL crate.
p = Path("tuner_hal2/service_runtime/src/lib.rs")
s = p.read_text()
s = s.replace(
    "    WorkerHandle, WorkerRuntime, CLEANUP_RETRY_SCHEDULE_MS,\n",
    "    WorkerHandle, WorkerRuntime, WorkerRuntimeReaperQueue, WorkerRuntimeSupervisor,\n    CLEANUP_RETRY_SCHEDULE_MS,\n",
    1,
)
p.write_text(s)

# Frontend: remove domain-owned channel/pending-map state; wrapper carries only
# canonical WorkerRuntime reaper queue plus domain state interpretation.
p = Path("tuner_hal2/service_runtime/src/frontend_worker_txn.rs")
s = p.read_text()
s = s.replace("use std::collections::BTreeMap;\n", "use std::collections::BTreeMap;\n", 1)
s = s.replace("use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};\n", "", 1)
s = s.replace(
    "use crate::worker_runtime::WorkerTerminalResult;\n",
    "use crate::worker_runtime::{WorkerRuntimeReaperQueue, WorkerTerminalResult};\n",
    1,
)
start = s.index("#[derive(Clone)]\npub(crate) struct FrontendWorkerReaperHandle")
end = s.index("fn ensure_frontend_worker_reaper", start)
new_block = r'''#[derive(Clone)]
pub(crate) struct FrontendWorkerReaperHandle {
    runtime: WorkerRuntimeReaperQueue<
        (i32, FrontendWorkerKind),
        Option<FrontendWorkerKind>,
        FrontendWorkerReaperJob,
    >,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrontendWorkerReaperPendingState {
    NotPending,
    CleanupOnly,
    Replacement(FrontendWorkerKind),
}

impl core::fmt::Debug for FrontendWorkerReaperHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrontendWorkerReaperHandle").finish()
    }
}

impl FrontendWorkerReaperHandle {
    fn start(
        runtime: Weak<Mutex<TunerServiceRuntime>>,
        capacity: usize,
        deadline: Duration,
    ) -> Result<Self, HalError> {
        let runner = Arc::new(move |
            job: FrontendWorkerReaperJob,
            pending: Arc<Mutex<BTreeMap<(i32, FrontendWorkerKind), Option<FrontendWorkerKind>>>>,
        | {
            job.run(&runtime, pending.as_ref(), deadline);
        });
        Ok(Self {
            runtime: WorkerRuntimeReaperQueue::start(
                capacity,
                "maleicacid-frontend-reaper",
                runner,
            )?,
        })
    }

    fn enqueue(&self, job: FrontendWorkerReaperJob) -> Result<(), HalError> {
        let continuation = job.continuation_kind;
        let reservations = job
            .keys
            .iter()
            .copied()
            .map(|key| (key, continuation))
            .collect::<Vec<_>>();
        self.runtime.enqueue_reserved(job, reservations)
    }

    fn is_pending(&self, frontend_id: i32, kind: FrontendWorkerKind) -> Result<bool, HalError> {
        self.runtime
            .pending_value(&(frontend_id, kind))
            .map(|value| value.is_some())
    }

    fn pending_state(
        &self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Result<FrontendWorkerReaperPendingState, HalError> {
        self.runtime
            .pending_value(&(frontend_id, kind))
            .map(|pending| match pending {
                None => FrontendWorkerReaperPendingState::NotPending,
                Some(None) => FrontendWorkerReaperPendingState::CleanupOnly,
                Some(Some(kind)) => FrontendWorkerReaperPendingState::Replacement(kind),
            })
    }
}

'''
s = s[:start] + new_block + s[end:]
p.write_text(s)

# DVR: generic active/reaping maps, capacity/deadline and wake condition live in
# WorkerRuntimeSupervisor. Domain wrapper keeps only DVR-specific transitions.
p = Path("tuner_hal2/aidl_service/src/dvr_callback_delivery.rs")
s = p.read_text()
s = s.replace("use std::collections::BTreeMap;\n", "", 1)
s = s.replace(
    "    ClassifiedWorkerTerminalResult, DvrStatusPollSnapshot, WorkerRuntime,\n",
    "    ClassifiedWorkerTerminalResult, DvrStatusPollSnapshot, WorkerRuntime,\n    WorkerRuntimeSupervisor,\n",
    1,
)
state_start = s.index("#[derive(Default)]\nstruct DvrStatusNotifierSupervisorState")
state_end = s.index("enum DvrStatusNotifierSupervisorAction", state_start)
s = s[:state_start] + s[state_end:]
old_struct = """pub(crate) struct DvrStatusNotifierSupervisor {
    capacity: usize,
    deadline: Duration,
    state: Mutex<DvrStatusNotifierSupervisorState>,
    wake: Condvar,
}
"""
new_struct = """pub(crate) struct DvrStatusNotifierSupervisor {
    runtime: WorkerRuntimeSupervisor<
        DvrStatusNotifierKey,
        DvrStatusNotifier,
        DvrStatusNotifierReaperJob,
    >,
}
"""
if s.count(old_struct) != 1:
    raise SystemExit("DVR supervisor struct anchor mismatch")
s = s.replace(old_struct, new_struct, 1)
old_new = """        Self {
            capacity: snapshot.cleanup_reaper_capacity.max(1),
            deadline: Duration::from_millis(snapshot.worker_reaper_deadline_ms),
            state: Mutex::new(DvrStatusNotifierSupervisorState::default()),
            wake: Condvar::new(),
        }
"""
new_new = """        Self {
            runtime: WorkerRuntimeSupervisor::new(
                snapshot.cleanup_reaper_capacity,
                Duration::from_millis(snapshot.worker_reaper_deadline_ms),
            ),
        }
"""
if s.count(old_new) != 1:
    raise SystemExit("DVR supervisor constructor anchor mismatch")
s = s.replace(old_new, new_new, 1)
s = s.replace("self.state.lock()", "self.runtime.state().lock()")
s = s.replace("self.wake.notify_one()", "self.runtime.wake().notify_one()")
s = s.replace("self.wake.notify_all()", "self.runtime.wake().notify_all()")
s = s.replace("self.deadline", "self.runtime.deadline()")
s = s.replace("self.capacity", "self.runtime.capacity()")
s = s.replace("state.active.len().saturating_add(state.reaping.len())", "state.total_len()")
# All map accesses go through canonical supervisor-map typed entries.
s = s.replace("state.active", "state.active_mut()")
s = s.replace("state.reaping", "state.reaping_mut()")
# Condvar wait sites.
s = s.replace("self.wake\n                        .wait_timeout", "self.runtime.wake()\n                        .wait_timeout")
s = s.replace("None => self.wake.wait(state)", "None => self.runtime.wake().wait(state)")
# Imports no longer own Mutex/Condvar; Mutex is still used elsewhere for service context? Check later via compiler.
s = s.replace("use std::sync::{Arc, Condvar, Mutex, Weak};", "use std::sync::{Arc, Weak};")
p.write_text(s)

# DESIGN: preserve sole-owner invariant and pin its physical implementation.
p = Path("tuner_hal2/DESIGN_JA.md")
s = p.read_text()
old = "`WorkerRuntime` は generic worker lifecycle の唯一の canonical A-state owner"
if old in s and "WorkerRuntimeReaperQueue" not in s:
    s = s.replace(
        old,
        "`WorkerRuntime` は generic worker lifecycle の唯一の canonical A-state owner（`worker_runtime.rs` の `WorkerRuntime` / `WorkerRuntimeReaperQueue` / `WorkerRuntimeSupervisor` を同一owner実装面とし、domain側はtyped job/outcomeだけを所有する）",
        1,
    )
p.write_text(s)
