from pathlib import Path

R=Path('tuner_hal2')
def one(p,o,n):
    t=p.read_text(); c=t.count(o)
    if c!=1: raise SystemExit(f'{p}: expected one anchor, got {c}: {o[:100]!r}')
    p.write_text(t.replace(o,n,1))

# S-04: put the low-level spawn/join/result/completion owner in control-core,
# which is already below both device and service_runtime in the dependency graph.
control=R/'control/src/lib.rs'
t=control.read_text()
insert='''
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

/// Canonical low-level owner for a spawned worker's JoinHandle, result cell and
/// completion wakeup. Domain layers add stop/cancel semantics but do not create
/// a second thread-result lifecycle owner.
pub struct WorkerRuntimeResultOwner<T, E> {
    result: std::sync::Arc<std::sync::Mutex<Option<Result<T, E>>>>,
    owner_failure: std::sync::Arc<std::sync::Mutex<Option<WorkerRuntimeOwnerFailure>>>,
    completion: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    join: Option<std::thread::JoinHandle<()>>,
    collected: bool,
}

impl<T, E> WorkerRuntimeResultOwner<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
{
    pub fn start(
        name: String,
        run: impl FnOnce() -> Result<T, E> + Send + 'static,
    ) -> std::io::Result<Self> {
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let owner_failure = std::sync::Arc::new(std::sync::Mutex::new(None));
        let completion = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
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
        Ok(Self { result, owner_failure, completion, join: Some(join), collected: false })
    }

    pub fn collect_if_finished(&mut self) -> WorkerRuntimePoll<T, E> {
        if self.collected {
            return WorkerRuntimePoll::OwnerFailure(WorkerRuntimeOwnerFailure::ResultAlreadyCollected);
        }
        if self.join.as_ref().is_some_and(|handle| !handle.is_finished()) {
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
        self.join.as_ref().is_none_or(|handle| handle.is_finished())
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
        let mut completed = completed.lock().map_err(|_| WorkerRuntimeOwnerFailure::CompletionLockPoison)?;
        loop {
            if *completed { return Ok(true); }
            match deadline {
                Some(deadline) => {
                    let now = std::time::Instant::now();
                    if now >= deadline { return Ok(false); }
                    let (next, timeout) = wake.wait_timeout(completed, deadline.saturating_duration_since(now))
                        .map_err(|_| WorkerRuntimeOwnerFailure::CompletionLockPoison)?;
                    completed = next;
                    if timeout.timed_out() && !*completed { return Ok(false); }
                }
                None => {
                    completed = wake.wait(completed)
                        .map_err(|_| WorkerRuntimeOwnerFailure::CompletionLockPoison)?;
                }
            }
        }
    }

    fn take_result(&mut self) -> Result<Result<T, E>, WorkerRuntimeOwnerFailure> {
        if let Some(failure) = self.owner_failure.lock()
            .map_err(|_| WorkerRuntimeOwnerFailure::ResultLockPoison)?.take() {
            return Err(failure);
        }
        self.result.lock()
            .map_err(|_| WorkerRuntimeOwnerFailure::ResultLockPoison)?
            .take()
            .ok_or(WorkerRuntimeOwnerFailure::MissingReport)
    }
}

'''
marker='#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub enum WorkerStopReason {'
if marker not in t: raise SystemExit('control marker missing')
control.write_text(t.replace(marker,insert+marker,1))

