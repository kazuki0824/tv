use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalInternalKind,
};
use maleicacid_tuner_hal2_demux::{DvrKind, QueueWaitHandle, QueueWaitResult};
use maleicacid_tuner_hal2_service_runtime::{
    CleanupExecutionStepOutcome, DvrPlaybackWorkerCleanupExecutionReport,
    DvrPlaybackWorkerCleanupOperation, DvrStartTransition,
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
const PLAYBACK_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

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
    run_id: u128,
    start_gate: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    waiter: Arc<QueueWaitHandle>,
    terminal: Arc<AtomicBool>,
    runtime_fail_closed: Arc<AtomicBool>,
    join: JoinHandle<Result<(), HalError>>,
}

impl DvrPlaybackWorker {
    pub(crate) const fn run_id(&self) -> u128 {
        self.run_id
    }
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
    run_id: u128,
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
        let key = DvrPlaybackWorkerKey::new(handle);
        // Do not hold the lifecycle cleanup lock while joining or terminalizing.
        // A terminating worker may race with replacement/stop; the run id stored in
        // the worker store is the ownership token, and the runtime mutex serializes
        // the final fail-close transition.
        let ownership_result = context.dvr_playback_worker_is_current(key, run_id);
        let owns_runtime = matches!(&ownership_result, Ok(true));
        let runtime_failure_result = match ownership_result {
            Ok(true) => match context.runtime().lock() {
                Ok(mut runtime) => runtime.rollback_started_dvr_after_playback_worker_failure(
                    handle.object_id(),
                    handle.generation(),
                ),
                Err(poisoned) => {
                    let lock_error = HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock was poisoned while terminalizing playback DVR worker",
                    );
                    let mut runtime = poisoned.into_inner();
                    match runtime.rollback_started_dvr_after_playback_worker_failure(
                        handle.object_id(),
                        handle.generation(),
                    ) {
                        Ok(()) => Err(lock_error),
                        Err(rollback_error) => Err(compose_primary_cleanup_failure(
                            "runtime lock was poisoned and playback DVR terminal fail-close failed",
                            lock_error,
                            rollback_error,
                        )),
                    }
                }
            },
            Ok(false) => Ok(()),
            Err(error) => Err(error),
        };
        let mut terminal_error = error.clone();
        match runtime_failure_result {
            Ok(()) => {
                if owns_runtime {
                    runtime_fail_closed.store(true, Ordering::Release);
                }
            }
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

fn push_cleanup_step(
    report: &mut DvrPlaybackWorkerCleanupExecutionReport,
    attempt_id: Option<u128>,
    operation: DvrPlaybackWorkerCleanupOperation,
    target: DvrPlaybackWorkerCleanupTarget,
    phase: DvrPlaybackWorkerCleanupPhase,
    result: Result<(), HalError>,
) {
    match attempt_id {
        Some(attempt_id) => report.push(DvrPlaybackWorkerCleanupStepOutcome::Step {
            attempt_id,
            operation,
            target,
            phase,
            result,
        }),
        None => report.push(DvrPlaybackWorkerCleanupStepOutcome::EmergencyStep {
            operation,
            target,
            phase,
            result,
        }),
    }
}

fn join_playback_worker_bounded(
    join: JoinHandle<Result<(), HalError>>,
) -> (Result<(), HalError>, Result<(), HalError>) {
    let deadline = Instant::now()
        .checked_add(PLAYBACK_JOIN_TIMEOUT)
        .unwrap_or_else(Instant::now);
    while !join.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    if join.is_finished() {
        return match join.join() {
            Ok(result) => (Ok(()), result),
            Err(_) => (
                Err(HalError::cleanup_failed(
                    "playback DVR worker join",
                    "playback DVR worker thread panicked",
                )),
                Err(HalError::cleanup_failed(
                    "playback DVR worker terminal result",
                    "terminal result is unavailable because playback DVR worker thread panicked",
                )),
            ),
        };
    }

    let holder = Arc::new(std::sync::Mutex::new(Some(join)));
    let watcher_holder = Arc::clone(&holder);
    let watcher = thread::Builder::new()
        .name("tuner-hal2-dvr-playback-join-watchdog".to_string())
        .spawn(move || {
            if let Ok(mut guard) = watcher_holder.lock() {
                if let Some(join) = guard.take() {
                    // The synchronous cleanup report already contains JoinTimeout. Keep ownership
                    // here until the thread terminates; the late terminal result cannot alter the
                    // completed public lifecycle outcome.
                    drop(join.join());
                }
            }
        });
    if let Err(spawn_error) = watcher {
        // Do not recreate an unbounded public stop by synchronously joining after watchdog spawn
        // failure. Detach the retained handle and return an explicit ownership-cleanup failure;
        // the caller fail-closes the DVR runtime.
        drop(holder);
        return (
            Err(HalError::cleanup_failed(
                "playback DVR worker join watchdog",
                format!("join timed out and watchdog spawn failed: {spawn_error}"),
            )),
            Err(HalError::cleanup_failed(
                "playback DVR worker terminal result",
                "terminal result is unavailable after join timeout and watchdog spawn failure",
            )),
        );
    }
    (
        Err(HalError::cleanup_failed(
            "playback DVR worker join timeout",
            "playback DVR worker did not terminate before the bounded join deadline",
        )),
        Err(HalError::cleanup_failed(
            "playback DVR worker terminal result",
            "terminal result is pending in the join watchdog after timeout",
        )),
    )
}

fn stop_joined_playback_worker(
    attempt_id: Option<u128>,
    operation: DvrPlaybackWorkerCleanupOperation,
    handle: AidlObjectHandle,
    worker: DvrPlaybackWorker,
) -> (bool, bool, DvrPlaybackWorkerCleanupExecutionReport) {
    let DvrPlaybackWorker {
        run_id: _,
        start_gate: _,
        cancel,
        waiter,
        terminal,
        runtime_fail_closed,
        join,
    } = worker;
    cancel.store(true, Ordering::Release);
    let wake_result = waiter.wake(TUNER_EVENT_DATA_READY).map_err(|_| {
        HalError::cleanup_failed(
            "playback DVR worker wake",
            "failed to wake playback DVR worker for cancellation",
        )
    });
    join.thread().unpark();
    let (join_result, terminal_result) = join_playback_worker_bounded(join);
    let was_terminal = terminal.load(Ordering::Acquire);
    let runtime_fail_closed = runtime_fail_closed.load(Ordering::Acquire);
    let mut report = DvrPlaybackWorkerCleanupExecutionReport::new();
    let target = DvrPlaybackWorkerCleanupTarget::Worker {
        object_id: handle.object_id(),
        generation: handle.generation(),
    };
    push_cleanup_step(
        &mut report,
        attempt_id,
        operation,
        target,
        DvrPlaybackWorkerCleanupPhase::Wake,
        wake_result,
    );
    let join_phase = if join_result.is_err()
        && !terminal.load(Ordering::Acquire)
    {
        DvrPlaybackWorkerCleanupPhase::JoinTimeout
    } else {
        DvrPlaybackWorkerCleanupPhase::Join
    };
    push_cleanup_step(
        &mut report,
        attempt_id,
        operation,
        target,
        join_phase,
        join_result,
    );
    push_cleanup_step(
        &mut report,
        attempt_id,
        operation,
        target,
        DvrPlaybackWorkerCleanupPhase::Terminal,
        terminal_result,
    );
    (was_terminal, runtime_fail_closed, report)
}

fn complete_cleanup_attempt(
    report: &mut DvrPlaybackWorkerCleanupExecutionReport,
    attempt_id: Option<u128>,
    operation: DvrPlaybackWorkerCleanupOperation,
) {
    if let Some(attempt_id) = attempt_id {
        let expected_step_count = report.outcomes().len().saturating_add(1);
        report.push(DvrPlaybackWorkerCleanupStepOutcome::AttemptComplete {
            attempt_id,
            operation,
            expected_step_count,
        });
    }
}

fn begin_cleanup_attempt(
    context: &AidlServiceContext,
    operation: DvrPlaybackWorkerCleanupOperation,
) -> (Option<u128>, DvrPlaybackWorkerCleanupExecutionReport) {
    let mut report = DvrPlaybackWorkerCleanupExecutionReport::new();
    match context.next_dvr_playback_worker_cleanup_attempt_id() {
        Ok(attempt_id) => (Some(attempt_id), report),
        Err(error) => {
            report.push(DvrPlaybackWorkerCleanupStepOutcome::AttemptIdentityFailure {
                operation,
                error,
            });
            (None, report)
        }
    }
}

fn record_cleanup_report(
    context: &AidlServiceContext,
    report: &DvrPlaybackWorkerCleanupExecutionReport,
) {
    context.record_dvr_playback_worker_cleanup_report(report);
}

fn fail_close_started_runtime_after_unrecoverable_replacement(
    context: &AidlServiceContext,
    handle: AidlObjectHandle,
    primary: HalError,
) -> HalError {
    let runtime_handle = context.runtime();
    let (mut runtime, lock_error) = match runtime_handle.lock() {
        Ok(runtime) => (runtime, None),
        Err(poisoned) => (
            poisoned.into_inner(),
            Some(HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while fail-closing unrecoverable playback replacement",
            )),
        ),
    };
    let fail_close_result = runtime.rollback_started_dvr_after_playback_worker_failure(
        handle.object_id(),
        handle.generation(),
    );
    let mut error = primary;
    if let Some(lock_error) = lock_error {
        error = compose_primary_cleanup_failure(
            "playback worker replacement failed and fail-close lock was poisoned",
            error,
            lock_error,
        );
    }
    if let Err(fail_close_error) = fail_close_result {
        error = compose_primary_cleanup_failure(
            "playback worker replacement failed and runtime fail-close failed",
            error,
            fail_close_error,
        );
    }
    error
}

