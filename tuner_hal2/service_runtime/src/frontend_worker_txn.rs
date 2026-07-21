use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::cleanup_execution::{
    CleanupExecutionDiagnosticSnapshot, CleanupExecutionReport, CleanupExecutionStepOutcome,
    SharedCleanupDiagnostics,
};
use crate::registry::FrontendRegistryEntry;
use crate::{
    object_lifecycle::{aidl_object_live, aidl_public_runtime_id_for_close_cleanup},
    object_method_txn::ObjectMethodExecutionToken,
    start_frontend_demux_live_pump_from_reader, TunerServiceRuntime,
};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FrontendBackendKind, FrontendDevicePath, FrontendScanMode,
    FrontendTuneRequest, HalError, HalInternalKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_demux::DemuxRuntimeRollbackToken;
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

type DemuxRollbackTokenList = Vec<(crate::registry::DemuxRuntimeId, DemuxRuntimeRollbackToken)>;
type SharedDemuxRollbackTokenList = Arc<Mutex<Option<DemuxRollbackTokenList>>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupDiagnosticKind {
    StopTuneObject,
    StopScanObject,
    TuneReplacementStop,
    ScanReplacementStop,
    TuneStartRollback,
    TuneWorkerStartRollback,
    TuneCommitRollback,
    TuneBackendRollbackStateRestore,
    ScanStartRollback,
    ScanWorkerStartRollback,
    ScanBackendRollbackStateRestore,
    FrontendClose,
    FrontendCloseOwnerLoss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupTarget {
    Object {
        frontend_id: i32,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
    },
    Frontend {
        frontend_id: i32,
    },
}

impl FrontendWorkerCleanupTarget {
    pub const fn object(
        frontend_id: i32,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
    ) -> Self {
        Self::Object {
            frontend_id,
            object_id,
            object_generation,
        }
    }

    pub const fn frontend(frontend_id: i32) -> Self {
        Self::Frontend { frontend_id }
    }

    pub fn frontend_id(&self) -> i32 {
        match *self {
            Self::Object { frontend_id, .. } | Self::Frontend { frontend_id } => frontend_id,
        }
    }

    pub fn object_id(&self) -> Option<AidlObjectId> {
        match *self {
            Self::Object { object_id, .. } => Some(object_id),
            Self::Frontend { .. } => None,
        }
    }

    pub fn object_generation(&self) -> Option<AidlObjectGeneration> {
        match *self {
            Self::Object {
                object_generation, ..
            } => Some(object_generation),
            Self::Frontend { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupWorkerGeneration {
    Known(u64),
    NotAvailable,
}

impl FrontendWorkerCleanupWorkerGeneration {
    pub const fn from_option(generation: Option<u64>) -> Self {
        match generation {
            Some(generation) => Self::Known(generation),
            None => Self::NotAvailable,
        }
    }

    pub const fn as_option(self) -> Option<u64> {
        match self {
            Self::Known(generation) => Some(generation),
            Self::NotAvailable => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupStep {
    StopWorker(FrontendWorkerKind),
    RecordScanCancelled,
    ClearLiveReaderDescriptor,
    StopLiveDataAndUnbind,
    CloseLiveDataAndUnbind,
    RestoreFrontendSnapshot,
    TakeDemuxRollbackTokens,
    RestoreBoundDemuxes,
    CompleteReplacement(FrontendWorkerKind),
    CompleteStopObject(FrontendWorkerKind),
    CloseOwnedLnb(i32),
    CloseFrontendWorkersAndLiveData,
}

#[derive(Clone, Debug)]
pub enum FrontendWorkerCleanupStepOutcome {
    StopWorker {
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        worker_generation: FrontendWorkerCleanupWorkerGeneration,
        result: Result<(), HalError>,
    },
    RecordScanCancelled {
        target: FrontendWorkerCleanupTarget,
        worker_generation: FrontendWorkerCleanupWorkerGeneration,
        result: Result<(), HalError>,
    },
    ClearLiveReaderDescriptor {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    StopLiveDataAndUnbind {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    CloseLiveDataAndUnbind {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    RestoreFrontendSnapshot {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    TakeDemuxRollbackTokens {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    RestoreBoundDemuxes {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    CompleteReplacement {
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        stopped_worker_generation: FrontendWorkerCleanupWorkerGeneration,
        new_worker_generation: u64,
        result: Result<(), HalError>,
    },
    CompleteStopObject {
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        worker_generation: FrontendWorkerCleanupWorkerGeneration,
        result: Result<(), HalError>,
    },
    CloseOwnedLnb {
        target: FrontendWorkerCleanupTarget,
        lnb_id: i32,
        result: Result<(), HalError>,
    },
    CloseFrontendWorkersAndLiveData {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
}

impl FrontendWorkerCleanupStepOutcome {
    fn stop_worker(
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        worker_generation: Option<u64>,
        result: Result<(), HalError>,
    ) -> Self {
        Self::StopWorker {
            target,
            worker_kind,
            worker_generation: FrontendWorkerCleanupWorkerGeneration::from_option(
                worker_generation,
            ),
            result,
        }
    }

    fn record_scan_cancelled(
        target: FrontendWorkerCleanupTarget,
        worker_generation: Option<u64>,
        result: Result<(), HalError>,
    ) -> Self {
        Self::RecordScanCancelled {
            target,
            worker_generation: FrontendWorkerCleanupWorkerGeneration::from_option(
                worker_generation,
            ),
            result,
        }
    }

    fn clear_live_reader_descriptor(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::ClearLiveReaderDescriptor { target, result }
    }

    fn stop_live_data_and_unbind(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::StopLiveDataAndUnbind { target, result }
    }

    fn close_live_data_and_unbind(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CloseLiveDataAndUnbind { target, result }
    }

    fn restore_frontend_snapshot(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::RestoreFrontendSnapshot { target, result }
    }

    fn take_demux_rollback_tokens(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::TakeDemuxRollbackTokens { target, result }
    }

    fn restore_bound_demuxes(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::RestoreBoundDemuxes { target, result }
    }

    fn complete_replacement(
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        stopped_worker_generation: Option<u64>,
        new_worker_generation: u64,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CompleteReplacement {
            target,
            worker_kind,
            stopped_worker_generation: FrontendWorkerCleanupWorkerGeneration::from_option(
                stopped_worker_generation,
            ),
            new_worker_generation,
            result,
        }
    }

    fn complete_stop_object(
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        worker_generation: Option<u64>,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CompleteStopObject {
            target,
            worker_kind,
            worker_generation: FrontendWorkerCleanupWorkerGeneration::from_option(
                worker_generation,
            ),
            result,
        }
    }

    fn close_owned_lnb(
        target: FrontendWorkerCleanupTarget,
        lnb_id: i32,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CloseOwnedLnb {
            target,
            lnb_id,
            result,
        }
    }

    fn close_frontend_workers_and_live_data(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CloseFrontendWorkersAndLiveData { target, result }
    }

    pub fn target(&self) -> FrontendWorkerCleanupTarget {
        match self {
            Self::StopWorker { target, .. }
            | Self::RecordScanCancelled { target, .. }
            | Self::ClearLiveReaderDescriptor { target, .. }
            | Self::StopLiveDataAndUnbind { target, .. }
            | Self::CloseLiveDataAndUnbind { target, .. }
            | Self::RestoreFrontendSnapshot { target, .. }
            | Self::TakeDemuxRollbackTokens { target, .. }
            | Self::RestoreBoundDemuxes { target, .. }
            | Self::CompleteReplacement { target, .. }
            | Self::CompleteStopObject { target, .. }
            | Self::CloseOwnedLnb { target, .. }
            | Self::CloseFrontendWorkersAndLiveData { target, .. } => *target,
        }
    }

    pub fn frontend_id(&self) -> i32 {
        self.target().frontend_id()
    }

    pub fn object_id(&self) -> Option<AidlObjectId> {
        self.target().object_id()
    }

    pub fn object_generation(&self) -> Option<AidlObjectGeneration> {
        self.target().object_generation()
    }

    pub fn worker_kind(&self) -> Option<FrontendWorkerKind> {
        match self {
            Self::StopWorker { worker_kind, .. }
            | Self::CompleteReplacement { worker_kind, .. }
            | Self::CompleteStopObject { worker_kind, .. } => Some(*worker_kind),
            Self::RecordScanCancelled { .. }
            | Self::ClearLiveReaderDescriptor { .. }
            | Self::StopLiveDataAndUnbind { .. }
            | Self::CloseLiveDataAndUnbind { .. }
            | Self::RestoreFrontendSnapshot { .. }
            | Self::TakeDemuxRollbackTokens { .. }
            | Self::RestoreBoundDemuxes { .. }
            | Self::CloseOwnedLnb { .. }
            | Self::CloseFrontendWorkersAndLiveData { .. } => None,
        }
    }

    pub fn worker_generation(&self) -> Option<u64> {
        match self {
            Self::StopWorker {
                worker_generation, ..
            }
            | Self::RecordScanCancelled {
                worker_generation, ..
            } => worker_generation.as_option(),
            Self::CompleteReplacement {
                stopped_worker_generation,
                ..
            } => stopped_worker_generation.as_option(),
            Self::CompleteStopObject {
                worker_generation, ..
            } => worker_generation.as_option(),
            Self::ClearLiveReaderDescriptor { .. }
            | Self::StopLiveDataAndUnbind { .. }
            | Self::CloseLiveDataAndUnbind { .. }
            | Self::RestoreFrontendSnapshot { .. }
            | Self::TakeDemuxRollbackTokens { .. }
            | Self::RestoreBoundDemuxes { .. }
            | Self::CloseOwnedLnb { .. }
            | Self::CloseFrontendWorkersAndLiveData { .. } => None,
        }
    }

    pub fn step(&self) -> FrontendWorkerCleanupStep {
        match self {
            Self::StopWorker { worker_kind, .. } => {
                FrontendWorkerCleanupStep::StopWorker(*worker_kind)
            }
            Self::RecordScanCancelled { .. } => FrontendWorkerCleanupStep::RecordScanCancelled,
            Self::ClearLiveReaderDescriptor { .. } => {
                FrontendWorkerCleanupStep::ClearLiveReaderDescriptor
            }
            Self::StopLiveDataAndUnbind { .. } => FrontendWorkerCleanupStep::StopLiveDataAndUnbind,
            Self::CloseLiveDataAndUnbind { .. } => {
                FrontendWorkerCleanupStep::CloseLiveDataAndUnbind
            }
            Self::RestoreFrontendSnapshot { .. } => {
                FrontendWorkerCleanupStep::RestoreFrontendSnapshot
            }
            Self::TakeDemuxRollbackTokens { .. } => {
                FrontendWorkerCleanupStep::TakeDemuxRollbackTokens
            }
            Self::RestoreBoundDemuxes { .. } => FrontendWorkerCleanupStep::RestoreBoundDemuxes,
            Self::CompleteReplacement { worker_kind, .. } => {
                FrontendWorkerCleanupStep::CompleteReplacement(*worker_kind)
            }
            Self::CompleteStopObject { worker_kind, .. } => {
                FrontendWorkerCleanupStep::CompleteStopObject(*worker_kind)
            }
            Self::CloseOwnedLnb { lnb_id, .. } => FrontendWorkerCleanupStep::CloseOwnedLnb(*lnb_id),
            Self::CloseFrontendWorkersAndLiveData { .. } => {
                FrontendWorkerCleanupStep::CloseFrontendWorkersAndLiveData
            }
        }
    }

    pub fn result(&self) -> Result<(), HalError> {
        match self {
            Self::StopWorker { result, .. }
            | Self::RecordScanCancelled { result, .. }
            | Self::ClearLiveReaderDescriptor { result, .. }
            | Self::StopLiveDataAndUnbind { result, .. }
            | Self::CloseLiveDataAndUnbind { result, .. }
            | Self::RestoreFrontendSnapshot { result, .. }
            | Self::TakeDemuxRollbackTokens { result, .. }
            | Self::RestoreBoundDemuxes { result, .. }
            | Self::CompleteReplacement { result, .. }
            | Self::CompleteStopObject { result, .. }
            | Self::CloseOwnedLnb { result, .. }
            | Self::CloseFrontendWorkersAndLiveData { result, .. } => result.clone(),
        }
    }

    pub fn into_result(self) -> Result<(), HalError> {
        match self {
            Self::StopWorker { result, .. }
            | Self::RecordScanCancelled { result, .. }
            | Self::ClearLiveReaderDescriptor { result, .. }
            | Self::StopLiveDataAndUnbind { result, .. }
            | Self::CloseLiveDataAndUnbind { result, .. }
            | Self::RestoreFrontendSnapshot { result, .. }
            | Self::TakeDemuxRollbackTokens { result, .. }
            | Self::RestoreBoundDemuxes { result, .. }
            | Self::CompleteReplacement { result, .. }
            | Self::CompleteStopObject { result, .. }
            | Self::CloseOwnedLnb { result, .. }
            | Self::CloseFrontendWorkersAndLiveData { result, .. } => result,
        }
    }
}

impl CleanupExecutionStepOutcome for FrontendWorkerCleanupStepOutcome {
    type Failure = HalError;

    fn result(&self) -> Result<(), Self::Failure> {
        FrontendWorkerCleanupStepOutcome::result(self)
    }

    fn into_result(self) -> Result<(), Self::Failure> {
        FrontendWorkerCleanupStepOutcome::into_result(self)
    }
}

pub type FrontendWorkerCleanupExecutionReport =
    CleanupExecutionReport<FrontendWorkerCleanupStepOutcome, HalError>;

#[derive(Clone, Debug)]
pub struct FrontendWorkerCleanupDiagnosticRecord {
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    report: FrontendWorkerCleanupExecutionReport,
    public_error: Option<HalError>,
}

impl FrontendWorkerCleanupDiagnosticRecord {
    pub fn new(
        kind: FrontendWorkerCleanupDiagnosticKind,
        target: FrontendWorkerCleanupTarget,
        report: FrontendWorkerCleanupExecutionReport,
        public_error: Option<HalError>,
    ) -> Self {
        Self {
            kind,
            target,
            report,
            public_error,
        }
    }

    pub fn kind(&self) -> FrontendWorkerCleanupDiagnosticKind {
        self.kind
    }

    pub fn target(&self) -> FrontendWorkerCleanupTarget {
        self.target
    }

    pub fn frontend_id(&self) -> i32 {
        self.target.frontend_id()
    }

    pub fn object_id(&self) -> Option<AidlObjectId> {
        self.target.object_id()
    }

    pub fn object_generation(&self) -> Option<AidlObjectGeneration> {
        self.target.object_generation()
    }

    pub fn report(&self) -> &FrontendWorkerCleanupExecutionReport {
        &self.report
    }

    pub fn public_error(&self) -> Option<&HalError> {
        self.public_error.as_ref()
    }
}

pub type FrontendWorkerCleanupDiagnosticSnapshot =
    CleanupExecutionDiagnosticSnapshot<FrontendWorkerCleanupDiagnosticRecord>;
pub type SharedFrontendWorkerCleanupDiagnostics =
    SharedCleanupDiagnostics<FrontendWorkerCleanupDiagnosticRecord>;

type BoundDemuxGenerationSnapshot = Vec<(crate::registry::DemuxRuntimeId, u64)>;

fn share_demux_rollback_tokens(tokens: DemuxRollbackTokenList) -> SharedDemuxRollbackTokenList {
    Arc::new(Mutex::new(Some(tokens)))
}

fn take_demux_rollback_tokens(
    tokens: &SharedDemuxRollbackTokenList,
    context: &'static str,
) -> Result<DemuxRollbackTokenList, HalError> {
    let mut guard = tokens.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "demux rollback token list lock poisoned",
        )
    })?;
    guard
        .take()
        .ok_or_else(|| HalError::internal(HalInternalKind::InvariantViolation, context))
}

struct FrontendWorkerReplacementTicket {
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    stopped_worker_generation: Option<u64>,
    new_worker_generation: u64,
    frontend_snapshot: FrontendRuntimeSnapshot,
    demux_rollback_tokens: DemuxRollbackTokenList,
    bound_demux_generations: BoundDemuxGenerationSnapshot,
    stop_ticket: FrontendWorkerStopTicket,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FrontendWorkerReplacementRollbackContext {
    worker_kind: FrontendWorkerKind,
    stopped_worker_generation: Option<u64>,
    new_worker_generation: u64,
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

fn bound_demux_generation_snapshot(
    tokens: &DemuxRollbackTokenList,
) -> BoundDemuxGenerationSnapshot {
    let mut generations = tokens
        .iter()
        .map(|(demux_id, token)| (*demux_id, token.generation()))
        .collect::<Vec<_>>();
    generations.sort();
    generations
}

fn current_bound_demux_generation_snapshot(
    guard: &TunerServiceRuntime,
    frontend_id: i32,
) -> Result<BoundDemuxGenerationSnapshot, HalError> {
    let mut generations = guard.query().bound_demux_runtime_generations(frontend_id)?;
    generations.sort();
    Ok(generations)
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
    cleanup_diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
    diagnostic_kind: FrontendWorkerCleanupDiagnosticKind,
    context: &'static str,
) -> Result<
    (
        MutexGuard<'a, TunerServiceRuntime>,
        i32,
        u64,
        FrontendWorkerStopOutcome,
        FrontendRuntimeSnapshot,
        DemuxRollbackTokenList,
    ),
    HalError,
> {
    let FrontendWorkerReplacementTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        stopped_worker_generation,
        new_worker_generation,
        frontend_snapshot,
        demux_rollback_tokens,
        bound_demux_generations,
        stop_ticket,
    } = ticket;
    let stop_outcome = stop_ticket.complete();
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let record_stop_outcome_for_failure = |primary: HalError| -> HalError {
        match record_frontend_worker_replacement_stop_diagnostic(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            None,
            Some((stopped_worker_generation, new_worker_generation, primary.clone())),
        ) {
            Ok(()) => primary,
            Err(record_error) => compose_frontend_worker_cleanup_record_failure(
                "frontend worker replacement stop diagnostic record failed after replacement failure",
                primary,
                record_error,
            ),
        }
    };
    if let Some(error) = frontend_worker_stop_failure(&stop_outcome) {
        return Err(record_stop_outcome_for_failure(error));
    }
    let guard = match lock_runtime(runtime, context) {
        Ok(guard) => guard,
        Err(error) => return Err(record_stop_outcome_for_failure(error)),
    };
    if let Err(error) = ensure_frontend_ticket_still_targets_object(
        &guard,
        object_id,
        object_generation,
        frontend_id,
    ) {
        return Err(record_stop_outcome_for_failure(error));
    }
    if let Err(error) = ensure_frontend_join_snapshot_still_matches(
        &guard,
        frontend_id,
        &frontend_snapshot,
        &bound_demux_generations,
    ) {
        return Err(record_stop_outcome_for_failure(error));
    }
    if frontend_worker_stop_outcome_generation(&stop_outcome) != stopped_worker_generation {
        return Err(record_stop_outcome_for_failure(HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend worker replacement ticket generation mismatch",
        )));
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
                    return Err(record_stop_outcome_for_failure(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend worker replacement ticket kind mismatch",
                    )));
                }
            }
            FrontendWorkerStopOutcome::NotRunning
            | FrontendWorkerStopOutcome::StopRequestFailed { .. } => {}
        }
    }
    Ok((
        guard,
        frontend_id,
        new_worker_generation,
        stop_outcome,
        frontend_snapshot,
        demux_rollback_tokens,
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
    let demux_rollback_tokens = runtime.prepare_bound_demux_runtime_rollback_tokens(frontend_id)?;
    let bound_demux_generations = bound_demux_generation_snapshot(&demux_rollback_tokens);
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
    cleanup_diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
    diagnostic_kind: FrontendWorkerCleanupDiagnosticKind,
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
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let record_stop_outcome_for_failure =
        |primary: HalError, include_complete_step: bool| -> HalError {
            let mut report = FrontendWorkerCleanupExecutionReport::new();
            report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
                target,
                kind,
                frontend_worker_stop_outcome_generation(&stop_outcome),
                frontend_worker_stop_result_from_outcome(&stop_outcome),
            ));
            if include_complete_step {
                report.push(FrontendWorkerCleanupStepOutcome::complete_stop_object(
                    target,
                    kind,
                    frontend_worker_stop_outcome_generation(&stop_outcome),
                    Err(primary.clone()),
                ));
            }
            let record = FrontendWorkerCleanupDiagnosticRecord::new(
                diagnostic_kind,
                target,
                report,
                Some(primary.clone()),
            );
            match cleanup_diagnostic_sink.record(record) {
                Ok(()) => primary,
                Err(record_error) => compose_frontend_worker_cleanup_record_failure(
                    "frontend worker stop object diagnostic record failed after stop failure",
                    primary,
                    record_error,
                ),
            }
        };
    if let Some(error) = frontend_worker_stop_request_failure(&stop_outcome) {
        return Err(record_stop_outcome_for_failure(error, false));
    }
    let guard = match lock_runtime(runtime, context) {
        Ok(guard) => guard,
        Err(error) => return Err(record_stop_outcome_for_failure(error, true)),
    };
    if let Err(error) = ensure_frontend_ticket_still_targets_object(
        &guard,
        object_id,
        object_generation,
        frontend_id,
    ) {
        return Err(record_stop_outcome_for_failure(error, true));
    }
    if let Err(error) = ensure_frontend_join_snapshot_still_matches(
        &guard,
        frontend_id,
        &frontend_snapshot,
        &bound_demux_generations,
    ) {
        return Err(record_stop_outcome_for_failure(error, true));
    }
    if frontend_worker_stop_outcome_generation(&stop_outcome) != worker_generation {
        return Err(record_stop_outcome_for_failure(
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend worker stop ticket generation mismatch",
            ),
            true,
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
                    return Err(record_stop_outcome_for_failure(
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "frontend worker stop ticket kind mismatch",
                        ),
                        true,
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

fn compose_frontend_worker_cleanup_record_failure(
    context: &'static str,
    primary: HalError,
    record_error: HalError,
) -> HalError {
    compose_primary_cleanup_failure(context, primary, record_error)
}

fn finish_frontend_worker_rollback_report(
    sink: Result<SharedFrontendWorkerCleanupDiagnostics, HalError>,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    report: FrontendWorkerCleanupExecutionReport,
    primary: HalError,
    context: &'static str,
) -> HalError {
    let rollback_error = report.first_error();
    let public_error = match rollback_error {
        Some(cleanup) => compose_frontend_cleanup_error(context, primary.clone(), cleanup),
        None => primary,
    };
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        kind,
        target,
        report,
        Some(public_error.clone()),
    );
    match sink.and_then(|sink| sink.record(record)) {
        Ok(()) => public_error,
        Err(record_error) => compose_frontend_worker_cleanup_record_failure(
            "frontend worker cleanup diagnostic record failed after rollback",
            public_error,
            record_error,
        ),
    }
}

fn restore_frontend_state_after_primary_failure(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    frontend_snapshot: FrontendRuntimeSnapshot,
    demux_rollback_tokens: DemuxRollbackTokenList,
    primary: HalError,
    context: &'static str,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> HalError {
    let sink = Ok(guard.frontend_worker_cleanup_diagnostic_sink());
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    if let Some(replacement_context) = replacement_context {
        report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
            target,
            replacement_context.worker_kind,
            replacement_context.stopped_worker_generation,
            replacement_context.new_worker_generation,
            Ok(()),
        ));
    }
    let restore_frontend_result = guard
        .frontend_txn()
        .restore_frontend_runtime_snapshot(frontend_id, frontend_snapshot);
    report.push(FrontendWorkerCleanupStepOutcome::restore_frontend_snapshot(
        target,
        restore_frontend_result,
    ));
    let restore_demux_result = guard
        .frontend_txn()
        .restore_bound_demux_runtime_rollback_tokens(demux_rollback_tokens);
    report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demuxes(
        target,
        restore_demux_result,
    ));
    finish_frontend_worker_rollback_report(sink, kind, target, report, primary, context)
}

fn restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    frontend_snapshot: FrontendRuntimeSnapshot,
    demux_rollback_tokens: &SharedDemuxRollbackTokenList,
    primary: HalError,
    context: &'static str,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> HalError {
    match take_demux_rollback_tokens(
        demux_rollback_tokens,
        "demux rollback token list was already consumed",
    ) {
        Ok(tokens) => restore_frontend_state_after_primary_failure(
            guard,
            frontend_id,
            frontend_snapshot,
            tokens,
            primary,
            context,
            kind,
            target,
            replacement_context,
        ),
        Err(take_error) => {
            let mut report = FrontendWorkerCleanupExecutionReport::new();
            if let Some(replacement_context) = replacement_context {
                report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
                    target,
                    replacement_context.worker_kind,
                    replacement_context.stopped_worker_generation,
                    replacement_context.new_worker_generation,
                    Ok(()),
                ));
            }
            report.push(
                FrontendWorkerCleanupStepOutcome::take_demux_rollback_tokens(
                    target,
                    Err(take_error.clone()),
                ),
            );
            finish_frontend_worker_rollback_report(
                Ok(guard.frontend_worker_cleanup_diagnostic_sink()),
                kind,
                target,
                report,
                primary,
                context,
            )
        }
    }
}

fn finish_frontend_state_restore_lock_failure_report(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    primary: HalError,
    lock_error: HalError,
    context: &'static str,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> HalError {
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    if let Some(replacement_context) = replacement_context {
        report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
            target,
            replacement_context.worker_kind,
            replacement_context.stopped_worker_generation,
            replacement_context.new_worker_generation,
            Ok(()),
        ));
    }
    report.push(FrontendWorkerCleanupStepOutcome::restore_frontend_snapshot(
        target,
        Err(lock_error),
    ));
    finish_frontend_worker_rollback_report(Ok(sink), kind, target, report, primary, context)
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

fn frontend_worker_stop_result(
    outcome: &Result<FrontendWorkerStopOutcome, HalError>,
) -> Result<(), HalError> {
    match outcome {
        Ok(outcome) => frontend_worker_stop_failure(outcome).map_or(Ok(()), Err),
        Err(error) => Err(error.clone()),
    }
}

fn frontend_worker_stop_result_from_outcome(
    outcome: &FrontendWorkerStopOutcome,
) -> Result<(), HalError> {
    frontend_worker_stop_failure(outcome).map_or(Ok(()), Err)
}

fn frontend_worker_stop_result_generation(
    outcome: &Result<FrontendWorkerStopOutcome, HalError>,
) -> Option<u64> {
    outcome
        .as_ref()
        .ok()
        .and_then(frontend_worker_stop_outcome_generation)
}

fn compose_frontend_worker_cleanup_finish_result(
    cleanup_result: Result<(), HalError>,
    record_result: Result<(), HalError>,
) -> Result<(), HalError> {
    match (cleanup_result, record_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(()), Err(record_error)) => Err(record_error),
        (Err(primary), Err(record_error)) => Err(compose_primary_cleanup_failure(
            "frontend worker cleanup diagnostic record failed",
            primary,
            record_error,
        )),
    }
}

fn finish_frontend_worker_cleanup_report(
    sink: Result<SharedFrontendWorkerCleanupDiagnostics, HalError>,
    record: FrontendWorkerCleanupDiagnosticRecord,
) -> Result<(), HalError> {
    let cleanup_result = record.report().clone().into_result();
    let record_result = sink.and_then(|sink| sink.record(record));
    compose_frontend_worker_cleanup_finish_result(cleanup_result, record_result)
}

fn build_frontend_worker_replacement_stop_report(
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    stop_outcome: &FrontendWorkerStopOutcome,
    scan_cancel_result: Option<Result<(), HalError>>,
) -> FrontendWorkerCleanupExecutionReport {
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
        target,
        worker_kind,
        frontend_worker_stop_outcome_generation(stop_outcome),
        frontend_worker_stop_result_from_outcome(stop_outcome),
    ));
    if let Some(scan_cancel_result) = scan_cancel_result {
        report.push(FrontendWorkerCleanupStepOutcome::record_scan_cancelled(
            target,
            frontend_worker_stop_outcome_generation(stop_outcome),
            scan_cancel_result,
        ));
    }
    report
}

fn record_frontend_worker_replacement_stop_diagnostic(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    stop_outcome: &FrontendWorkerStopOutcome,
    scan_cancel_result: Option<Result<(), HalError>>,
    post_stop_failure: Option<(Option<u64>, u64, HalError)>,
) -> Result<(), HalError> {
    let mut report = build_frontend_worker_replacement_stop_report(
        target,
        worker_kind,
        stop_outcome,
        scan_cancel_result,
    );
    let public_error =
        if let Some((stopped_generation, new_generation, primary)) = post_stop_failure {
            report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
                target,
                worker_kind,
                stopped_generation,
                new_generation,
                Err(primary.clone()),
            ));
            Some(primary)
        } else {
            report.clone().into_result().err()
        };
    let record = FrontendWorkerCleanupDiagnosticRecord::new(kind, target, report, public_error);
    sink.record(record)
}