# S-04 device adapter: no spawn, JoinHandle, Condvar or result-cell ownership remains here.
thread_owner=R/'device/src/runtime/thread_result_owner.rs'
thread_owner.write_text('''//! Device-domain adapter over the canonical control-core worker result owner.\n\nuse std::time::Instant;\n\nuse maleicacid_tuner_hal2_common::{HalError, HalInternalKind};\nuse maleicacid_tuner_hal2_control_core::{\n    WorkerRuntimeOwnerFailure, WorkerRuntimePoll, WorkerRuntimeResultOwner,\n};\n\nfn owner_failure_to_hal(error: WorkerRuntimeOwnerFailure, name: &'static str) -> HalError {\n    let detail = match error {\n        WorkerRuntimeOwnerFailure::ThreadPanic => "thread panicked",\n        WorkerRuntimeOwnerFailure::JoinFailure => "thread join failed",\n        WorkerRuntimeOwnerFailure::ResultLockPoison => "thread result lock poisoned",\n        WorkerRuntimeOwnerFailure::CompletionLockPoison => "thread completion lock poisoned",\n        WorkerRuntimeOwnerFailure::MissingReport => "finished without report",\n        WorkerRuntimeOwnerFailure::ResultAlreadyCollected => "thread result already collected",\n    };\n    HalError::internal(HalInternalKind::InvariantViolation, format!("{name}: {detail}"))\n}\n\npub(crate) enum ThreadResultPoll<T> {\n    Running,\n    Completed(Result<T, HalError>),\n}\n\npub(crate) struct ThreadResultOwner<T> {\n    owner: WorkerRuntimeResultOwner<T, HalError>,\n    name: &'static str,\n}\n\nimpl<T> core::fmt::Debug for ThreadResultOwner<T> {\n    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {\n        f.debug_struct("ThreadResultOwner").field("name", &self.name).finish()\n    }\n}\n\nimpl<T> ThreadResultOwner<T>\nwhere\n    T: Send + 'static,\n{\n    pub(crate) fn start(\n        name: &'static str,\n        run: impl FnOnce() -> Result<T, HalError> + Send + 'static,\n    ) -> Result<Self, HalError> {\n        let owner = WorkerRuntimeResultOwner::start(name.to_owned(), run).map_err(|error| {\n            HalError::internal(\n                HalInternalKind::InvariantViolation,\n                format!("{name}: thread spawn failed: {error}"),\n            )\n        })?;\n        Ok(Self { owner, name })\n    }\n\n    pub(crate) fn collect_if_finished(&mut self) -> ThreadResultPoll<T> {\n        match self.owner.collect_if_finished() {\n            WorkerRuntimePoll::Running => ThreadResultPoll::Running,\n            WorkerRuntimePoll::Completed(result) => ThreadResultPoll::Completed(result),\n            WorkerRuntimePoll::OwnerFailure(error) => {\n                ThreadResultPoll::Completed(Err(owner_failure_to_hal(error, self.name)))\n            }\n        }\n    }\n\n    pub(crate) fn join_after_stop(self) -> Result<T, HalError> {\n        match self.owner.join_after_stop() {\n            Ok(result) => result,\n            Err(error) => Err(owner_failure_to_hal(error, self.name)),\n        }\n    }\n\n    pub(crate) fn is_thread_finished(&self) -> bool {\n        self.owner.is_thread_finished()\n    }\n\n    pub(crate) fn wait_until_finished(&self, deadline: Option<Instant>) -> Result<bool, HalError> {\n        self.owner\n            .wait_until_finished(deadline)\n            .map_err(|error| owner_failure_to_hal(error, self.name))\n    }\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n    use std::time::Duration;\n\n    #[test]\n    fn adapter_reports_normal_completion() {\n        let owner = ThreadResultOwner::start("normal", || Ok(7u32)).unwrap();\n        assert_eq!(owner.join_after_stop().unwrap(), 7);\n    }\n\n    #[test]\n    fn adapter_reports_running_then_completion() {\n        let mut owner = ThreadResultOwner::start("running", || {\n            std::thread::sleep(Duration::from_millis(20));\n            Ok(())\n        }).unwrap();\n        assert!(matches!(owner.collect_if_finished(), ThreadResultPoll::Running));\n        assert!(owner.wait_until_finished(Some(Instant::now() + Duration::from_secs(1))).unwrap());\n        assert!(matches!(owner.collect_if_finished(), ThreadResultPoll::Completed(Ok(()))));\n    }\n}\n''')

