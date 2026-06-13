use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{start_frontend_demux_live_pump_from_reader, FrontendRegistryEntry, TunerServiceRuntime};
use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, FrontendDevicePath, FrontendScanMode, FrontendTuneRequest, HalError,
    HalInternalKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeSnapshot;
use maleicacid_tuner_hal2_device::{
    FrontendBackendSession, FrontendBackendTunePlan, FrontendLivePumpJoinOutcome,
    FrontendRuntimeSnapshot, FrontendWorkerCancelReason, FrontendWorkerContext,
    FrontendWorkerKind, FrontendWorkerStartError, FrontendWorkerStopOutcome,
};

pub type FrontendScanEndNotifier = Arc<dyn Fn(i32, u64) -> Result<(), HalError> + Send + Sync + 'static>;

type SharedRuntime = Arc<Mutex<TunerServiceRuntime>>;

type DemuxSnapshotList = Vec<(crate::DemuxRuntimeId, DemuxRuntimeSnapshot)>;

fn lock_runtime<'a>(runtime: &'a SharedRuntime, context: &'static str) -> Result<std::sync::MutexGuard<'a, TunerServiceRuntime>, HalError> {
    runtime.lock().map_err(|_| HalError::internal(HalInternalKind::InvariantViolation, context))
}

fn map_frontend_worker_start_error(error: FrontendWorkerStartError) -> HalError {
    match error {
        FrontendWorkerStartError::AlreadyRunning { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker is already running",
        ),
        FrontendWorkerStartError::SpawnFailed { detail } => HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("frontend worker spawn failed: {detail}"),
        ),
    }
}

fn mark_tune_worker_failed(
    runtime: &SharedRuntime,
    frontend_id: i32,
    generation: u64,
    error: HalError,
) -> Result<(), HalError> {
    let mut guard = lock_runtime(runtime, "service runtime lock poisoned while marking tune worker failure")?;
    guard.mark_frontend_tune_worker_failed(frontend_id, generation, error)
}

