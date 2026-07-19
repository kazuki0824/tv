use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};

use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalInternalKind,
};
use maleicacid_tuner_hal2_demux::{DvrKind, QueueWaitHandle, QueueWaitResult};
use maleicacid_tuner_hal2_service_runtime::{
    DvrPlaybackWorkerCleanupExecutionReport, DvrPlaybackWorkerCleanupOperation,
    DvrPlaybackWorkerCleanupPhase, DvrPlaybackWorkerCleanupStepOutcome,
    DvrPlaybackWorkerCleanupTarget,
    DvrPostCommitNotificationDiagnosticRecord,
    DvrPostCommitNotificationFailureKind, DvrPostCommitNotificationPhase,
};

use crate::object_handle::{
    AidlObjectGeneration, AidlObjectHandle, AidlObjectId, AidlObjectKind,
};
use crate::service_context::{AidlServiceContext, SharedAidlServiceContext};

const TUNER_EVENT_DATA_READY: u32 = 1;
const PLAYBACK_WAIT_TIMEOUT_NS: i64 = 100_000_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct DvrPlaybackWorkerKey {
    object_id: i64,
    generation: u64,
}

impl DvrPlaybackWorkerKey {
    fn new(handle: AidlObjectHandle) -> Self {
        Self {
            object_id: handle.object_id().0,
            generation: handle.generation().0,
        }
    }
}

pub(crate) struct DvrPlaybackWorker {
    cancel: Arc<AtomicBool>,
    waiter: Arc<QueueWaitHandle>,
    terminal: Arc<AtomicBool>,
    runtime_fail_closed: Arc<AtomicBool>,
    join: JoinHandle<Result<(), HalError>>,
}