fn record_frontend_worker_replacement_stop_report(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    stop_outcome: &FrontendWorkerStopOutcome,
    scan_cancel_result: Option<Result<(), HalError>>,
) -> Result<(), HalError> {
    let report = build_frontend_worker_replacement_stop_report(
        target,
        worker_kind,
        stop_outcome,
        scan_cancel_result,
    );
    let cleanup_result = report.clone().into_result();
    let public_error = cleanup_result.clone().err();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(kind, target, report, public_error);
    let record_result = sink.record(record);
    compose_frontend_worker_cleanup_finish_result(cleanup_result, record_result)
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
    cleanup_diagnostic_sink: Result<SharedFrontendWorkerCleanupDiagnostics, HalError>,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    snapshot: FrontendRuntimeSnapshot,
    demux_rollback_tokens: &SharedDemuxRollbackTokenList,
    commit_error: HalError,
    target: FrontendWorkerCleanupTarget,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> HalError {
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    if let Some(replacement_context) = replacement_context {
        report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
            target,
            replacement_context.worker_kind,
            replacement_context.stopped_worker_generation,
            replacement_context.new_worker_generation,
            Ok(()),
        ));
    }
    match stop_frontend_worker(
        Arc::clone(runtime),
        frontend_id,
        kind,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    ) {
        Ok(outcome) => report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            kind,
            frontend_worker_stop_outcome_generation(&outcome),
            frontend_worker_stop_result_from_outcome(&outcome),
        )),
        Err(error) => report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            kind,
            None,
            Err(error),
        )),
    }

    match lock_runtime(
        runtime,
        "service runtime lock poisoned while rolling back tune commit failure",
    ) {
        Ok(mut guard) => {
            let restore_frontend_result = guard
                .frontend_txn()
                .restore_frontend_runtime_snapshot(frontend_id, snapshot);
            report.push(FrontendWorkerCleanupStepOutcome::restore_frontend_snapshot(
                target,
                restore_frontend_result,
            ));
            let take_result = take_demux_rollback_tokens(
                demux_rollback_tokens,
                "demux rollback token list was already consumed during tune commit rollback",
            );
            match take_result {
                Ok(tokens) => {
                    report.push(
                        FrontendWorkerCleanupStepOutcome::take_demux_rollback_tokens(
                            target,
                            Ok(()),
                        ),
                    );
                    let demux_restore_result = guard
                        .frontend_txn()
                        .restore_bound_demux_runtime_rollback_tokens(tokens);
                    report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demuxes(
                        target,
                        demux_restore_result,
                    ));
                }
                Err(error) => {
                    report.push(
                        FrontendWorkerCleanupStepOutcome::take_demux_rollback_tokens(
                            target,
                            Err(error),
                        ),
                    );
                }
            }
        }
        Err(error) => report.push(FrontendWorkerCleanupStepOutcome::restore_frontend_snapshot(
            target,
            Err(error),
        )),
    }

    finish_frontend_worker_rollback_report(
        cleanup_diagnostic_sink,
        FrontendWorkerCleanupDiagnosticKind::TuneCommitRollback,
        target,
        report,
        commit_error,
        "frontend tune commit failed after worker start",
    )
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
    let entry = guard.validate_frontend_request_for_id(frontend_id, &request)?;
    let frontend_snapshot = guard.query().frontend_runtime_snapshot(frontend_id)?;
    let demux_rollback_tokens = guard.prepare_bound_demux_runtime_rollback_tokens(frontend_id)?;
    let bound_demux_generations = bound_demux_generation_snapshot(&demux_rollback_tokens);
    let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
    let generation = guard
        .frontend_txn()
        .prepare_frontend_worker_replacement_generation(frontend_id, kind)?;
    let stop_ticket = request_tune_worker_replacement_stop(&mut guard, frontend_id);
    let replacement_ticket = FrontendWorkerReplacementTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        stopped_worker_generation: stop_ticket.worker_generation(),
        new_worker_generation: generation,
        frontend_snapshot,
        demux_rollback_tokens,
        bound_demux_generations,
        stop_ticket,
    };
    drop(guard);
    let (mut guard, frontend_id, generation, stop_outcome, snapshot, demux_rollback_tokens) =
        complete_frontend_worker_replacement_ticket(
            &runtime,
            replacement_ticket,
            cleanup_diagnostic_sink.clone(),
            FrontendWorkerCleanupDiagnosticKind::TuneReplacementStop,
            "service runtime lock poisoned after tune worker join",
        )?;
    let demux_rollback_tokens = share_demux_rollback_tokens(demux_rollback_tokens);
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let replacement_context = Some(FrontendWorkerReplacementRollbackContext {
        worker_kind: kind,
        stopped_worker_generation: frontend_worker_stop_outcome_generation(&stop_outcome),
        new_worker_generation: generation,
    });
    record_frontend_worker_replacement_stop_report(
        cleanup_diagnostic_sink.clone(),
        FrontendWorkerCleanupDiagnosticKind::TuneReplacementStop,
        target,
        kind,
        &stop_outcome,
        None,
    )?;
    if let Err(error) = guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id) {
        return Err(
            restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                &mut guard,
                frontend_id,
                snapshot,
                &demux_rollback_tokens,
                error,
                "frontend tune start reset rollback",
                FrontendWorkerCleanupDiagnosticKind::TuneStartRollback,
                target,
                replacement_context,
            ),
        );
    }
    if let Err(error) = guard
        .frontend_txn()
        .install_frontend_live_reader_descriptor_for_generation(frontend_id, kind, generation)
    {
        return Err(
            restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                &mut guard,
                frontend_id,
                snapshot,
                &demux_rollback_tokens,
                error,
                "frontend tune live reader install rollback",
                FrontendWorkerCleanupDiagnosticKind::TuneStartRollback,
                target,
                replacement_context,
            ),
        );
    }
    let plan = FrontendBackendTunePlan::new(
        frontend_id,
        generation,
        entry.backend,
        FrontendDevicePath::new(entry.device_path.clone()),
        request.clone(),
    );
    let previous_tune_for_worker = snapshot.active_tune_request.clone();
    let target_for_worker = target;
    let replacement_context_for_worker = replacement_context;
    let frontend_snapshot_for_worker = snapshot.clone();
    let demux_rollback_tokens_for_worker = Arc::clone(&demux_rollback_tokens);
    let runtime_for_worker = Arc::clone(&runtime);
    let cleanup_diagnostic_sink_for_worker = cleanup_diagnostic_sink.clone();
    if let Err(error) = guard.frontend_txn().start_worker(frontend_id, kind, generation, move |ctx| {
        plan.validate_worker_generation(ctx.generation())?;
        let session = match FrontendBackendSession::open_and_submit_with_previous_report(
            &plan,
            previous_tune_for_worker,
        ) {
            Ok(session) => session,
            Err(failure) if failure.rollback_succeeded => {
                let report_error = failure.error;
                let mut guard = match lock_runtime(
                    &runtime_for_worker,
                    "service runtime lock poisoned while restoring tune rollback state",
                ) {
                    Ok(guard) => guard,
                    Err(lock_error) => {
                        return Err(finish_frontend_state_restore_lock_failure_report(
                            cleanup_diagnostic_sink_for_worker.clone(),
                            report_error,
                            lock_error,
                            "frontend tune backend rollback state restore",
                            FrontendWorkerCleanupDiagnosticKind::TuneBackendRollbackStateRestore,
                            target_for_worker,
                            replacement_context_for_worker,
                        ));
                    }
                };
                return Err(restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                    &mut guard,
                    frontend_id,
                    frontend_snapshot_for_worker.clone(),
                    &demux_rollback_tokens_for_worker,
                    report_error,
                    "frontend tune backend rollback state restore",
                    FrontendWorkerCleanupDiagnosticKind::TuneBackendRollbackStateRestore,
                    target_for_worker,
                    replacement_context_for_worker,
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
                        return Err(finish_frontend_state_restore_lock_failure_report(
                            cleanup_diagnostic_sink_for_worker.clone(),
                            report_error,
                            mark_error,
                            "frontend tune backend failure marking failed",
                            FrontendWorkerCleanupDiagnosticKind::TuneBackendRollbackStateRestore,
                            target_for_worker,
                            replacement_context_for_worker,
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
        return Err(restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
            &mut guard,
            frontend_id,
            snapshot,
            &demux_rollback_tokens,
            primary,
            "frontend tune worker start rollback",
            FrontendWorkerCleanupDiagnosticKind::TuneWorkerStartRollback,
            target,
            replacement_context,
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
            Ok(cleanup_diagnostic_sink.clone()),
            frontend_id,
            kind,
            snapshot,
            &demux_rollback_tokens,
            error,
            target,
            replacement_context,
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
    demux_rollback_tokens: SharedDemuxRollbackTokenList,
    target_for_worker: FrontendWorkerCleanupTarget,
    scan_end_notifier: FrontendScanEndNotifier,
    cleanup_diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
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
                let primary = failure.error;
                let mut guard = match lock_runtime(
                    &runtime,
                    "service runtime lock poisoned while restoring scan rollback state",
                ) {
                    Ok(guard) => guard,
                    Err(lock_error) => {
                        return Err(finish_frontend_state_restore_lock_failure_report(
                            cleanup_diagnostic_sink.clone(),
                            primary,
                            lock_error,
                            "frontend scan backend rollback state restore",
                            FrontendWorkerCleanupDiagnosticKind::ScanBackendRollbackStateRestore,
                            target_for_worker,
                            replacement_context,
                        ));
                    }
                };
                return Err(
                    restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                        &mut guard,
                        ctx.frontend_id(),
                        frontend_snapshot.clone(),
                        &demux_rollback_tokens,
                        primary,
                        "frontend scan backend rollback state restore",
                        FrontendWorkerCleanupDiagnosticKind::ScanBackendRollbackStateRestore,
                        target_for_worker,
                        replacement_context,
                    ),
                );
            }
            Err(failure) => {
                let primary = failure.error;
                let mut guard = match lock_runtime(
                    &runtime,
                    "service runtime lock poisoned while marking scan backend failure",
                ) {
                    Ok(guard) => guard,
                    Err(lock_error) => {
                        return Err(finish_frontend_state_restore_lock_failure_report(
                            cleanup_diagnostic_sink.clone(),
                            primary,
                            lock_error,
                            "frontend scan backend failure marking failed",
                            FrontendWorkerCleanupDiagnosticKind::ScanBackendRollbackStateRestore,
                            target_for_worker,
                            replacement_context,
                        ));
                    }
                };
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
    let entry = guard.validate_frontend_request_for_id(frontend_id, &request)?;
    let candidates = guard.scan_candidates_for_frontend_entry(&entry, &request, scan_mode)?;
    let frontend_snapshot = guard.query().frontend_runtime_snapshot(frontend_id)?;
    let demux_rollback_tokens = guard.prepare_bound_demux_runtime_rollback_tokens(frontend_id)?;
    let bound_demux_generations = bound_demux_generation_snapshot(&demux_rollback_tokens);
    let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
    let generation = guard
        .frontend_txn()
        .prepare_frontend_worker_replacement_generation(frontend_id, FrontendWorkerKind::Scan)?;
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
        stopped_worker_generation: stop_ticket.worker_generation(),
        new_worker_generation: generation,
        frontend_snapshot,
        demux_rollback_tokens,
        bound_demux_generations,
        stop_ticket,
    };
    drop(guard);
    let (mut guard, frontend_id, generation, stop_outcome, snapshot, demux_rollback_tokens) =
        complete_frontend_worker_replacement_ticket(
            &runtime,
            replacement_ticket,
            cleanup_diagnostic_sink.clone(),
            FrontendWorkerCleanupDiagnosticKind::ScanReplacementStop,
            "service runtime lock poisoned after scan worker join",
        )?;
    let demux_rollback_tokens = share_demux_rollback_tokens(demux_rollback_tokens);
    let scan_cancel_result = record_scan_cancelled_from_stop_outcome_locked(
        &mut guard,
        frontend_id,
        &stop_outcome,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    );
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let replacement_context = Some(FrontendWorkerReplacementRollbackContext {
        worker_kind: FrontendWorkerKind::Scan,
        stopped_worker_generation: frontend_worker_stop_outcome_generation(&stop_outcome),
        new_worker_generation: generation,
    });
    record_frontend_worker_replacement_stop_report(
        cleanup_diagnostic_sink.clone(),
        FrontendWorkerCleanupDiagnosticKind::ScanReplacementStop,
        target,
        FrontendWorkerKind::Scan,
        &stop_outcome,
        Some(scan_cancel_result),
    )?;
    if let Err(error) = guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id) {
        return Err(
            restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                &mut guard,
                frontend_id,
                snapshot,
                &demux_rollback_tokens,
                error,
                "frontend scan start reset rollback",
                FrontendWorkerCleanupDiagnosticKind::ScanStartRollback,
                target,
                replacement_context,
            ),
        );
    }
    if let Err(error) = guard
        .frontend_txn()
        .install_frontend_live_reader_descriptor_for_generation(
            frontend_id,
            FrontendWorkerKind::Scan,
            generation,
        )
    {
        return Err(
            restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                &mut guard,
                frontend_id,
                snapshot,
                &demux_rollback_tokens,
                error,
                "frontend scan live reader install rollback",
                FrontendWorkerCleanupDiagnosticKind::ScanStartRollback,
                target,
                replacement_context,
            ),
        );
    }
    if let Err(error) = guard.frontend_txn().begin_frontend_scan_session(
        frontend_id,
        generation,
        fingerprint,
        candidates.clone(),
    ) {
        return Err(
            restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                &mut guard,
                frontend_id,
                snapshot,
                &demux_rollback_tokens,
                error,
                "frontend scan session begin rollback",
                FrontendWorkerCleanupDiagnosticKind::ScanStartRollback,
                target,
                replacement_context,
            ),
        );
    }
    let previous_tune_for_worker = snapshot.active_tune_request.clone();
    let target_for_worker = target;
    let replacement_context_for_worker = replacement_context;
    let frontend_snapshot_for_worker = snapshot.clone();
    let demux_rollback_tokens_for_worker = Arc::clone(&demux_rollback_tokens);
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
                demux_rollback_tokens_for_worker,
                target_for_worker,
                scan_end_notifier,
                cleanup_diagnostic_sink.clone(),
                replacement_context_for_worker,
            )
        },
    ) {
        let primary = map_frontend_worker_start_error(error);
        return Err(
            restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                &mut guard,
                frontend_id,
                snapshot,
                &demux_rollback_tokens,
                primary,
                "frontend scan worker start rollback",
                FrontendWorkerCleanupDiagnosticKind::ScanWorkerStartRollback,
                target,
                replacement_context,
            ),
        );
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
    let (stop_ticket, cleanup_diagnostic_sink) = {
        let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
        dispatch.consume_for_object(
            &mut guard,
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
        let stop_ticket = prepare_frontend_worker_stop_object_ticket(
            &mut guard,
            object_id,
            object_generation,
            FrontendWorkerKind::Tune,
            reason,
        )?;
        (stop_ticket, cleanup_diagnostic_sink)
    };
    let (mut guard, frontend_id, _reason, outcome) = complete_frontend_worker_stop_object_ticket(
        &runtime,
        stop_ticket,
        cleanup_diagnostic_sink.clone(),
        FrontendWorkerCleanupDiagnosticKind::StopTuneObject,
        "service runtime lock poisoned after tune worker stop",
    )?;
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
        target,
        FrontendWorkerKind::Tune,
        frontend_worker_stop_outcome_generation(&outcome),
        frontend_worker_stop_result_from_outcome(&outcome),
    ));
    let live_data_result = guard
        .frontend_txn()
        .stop_frontend_live_data_and_unbind(frontend_id)
        .map(|_| ());
    report.push(FrontendWorkerCleanupStepOutcome::stop_live_data_and_unbind(
        target,
        live_data_result,
    ));
    let public_error = report.first_error();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::StopTuneObject,
        target,
        report,
        public_error,
    );
    finish_frontend_worker_cleanup_report(Ok(cleanup_diagnostic_sink), record)
}

