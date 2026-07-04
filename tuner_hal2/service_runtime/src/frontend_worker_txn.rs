use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::registry::FrontendRegistryEntry;
use crate::{
    object_lifecycle::{aidl_object_live, aidl_public_runtime_id_for_close_cleanup},
    object_method_txn::ObjectMethodExecutionToken,
    start_frontend_demux_live_pump_from_reader, TunerServiceRuntime,
};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FirstErrorCollector, FrontendBackendKind, FrontendDevicePath,
    FrontendScanMode, FrontendTuneRequest, HalError, HalInternalKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_demux::DemuxRuntimeSnapshot;
use maleicacid_tuner_hal2_device::{
    FrontendBackendSession, FrontendBackendTunePlan, FrontendLivePumpJoinOutcome,
    FrontendLivePumpOwner, FrontendRuntimeSnapshot, FrontendWorkerCancelReason,
    FrontendWorkerContext, FrontendWorkerKind, FrontendWorkerStartError, FrontendWorkerStopOutcome,
    FrontendWorkerStopTicket,
};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

pub type FrontendScanEndNotifier =
    Arc<dyn Fn(i32, u64) -> Result<(), HalError> + Send + Sync + 'static>;

type SharedRuntime = Arc<Mutex<TunerServiceRuntime>>;

type DemuxSnapshotList = Vec<(crate::registry::DemuxRuntimeId, DemuxRuntimeSnapshot)>;

type BoundDemuxGenerationSnapshot = Vec<(crate::registry::DemuxRuntimeId, u64)>;

struct FrontendWorkerReplacementTicket {
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    worker_generation: Option<u64>,
    frontend_snapshot: FrontendRuntimeSnapshot,
    bound_demux_generations: BoundDemuxGenerationSnapshot,
    stop_ticket: FrontendWorkerStopTicket,
}

struct FrontendWorkerStopObjectTicket {
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    reason: FrontendWorkerCancelReason,
    worker_generation: Option<u64>,
    frontend_snapshot: FrontendRuntimeSnapshot,
    bound_demux_generations: BoundDemuxGenerationSnapshot,
    stop_ticket: FrontendWorkerStopTicket,
}

fn ensure_frontend_ticket_still_targets_object(
    guard: &TunerServiceRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
) -> Result<(), HalError> {
    ensure_frontend_object_still_live(guard, object_id, object_generation)?;
    let (resolved_frontend_id, _) =
        resolve_frontend_object_for_method(guard, object_id, object_generation)?;
    if resolved_frontend_id != frontend_id {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker ticket target changed after external join",
        ));
    }
    Ok(())
}

fn frontend_worker_stop_outcome_generation(outcome: &FrontendWorkerStopOutcome) -> Option<u64> {
    match outcome {
        FrontendWorkerStopOutcome::NotRunning => None,
        FrontendWorkerStopOutcome::CancelRequested { generation, .. }
        | FrontendWorkerStopOutcome::Completed { generation, .. }
        | FrontendWorkerStopOutcome::StopRequestFailed { generation, .. } => Some(*generation),
    }
}

fn bound_demux_generation_snapshot(snapshots: &DemuxSnapshotList) -> BoundDemuxGenerationSnapshot {
    let mut generations = snapshots
        .iter()
        .map(|(demux_id, snapshot)| (*demux_id, snapshot.generation()))
        .collect::<Vec<_>>();
    generations.sort();
    generations
}

fn current_bound_demux_generation_snapshot(
    guard: &TunerServiceRuntime,
    frontend_id: i32,
) -> Result<BoundDemuxGenerationSnapshot, HalError> {
    guard
        .query()
        .bound_demux_runtime_snapshots(frontend_id)
        .map(|snapshots| bound_demux_generation_snapshot(&snapshots))
}

fn ensure_frontend_join_snapshot_still_matches(
    guard: &TunerServiceRuntime,
    frontend_id: i32,
    expected_frontend: &FrontendRuntimeSnapshot,
    expected_demux_generations: &BoundDemuxGenerationSnapshot,
) -> Result<(), HalError> {
    let current_frontend = guard.query().frontend_runtime_snapshot(frontend_id)?;
    if current_frontend.generation != expected_frontend.generation
        || current_frontend.live_reader_descriptor != expected_frontend.live_reader_descriptor
        || current_frontend.scan_session != expected_frontend.scan_session
    {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker ticket runtime snapshot changed during external join",
        ));
    }
    let current_demux_generations = current_bound_demux_generation_snapshot(guard, frontend_id)?;
    if current_demux_generations != *expected_demux_generations {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker ticket bound demux snapshot changed during external join",
        ));
    }
    Ok(())
}