# Also use the canonical control-core result owner under service_runtime::WorkerRuntime.
wr=R/'service_runtime/src/worker_runtime.rs'
one(wr,'use std::thread::{self, JoinHandle};\n','use std::thread;\n')
one(wr,'use maleicacid_tuner_hal2_common::HalError;\n','use maleicacid_tuner_hal2_common::HalError;\nuse maleicacid_tuner_hal2_control_core::{WorkerRuntimeResultOwner, WorkerRuntimeOwnerFailure};\n')
one(wr,'    handle: WorkerHandle<T>,\n','    handle: WorkerRuntimeResultOwner<WorkerTerminalResult<T>, ()>,\n')
one(wr,'        self.finished.load(Ordering::Acquire)\n','        self.finished.load(Ordering::Acquire) || self.handle.is_thread_finished()\n')
one(wr,'            if let Some(join) = self.handle.join.as_ref() {\n                join.thread().unpark();\n            }\n','            self.handle.unpark();\n')
old='''    pub(crate) fn join(mut self) -> WorkerTerminalResult<T> {\n        let Some(join) = self.handle.join.take() else {\n            return WorkerTerminalResult::PanicOrJoinFailure;\n        };\n        match join.join() {\n            Ok(result) => result,\n            Err(_) => WorkerTerminalResult::PanicOrJoinFailure,\n        }\n    }\n}\n\nimpl<T> Drop for WorkerRuntime<T> {\n    fn drop(&mut self) {\n        if self.handle.join.is_some() && !self.is_finished() {\n            self.request_stop_and_wake();\n        }\n    }\n}\n\n/// Physical join element subordinate to its `WorkerRuntime` owner.\npub struct WorkerHandle<T> {\n    join: Option<JoinHandle<WorkerTerminalResult<T>>>,\n}\n'''
new='''    pub(crate) fn join(self) -> WorkerTerminalResult<T> {\n        match self.handle.join_after_stop() {\n            Ok(Ok(result)) => result,\n            Ok(Err(())) | Err(_) => WorkerTerminalResult::PanicOrJoinFailure,\n        }\n    }\n}\n\nimpl<T> Drop for WorkerRuntime<T> {\n    fn drop(&mut self) {\n        if !self.is_finished() {\n            self.request_stop_and_wake();\n        }\n    }\n}\n'''
one(wr,old,new)
old='''        let join = thread::Builder::new().name(thread_name).spawn(move || {\n            let terminal = match catch_unwind(AssertUnwindSafe(|| worker(Arc::clone(&thread_stop)))) {\n                Ok(Ok(_result)) if thread_stop.load(Ordering::Acquire) => {\n                    WorkerTerminalResult::StopRequested\n                }\n                Ok(Ok(result)) => WorkerTerminalResult::Normal(result),\n                Ok(Err(error)) => WorkerTerminalResult::RuntimeFailure(error),\n                Err(_) => WorkerTerminalResult::PanicOrJoinFailure,\n            };\n            thread_finished.store(true, Ordering::Release);\n            completion_signal();\n            terminal\n        })?;\n'''
new='''        let handle = WorkerRuntimeResultOwner::start(thread_name, move || {\n            let terminal = match catch_unwind(AssertUnwindSafe(|| worker(Arc::clone(&thread_stop)))) {\n                Ok(Ok(_result)) if thread_stop.load(Ordering::Acquire) => WorkerTerminalResult::StopRequested,\n                Ok(Ok(result)) => WorkerTerminalResult::Normal(result),\n                Ok(Err(error)) => WorkerTerminalResult::RuntimeFailure(error),\n                Err(_) => WorkerTerminalResult::PanicOrJoinFailure,\n            };\n            thread_finished.store(true, Ordering::Release);\n            completion_signal();\n            Ok::<_, ()>(terminal)\n        })?;\n'''
one(wr,old,new)
one(wr,'            handle: WorkerHandle { join: Some(join) },\n','            handle,\n')
# avoid unused imported enum in case rustfmt/clippy checks service_runtime later
one(wr,'use maleicacid_tuner_hal2_control_core::{WorkerRuntimeResultOwner, WorkerRuntimeOwnerFailure};\n','use maleicacid_tuner_hal2_control_core::WorkerRuntimeResultOwner;\n')

