use std::sync::{Arc, Condvar, Mutex, Weak};
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
            retry_delays: snapshot.cleanup_retry_schedule_ms.map(Duration::from_millis),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanupJobState {
    Queued,
    Running,
    WaitingForRetry,
    Released,
    Quarantined,
    Complete,
}

#[derive(Clone, Copy, Debug)]
struct CleanupJob {
    handle: AidlObjectHandle,
    dependency: CleanupStep,
    registered_at: Instant,
    next_attempt_at: Instant,
    attempt: usize,
    state: CleanupJobState,
}

#[derive(Debug, Default)]
struct CleanupQueueState {
    jobs: Vec<CleanupJob>,
}

#[derive(Debug)]
pub(crate) struct CleanupReaperQueue {
    policy: CleanupReaperPolicy,
    state: Mutex<CleanupQueueState>,
    wake: Condvar,
}

impl CleanupReaperQueue {
    pub(crate) fn from_snapshot(snapshot: CapabilitySnapshot) -> Self {
        Self {
            policy: CleanupReaperPolicy::from_snapshot(snapshot),
            state: Mutex::new(CleanupQueueState::default()),
            wake: Condvar::new(),
        }
    }

    pub(crate) fn enqueue(
        &self,
        handle: AidlObjectHandle,
        dependency: CleanupStep,
    ) -> Result<(), HalError> {
        let mut state = self.state.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper queue lock poisoned while enqueueing cleanup",
            )
        })?;
        if let Some(job) = state
            .jobs
            .iter_mut()
            .find(|job| job.handle == handle && job.dependency == dependency)
        {
            if matches!(
                job.state,
                CleanupJobState::Queued | CleanupJobState::WaitingForRetry
            ) {
                job.next_attempt_at = Instant::now();
                job.state = CleanupJobState::Queued;
            }
            self.wake.notify_one();
            return Ok(());
        }
        if state.jobs.len() >= self.policy.max_jobs {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper queue capacity exhausted",
            ));
        }
        let now = Instant::now();
        state.jobs.push(CleanupJob {
            handle,
            dependency,
            registered_at: now,
            next_attempt_at: now,
            attempt: 0,
            state: CleanupJobState::Queued,
        });
        self.wake.notify_one();
        Ok(())
    }

    fn take_ready(
        &self,
        context: &Weak<AidlServiceContext>,
    ) -> Result<Option<CleanupJob>, HalError> {
        let mut state = self.state.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper queue lock poisoned while taking cleanup",
            )
        })?;
        loop {
            if context.strong_count() == 0 {
                return Ok(None);
            }
            let now = Instant::now();
            if let Some(index) = state
                .jobs
                .iter()
                .position(|job| {
                    matches!(
                        job.state,
                        CleanupJobState::Queued | CleanupJobState::WaitingForRetry
                    ) && job.next_attempt_at <= now
                })
            {
                state.jobs[index].state = CleanupJobState::Running;
                return Ok(Some(state.jobs[index]));
            }
            let next_wait = state
                .jobs
                .iter()
                .map(|job| job.next_attempt_at.saturating_duration_since(now))
                .min();
            state = match next_wait {
                Some(wait) => {
                    self.wake
                        .wait_timeout(state, wait)
                        .map_err(|_| {
                            HalError::internal(
                                HalInternalKind::InvariantViolation,
                                "cleanup reaper queue lock poisoned while waiting",
                            )
                        })?
                        .0
                }
                None => self.wake.wait(state).map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "cleanup reaper queue lock poisoned while waiting",
                    )
                })?,
            };
        }
    }

    fn transition_terminal(
        &self,
        job: CleanupJob,
        terminal: CleanupJobState,
    ) -> Result<(), HalError> {
        let mut state = self.state.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper queue lock poisoned while completing cleanup",
            )
        })?;
        let Some(index) = state.jobs.iter().position(|candidate| {
            candidate.handle == job.handle
                && candidate.dependency == job.dependency
                && candidate.state == CleanupJobState::Running
        }) else {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper job disappeared before terminal transition",
            ));
        };
        state.jobs[index].state = terminal;
        state.jobs[index].state = CleanupJobState::Complete;
        state.jobs.swap_remove(index);
        Ok(())
    }

    fn requeue(&self, mut job: CleanupJob, dependency: CleanupStep) -> Result<(), HalError> {
        let previous_dependency = job.dependency;
        job.attempt = job.attempt.saturating_add(1);
        let delay = match self.policy.retry_delays.get(job.attempt) {
            Some(delay) => *delay,
            None => Duration::from_millis(1_000),
        };
        let terminal_at = job.registered_at + self.policy.terminal_deadline;
        job.next_attempt_at = (Instant::now() + delay).min(terminal_at);
        job.dependency = dependency;
        job.state = CleanupJobState::WaitingForRetry;
        let mut state = self.state.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "cleanup reaper queue lock poisoned while scheduling retry",
            )
        })?;
        let running_index = state
            .jobs
            .iter()
            .position(|existing| {
                existing.handle == job.handle
                    && existing.dependency == previous_dependency
                    && existing.state == CleanupJobState::Running
            })
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "cleanup reaper job disappeared before retry scheduling",
                )
            })?;
        if previous_dependency == dependency {
            state.jobs[running_index] = job;
        } else {
            state.jobs.swap_remove(running_index);
            if let Some(existing) = state.jobs.iter_mut().find(|existing| {
                existing.handle == job.handle && existing.dependency == dependency
            }) {
                existing.registered_at = existing.registered_at.min(job.registered_at);
                existing.next_attempt_at = existing.next_attempt_at.min(job.next_attempt_at);
                existing.attempt = existing.attempt.max(job.attempt);
                existing.state = if existing.next_attempt_at <= Instant::now() {
                    CleanupJobState::Queued
                } else {
                    CleanupJobState::WaitingForRetry
                };
            } else {
                state.jobs.push(job);
            }
        }
        self.wake.notify_one();
        Ok(())
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