fn complete_frontend_worker_replacement_ticket<'a>(
    runtime: &'a SharedRuntime,
    ticket: FrontendWorkerReplacementTicket,
    context: &'static str,
) -> Result<
    (
        MutexGuard<'a, TunerServiceRuntime>,
        i32,
        FrontendWorkerStopOutcome,
        FrontendRuntimeSnapshot,
        DemuxSnapshotList,
    ),
    HalError,
> {
    let FrontendWorkerReplacementTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        worker_generation,
        frontend_snapshot,
        bound_demux_generations,
        stop_ticket,
    } = ticket;
    let stop_outcome = stop_ticket.complete();
    if let Some(error) = frontend_worker_stop_failure(&stop_outcome) {
        return Err(error);
    }
    let guard = lock_runtime(runtime, context)?;
    ensure_frontend_ticket_still_targets_object(&guard, object_id, object_generation, frontend_id)?;
    ensure_frontend_join_snapshot_still_matches(
        &guard,
        frontend_id,
        &frontend_snapshot,
        &bound_demux_generations,
    )?;
    if frontend_worker_stop_outcome_generation(&stop_outcome) != worker_generation {
        return Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend worker replacement ticket generation mismatch",
        ));
    }
    if !matches!(stop_outcome, FrontendWorkerStopOutcome::NotRunning) {
        match &stop_outcome {
            FrontendWorkerStopOutcome::CancelRequested {
                kind: outcome_kind, ..
            }
            | FrontendWorkerStopOutcome::Completed {
                kind: outcome_kind, ..
            } => {
                if *outcome_kind != kind {
                    return Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend worker replacement ticket kind mismatch",
                    ));
                }
            }
            FrontendWorkerStopOutcome::NotRunning
            | FrontendWorkerStopOutcome::StopRequestFailed { .. } => {}
        }
    }
    let demux_snapshots = guard.query().bound_demux_runtime_snapshots(frontend_id)?;
    Ok((
        guard,
        frontend_id,
        stop_outcome,
        frontend_snapshot,
        demux_snapshots,
    ))
}

fn prepare_frontend_worker_stop_object_ticket(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    kind: FrontendWorkerKind,
    reason: FrontendWorkerCancelReason,
) -> Result<FrontendWorkerStopObjectTicket, HalError> {
    let (frontend_id, _) =
        resolve_frontend_object_for_method(runtime, object_id, object_generation)?;
    let frontend_snapshot = runtime.query().frontend_runtime_snapshot(frontend_id)?;
    let demux_snapshots = runtime.query().bound_demux_runtime_snapshots(frontend_id)?;
    let bound_demux_generations = bound_demux_generation_snapshot(&demux_snapshots);
    let stop_ticket =
        runtime
            .frontend_txn()
            .request_worker_stop_for_join(frontend_id, kind, reason);
    let worker_generation = stop_ticket.worker_generation();
    Ok(FrontendWorkerStopObjectTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        reason,
        worker_generation,
        frontend_snapshot,
        bound_demux_generations,
        stop_ticket,
    })
}

fn complete_frontend_worker_stop_object_ticket<'a>(
    runtime: &'a SharedRuntime,
    ticket: FrontendWorkerStopObjectTicket,
    context: &'static str,
) -> Result<
    (
        MutexGuard<'a, TunerServiceRuntime>,
        i32,
        FrontendWorkerCancelReason,
        FrontendWorkerStopOutcome,
    ),
    HalError,