fn run_playback_worker(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
    cancel: Arc<AtomicBool>,
    waiter: Arc<QueueWaitHandle>,
    start_gate: Arc<AtomicBool>,
) -> Result<(), HalError> {
    while !start_gate.load(Ordering::Acquire) && !cancel.load(Ordering::Acquire) {
        thread::park();
    }
    while !cancel.load(Ordering::Acquire) {
        match waiter
            .wait(TUNER_EVENT_DATA_READY, PLAYBACK_WAIT_TIMEOUT_NS)
            .map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "playback DVR EventFlag wait failed",
                )
            })? {
            QueueWaitResult::TimedOut => continue,
            QueueWaitResult::Signaled(_) => {
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                let (_report, events, dispatcher, processing_error) = {
                    let mut runtime = context.runtime().lock().map_err(|_| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "service runtime lock poisoned in playback DVR worker",
                        )
                    })?;
                    runtime.consume_playback_dvr_queue_for_object_and_filter_events(
                        handle.object_id(),
                        handle.generation(),
                    )?
                };
                let dispatch_result = if events.is_empty() {
                    Ok(())
                } else {
                    let runtime_handle = context.runtime();
                    dispatcher.dispatch(&runtime_handle, events)
                };
                match (processing_error, dispatch_result) {
                    (None, Ok(())) => {}
                    (Some(error), Ok(())) | (None, Err(error)) => return Err(error),
                    (Some(primary), Err(dispatch_error)) => {
                        return Err(compose_primary_cleanup_failure(
                            "playback DVR packet processing failed and filter event dispatch failed",
                            primary,
                            dispatch_error,
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn run_playback_worker_with_terminal_diagnostic(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
    cancel: Arc<AtomicBool>,
    waiter: Arc<QueueWaitHandle>,
    start_gate: Arc<AtomicBool>,
    terminal: Arc<AtomicBool>,
    runtime_fail_closed: Arc<AtomicBool>,
) -> Result<(), HalError> {
    let result = run_playback_worker(
        Arc::clone(&context),
        handle,
        cancel,
        waiter,
        start_gate,
    );
    terminal.store(true, Ordering::Release);
    if let Err(error) = &result {
        let runtime_failure_result = context.runtime().lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while terminalizing playback DVR worker",
            )
        }).and_then(|mut runtime| {
            runtime.rollback_started_dvr_after_playback_worker_failure(
                handle.object_id(),
                handle.generation(),
            )
        });
        let mut terminal_error = error.clone();
        match runtime_failure_result {
            Ok(()) => runtime_fail_closed.store(true, Ordering::Release),
            Err(runtime_error) => {
                terminal_error = compose_primary_cleanup_failure(
                    "playback DVR worker failed and runtime terminalization failed",
                    terminal_error,
                    runtime_error,
                );
            }
        }
        let diagnostic_result = context.record_dvr_post_commit_notification_diagnostic_fallback(
            DvrPostCommitNotificationDiagnosticRecord {
                phase: DvrPostCommitNotificationPhase::PlaybackWorkerRuntimeFailure,
                failure_kind: DvrPostCommitNotificationFailureKind::PlaybackWorkerTerminal,
                object_id: handle.object_id(),
                generation: handle.generation(),
                error: terminal_error.clone(),
            },
        );
        return match diagnostic_result {
            Ok(()) => Err(terminal_error),
            Err(record_error) => Err(compose_primary_cleanup_failure(
                "playback DVR worker terminal diagnostic record failed",
                terminal_error,
                record_error,
            )),
        };
    }
    result
}

fn stop_joined_playback_worker(
    attempt_id: Option<u64>,
    operation: DvrPlaybackWorkerCleanupOperation,
    handle: AidlObjectHandle,
    worker: DvrPlaybackWorker,
) -> (bool, bool, DvrPlaybackWorkerCleanupExecutionReport) {
    worker.cancel.store(true, Ordering::Release);
    let wake_result = worker.waiter.wake(TUNER_EVENT_DATA_READY).map_err(|_| {
        HalError::cleanup_failed(
            "playback DVR worker wake",
            "failed to wake playback DVR worker for cancellation",
        )
    });
    worker.join.thread().unpark();
    let (join_result, terminal_result) = match worker.join.join() {
        Ok(result) => (Ok(()), result),
        Err(_) => {
            let join_error = HalError::cleanup_failed(
                "playback DVR worker join",
                "playback DVR worker thread panicked",
            );
            let terminal_error = HalError::cleanup_failed(
                "playback DVR worker terminal result",
                "terminal result is unavailable because playback DVR worker thread panicked",
            );
            (Err(join_error), Err(terminal_error))
        }
    };
    let was_terminal = worker.terminal.load(Ordering::Acquire);
    let runtime_fail_closed = worker.runtime_fail_closed.load(Ordering::Acquire);
    let mut report = DvrPlaybackWorkerCleanupExecutionReport::new();
    report.push(DvrPlaybackWorkerCleanupStepOutcome {
        attempt_id,
        operation,
        target: DvrPlaybackWorkerCleanupTarget::Worker {
            object_id: handle.object_id(),
            generation: handle.generation(),
        },
        phase: DvrPlaybackWorkerCleanupPhase::Wake,
        expected_step_count: None,
        result: wake_result,
    });
    report.push(DvrPlaybackWorkerCleanupStepOutcome {
        attempt_id,
        operation,
        target: DvrPlaybackWorkerCleanupTarget::Worker {
            object_id: handle.object_id(),
            generation: handle.generation(),
        },
        phase: DvrPlaybackWorkerCleanupPhase::Join,
        expected_step_count: None,
        result: join_result,
    });
    report.push(DvrPlaybackWorkerCleanupStepOutcome {
        attempt_id,
        operation,
        target: DvrPlaybackWorkerCleanupTarget::Worker {
            object_id: handle.object_id(),
            generation: handle.generation(),
        },
        phase: DvrPlaybackWorkerCleanupPhase::Terminal,
        expected_step_count: None,
        result: terminal_result,
    });
    (was_terminal, runtime_fail_closed, report)
}

fn replacement_cleanup_result(
    report: &DvrPlaybackWorkerCleanupExecutionReport,
    was_terminal: bool,
    runtime_fail_closed: bool,
) -> Result<(), HalError> {
    let join_succeeded = report.outcomes().iter().any(|outcome| {
        outcome.phase == DvrPlaybackWorkerCleanupPhase::Join && outcome.result.is_ok()
    });
    let terminal_succeeded = report.outcomes().iter().any(|outcome| {
        outcome.phase == DvrPlaybackWorkerCleanupPhase::Terminal && outcome.result.is_ok()
    });
    report
        .outcomes()
        .iter()
        .filter(|outcome| {
            // EventFlag wake is an auxiliary cancellation nudge. If the worker joined and its
            // terminal result was collected, wake failure must remain diagnostic-only.
            if outcome.phase == DvrPlaybackWorkerCleanupPhase::Wake
                && join_succeeded
                && terminal_succeeded
            {
                return false;
            }
            // A terminal worker failure is already owned by its terminalization path only when
            // that worker itself successfully fail-closed the corresponding runtime.
            !(was_terminal
                && runtime_fail_closed
                && outcome.phase == DvrPlaybackWorkerCleanupPhase::Terminal)
        })
        .find_map(|outcome| outcome.result.clone().err())
        .map_or(Ok(()), Err)
}

fn complete_cleanup_attempt(
    report: &mut DvrPlaybackWorkerCleanupExecutionReport,
    attempt_id: Option<u64>,
    operation: DvrPlaybackWorkerCleanupOperation,
) {
    let expected_step_count = u16::try_from(report.outcomes().len().saturating_add(1))
        .unwrap_or(u16::MAX);
    report.push(DvrPlaybackWorkerCleanupStepOutcome {
        attempt_id,
        operation,
        target: DvrPlaybackWorkerCleanupTarget::Store,
        phase: DvrPlaybackWorkerCleanupPhase::AttemptComplete,
        expected_step_count: Some(expected_step_count),
        result: Ok(()),
    });
}

fn begin_cleanup_attempt(
    context: &AidlServiceContext,
    operation: DvrPlaybackWorkerCleanupOperation,
) -> (Option<u64>, DvrPlaybackWorkerCleanupExecutionReport) {
    let (attempt_id, allocation_result) =
        context.next_dvr_playback_worker_cleanup_attempt_id();
    let mut report = DvrPlaybackWorkerCleanupExecutionReport::new();
    report.push(DvrPlaybackWorkerCleanupStepOutcome {
        attempt_id,
        operation,
        target: DvrPlaybackWorkerCleanupTarget::Store,
        phase: DvrPlaybackWorkerCleanupPhase::AttemptIdAllocation,
        expected_step_count: None,
        result: allocation_result,
    });
    (attempt_id, report)
}

fn record_cleanup_report(
    context: &AidlServiceContext,
    report: &DvrPlaybackWorkerCleanupExecutionReport,
) {
    context.record_dvr_playback_worker_cleanup_report(report);
}

pub(crate) fn start_dvr_playback_worker(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let _lifecycle = context.dvr_playback_worker_lifecycle_lock()?;
    let kind = {
        let mut runtime = context.runtime().lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while preparing playback DVR worker",
            )
        })?;
        runtime.dvr_kind_for_object(handle.object_id(), handle.generation())?
    };
    if kind != DvrKind::Playback {
        return Ok(());
    }

    let key = DvrPlaybackWorkerKey::new(handle);
    let terminal_worker = {
        let mut store = context.dvr_playback_workers_lock()?;
        match store.get(&key) {
            Some(worker) if !worker.terminal.load(Ordering::Acquire) => return Ok(()),
            Some(_) => store.remove(&key),
            None => None,
        }
    };
    let terminal_cleanup = terminal_worker.map(|worker| {
        let (attempt_id, mut report) =
            begin_cleanup_attempt(context, DvrPlaybackWorkerCleanupOperation::Replacement);
        let (was_terminal, runtime_fail_closed, worker_report) = stop_joined_playback_worker(
            attempt_id,
            DvrPlaybackWorkerCleanupOperation::Replacement,
            handle,
            worker,
        );
        report.extend(worker_report);
        complete_cleanup_attempt(
            &mut report,
            attempt_id,
            DvrPlaybackWorkerCleanupOperation::Replacement,
        );
        record_cleanup_report(context, &report);
        (was_terminal, runtime_fail_closed, report)
    });

    // Acquire a fresh waiter only after old-worker cleanup, and keep the runtime lock until
    // the gated worker has been inserted. Configure/stop cannot replace the queue endpoint
    // between waiter acquisition and worker commit.
    let mut runtime_guard = context.runtime().lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while revalidating playback DVR worker start",
        )
    })?;
    let runtime_still_started = runtime_guard
        .dvr_started_for_playback_worker(handle.object_id(), handle.generation())?;

    if let Some((was_terminal, runtime_fail_closed, old_report)) = terminal_cleanup {
        if let Err(cleanup_error) =
            replacement_cleanup_result(&old_report, was_terminal, runtime_fail_closed)
        {
            let runtime_rollback_result =
                runtime_guard.rollback_started_dvr_after_playback_worker_failure(
                    handle.object_id(),
                    handle.generation(),
                );
            return Err(match runtime_rollback_result {
                Ok(()) => cleanup_error,
                Err(rollback_error) => compose_primary_cleanup_failure(
                    "terminal playback worker cleanup failed and DVR runtime fail-close failed",
                    cleanup_error,
                    rollback_error,
                ),
            });
        }
    }

    if !runtime_still_started {
        return Err(HalError::invalid_state(
            maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
            "playback DVR runtime is no longer started before worker commit",
        ));
    }
    let waiter = runtime_guard.playback_dvr_wait_handle_for_object(
        handle.object_id(),
        handle.generation(),
    )?;

    let mut store = context.dvr_playback_workers_lock()?;
    if store.contains_key(&key) {
        return Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "playback DVR worker appeared during serialized start",
        ));
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let waiter = Arc::new(waiter);
    let start_gate = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    let thread_waiter = Arc::clone(&waiter);
    let thread_gate = Arc::clone(&start_gate);
    let terminal = Arc::new(AtomicBool::new(false));
    let thread_terminal = Arc::clone(&terminal);
    let runtime_fail_closed = Arc::new(AtomicBool::new(false));
    let thread_runtime_fail_closed = Arc::clone(&runtime_fail_closed);
    let thread_context = Arc::clone(context);
    let join = match thread::Builder::new()
        .name(format!(
            "tuner-hal2-dvr-playback-{}-{}",
            handle.object_id().0,
            handle.generation().0
        ))
        .spawn(move || {
            run_playback_worker_with_terminal_diagnostic(
                thread_context,
                handle,
                thread_cancel,
                thread_waiter,
                thread_gate,
                thread_terminal,
                thread_runtime_fail_closed,
            )
        }) {
        Ok(join) => join,
        Err(error) => {
            let spawn_error = HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("failed to spawn playback DVR worker: {error}"),
            );
            let fail_close_result =
                runtime_guard.rollback_started_dvr_after_playback_worker_failure(
                    handle.object_id(),
                    handle.generation(),
                );
            return Err(match fail_close_result {
                Ok(()) => spawn_error,
                Err(fail_close_error) => compose_primary_cleanup_failure(
                    "playback DVR worker spawn failed and runtime fail-close failed",
                    spawn_error,
                    fail_close_error,
                ),
            });
        }
    };
    let thread = join.thread().clone();
    let new_worker = DvrPlaybackWorker {
        cancel,
        waiter,
        terminal,
        runtime_fail_closed,
        join,
    };
    store.insert(key, new_worker);
    drop(store);
    drop(runtime_guard);
    start_gate.store(true, Ordering::Release);
    thread.unpark();
    Ok(())
}