pub(crate) fn start_dvr_playback_worker(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    start_transition: DvrStartTransition,
) -> Result<(), HalError> {
    let _lifecycle = context.dvr_playback_worker_lifecycle_lock()?;
    let key = DvrPlaybackWorkerKey::new(handle);

    // An active committed worker makes start idempotent. Terminal or never-opened gated workers are
    // removed as a reversible ownership handoff. Until the replacement thread is committed, every
    // failure path restores this old entry instead of destroying an AlreadyStarted session.
    let mut old_worker = {
        let mut store = context.dvr_playback_workers_lock()?;
        if let Some(worker) = store.get(&key) {
            if !worker.terminal.load(Ordering::Acquire)
                && worker.start_gate.load(Ordering::Acquire)
            {
                return Ok(());
            }
        }
        store.remove(&key)
    };

    let recover_precommit_failure = |primary: HalError,
                                     old_worker: &mut Option<DvrPlaybackWorker>|
     -> HalError {
        let old_is_terminal = old_worker
            .as_ref()
            .is_some_and(|worker| worker.terminal.load(Ordering::Acquire));
        if !old_is_terminal {
            let (mut store, lock_result) = context.dvr_playback_workers_lock_for_recovery();
            let mut error = primary;
            if let Err(lock_error) = lock_result {
                error = compose_primary_cleanup_failure(
                    "playback worker replacement failed and store recovery lock was poisoned",
                    error,
                    lock_error,
                );
            }
            if store.contains_key(&key) {
                return compose_primary_cleanup_failure(
                    "playback worker replacement failed and old ownership could not be restored",
                    error,
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "playback worker store changed during serialized ownership recovery",
                    ),
                );
            }
            if let Some(worker) = old_worker.take() {
                store.insert(key, worker);
                return error;
            }
            if start_transition != DvrStartTransition::AlreadyStarted {
                return error;
            }
            drop(store);
            return fail_close_started_runtime_after_unrecoverable_replacement(
                context,
                handle,
                error,
            );
        }

        let mut error = primary;
        if let Some(worker) = old_worker.take() {
            let (attempt_id, mut report) =
                begin_cleanup_attempt(context, DvrPlaybackWorkerCleanupOperation::Replacement);
            let (_, _, worker_report) = stop_joined_playback_worker(
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
            if let Err(cleanup_error) = playback_worker_cleanup_ownership_result(&report) {
                error = compose_primary_cleanup_failure(
                    "playback worker replacement failed and terminal old worker cleanup failed",
                    error,
                    cleanup_error,
                );
            }
            record_cleanup_report(context, &report);
        }
        fail_close_started_runtime_after_unrecoverable_replacement(context, handle, error)
    };

    // Acquire fresh runtime state and FMQ endpoint after revoking old store ownership. The old run
    // therefore cannot fail-close a later run, while any pre-commit failure remains reversible by
    // restoring its retained store entry.
    let mut runtime_guard = match context.runtime().lock() {
        Ok(runtime) => runtime,
        Err(poisoned) => {
            drop(poisoned.into_inner());
            return Err(recover_precommit_failure(
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while preparing playback DVR replacement",
                ),
                &mut old_worker,
            ));
        }
    };
    let kind = match runtime_guard.dvr_kind_for_object(handle.object_id(), handle.generation()) {
        Ok(kind) => kind,
        Err(error) => {
            drop(runtime_guard);
            return Err(recover_precommit_failure(error, &mut old_worker));
        }
    };
    if kind != DvrKind::Playback {
        drop(runtime_guard);
        if let Some(worker) = old_worker {
            let (attempt_id, mut report) =
                begin_cleanup_attempt(context, DvrPlaybackWorkerCleanupOperation::Replacement);
            let (_, _, worker_report) = stop_joined_playback_worker(
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
        }
        return Ok(());
    }
    let started = match runtime_guard
        .dvr_started_for_playback_worker(handle.object_id(), handle.generation())
    {
        Ok(started) => started,
        Err(error) => {
            drop(runtime_guard);
            return Err(recover_precommit_failure(error, &mut old_worker));
        }
    };
    if !started {
        drop(runtime_guard);
        return Err(recover_precommit_failure(
            HalError::invalid_state(
                maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                "playback DVR runtime is no longer started before worker commit",
            ),
            &mut old_worker,
        ));
    }
    let waiter = match runtime_guard.playback_dvr_wait_handle_for_object(
        handle.object_id(),
        handle.generation(),
    ) {
        Ok(waiter) => waiter,
        Err(error) => {
            drop(runtime_guard);
            return Err(recover_precommit_failure(error, &mut old_worker));
        }
    };
    let run_id = match context.next_dvr_playback_worker_run_id() {
        Ok(run_id) => run_id,
        Err(error) => {
            drop(runtime_guard);
            return Err(recover_precommit_failure(error, &mut old_worker));
        }
    };

    // Keep the store guard from before spawn through insertion. No fallible store acquisition exists
    // after spawn, so a closed-gate thread cannot become detached.
    let (mut store, store_lock_result) = context.dvr_playback_workers_lock_for_recovery();
    if let Err(error) = store_lock_result {
        drop(store);
        drop(runtime_guard);
        return Err(recover_precommit_failure(error, &mut old_worker));
    }
    if store.contains_key(&key) {
        let primary = HalError::invalid_state(
            maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
            "playback worker store changed during serialized replacement",
        );
        drop(store);
        drop(runtime_guard);
        return Err(recover_precommit_failure(primary, &mut old_worker));
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
            "tuner-hal2-dvr-playback-{}-{}-{}",
            handle.object_id().0,
            handle.generation().0,
            run_id,
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
                run_id,
            )
        }) {
        Ok(join) => join,
        Err(spawn_error) => {
            drop(store);
            drop(runtime_guard);
            return Err(recover_precommit_failure(
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    format!("failed to spawn playback DVR worker: {spawn_error}"),
                ),
                &mut old_worker,
            ));
        }
    };
    let thread = join.thread().clone();
    store.insert(
        key,
        DvrPlaybackWorker {
            run_id,
            start_gate: Arc::clone(&start_gate),
            cancel,
            waiter,
            terminal,
            runtime_fail_closed,
            join,
        },
    );
    drop(store);
    drop(runtime_guard);
    start_gate.store(true, Ordering::Release);
    thread.unpark();

    // Replacement commit is complete. Old terminal/gated cleanup is now post-commit artifact
    // cleanup: record all outcomes but never tear down the newly committed worker or runtime.
    if let Some(worker) = old_worker {
        let (attempt_id, mut report) =
            begin_cleanup_attempt(context, DvrPlaybackWorkerCleanupOperation::Replacement);
        let (_, _, worker_report) = stop_joined_playback_worker(
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
    }
    Ok(())
}