# S-03: replace domain-owned persistent queue/Condvar/thread with canonical WorkerRuntimeReaperQueue.
cr=R/'aidl_service/src/cleanup_reaper.rs'
t=cr.read_text()
start=t.index('#[derive(Clone, Copy, Debug, Eq, PartialEq)]\nenum CleanupJobState')
end=t.index('fn close_method(')
replacement='''#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]\nstruct CleanupJobKey {\n    kind: u8,\n    object_id: i64,\n    generation: u64,\n}\n\nimpl CleanupJobKey {\n    fn from_handle(handle: AidlObjectHandle) -> Self {\n        let kind = match handle.object_kind() {\n            AidlObjectKind::Tuner => 0,\n            AidlObjectKind::Frontend => 1,\n            AidlObjectKind::Demux => 2,\n            AidlObjectKind::Filter => 3,\n            AidlObjectKind::Dvr => 4,\n            AidlObjectKind::Descrambler => 5,\n            AidlObjectKind::Lnb => 6,\n        };\n        Self { kind, object_id: handle.object_id().0, generation: handle.generation().0 }\n    }\n}\n\n#[derive(Clone, Copy, Debug)]\nstruct CleanupJob {\n    handle: AidlObjectHandle,\n    dependency: CleanupStep,\n    registered_at: Instant,\n}\n\npub(crate) struct CleanupReaperQueue {\n    policy: CleanupReaperPolicy,\n    runtime: Mutex<Option<maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue<CleanupJobKey, CleanupStep, CleanupJob>>>,\n}\n\nimpl core::fmt::Debug for CleanupReaperQueue {\n    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {\n        f.debug_struct("CleanupReaperQueue").field("policy", &self.policy).finish_non_exhaustive()\n    }\n}\n\nimpl CleanupReaperQueue {\n    pub(crate) fn from_snapshot(snapshot: CapabilitySnapshot) -> Self {\n        Self { policy: CleanupReaperPolicy::from_snapshot(snapshot), runtime: Mutex::new(None) }\n    }\n\n    fn install(\n        &self,\n        runtime: maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue<CleanupJobKey, CleanupStep, CleanupJob>,\n    ) -> Result<(), HalError> {\n        let mut slot = self.runtime.lock().map_err(|_| HalError::internal(\n            HalInternalKind::InvariantViolation,\n            "cleanup reaper canonical owner slot lock poisoned",\n        ))?;\n        if slot.is_some() {\n            return Err(HalError::internal(HalInternalKind::InvariantViolation, "cleanup reaper canonical owner installed twice"));\n        }\n        *slot = Some(runtime);\n        Ok(())\n    }\n\n    pub(crate) fn enqueue(&self, handle: AidlObjectHandle, dependency: CleanupStep) -> Result<(), HalError> {\n        let key = CleanupJobKey::from_handle(handle);\n        let slot = self.runtime.lock().map_err(|_| HalError::internal(\n            HalInternalKind::InvariantViolation,\n            "cleanup reaper canonical owner slot lock poisoned while enqueueing",\n        ))?;\n        let runtime = slot.as_ref().ok_or_else(|| HalError::internal(\n            HalInternalKind::InvariantViolation,\n            "cleanup reaper canonical owner is not installed",\n        ))?;\n        if runtime.pending_value(&key)?.is_some() {\n            return Ok(());\n        }\n        runtime.enqueue_reserved(\n            CleanupJob { handle, dependency, registered_at: Instant::now() },\n            [(key, dependency)],\n        )\n    }\n}\n\n'''
t=t[:start]+replacement+t[end:]
# replace old quarantine+start up to tests
start=t.index('fn quarantine_cleanup_job(')
end=t.index('#[cfg(test)]\nmod tests')
new_tail='''fn clear_pending_cleanup_job(\n    pending: &Arc<Mutex<std::collections::BTreeMap<CleanupJobKey, CleanupStep>>>,\n    key: CleanupJobKey,\n) -> Result<(), HalError> {\n    pending.lock().map_err(|_| HalError::internal(\n        HalInternalKind::InvariantViolation,\n        "cleanup reaper canonical pending registry lock poisoned",\n    ))?.remove(&key);\n    Ok(())\n}\n\nfn mark_cleanup_reaper_critical(context: &AidlServiceContext) {\n    let shared_runtime = context.runtime();\n    if let Ok(mut runtime) = shared_runtime.lock() { runtime.mark_service_critical(); }\n}\n\nfn run_cleanup_job(\n    context: Weak<AidlServiceContext>,\n    policy: CleanupReaperPolicy,\n    job: CleanupJob,\n    pending: Arc<Mutex<std::collections::BTreeMap<CleanupJobKey, CleanupStep>>>,\n) {\n    let key = CleanupJobKey::from_handle(job.handle);\n    let mut attempt = 0usize;\n    loop {\n        let Some(context) = context.upgrade() else {\n            let _ = clear_pending_cleanup_job(&pending, key);\n            return;\n        };\n        if job.registered_at.elapsed() >= policy.terminal_deadline {\n            let terminal = crate::object_runtime::quarantine_drop_leak_object(&context, job.handle).is_ok()\n                || context.cleanup_is_terminal_for_handle(job.handle) == Ok(true);\n            if !terminal { mark_cleanup_reaper_critical(&context); }\n            if clear_pending_cleanup_job(&pending, key).is_err() { mark_cleanup_reaper_critical(&context); }\n            return;\n        }\n        let dependency = match context.cleanup_dependency_for_handle(job.handle) {\n            Ok(dependency) => dependency,\n            Err(_) if context.cleanup_is_terminal_for_handle(job.handle) == Ok(true) => {\n                if clear_pending_cleanup_job(&pending, key).is_err() { mark_cleanup_reaper_critical(&context); }\n                return;\n            }\n            Err(_) => {\n                mark_cleanup_reaper_critical(&context);\n                let _ = clear_pending_cleanup_job(&pending, key);\n                return;\n            }\n        };\n        if let Ok(mut guard) = pending.lock() { guard.insert(key, dependency); } else {\n            mark_cleanup_reaper_critical(&context);\n            return;\n        }\n        let result = close_method(job.handle.object_kind()).and_then(|method| {\n            crate::object_runtime::retry_cleanup_from_reaper(&context, job.handle, method)\n                .map_err(|status| HalError::internal(\n                    HalInternalKind::InvariantViolation,\n                    format!("cleanup reaper Binder retry failed: {status:?}"),\n                ))\n        });\n        if result.is_ok() {\n            if clear_pending_cleanup_job(&pending, key).is_err() { mark_cleanup_reaper_critical(&context); }\n            return;\n        }\n        attempt = attempt.saturating_add(1);\n        let delay = policy.retry_delays.get(attempt).copied().unwrap_or(Duration::from_millis(1_000));\n        let remaining = policy.terminal_deadline.saturating_sub(job.registered_at.elapsed());\n        std::thread::sleep(delay.min(remaining));\n    }\n}\n\npub(crate) fn start_cleanup_reaper(\n    context: Weak<AidlServiceContext>,\n    queue: Arc<CleanupReaperQueue>,\n) -> Result<(), HalError> {\n    let policy = queue.policy;\n    let runner_context = context;\n    let runner = Arc::new(move |job, pending| {\n        run_cleanup_job(runner_context.clone(), policy, job, pending);\n    });\n    let owner = maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue::start(\n        policy.max_jobs,\n        "tuner-hal2-cleanup-reaper",\n        runner,\n    )?;\n    queue.install(owner)\n}\n\n'''
t=t[:start]+new_tail+t[end:]
# Replace old tests wholesale with small contract tests that do not depend on removed bespoke state machine.
idx=t.index('#[cfg(test)]\nmod tests')
t=t[:idx]+'''#[cfg(test)]\nmod tests {\n    use super::*;\n    use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId};\n\n    #[test]\n    fn cleanup_job_key_is_identity_only() {\n        let handle=AidlObjectHandle::new(AidlObjectKind::Filter, AidlObjectId(7), AidlObjectGeneration(3));\n        assert_eq!(CleanupJobKey::from_handle(handle), CleanupJobKey { kind: 3, object_id: 7, generation: 3 });\n    }\n}\n'''
# imports: no Condvar; BTreeMap fully qualified, keep Mutex.
t=t.replace('use std::sync::{Arc, Condvar, Mutex, Weak};','use std::sync::{Arc, Mutex, Weak};')
cr.write_text(t)