pub(crate) fn stop_dvr_playback_worker(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    operation: DvrPlaybackWorkerCleanupOperation,
) -> DvrPlaybackWorkerCleanupExecutionReport {
    let (attempt_id, mut report) = begin_cleanup_attempt(context, operation);
    let (_lifecycle, lifecycle_result) = context.dvr_playback_worker_lifecycle_lock_for_cleanup();
    report.push(DvrPlaybackWorkerCleanupStepOutcome {
        attempt_id,
        operation,
        target: DvrPlaybackWorkerCleanupTarget::Store,
        phase: DvrPlaybackWorkerCleanupPhase::LifecycleLock,
        expected_step_count: None,
        result: lifecycle_result,
    });
    let (worker, store_result) =
        context.take_dvr_playback_worker_for_cleanup(DvrPlaybackWorkerKey::new(handle));
    report.push(DvrPlaybackWorkerCleanupStepOutcome {
        attempt_id,
        operation,
        target: DvrPlaybackWorkerCleanupTarget::Store,
        phase: DvrPlaybackWorkerCleanupPhase::StoreAccess,
        expected_step_count: None,
        result: store_result,
    });
    if let Some(worker) = worker {
        let (_, _, worker_report) =
            stop_joined_playback_worker(attempt_id, operation, handle, worker);
        report.extend(worker_report);
    }
    complete_cleanup_attempt(&mut report, attempt_id, operation);
    record_cleanup_report(context, &report);
    report
}