pub fn start_frontend_backend_tune_worker(
    runtime: SharedRuntime,
    frontend_id: i32,
    entry: FrontendRegistryEntry,
    request: FrontendTuneRequest,
    kind: FrontendWorkerKind,
) -> Result<(), HalError> {
    let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
    if guard.frontend_has_same_active_tune(frontend_id, &request)? {
        return Ok(());
    }
    let snapshot = guard.frontend_runtime_snapshot(frontend_id)?;
    let demux_snapshots = guard.bound_demux_runtime_snapshots(frontend_id)?;
    let generation = guard.prepare_frontend_worker_generation(frontend_id, kind)?;
    if let Err(error) = guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id) {
        guard.restore_frontend_runtime_snapshot(frontend_id, snapshot)?;
        guard.restore_bound_demux_runtime_snapshots(demux_snapshots.clone())?;
        return Err(error);
    }
    if let Err(error) =
        guard.install_frontend_live_reader_descriptor_for_generation(frontend_id, kind, generation)
    {
        guard.restore_frontend_runtime_snapshot(frontend_id, snapshot)?;
        guard.restore_bound_demux_runtime_snapshots(demux_snapshots.clone())?;
        return Err(error);
    }
    let plan = FrontendBackendTunePlan::new(
        frontend_id,
        generation,
        entry.backend,
        FrontendDevicePath::new(entry.device_path.clone()),
        request.clone(),
    );
    let previous_tune_for_worker = snapshot.active_tune_request.clone();
    let frontend_snapshot_for_worker = snapshot.clone();
    let demux_snapshots_for_worker = demux_snapshots.clone();
    let runtime_for_worker = Arc::clone(&runtime);
    if let Err(error) = guard.start_frontend_worker(frontend_id, kind, generation, move |ctx| {
        plan.validate_worker_generation(ctx.generation())?;
        let session = match FrontendBackendSession::open_and_submit_with_previous_report(
            &plan,
            previous_tune_for_worker,
        ) {
            Ok(session) => session,
            Err(failure) if failure.rollback_succeeded => {
                let report_error = failure.error;
                let mut guard = lock_runtime(
                    &runtime_for_worker,
                    "service runtime lock poisoned while restoring tune rollback state",
                )?;
                guard.restore_frontend_runtime_snapshot(
                    frontend_id,
                    frontend_snapshot_for_worker.clone(),
                )?;
                guard.restore_bound_demux_runtime_snapshots(demux_snapshots_for_worker.clone())?;
                return Err(report_error);
            }
            Err(failure) => {
                let report_error = failure.error.clone();
                match mark_tune_worker_failed(
                    &runtime_for_worker,
                    frontend_id,
                    generation,
                    failure.error,
                ) {
                    Ok(()) => return Err(report_error),
                    Err(mark_error) => return Err(mark_error),
                }
            }
        };
        {
            let mut guard = lock_runtime(
                &runtime_for_worker,
                "service runtime lock poisoned while recording frontend signal state",
            )?;
            guard.record_frontend_signal_state(
                frontend_id,
                generation,
                session.initial_signal_state(),
            )?;
        }
        let mut live_pump = None;
        while !ctx.cancel_requested() {
            if live_pump.is_none() {
                let live_reader_descriptor = {
                    let guard = lock_runtime(
                        &runtime_for_worker,
                        "service runtime lock poisoned while checking frontend live pump readiness",
                    )?;
                    guard.frontend_live_reader_descriptor_for_live_pump(frontend_id)?
                };
                if let Some(descriptor) = live_reader_descriptor {
                    let reader = session.open_live_reader(&descriptor)?;
                    live_pump = Some(start_frontend_demux_live_pump_from_reader(
                        Arc::clone(&runtime_for_worker),
                        frontend_id,
                        reader,
                    )?);
                }
            }
            if let Some(owner) = live_pump.as_mut() {
                match owner.collect_if_finished() {
                    FrontendLivePumpJoinOutcome::Running => {}
                    FrontendLivePumpJoinOutcome::Completed(result) => {
                        let report = result?;
                        let mut guard = lock_runtime(
                            &runtime_for_worker,
                            "service runtime lock poisoned while recording completed live pump report",
                        )?;
                        guard.record_live_pump_report(
                            frontend_id,
                            generation,
                            report,
                            ctx.cancel_reason(),
                        )?;
                        return session.stop();
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Some(owner) = live_pump {
            let report = owner.join_after_stop()?;
            let mut guard = lock_runtime(
                &runtime_for_worker,
                "service runtime lock poisoned while recording stopped live pump report",
            )?;
            guard.record_live_pump_report(frontend_id, generation, report, ctx.cancel_reason())?;
        }
        session.stop()
    }) {
        guard.restore_frontend_runtime_snapshot(frontend_id, snapshot)?;
        guard.restore_bound_demux_runtime_snapshots(demux_snapshots.clone())?;
        return Err(map_frontend_worker_start_error(error));
    }
    guard.commit_frontend_active_tune_request(frontend_id, generation, request)
}

fn run_frontend_backend_scan_session_worker(
    runtime: SharedRuntime,
    ctx: FrontendWorkerContext,
    backend: FrontendBackendKind,
    device_path: FrontendDevicePath,
    candidates: Vec<FrontendTuneRequest>,
    previous_request: Option<FrontendTuneRequest>,
    frontend_snapshot: FrontendRuntimeSnapshot,
    demux_snapshots: DemuxSnapshotList,
    scan_end_notifier: FrontendScanEndNotifier,
) -> Result<(), HalError> {
    for candidate in candidates {
        if ctx.cancel_requested() {
            return Ok(());
        }
        let plan = FrontendBackendTunePlan::new(
            ctx.frontend_id(),
            ctx.generation(),
            backend,
            device_path.clone(),
            candidate,
        );
        plan.validate_worker_generation(ctx.generation())?;
        let session = match FrontendBackendSession::open_and_submit_with_previous_report(
            &plan,
            previous_request.clone(),
        ) {
            Ok(session) => session,
            Err(failure) if failure.rollback_succeeded => {
                let mut guard = lock_runtime(
                    &runtime,
                    "service runtime lock poisoned while restoring scan rollback state",
                )?;
                guard.restore_frontend_runtime_snapshot(ctx.frontend_id(), frontend_snapshot.clone())?;
                guard.restore_bound_demux_runtime_snapshots(demux_snapshots.clone())?;
                return Err(failure.error);
            }
            Err(failure) => {
                let mut guard = lock_runtime(
                    &runtime,
                    "service runtime lock poisoned while marking scan backend failure",
                )?;
                guard.mark_frontend_scan_session_backend_failed(ctx.frontend_id(), ctx.generation())?;
                return Err(failure.error);
            }
        };
        {
            let mut guard = lock_runtime(
                &runtime,
                "service runtime lock poisoned while recording scan signal state",
            )?;
            guard.record_frontend_signal_state(
                ctx.frontend_id(),
                ctx.generation(),
                session.initial_signal_state(),
            )?;
        }
        for _ in 0..5 {
            if ctx.cancel_requested() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        session.stop()?;
        if ctx.cancel_requested() {
            return Ok(());
        }
        let mut guard = lock_runtime(
            &runtime,
            "service runtime lock poisoned while advancing scan session",
        )?;
        let has_next = guard.advance_frontend_scan_session_after_candidate(
            ctx.frontend_id(),
            ctx.generation(),
        )?;
        drop(guard);
        if !has_next {
            scan_end_notifier(ctx.frontend_id(), ctx.generation())?;
            return Ok(());
        }
    }
    Ok(())
}

pub fn start_frontend_backend_scan_session_worker(
    runtime: SharedRuntime,
    frontend_id: i32,
    entry: FrontendRegistryEntry,
    request: FrontendTuneRequest,
    scan_mode: FrontendScanMode,
    candidates: Vec<FrontendTuneRequest>,
    scan_end_notifier: FrontendScanEndNotifier,
) -> Result<(), HalError> {
    stop_frontend_scan_worker(
        Arc::clone(&runtime),
        frontend_id,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    )?;
    let fingerprint = format!("{:?}:{:?}", scan_mode, request);
    let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
    let snapshot = guard.frontend_runtime_snapshot(frontend_id)?;
    let demux_snapshots = guard.bound_demux_runtime_snapshots(frontend_id)?;
    let generation = guard.prepare_frontend_worker_generation(frontend_id, FrontendWorkerKind::Scan)?;
    if let Err(error) = guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id) {
        guard.restore_frontend_runtime_snapshot(frontend_id, snapshot)?;
        guard.restore_bound_demux_runtime_snapshots(demux_snapshots.clone())?;
        return Err(error);
    }
    if let Err(error) = guard.install_frontend_live_reader_descriptor_for_generation(
        frontend_id,
        FrontendWorkerKind::Scan,
        generation,
    ) {
        guard.restore_frontend_runtime_snapshot(frontend_id, snapshot)?;
        guard.restore_bound_demux_runtime_snapshots(demux_snapshots.clone())?;
        return Err(error);
    }
    if let Err(error) = guard.begin_frontend_scan_session(
        frontend_id,
        generation,
        fingerprint,
        candidates.clone(),
    ) {
        guard.restore_frontend_runtime_snapshot(frontend_id, snapshot)?;
        guard.restore_bound_demux_runtime_snapshots(demux_snapshots.clone())?;
        return Err(error);
    }
    let previous_tune_for_worker = snapshot.active_tune_request.clone();
    let frontend_snapshot_for_worker = snapshot.clone();
    let demux_snapshots_for_worker = demux_snapshots.clone();
    let runtime_for_worker = Arc::clone(&runtime);
    let backend = entry.backend;
    let device_path = FrontendDevicePath::new(entry.device_path.clone());
    if let Err(error) = guard.start_frontend_worker(
        frontend_id,
        FrontendWorkerKind::Scan,
        generation,
        move |ctx| {
            run_frontend_backend_scan_session_worker(
                runtime_for_worker,
                ctx,
                backend,
                device_path,
                candidates,
                previous_tune_for_worker,
                frontend_snapshot_for_worker,
                demux_snapshots_for_worker,
                scan_end_notifier,
            )
        },
    ) {
        guard.restore_frontend_runtime_snapshot(frontend_id, snapshot)?;
        guard.restore_bound_demux_runtime_snapshots(demux_snapshots.clone())?;
        return Err(map_frontend_worker_start_error(error));
    }
    Ok(())
}

pub fn stop_frontend_worker(
    runtime: SharedRuntime,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    reason: FrontendWorkerCancelReason,
) -> Result<FrontendWorkerStopOutcome, HalError> {
    let outcome = lock_runtime(&runtime, "service runtime lock poisoned")?
        .request_frontend_worker_stop_and_join(frontend_id, kind, reason);
    if let FrontendWorkerStopOutcome::Completed { result: Err(error), .. } = &outcome {
        return Err(error.clone());
    }
    Ok(outcome)
}

fn record_scan_cancelled_terminal_event(
    runtime: &SharedRuntime,
    frontend_id: i32,
    generation: u64,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    lock_runtime(runtime, "service runtime lock poisoned")?
        .cancel_frontend_scan_session(frontend_id, generation, reason)
}

fn record_scan_cancelled_from_stop_outcome(
    runtime: &SharedRuntime,
    frontend_id: i32,
    outcome: &FrontendWorkerStopOutcome,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    let generation = match outcome {
        FrontendWorkerStopOutcome::NotRunning => return Ok(()),
        FrontendWorkerStopOutcome::CancelRequested { generation, .. }
        | FrontendWorkerStopOutcome::Completed { generation, result: Ok(()), .. } => *generation,
        FrontendWorkerStopOutcome::Completed { result: Err(error), .. } => return Err(error.clone()),
    };
    record_scan_cancelled_terminal_event(runtime, frontend_id, generation, reason)
}

pub fn stop_frontend_tune_worker(
    runtime: SharedRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    stop_frontend_worker(runtime, frontend_id, FrontendWorkerKind::Tune, reason).map(|_| ())
}

pub fn stop_frontend_scan_worker(
    runtime: SharedRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    let outcome = stop_frontend_worker(Arc::clone(&runtime), frontend_id, FrontendWorkerKind::Scan, reason)?;
    record_scan_cancelled_from_stop_outcome(&runtime, frontend_id, &outcome, reason)?;
    if !matches!(outcome, FrontendWorkerStopOutcome::NotRunning) {
        lock_runtime(&runtime, "service runtime lock poisoned")?
            .clear_frontend_live_reader_descriptor_and_idle(frontend_id)?;
    }
    Ok(())
}

pub fn clear_frontend_live_reader_descriptor(
    runtime: SharedRuntime,
    frontend_id: i32,
) -> Result<(), HalError> {
    lock_runtime(&runtime, "service runtime lock poisoned")?
        .clear_frontend_live_reader_descriptor_and_idle(frontend_id)
}

pub fn stop_frontend_live_data_and_unbind(
    runtime: SharedRuntime,
    frontend_id: i32,
) -> Result<(), HalError> {
    lock_runtime(&runtime, "service runtime lock poisoned")?
        .stop_frontend_live_data_and_unbind(frontend_id)
        .map(|_| ())
}

pub fn close_frontend_live_data_and_unbind(
    runtime: SharedRuntime,
    frontend_id: i32,
) -> Result<(), HalError> {
    lock_runtime(&runtime, "service runtime lock poisoned")?
        .close_frontend_live_data_and_unbind(frontend_id)
        .map(|_| ())
}

pub fn close_frontend_workers_and_live_data(
    runtime: SharedRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    let tune_outcome = stop_frontend_worker(Arc::clone(&runtime), frontend_id, FrontendWorkerKind::Tune, reason);
    let scan_outcome = stop_frontend_worker(Arc::clone(&runtime), frontend_id, FrontendWorkerKind::Scan, reason);

    let mut first_error = None;
    if let Ok(outcome) = &scan_outcome {
        if let Err(error) = record_scan_cancelled_from_stop_outcome(&runtime, frontend_id, outcome, reason) {
            first_error = Some(error);
        }
    }
    if let Err(error) = tune_outcome {
        if first_error.is_none() {
            first_error = Some(error);
        }
    }
    if let Err(error) = scan_outcome {
        if first_error.is_none() {
            first_error = Some(error);
        }
    }
    close_frontend_live_data_and_unbind(Arc::clone(&runtime), frontend_id)?;
    if let Some(error) = first_error {
        Err(error)
    } else {
        Ok(())
    }
}