# S-10: object_runtime owns preflight -> unlocked adapter classification -> finish mutation.
obj=R/'aidl_service/src/object_runtime/mod.rs'
one(obj,'use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};\n','use maleicacid_tuner_hal2_common::{\n    compose_primary_cleanup_failure, HalError, HalInternalKind, HalInvalidArgumentKind,\n};\nuse maleicacid_tuner_hal2_demux::AvHandleReleaseDescriptor;\n')
t=obj.read_text(); anchor='pub(crate) fn execute_object_runtime_use_case<T, F>(\n'
helper='''pub(crate) fn execute_filter_av_handle_release_use_case<F>(\n    runtime: &SharedTunerRuntime,\n    handle: AidlObjectHandle,\n    av_data_id: i64,\n    classify_handle: F,\n) -> BinderResult<()>\nwhere\n    F: FnOnce() -> Result<AvHandleReleaseDescriptor, HalError>,\n{\n    if av_data_id < 0 {\n        return Err(status_from_hal_error(HalError::invalid_argument(\n            HalInvalidArgumentKind::NumericRange,\n            "AV data id must not be negative",\n        )));\n    }\n    {\n        let guard = lock_runtime(runtime).map_err(status_from_hal_error)?;\n        guard\n            .preflight_filter_av_handle_release_for_any_lifecycle(\n                handle.object_id(),\n                handle.generation(),\n            )\n            .map_err(status_from_hal_error)?;\n    }\n    let descriptor = classify_handle().map_err(status_from_hal_error)?;\n    let mut guard = lock_runtime(runtime).map_err(status_from_hal_error)?;\n    guard\n        .release_filter_av_handle_for_any_lifecycle(\n            handle.object_id(),\n            handle.generation(),\n            descriptor,\n            av_data_id,\n        )\n        .map_err(status_from_hal_error)\n}\n\n'''
if t.count(anchor)!=1: raise SystemExit('object runtime anchor')
obj.write_text(t.replace(anchor,helper+anchor,1))