pub(crate) fn stop_all_dvr_playback_workers(
    context: &AidlServiceContext,
) -> DvrPlaybackWorkerCleanupExecutionReport {
    let operation = DvrPlaybackWorkerCleanupOperation::ServiceReset;
    let (attempt_id, mut report) = begin_cleanup_attempt(context, operation);
    let (_lifecycle, lifecycle_result) = context.dvr_playback_worker_lifecycle_lock_for_cleanup();
    report.push(DvrPlaybackWorkerCleanupStepOutcome {
        attempt_id,
        operation,
        target: DvrPlaybackWorkerCleanupTarget::Store,
        phase: DvrPlaybackWorkerCleanupPhase::LifecycleLock,
        expected_step_count: None,
        result: lifecycle_result,
    });
    let (workers, store_result) = context.take_dvr_playback_workers_for_reset();
    report.push(DvrPlaybackWorkerCleanupStepOutcome {
        attempt_id,
        operation,
        target: DvrPlaybackWorkerCleanupTarget::Store,
        phase: DvrPlaybackWorkerCleanupPhase::StoreAccess,
        expected_step_count: None,
        result: store_result,
    });
    for (key, worker) in workers {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Dvr,
            AidlObjectId(key.object_id),
            AidlObjectGeneration(key.generation),
        );
        let (_, _, worker_report) =
            stop_joined_playback_worker(attempt_id, operation, handle, worker);
        report.extend(worker_report);
    }
    complete_cleanup_attempt(&mut report, attempt_id, operation);
    record_cleanup_report(context, &report);
    report
}

pub(crate) type DvrPlaybackWorkerMap =
    BTreeMap<DvrPlaybackWorkerKey, DvrPlaybackWorker>;