> {
    let FrontendWorkerStopObjectTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        reason,
        worker_generation,
        frontend_snapshot,
        bound_demux_generations,
        stop_ticket,
    } = ticket;
    let stop_outcome = stop_ticket.complete();
    if let Some(error) = frontend_worker_stop_request_failure(&stop_outcome) {
        return Err(error);
    }
    let guard = lock_runtime(runtime, context)?;
    ensure_frontend_ticket_still_targets_object(&guard, object_id, object_generation, frontend_id)?;
    ensure_frontend_join_snapshot_still_matches(
        &guard,
        frontend_id,
        &frontend_snapshot,
        &bound_demux_generations,
    )?;
    if frontend_worker_stop_outcome_generation(&stop_outcome) != worker_generation {
        return Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend worker stop ticket generation mismatch",
        ));
    }
    if !matches!(stop_outcome, FrontendWorkerStopOutcome::NotRunning) {
        match &stop_outcome {
            FrontendWorkerStopOutcome::CancelRequested {
                kind: outcome_kind, ..
            }
            | FrontendWorkerStopOutcome::Completed {
                kind: outcome_kind, ..
            } => {
                if *outcome_kind != kind {
                    return Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend worker stop ticket kind mismatch",
                    ));
                }
            }
            FrontendWorkerStopOutcome::NotRunning
            | FrontendWorkerStopOutcome::StopRequestFailed { .. } => {}
        }
    }
    Ok((guard, frontend_id, reason, stop_outcome))
}

#[derive(Debug)]
pub struct FrontendCloseCleanupReport {
    pub frontend_id: i32,
    pub closed_lnb_ids: Vec<i32>,
    pub cleanup_result: Result<(), HalError>,
}

fn lock_runtime<'a>(
    runtime: &'a SharedRuntime,
    context: &'static str,
) -> Result<std::sync::MutexGuard<'a, TunerServiceRuntime>, HalError> {
    runtime
        .lock()
        .map_err(|_| HalError::internal(HalInternalKind::InvariantViolation, context))
}

fn map_frontend_worker_start_error(error: FrontendWorkerStartError) -> HalError {
    match error {
        FrontendWorkerStartError::AlreadyRunning { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker is already running",
        ),
        FrontendWorkerStartError::CompletedFailurePending { detail, .. } => HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("frontend worker previous failure is pending and must be reported before replacement: {detail}"),
        ),
        FrontendWorkerStartError::SpawnFailed { detail } => HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("frontend worker spawn failed: {detail}"),
        ),
    }
}

fn compose_frontend_cleanup_error(
    context: &'static str,
    primary: HalError,
    cleanup: HalError,
) -> HalError {
    compose_primary_cleanup_failure(context, primary, cleanup)
}

fn restore_frontend_state_after_primary_failure(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    frontend_snapshot: FrontendRuntimeSnapshot,
    demux_snapshots: DemuxSnapshotList,
    primary: HalError,
    context: &'static str,
) -> HalError {
    let mut cleanup_collector = FirstErrorCollector::new();
    cleanup_collector.push_result(
        guard
            .frontend_txn()
            .restore_frontend_runtime_snapshot(frontend_id, frontend_snapshot),
    );
    cleanup_collector.push_result(
        guard
            .frontend_txn()
            .restore_bound_demux_runtime_snapshots(demux_snapshots),
    );
    match cleanup_collector.into_result() {
        Ok(()) => primary,
        Err(cleanup) => compose_frontend_cleanup_error(context, primary, cleanup),
    }
}

fn finish_backend_session_after_worker_body(
    session: FrontendBackendSession,
    body_result: Result<(), HalError>,
) -> Result<(), HalError> {
    let stop_result = session.stop();
    match (body_result, stop_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(stop_error)) => Err(stop_error),
        (Err(primary), Ok(())) => Err(primary),
        (Err(primary), Err(stop_error)) => Err(compose_frontend_cleanup_error(
            "frontend backend session stop failed after worker body error",
            primary,
            stop_error,
        )),
    }
}

fn stop_live_pump_after_worker_error(
    live_pump: &mut Option<FrontendLivePumpOwner>,
    body_result: &mut Result<(), HalError>,
) {
    if body_result.is_ok() {
        return;
    }
    let Some(owner) = live_pump.take() else {
        return;
    };
    if let Err(stop_error) = owner.join_after_stop() {
        let primary = match std::mem::replace(body_result, Ok(())) {
            Err(error) => error,
            Ok(()) => return,
        };
        *body_result = Err(compose_frontend_cleanup_error(
            "frontend live pump stop failed after worker body error",
            primary,
            stop_error,
        ));
    }
}