pub fn stop_frontend_scan_object(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    reason: FrontendWorkerCancelReason,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let (stop_ticket, cleanup_diagnostic_sink) = {
        let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
        dispatch.consume_for_object(
            &mut guard,
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
        let stop_ticket = prepare_frontend_worker_stop_object_ticket(
            &mut guard,
            object_id,
            object_generation,
            FrontendWorkerKind::Scan,
            reason,
        )?;
        (stop_ticket, cleanup_diagnostic_sink)
    };
    let (mut guard, frontend_id, reason, outcome) = complete_frontend_worker_stop_object_ticket(
        &runtime,
        stop_ticket,
        cleanup_diagnostic_sink.clone(),
        FrontendWorkerCleanupDiagnosticKind::StopScanObject,
        "service runtime lock poisoned after scan worker stop",
    )?;
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
        target,
        FrontendWorkerKind::Scan,
        frontend_worker_stop_outcome_generation(&outcome),
        frontend_worker_stop_result_from_outcome(&outcome),
    ));
    let scan_cancel_result =
        record_scan_cancelled_from_stop_outcome_locked(&mut guard, frontend_id, &outcome, reason);
    report.push(FrontendWorkerCleanupStepOutcome::record_scan_cancelled(
        target,
        frontend_worker_stop_outcome_generation(&outcome),
        scan_cancel_result,
    ));
    if !matches!(outcome, FrontendWorkerStopOutcome::NotRunning) {
        let clear_result = guard
            .frontend_txn()
            .clear_frontend_live_reader_descriptor_and_idle(frontend_id);
        report.push(
            FrontendWorkerCleanupStepOutcome::clear_live_reader_descriptor(target, clear_result),
        );
    }
    let public_error = report.first_error();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::StopScanObject,
        target,
        report,
        public_error,
    );
    finish_frontend_worker_cleanup_report(Ok(cleanup_diagnostic_sink), record)
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
    let (frontend_id, lnb_outcomes, cleanup_diagnostic_sink) = {
        let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
        let (frontend_id, _) =
            resolve_frontend_object_for_close_cleanup(&guard, object_id, object_generation)?;
        let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
        let lnb_outcomes = guard.close_lnb_from_frontend_owner_loss_report(frontend_id);
        (frontend_id, lnb_outcomes, cleanup_diagnostic_sink)
    };
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    let mut closed_lnb_ids = Vec::with_capacity(lnb_outcomes.len());
    for (lnb_id, result) in lnb_outcomes {
        if result.is_ok() {
            closed_lnb_ids.push(lnb_id);
        }
        report.push(FrontendWorkerCleanupStepOutcome::close_owned_lnb(
            target, lnb_id, result,
        ));
    }
    let worker_cleanup_result = close_frontend_workers_and_live_data_with_sink(
        Arc::clone(&runtime),
        frontend_id,
        reason,
        Ok(cleanup_diagnostic_sink.clone()),
    );
    report.push(
        FrontendWorkerCleanupStepOutcome::close_frontend_workers_and_live_data(
            target,
            worker_cleanup_result,
        ),
    );
    let cleanup_result = report.clone().into_result();
    let public_error = cleanup_result.clone().err();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::FrontendCloseOwnerLoss,
        target,
        report,
        public_error,
    );
    let record_result = cleanup_diagnostic_sink.record(record);
    let cleanup_result =
        compose_frontend_worker_cleanup_finish_result(cleanup_result, record_result);
    Ok(FrontendCloseCleanupReport {
        frontend_id,
        closed_lnb_ids,
        cleanup_result,
    })
}

