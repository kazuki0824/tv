use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_binder_adapter::AidlMethodCall;
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_domain_request::AidlObjectKind;
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
use maleicacid_tuner_hal2_service_runtime::CapabilitySnapshot;

use crate::object_handle::AidlObjectHandle;
use crate::service_context::AidlServiceContext;

#[derive(Clone, Copy, Debug)]
struct CleanupReaperPolicy {
    max_jobs: usize,
    terminal_deadline: Duration,
    retry_delays: [Duration; 4],
}

impl CleanupReaperPolicy {
    fn from_snapshot(snapshot: CapabilitySnapshot) -> Self {
        Self {
            max_jobs: snapshot.cleanup_reaper_capacity,
            terminal_deadline: Duration::from_millis(snapshot.cleanup_terminal_deadline_ms),
            retry_delays: snapshot
                .cleanup_retry_schedule_ms
                .map(Duration::from_millis),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CleanupJobKey {
    kind: u8,
    object_id: i64,
    generation: u64,
}

impl CleanupJobKey {
    fn from_handle(handle: AidlObjectHandle) -> Self {
        let kind = match handle.object_kind() {
            AidlObjectKind::Tuner => 0,
            AidlObjectKind::Frontend => 1,
            AidlObjectKind::Demux => 2,
            AidlObjectKind::Filter => 3,
            AidlObjectKind::Dvr => 4,
            AidlObjectKind::Descrambler => 5,
            AidlObjectKind::Lnb => 6,
        };
        Self {
            kind,
            object_id: handle.object_id().0,
            generation: handle.generation().0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CleanupJob {
    handle: AidlObjectHandle,
    dependency: CleanupStep,
    registered_at: Instant,
}

pub(crate) struct CleanupReaperQueue {
    policy: CleanupReaperPolicy,
    runtime: Mutex<
        Option<
            maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue<
                CleanupJobKey,
                CleanupStep,
                CleanupJob,
            >,
        >,
    >,
}

impl core::fmt::Debug for CleanupReaperQueue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CleanupReaperQueue")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl CleanupReaperQueue {
    pub(crate) fn from_snapshot(snapshot: CapabilitySnapshot) -> Self {
        Self {
            policy: CleanupReaperPolicy::from_snapshot(snapshot),
            runtime: Mutex::new(None),
        }
    }

    fn install(
        &self,
        runtime: maleicacid_tuner_hal2_service_runtime::WorkerRuntimeReaperQueue<
            CleanupJobKey,
            CleanupStep,
            CleanupJob,
        >,
    ) -> Result<(), HalError> {
        let mut slot = self.runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper canonical owner slot lock poisoned",
            )
        })?;
        if slot.is_some() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper canonical owner installed twice",
            ));
        }
        *slot = Some(runtime);
        Ok(())
    }

    pub(crate) fn enqueue(
        &self,
        handle: AidlObjectHandle,
        dependency: CleanupStep,
    ) -> Result<(), HalError> {
        let key = CleanupJobKey::from_handle(handle);
        let slot = self.runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper canonical owner slot lock poisoned while enqueueing",
            )
        })?;
        let runtime = slot.as_ref().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper canonical owner is not installed",
            )
        })?;
        if runtime.pending_value(&key)?.is_some() {
            return Ok(());
        }
        runtime.enqueue_reserved(
            CleanupJob {
                handle,
                dependency,
                registered_at: Instant::now(),
            },
            [(key, dependency)],
        )
    }
}

fn close_method(kind: AidlObjectKind) -> Result<AidlMethodCall, HalError> {
    match kind {
        AidlObjectKind::Frontend => Ok(AidlMethodCall::FrontendClose),
        AidlObjectKind::Demux => Ok(AidlMethodCall::DemuxClose),
        AidlObjectKind::Filter => Ok(AidlMethodCall::FilterClose),
        AidlObjectKind::Dvr => Ok(AidlMethodCall::DvrClose),
        AidlObjectKind::Descrambler => Ok(AidlMethodCall::DescramblerClose),
        AidlObjectKind::Lnb => Ok(AidlMethodCall::LnbClose),
        AidlObjectKind::Tuner => Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "root tuner object entered cleanup reaper queue",
        )),
    }
}