pub(crate) fn playback_worker_cleanup_ownership_result(
    report: &DvrPlaybackWorkerCleanupExecutionReport,
) -> Result<(), HalError> {
    report
        .outcomes()
        .iter()
        .filter(|outcome| {
            matches!(
                outcome.phase(),
                Some(DvrPlaybackWorkerCleanupPhase::Join)
                    | Some(DvrPlaybackWorkerCleanupPhase::JoinTimeout)
                    | Some(DvrPlaybackWorkerCleanupPhase::Terminal)
            )
        })
        .find_map(|outcome| outcome.result().err())
        .map_or(Ok(()), Err)
}

pub(crate) fn stop_dvr_playback_worker(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    operation: DvrPlaybackWorkerCleanupOperation,
) -> DvrPlaybackWorkerCleanupExecutionReport {
    let (attempt_id, mut report) = begin_cleanup_attempt(context, operation);
    let (_lifecycle, lifecycle_result) = context.dvr_playback_worker_lifecycle_lock_for_cleanup();
    push_cleanup_step(
        &mut report,
        attempt_id,
        operation,
        DvrPlaybackWorkerCleanupTarget::Store,
        DvrPlaybackWorkerCleanupPhase::LifecycleLock,
        lifecycle_result,
    );
    let (worker, store_result) =
        context.take_dvr_playback_worker_for_cleanup(DvrPlaybackWorkerKey::new(handle));
    push_cleanup_step(
        &mut report,
        attempt_id,
        operation,
        DvrPlaybackWorkerCleanupTarget::Store,
        DvrPlaybackWorkerCleanupPhase::StoreAccess,
        store_result,
    );
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
    push_cleanup_step(
        &mut report,
        attempt_id,
        operation,
        DvrPlaybackWorkerCleanupTarget::Store,
        DvrPlaybackWorkerCleanupPhase::LifecycleLock,
        lifecycle_result,
    );
    let (workers, store_result) = context.take_dvr_playback_workers_for_reset();
    push_cleanup_step(
        &mut report,
        attempt_id,
        operation,
        DvrPlaybackWorkerCleanupTarget::Store,
        DvrPlaybackWorkerCleanupPhase::StoreAccess,
        store_result,
    );
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