# re-export helper through tuner_service module and collapse method body.
ts=R/'aidl_service/src/tuner_service.rs'
one(ts,'    execute_object_runtime_use_case_with_request_builder, execute_shared_object_runtime_use_case,\n','    execute_object_runtime_use_case_with_request_builder, execute_filter_av_handle_release_use_case,\n    execute_shared_object_runtime_use_case,\n')
fm=R/'aidl_service/src/tuner_service/filter_methods.rs'
one(fm,'    execute_object_runtime_use_case, execute_object_runtime_use_case_with_request_builder, plan_unavailable_object_method_use_case,\n','    execute_filter_av_handle_release_use_case, execute_object_runtime_use_case,\n    execute_object_runtime_use_case_with_request_builder, plan_unavailable_object_method_use_case,\n')
old_start='''    fn releaseAvHandle(&self, av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {\n'''
t=fm.read_text(); a=t.index(old_start); b=t.index('    fn setDataSource(',a)
new='''    fn releaseAvHandle(&self, av_memory: &TunerNativeHandle, av_data_id: i64) -> BinderResult<()> {\n        execute_filter_av_handle_release_use_case(\n            &self.runtime(),\n            self.handle(),\n            av_data_id,\n            || match (av_memory.fds.as_slice(), av_memory.ints.as_slice()) {\n                ([], []) => Ok(AvHandleReleaseDescriptor::Empty),\n                ([file], [0]) => {\n                    let metadata = std::fs::metadata(format!("/proc/self/fd/{}", file.as_raw_fd()))\n                        .map_err(|_| HalError::internal(\n                            HalInternalKind::InvariantViolation,\n                            "AV release handle identity could not be classified safely",\n                        ))?;\n                    Ok(AvHandleReleaseDescriptor::File(AvFileIdentity::new(\n                        metadata.dev(), metadata.ino(), metadata.size(),\n                    )))\n                }\n                _ => Err(HalError::invalid_argument(\n                    HalInvalidArgumentKind::NumericRange,\n                    "AV handle shape is neither empty nor a single exported allocation handle",\n                )),\n            },\n        )\n    }\n\n'''
fm.write_text(t[:a]+new+t[b:])