fn clear_pending_cleanup_job(
    pending: &Arc<Mutex<std::collections::BTreeMap<CleanupJobKey, CleanupStep>>>,
    key: CleanupJobKey,
) -> Result<(), HalError> {
    pending
        .lock()
        .map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper canonical pending registry lock poisoned",
            )
        })?
        .remove(&key);
    Ok(())
}

fn mark_cleanup_reaper_critical(context: &AidlServiceContext) {
    let shared_runtime = context.runtime();
    if let Ok(mut runtime) = shared_runtime.lock() {
        runtime.mark_service_critical();
    }
}

fn run_cleanup_job(
    context: Weak<AidlServiceContext>,
    policy: CleanupReaperPolicy,
    job: CleanupJob,
    pending: Arc<Mutex<std::collections::BTreeMap<CleanupJobKey, CleanupStep>>>,
) {
    let key = CleanupJobKey::from_handle(job.handle);
    let mut attempt = 0usize;
    loop {
        let Some(context) = context.upgrade() else {
            let _ = clear_pending_cleanup_job(&pending, key);
            return;
        };
        if job.registered_at.elapsed() >= policy.terminal_deadline {
            let terminal = crate::object_runtime::quarantine_drop_leak_object(&context, job.handle)
                .is_ok()
                || context.cleanup_is_terminal_for_handle(job.handle) == Ok(true);
            if !terminal {
                mark_cleanup_reaper_critical(&context);
            }
            if clear_pending_cleanup_job(&pending, key).is_err() {
                mark_cleanup_reaper_critical(&context);
            }
            return;
        }
        let dependency = match context.cleanup_dependency_for_handle(job.handle) {
            Ok(dependency) => dependency,
            Err(_) if context.cleanup_is_terminal_for_handle(job.handle) == Ok(true) => {
                if clear_pending_cleanup_job(&pending, key).is_err() {
                    mark_cleanup_reaper_critical(&context);
                }
                return;
            }
            Err(_) => {
                mark_cleanup_reaper_critical(&context);
                let _ = clear_pending_cleanup_job(&pending, key);
                return;
            }
        };
        if let Ok(mut guard) = pending.lock() {
            guard.insert(key, dependency);
        } else {
            mark_cleanup_reaper_critical(&context);
            return;
        }
        let result = close_method(job.handle.object_kind()).and_then(|method| {
            crate::object_runtime::retry_cleanup_from_reaper(&context, job.handle, method).map_err(
                |status| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        format!("cleanup reaper Binder retry failed: {status:?}"),
                    )
                },
            )
        });
        if result.is_ok() {
            if clear_pending_cleanup_job(&pending, key).is_err() {
                mark_cleanup_reaper_critical(&context);
            }
            return;
        }
        attempt = attempt.saturating_add(1);
        let delay = policy
            .retry_delays
            .get(attempt)
            .copied()
            .unwrap_or(Duration::from_millis(1_000));
        let remaining = policy
            .terminal_deadline
            .saturating_sub(job.registered_at.elapsed());
        std::thread::sleep(delay.min(remaining));
    }
}

pub(crate) fn start_cleanup_reaper(
    context: Weak<AidlServiceContext>,
    queue: Arc<CleanupReaperQueue>,
) -> Result<(), HalError> {
    let policy = queue.policy;
    let runner_context = context;
    let runner = Arc::new(move |job, pending| {
        run_cleanup_job(runner_context.clone(), policy, job, pending);
    });
    let owner = maleicacid_tuner_hal2_service_runtime::WorkerRuntime::start_reaper_queue(
        policy.max_jobs,
        "tuner-hal2-cleanup-reaper",
        runner,
    )?;
    queue.install(owner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId};

    #[test]
    fn cleanup_job_key_is_identity_only() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(7),
            AidlObjectGeneration(3),
        );
        assert_eq!(
            CleanupJobKey::from_handle(handle),
            CleanupJobKey {
                kind: 3,
                object_id: 7,
                generation: 3
            }
        );
    }
}