fn quarantine_cleanup_job(
    context: &AidlServiceContext,
    queue: &CleanupReaperQueue,
    job: CleanupJob,
) -> bool {
    let terminal = if crate::object_runtime::drop_leak_object(context, job.handle).is_ok() {
        Some(CleanupJobState::Quarantined)
    } else if context.cleanup_is_terminal_for_handle(job.handle) == Ok(true) {
        Some(CleanupJobState::Released)
    } else {
        None
    };
    if terminal.is_some_and(|terminal| queue.transition_terminal(job, terminal).is_ok()) {
        return true;
    }
    let shared_runtime = context.runtime();
    if let Ok(mut runtime) = shared_runtime.lock() {
        runtime.mark_service_critical();
    }
    false
}

pub(crate) fn start_cleanup_reaper(
    context: Weak<AidlServiceContext>,
    queue: Arc<CleanupReaperQueue>,
) -> Result<(), HalError> {
    std::thread::Builder::new()
        .name("tuner-hal2-cleanup-reaper".to_owned())
        .spawn(move || loop {
            let job = match queue.take_ready(&context) {
                Ok(Some(job)) => job,
                Ok(None) => break,
                Err(_) => {
                    if let Some(context) = context.upgrade() {
                        let shared_runtime = context.runtime();
                        if let Ok(mut runtime) = shared_runtime.lock() {
                            runtime.mark_service_critical();
                        }
                    }
                    break;
                }
            };
            let Some(context) = context.upgrade() else {
                break;
            };
            if job.registered_at.elapsed() >= queue.policy.terminal_deadline {
                if !quarantine_cleanup_job(&context, &queue, job) {
                    break;
                }
                continue;
            }
            match context.cleanup_dependency_for_handle(job.handle) {
                Ok(current_dependency) if current_dependency != job.dependency => {
                    if queue.requeue(job, current_dependency).is_err() {
                        let shared_runtime = context.runtime();
                        if let Ok(mut runtime) = shared_runtime.lock() {
                            runtime.mark_service_critical();
                        }
                        break;
                    }
                    continue;
                }
                Ok(_) => {}
                Err(_) => match context.cleanup_is_terminal_for_handle(job.handle) {
                    Ok(true) => {
                        if queue
                            .transition_terminal(job, CleanupJobState::Released)
                            .is_err()
                        {
                            let shared_runtime = context.runtime();
                            if let Ok(mut runtime) = shared_runtime.lock() {
                                runtime.mark_service_critical();
                            }
                            break;
                        }
                        continue;
                    }
                    Ok(false) | Err(_) => {
                        let shared_runtime = context.runtime();
                        if let Ok(mut runtime) = shared_runtime.lock() {
                            runtime.mark_service_critical();
                        }
                        break;
                    }
                },
            }
            let result = close_method(job.handle.object_kind()).and_then(|method| {
                crate::object_runtime::retry_cleanup_from_reaper(&context, job.handle, method)
                    .map_err(|status| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            format!("cleanup reaper Binder retry failed: {status:?}"),
                        )
                    })
            });
            if result.is_ok() {
                if queue
                    .transition_terminal(job, CleanupJobState::Released)
                    .is_err()
                {
                    let shared_runtime = context.runtime();
                    if let Ok(mut runtime) = shared_runtime.lock() {
                        runtime.mark_service_critical();
                    }
                    break;
                }
                continue;
            }
            if job.registered_at.elapsed() >= queue.policy.terminal_deadline {
                if !quarantine_cleanup_job(&context, &queue, job) {
                    break;
                }
            } else {
                let dependency = context.cleanup_dependency_for_handle(job.handle);
                let scheduling_result = match dependency {
                    Ok(dependency) => queue.requeue(job, dependency),
                    Err(_) => match context.cleanup_is_terminal_for_handle(job.handle) {
                        Ok(true) => queue.transition_terminal(job, CleanupJobState::Released),
                        Ok(false) | Err(_) => Err(HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "cleanup dependency disappeared before reaching a terminal state",
                        )),
                    },
                };
                if scheduling_result.is_err() {
                    let shared_runtime = context.runtime();
                    if let Ok(mut runtime) = shared_runtime.lock() {
                        runtime.mark_service_critical();
                    }
                    break;
                }
            }
        })
        .map(|_| ())
        .map_err(|error| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("failed to spawn cleanup reaper: {error}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId};

    fn filter_handle() -> AidlObjectHandle {
        AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(7),
            AidlObjectGeneration(3),
        )
    }

    #[test]
    fn dependency_advance_merges_concurrent_enqueue_without_losing_cleanup() {
        let queue = CleanupReaperQueue::from_snapshot(CapabilitySnapshot::product_default());
        let handle = filter_handle();
        let now = Instant::now();
        let running = CleanupJob {
            handle,
            dependency: CleanupStep::StopWorker,
            registered_at: now,
            next_attempt_at: now,
            attempt: 0,
            state: CleanupJobState::Running,
        };
        queue.state.lock().unwrap().jobs.push(running);
        queue.enqueue(handle, CleanupStep::ClearQueue).unwrap();

        queue.requeue(running, CleanupStep::ClearQueue).unwrap();

        let state = queue.state.lock().unwrap();
        assert_eq!(state.jobs.len(), 1);
        assert_eq!(state.jobs[0].handle, handle);
        assert_eq!(state.jobs[0].dependency, CleanupStep::ClearQueue);
        assert!(matches!(
            state.jobs[0].state,
            CleanupJobState::Queued | CleanupJobState::WaitingForRetry
        ));
    }

    #[test]
    fn exact_cleanup_dependency_enqueue_is_deduplicated() {
        let queue = CleanupReaperQueue::from_snapshot(CapabilitySnapshot::product_default());
        let handle = filter_handle();
        queue.enqueue(handle, CleanupStep::ClearQueue).unwrap();
        queue.enqueue(handle, CleanupStep::ClearQueue).unwrap();

        let state = queue.state.lock().unwrap();
        assert_eq!(state.jobs.len(), 1);
        assert_eq!(state.jobs[0].dependency, CleanupStep::ClearQueue);
    }
}