#[cfg(test)]
pub fn close_frontend_workers_and_live_data(
    runtime: SharedRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    let cleanup_diagnostic_sink = lock_runtime(
        &runtime,
        "service runtime lock poisoned while preparing frontend worker cleanup diagnostic",
    )
    .map(|guard| guard.frontend_worker_cleanup_diagnostic_sink());
    close_frontend_workers_and_live_data_with_sink(
        runtime,
        frontend_id,
        reason,
        cleanup_diagnostic_sink,
    )
}

fn close_frontend_workers_and_live_data_with_sink(
    runtime: SharedRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
    cleanup_diagnostic_sink: Result<SharedFrontendWorkerCleanupDiagnostics, HalError>,
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

    let target = FrontendWorkerCleanupTarget::frontend(frontend_id);
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
        target,
        FrontendWorkerKind::Tune,
        frontend_worker_stop_result_generation(&tune_outcome),
        frontend_worker_stop_result(&tune_outcome),
    ));
    report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
        target,
        FrontendWorkerKind::Scan,
        frontend_worker_stop_result_generation(&scan_outcome),
        frontend_worker_stop_result(&scan_outcome),
    ));
    if let Ok(outcome) = &scan_outcome {
        let scan_cancel_result =
            record_scan_cancelled_from_stop_outcome(&runtime, frontend_id, outcome, reason);
        report.push(FrontendWorkerCleanupStepOutcome::record_scan_cancelled(
            target,
            frontend_worker_stop_outcome_generation(outcome),
            scan_cancel_result,
        ));
    } else if let Err(error) = &scan_outcome {
        report.push(FrontendWorkerCleanupStepOutcome::record_scan_cancelled(
            target,
            None,
            Err(HalError::cleanup_failed(
                "frontend scan cancel record skipped",
                format!(
                    "scan worker stop failed before scan cancel record could be attempted: {error:?}"
                ),
            )),
        ));
    }
    let close_result = close_frontend_live_data_and_unbind(Arc::clone(&runtime), frontend_id);
    report.push(FrontendWorkerCleanupStepOutcome::close_live_data_and_unbind(target, close_result));
    let public_error = report.first_error();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::FrontendClose,
        target,
        report,
        public_error,
    );
    finish_frontend_worker_cleanup_report(cleanup_diagnostic_sink, record)
}