fn frontend_worker_stop_failure(outcome: &FrontendWorkerStopOutcome) -> Option<HalError> {
    match outcome {
        FrontendWorkerStopOutcome::StopRequestFailed { error, .. }
        | FrontendWorkerStopOutcome::Completed {
            result: Err(error), ..
        } => Some(error.clone()),
        _ => None,
    }
}

fn frontend_worker_stop_request_failure(outcome: &FrontendWorkerStopOutcome) -> Option<HalError> {
    match outcome {
        FrontendWorkerStopOutcome::StopRequestFailed { error, .. } => Some(error.clone()),
        _ => None,
    }
}

fn collect_frontend_worker_stop_error(
    collector: &mut FirstErrorCollector<HalError>,
    outcome: &Result<FrontendWorkerStopOutcome, HalError>,
) {
    match outcome {
        Ok(outcome) => {
            if let Some(error) = frontend_worker_stop_failure(outcome) {
                collector.push_error(error);
            }
        }
        Err(error) => collector.push_error(error.clone()),
    }
}

fn resolve_frontend_object_for_method(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<(i32, FrontendRegistryEntry), HalError> {
    let entry = runtime.frontend_entry_for_aidl_object(object_id, generation)?;
    Ok((entry.id.0, entry))
}

fn ensure_frontend_object_still_live(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<(), HalError> {
    aidl_object_live(runtime, object_id, generation, AidlObjectKind::Frontend)
}

fn resolve_frontend_object_for_close_cleanup(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<(i32, FrontendRegistryEntry), HalError> {
    let frontend_id = aidl_public_runtime_id_for_close_cleanup(
        runtime,
        object_id,
        generation,
        AidlObjectKind::Frontend,
    )?;
    let entry = runtime
        .frontend_entry(frontend_id)
        .ok_or_else(|| HalError::Unsupported("frontend runtime entry is not available"))?;
    Ok((frontend_id, entry))
}

fn record_scan_cancelled_from_stop_outcome_locked(
    runtime: &mut TunerServiceRuntime,
    frontend_id: i32,
    outcome: &FrontendWorkerStopOutcome,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    let generation = match outcome {
        FrontendWorkerStopOutcome::NotRunning => return Ok(()),
        FrontendWorkerStopOutcome::StopRequestFailed { error, .. } => return Err(error.clone()),
        FrontendWorkerStopOutcome::CancelRequested { generation, .. }
        | FrontendWorkerStopOutcome::Completed { generation, .. } => *generation,
    };
    runtime
        .frontend_txn()
        .cancel_frontend_scan_session(frontend_id, generation, reason)
}

fn mark_tune_worker_failed(
    runtime: &SharedRuntime,
    frontend_id: i32,
    generation: u64,
    error: HalError,
) -> Result<(), HalError> {
    let mut guard = lock_runtime(
        runtime,
        "service runtime lock poisoned while marking tune worker failure",
    )?;
    guard
        .frontend_txn()
        .mark_frontend_tune_worker_failed(frontend_id, generation, error)
}

fn rollback_started_tune_worker_after_commit_failure(
    runtime: &SharedRuntime,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    snapshot: FrontendRuntimeSnapshot,
    demux_snapshots: DemuxSnapshotList,
    commit_error: HalError,
) -> HalError {
    let mut rollback_collector = FirstErrorCollector::new();
    match stop_frontend_worker(
        Arc::clone(runtime),
        frontend_id,
        kind,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    ) {
        Ok(outcome) => {
            if let Some(error) = frontend_worker_stop_failure(&outcome) {
                rollback_collector.push_error(error);
            }
        }
        Err(error) => rollback_collector.push_error(error),
    }

    match lock_runtime(
        runtime,
        "service runtime lock poisoned while rolling back tune commit failure",
    ) {
        Ok(mut guard) => {
            rollback_collector.push_result(
                guard
                    .frontend_txn()
                    .restore_frontend_runtime_snapshot(frontend_id, snapshot),
            );
            rollback_collector.push_result(
                guard
                    .frontend_txn()
                    .restore_bound_demux_runtime_snapshots(demux_snapshots),
            );
        }
        Err(error) => rollback_collector.push_error(error),
    }

    match rollback_collector.into_result() {
        Err(secondary) => compose_frontend_cleanup_error(
            "frontend tune commit failed after worker start",
            commit_error,
            secondary,
        ),
        Ok(()) => commit_error,
    }
}

pub(crate) fn request_tune_worker_replacement_stop(
    runtime: &mut TunerServiceRuntime,
    frontend_id: i32,
) -> FrontendWorkerStopTicket {
    runtime.frontend_txn().request_worker_stop_for_join(
        frontend_id,
        FrontendWorkerKind::Tune,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    )
}

pub fn start_frontend_backend_tune_worker(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    request: FrontendTuneRequest,
    kind: FrontendWorkerKind,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
    dispatch.consume_for_object(
        &mut guard,
        object_id,
        object_generation,
        AidlObjectKind::Frontend,
    )?;
    let (frontend_id, _resolved_entry) =
        resolve_frontend_object_for_method(&guard, object_id, object_generation)?;
    let frontend_snapshot = guard.query().frontend_runtime_snapshot(frontend_id)?;
    let demux_snapshots = guard.query().bound_demux_runtime_snapshots(frontend_id)?;
    let bound_demux_generations = bound_demux_generation_snapshot(&demux_snapshots);
    let stop_ticket = request_tune_worker_replacement_stop(&mut guard, frontend_id);
    let replacement_ticket = FrontendWorkerReplacementTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        worker_generation: stop_ticket.worker_generation(),
        frontend_snapshot,
        bound_demux_generations,
        stop_ticket,
    };
    drop(guard);
    let (mut guard, frontend_id, _stop_outcome, snapshot, demux_snapshots) =
        complete_frontend_worker_replacement_ticket(
            &runtime,
            replacement_ticket,
            "service runtime lock poisoned after tune worker join",
        )?;
    let entry = guard.validate_frontend_request_for_id(frontend_id, &request)?;
    let generation = guard
        .frontend_txn()
        .prepare_frontend_worker_generation(frontend_id, kind)?;
    if let Err(error) = guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id) {
        return Err(restore_frontend_state_after_primary_failure(
            &mut guard,
            frontend_id,
            snapshot,
            demux_snapshots.clone(),
            error,
            "frontend tune start reset rollback",
        ));
    }
    if let Err(error) = guard
        .frontend_txn()
        .install_frontend_live_reader_descriptor_for_generation(frontend_id, kind, generation)
    {
        return Err(restore_frontend_state_after_primary_failure(
            &mut guard,
            frontend_id,
            snapshot,
            demux_snapshots.clone(),
            error,
            "frontend tune live reader install rollback",
        ));
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
    if let Err(error) = guard.frontend_txn().start_worker(frontend_id, kind, generation, move |ctx| {
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
                return Err(restore_frontend_state_after_primary_failure(
                    &mut guard,
                    frontend_id,
                    frontend_snapshot_for_worker.clone(),
                    demux_snapshots_for_worker.clone(),
                    report_error,
                    "frontend tune backend rollback state restore",
                ));
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
                    Err(mark_error) => {
                        return Err(compose_frontend_cleanup_error(
                            "frontend tune backend failure marking failed",
                            report_error,
                            mark_error,
                        ));
                    }
                }
            }
        };
        let mut live_pump = None;
        let mut body_result = (|| {
            {
                let mut guard = lock_runtime(
                    &runtime_for_worker,
                    "service runtime lock poisoned while recording frontend signal state",
                )?;
                guard.frontend_txn().record_frontend_signal_state(
                    frontend_id,
                    generation,
                    session.initial_signal_state(),
                )?;
            }
            while !ctx.cancel_requested() {
                if live_pump.is_none() {
                    let live_reader_descriptor = {
                        let guard = lock_runtime(
                            &runtime_for_worker,
                            "service runtime lock poisoned while checking frontend live pump readiness",
                        )?;
                        guard.query().frontend_live_reader_descriptor_for_live_pump(frontend_id)?
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
                let completed_live_pump = live_pump
                    .as_mut()
                    .and_then(|owner| match owner.collect_if_finished() {
                        FrontendLivePumpJoinOutcome::Running => None,
                        FrontendLivePumpJoinOutcome::Completed(result) => Some(result),
                    });
                if let Some(result) = completed_live_pump {
                    live_pump = None;
                    let report = result?;
                    let mut guard = lock_runtime(
                        &runtime_for_worker,
                        "service runtime lock poisoned while recording completed live pump report",
                    )?;
                    guard.frontend_txn().record_live_pump_report(
                        frontend_id,
                        generation,
                        report,
                        ctx.cancel_reason()?,
                    )?;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if let Some(owner) = live_pump.take() {
                let report = owner.join_after_stop()?;
                let mut guard = lock_runtime(
                    &runtime_for_worker,
                    "service runtime lock poisoned while recording stopped live pump report",
                )?;
                guard.frontend_txn().record_live_pump_report(
                    frontend_id,
                    generation,
                    report,
                    ctx.cancel_reason()?,
                )?;
            }
            Ok(())
        })();
        stop_live_pump_after_worker_error(&mut live_pump, &mut body_result);
        finish_backend_session_after_worker_body(session, body_result)
    }) {
        let primary = map_frontend_worker_start_error(error);
        return Err(restore_frontend_state_after_primary_failure(
            &mut guard,
            frontend_id,
            snapshot,
            demux_snapshots.clone(),
            primary,
            "frontend tune worker start rollback",
        ));
    }
    if let Err(error) =
        guard
            .frontend_txn()
            .commit_frontend_active_tune_request(frontend_id, generation, request)
    {
        drop(guard);
        return Err(rollback_started_tune_worker_after_commit_failure(
            &runtime,
            frontend_id,
            kind,
            snapshot,
            demux_snapshots,
            error,
        ));
    }
    Ok(())
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
                return Err(restore_frontend_state_after_primary_failure(
                    &mut guard,
                    ctx.frontend_id(),
                    frontend_snapshot.clone(),
                    demux_snapshots.clone(),
                    failure.error,
                    "frontend scan backend rollback state restore",
                ));
            }
            Err(failure) => {
                let mut guard = lock_runtime(
                    &runtime,
                    "service runtime lock poisoned while marking scan backend failure",
                )?;
                let primary = failure.error;
                if let Err(mark_error) = guard
                    .frontend_txn()
                    .mark_frontend_scan_session_backend_failed(ctx.frontend_id(), ctx.generation())
                {
                    return Err(compose_frontend_cleanup_error(
                        "frontend scan backend failure marking failed",
                        primary,
                        mark_error,
                    ));
                }
                return Err(primary);
            }
        };
        let body_result = (|| {
            {
                let mut guard = lock_runtime(
                    &runtime,
                    "service runtime lock poisoned while recording scan signal state",
                )?;
                guard.frontend_txn().record_frontend_signal_state(
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
            Ok(())
        })();
        finish_backend_session_after_worker_body(session, body_result)?;
        if ctx.cancel_requested() {
            return Ok(());
        }
        let mut guard = lock_runtime(
            &runtime,
            "service runtime lock poisoned while advancing scan session",
        )?;
        let has_next = guard
            .frontend_txn()
            .advance_frontend_scan_session_after_candidate(ctx.frontend_id(), ctx.generation())?;
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
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    request: FrontendTuneRequest,
    scan_mode: FrontendScanMode,
    scan_end_notifier: FrontendScanEndNotifier,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let fingerprint = format!("{:?}:{:?}", scan_mode, request);
    let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
    dispatch.consume_for_object(
        &mut guard,
        object_id,
        object_generation,
        AidlObjectKind::Frontend,
    )?;
    let (frontend_id, _resolved_entry) =
        resolve_frontend_object_for_method(&guard, object_id, object_generation)?;
    let frontend_snapshot = guard.query().frontend_runtime_snapshot(frontend_id)?;
    let demux_snapshots = guard.query().bound_demux_runtime_snapshots(frontend_id)?;
    let bound_demux_generations = bound_demux_generation_snapshot(&demux_snapshots);
    let stop_ticket = guard.frontend_txn().request_worker_stop_for_join(
        frontend_id,
        FrontendWorkerKind::Scan,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    );
    let replacement_ticket = FrontendWorkerReplacementTicket {
        object_id,
        object_generation,
        frontend_id,
        kind: FrontendWorkerKind::Scan,
        worker_generation: stop_ticket.worker_generation(),
        frontend_snapshot,
        bound_demux_generations,
        stop_ticket,
    };
    drop(guard);
    let (mut guard, frontend_id, stop_outcome, snapshot, demux_snapshots) =
        complete_frontend_worker_replacement_ticket(
            &runtime,
            replacement_ticket,
            "service runtime lock poisoned after scan worker join",
        )?;
    let entry = guard.validate_frontend_request_for_id(frontend_id, &request)?;
    let candidates = guard.scan_candidates_for_frontend_entry(&entry, &request, scan_mode)?;
    let mut stop_collector = FirstErrorCollector::new();
    if let Some(error) = frontend_worker_stop_failure(&stop_outcome) {
        stop_collector.push_error(error);
    }
    stop_collector.push_result(record_scan_cancelled_from_stop_outcome_locked(
        &mut guard,
        frontend_id,
        &stop_outcome,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    ));
    stop_collector.into_result()?;
    let generation = guard
        .frontend_txn()
        .prepare_frontend_worker_generation(frontend_id, FrontendWorkerKind::Scan)?;
    if let Err(error) = guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id) {
        return Err(restore_frontend_state_after_primary_failure(
            &mut guard,
            frontend_id,
            snapshot,
            demux_snapshots.clone(),
            error,
            "frontend scan start reset rollback",
        ));
    }
    if let Err(error) = guard
        .frontend_txn()
        .install_frontend_live_reader_descriptor_for_generation(
            frontend_id,
            FrontendWorkerKind::Scan,
            generation,
        )
    {
        return Err(restore_frontend_state_after_primary_failure(
            &mut guard,
            frontend_id,
            snapshot,
            demux_snapshots.clone(),
            error,
            "frontend scan live reader install rollback",
        ));
    }
    if let Err(error) = guard.frontend_txn().begin_frontend_scan_session(
        frontend_id,
        generation,
        fingerprint,
        candidates.clone(),
    ) {
        return Err(restore_frontend_state_after_primary_failure(
            &mut guard,
            frontend_id,
            snapshot,
            demux_snapshots.clone(),
            error,
            "frontend scan session begin rollback",
        ));
    }
    let previous_tune_for_worker = snapshot.active_tune_request.clone();
    let frontend_snapshot_for_worker = snapshot.clone();
    let demux_snapshots_for_worker = demux_snapshots.clone();
    let runtime_for_worker = Arc::clone(&runtime);
    let backend = entry.backend;
    let device_path = FrontendDevicePath::new(entry.device_path.clone());
    if let Err(error) = guard.frontend_txn().start_worker(
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
        let primary = map_frontend_worker_start_error(error);
        return Err(restore_frontend_state_after_primary_failure(
            &mut guard,
            frontend_id,
            snapshot,
            demux_snapshots.clone(),
            primary,
            "frontend scan worker start rollback",
        ));
    }
    Ok(())
}

pub fn stop_frontend_worker(
    runtime: SharedRuntime,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    reason: FrontendWorkerCancelReason,
) -> Result<FrontendWorkerStopOutcome, HalError> {
    let ticket = {
        let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
        guard
            .frontend_txn()
            .request_worker_stop_for_join(frontend_id, kind, reason)
    };
    Ok(ticket.complete())
}

fn record_scan_cancelled_terminal_event(
    runtime: &SharedRuntime,
    frontend_id: i32,
    generation: u64,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    lock_runtime(runtime, "service runtime lock poisoned")?
        .frontend_txn()
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
        FrontendWorkerStopOutcome::StopRequestFailed { error, .. } => return Err(error.clone()),
        FrontendWorkerStopOutcome::CancelRequested { generation, .. }
        | FrontendWorkerStopOutcome::Completed { generation, .. } => *generation,
    };
    record_scan_cancelled_terminal_event(runtime, frontend_id, generation, reason)
}

pub fn stop_frontend_tune_object(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    reason: FrontendWorkerCancelReason,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let stop_ticket = {
        let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
        dispatch.consume_for_object(
            &mut guard,
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        prepare_frontend_worker_stop_object_ticket(
            &mut guard,
            object_id,
            object_generation,
            FrontendWorkerKind::Tune,
            reason,
        )?
    };
    let (mut guard, frontend_id, _reason, outcome) = complete_frontend_worker_stop_object_ticket(
        &runtime,
        stop_ticket,
        "service runtime lock poisoned after tune worker stop",
    )?;
    let mut cleanup_collector = FirstErrorCollector::new();
    if let Some(error) = frontend_worker_stop_failure(&outcome) {
        cleanup_collector.push_error(error);
    }
    cleanup_collector.push_result(
        guard
            .frontend_txn()
            .stop_frontend_live_data_and_unbind(frontend_id)
            .map(|_| ()),
    );
    cleanup_collector.into_result()
}

pub fn stop_frontend_scan_object(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    reason: FrontendWorkerCancelReason,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let stop_ticket = {
        let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
        dispatch.consume_for_object(
            &mut guard,
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        prepare_frontend_worker_stop_object_ticket(
            &mut guard,
            object_id,
            object_generation,
            FrontendWorkerKind::Scan,
            reason,
        )?
    };
    let (mut guard, frontend_id, reason, outcome) = complete_frontend_worker_stop_object_ticket(
        &runtime,
        stop_ticket,
        "service runtime lock poisoned after scan worker stop",
    )?;
    let mut cleanup_collector = FirstErrorCollector::new();
    if let Some(error) = frontend_worker_stop_failure(&outcome) {
        cleanup_collector.push_error(error);
    }
    cleanup_collector.push_result(record_scan_cancelled_from_stop_outcome_locked(
        &mut guard,
        frontend_id,
        &outcome,
        reason,
    ));
    if !matches!(outcome, FrontendWorkerStopOutcome::NotRunning) {
        cleanup_collector.push_result(
            guard
                .frontend_txn()
                .clear_frontend_live_reader_descriptor_and_idle(frontend_id),
        );
    }
    cleanup_collector.into_result()
}

pub fn close_frontend_live_data_and_unbind(
    runtime: SharedRuntime,
    frontend_id: i32,
) -> Result<(), HalError> {
    lock_runtime(&runtime, "service runtime lock poisoned")?
        .frontend_txn()
        .close_frontend_live_data_and_unbind(frontend_id)
        .map(|_| ())
}

pub fn cleanup_frontend_object_after_close_begin(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    reason: FrontendWorkerCancelReason,
) -> Result<FrontendCloseCleanupReport, HalError> {
    let (frontend_id, closed_lnb_ids, lnb_cleanup_result) = {
        let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
        let (frontend_id, _) =
            resolve_frontend_object_for_close_cleanup(&guard, object_id, object_generation)?;
        let (closed_lnb_ids, lnb_cleanup_result) =
            guard.close_lnb_from_frontend_owner_loss(frontend_id);
        (frontend_id, closed_lnb_ids, lnb_cleanup_result)
    };
    let worker_cleanup_result =
        close_frontend_workers_and_live_data(Arc::clone(&runtime), frontend_id, reason);
    let mut cleanup_collector = FirstErrorCollector::new();
    cleanup_collector.push_result(lnb_cleanup_result);
    cleanup_collector.push_result(worker_cleanup_result);
    Ok(FrontendCloseCleanupReport {
        frontend_id,
        closed_lnb_ids,
        cleanup_result: cleanup_collector.into_result(),
    })
}

pub fn close_frontend_workers_and_live_data(
    runtime: SharedRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    let tune_outcome = stop_frontend_worker(
        Arc::clone(&runtime),
        frontend_id,
        FrontendWorkerKind::Tune,
        reason,
    );
    let scan_outcome = stop_frontend_worker(
        Arc::clone(&runtime),
        frontend_id,
        FrontendWorkerKind::Scan,
        reason,
    );

    let mut cleanup_collector = FirstErrorCollector::new();
    collect_frontend_worker_stop_error(&mut cleanup_collector, &tune_outcome);
    collect_frontend_worker_stop_error(&mut cleanup_collector, &scan_outcome);
    if let Ok(outcome) = &scan_outcome {
        cleanup_collector.push_result(record_scan_cancelled_from_stop_outcome(
            &runtime,
            frontend_id,
            outcome,
            reason,
        ));
    }
    cleanup_collector.push_result(close_frontend_live_data_and_unbind(
        Arc::clone(&runtime),
        frontend_id,
    ));
    cleanup_collector.into_result()
}