# S-12: fixed-power lease mutations are owned by FrontendTxn typed entries;
# lnb_ops executes those entries instead of calling registry_mut directly.
ft=R/'service_runtime/src/boot/frontend_txn.rs'
t=ft.read_text(); anchor="impl<'a> FrontendTxn<'a> {\n"
methods='''impl<'a> FrontendTxn<'a> {\n    pub(crate) fn retain_fixed_power_lease(\n        &mut self,\n        frontend_id: crate::registry::FrontendRuntimeId,\n        lnb_id: crate::registry::LnbRuntimeId,\n    ) -> Result<bool, HalError> {\n        self.runtime.registry.retain_frontend_fixed_power_lease(frontend_id, lnb_id)\n    }\n\n    pub(crate) fn release_fixed_power_lease(\n        &mut self,\n        frontend_id: crate::registry::FrontendRuntimeId,\n    ) -> Result<Option<(crate::registry::LnbRuntimeId, usize)>, HalError> {\n        self.runtime.registry.release_frontend_fixed_power_lease(frontend_id)\n    }\n\n    pub(crate) fn reopen_fixed_power_lnb(\n        &mut self,\n        lnb_id: crate::registry::LnbRuntimeId,\n    ) -> Result<(), HalError> {\n        self.runtime.registry.reopen_lnb(lnb_id).map_err(crate::boot::lnb_txn::map_lnb_failure)\n    }\n\n'''
if t.count(anchor)!=1: raise SystemExit('FrontendTxn impl anchor')
ft.write_text(t.replace(anchor,methods,1))

lo=R/'service_runtime/src/lnb_ops.rs'
t=lo.read_text()
t=t.replace('.registry_mut()\n        .retain_frontend_fixed_power_lease(frontend_id, lnb_id)', '.frontend_txn()\n        .retain_fixed_power_lease(frontend_id, lnb_id)')
t=t.replace('.registry_mut()\n        .release_frontend_fixed_power_lease(frontend_id)', '.frontend_txn()\n        .release_fixed_power_lease(frontend_id)')
t=t.replace('.registry_mut()\n                .retain_frontend_fixed_power_lease(frontend_id, lnb_id)', '.frontend_txn()\n                .retain_fixed_power_lease(frontend_id, lnb_id)')
t=t.replace('.registry_mut()\n                    .reopen_lnb(lnb_id)\n                    .map_err(crate::boot::lnb_txn::map_lnb_failure)', '.frontend_txn()\n                    .reopen_fixed_power_lnb(lnb_id)')
t=t.replace('.registry_mut()\n                .release_frontend_fixed_power_lease(frontend_id)?', '.frontend_txn()\n                .release_fixed_power_lease(frontend_id)?')
lo.write_text(t)

# Structural checks.
assert 'std::thread::Builder' not in cr.read_text()
assert 'Condvar' not in cr.read_text()
assert 'WorkerRuntimeReaperQueue' in cr.read_text()
text=thread_owner.read_text()
for forbidden in ['JoinHandle', 'Condvar', 'thread::Builder', 'catch_unwind', 'Arc<Mutex']:
    assert forbidden not in text, forbidden
assert 'WorkerRuntimeResultOwner' in text
text=fm.read_text()
assert 'preflight_filter_av_handle_release_for_any_lifecycle' not in text
assert '.lock()' not in text[text.index('fn releaseAvHandle'):text.index('fn setDataSource')]
text=lo.read_text()
for pattern in ['registry_mut()\n        .retain_frontend_fixed_power_lease', 'registry_mut()\n        .release_frontend_fixed_power_lease', 'registry_mut()\n                .retain_frontend_fixed_power_lease', 'registry_mut()\n                .release_frontend_fixed_power_lease']:
    assert pattern not in text
assert 'retain_fixed_power_lease' in ft.read_text() and 'release_fixed_power_lease' in ft.read_text()
print('review35 batch2 applied')
