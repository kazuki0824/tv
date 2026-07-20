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
    FrontendLivePumpOwner, FrontendRuntimeRollbackToken, FrontendWorkerCancelReason,
    FrontendWorkerContext, FrontendWorkerKind, FrontendWorkerStartError, FrontendWorkerStopOutcome,
    FrontendWorkerStopTicket,
};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

pub type FrontendScanEndNotifier =
    Arc<dyn Fn(i32, u64) -> Result<(), HalError> + Send + Sync + 'static>;

type SharedRuntime = Arc<Mutex<TunerServiceRuntime>>;

type DemuxRollbackTokenList = Vec<(crate::registry::DemuxRuntimeId, DemuxRuntimeRollbackToken)>;
type SharedDemuxRollbackTokenList = Arc<Mutex<Option<DemuxRollbackTokenList>>>;
type SharedFrontendRuntimeRollbackToken = Arc<Mutex<Option<FrontendRuntimeRollbackToken>>>;

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundDemuxRollbackPhase {
    PrepareAuthority,
    Restore,
    Quarantine,
    DiscardAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundDemuxRollbackTarget {
    Demux(i32),
    TokenList,
}

#[derive(Clone, Debug)]
pub(crate) struct BoundDemuxRollbackStepOutcome {
    pub target: BoundDemuxRollbackTarget,
    pub phase: BoundDemuxRollbackPhase,
    pub result: Result<(), HalError>,
}

impl CleanupExecutionStepOutcome for BoundDemuxRollbackStepOutcome {
    type Failure = HalError;

    fn result(&self) -> Result<(), Self::Failure> {
        self.result.clone()
    }

    fn into_result(self) -> Result<(), Self::Failure> {
        self.result
    }
}

pub(crate) type BoundDemuxRollbackExecutionReport =
    CleanupExecutionReport<BoundDemuxRollbackStepOutcome, HalError>;

pub(crate) fn first_bound_demux_error_for_phase(
    report: &BoundDemuxRollbackExecutionReport,
    phase: BoundDemuxRollbackPhase,
) -> Option<HalError> {
    report
        .outcomes()
        .iter()
        .filter(|outcome| outcome.phase == phase)
        .find_map(|outcome| outcome.result.clone().err())
}

#[derive(Debug)]
pub(crate) struct BoundDemuxRollbackPreparation {
    tokens: DemuxRollbackTokenList,
    report: BoundDemuxRollbackExecutionReport,
}

impl BoundDemuxRollbackPreparation {
    pub(crate) fn new(
        tokens: DemuxRollbackTokenList,
        report: BoundDemuxRollbackExecutionReport,
    ) -> Self {
        Self { tokens, report }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (DemuxRollbackTokenList, BoundDemuxRollbackExecutionReport) {
        (self.tokens, self.report)
    }
}

#[derive(Debug)]
pub(crate) struct BoundDemuxRollbackPreparationFailure {
    pub(crate) error: HalError,
    pub(crate) report: BoundDemuxRollbackExecutionReport,
}

impl BoundDemuxRollbackPreparationFailure {
    pub(crate) fn new(error: HalError, report: BoundDemuxRollbackExecutionReport) -> Self {
        Self { error, report }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupStep {
    StopWorker(FrontendWorkerKind),
    RecordScanCancelled,
    ClearLiveReaderDescriptor,
    StopLiveDataAndUnbind,
    CloseLiveDataAndUnbind,
    RestoreFrontendRollbackToken,
    AcquireFrontendRuntimeForRollback,
    DiscardFrontendRollbackAuthority,
    QuarantineFrontendAfterRollbackFailure,
    TakeDemuxRollbackTokens,
    RestoreBoundDemuxes,
    RestoreBoundDemuxStep(BoundDemuxRollbackTarget, BoundDemuxRollbackPhase),
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
    RestoreFrontendRollbackToken {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    AcquireFrontendRuntimeForRollback {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    DiscardFrontendRollbackAuthority {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    QuarantineFrontendAfterRollbackFailure {
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
    RestoreBoundDemuxStep {
        target: FrontendWorkerCleanupTarget,
        rollback_target: BoundDemuxRollbackTarget,
        phase: BoundDemuxRollbackPhase,
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

    fn restore_frontend_rollback_token(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::RestoreFrontendRollbackToken { target, result }
    }

    fn acquire_frontend_runtime_for_rollback(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::AcquireFrontendRuntimeForRollback { target, result }
    }

    fn discard_frontend_rollback_authority(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::DiscardFrontendRollbackAuthority { target, result }
    }

    fn quarantine_frontend_after_rollback_failure(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::QuarantineFrontendAfterRollbackFailure { target, result }
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

    fn restore_bound_demux_step(
        target: FrontendWorkerCleanupTarget,
        outcome: &BoundDemuxRollbackStepOutcome,
    ) -> Self {
        Self::RestoreBoundDemuxStep {
            target,
            rollback_target: outcome.target,
            phase: outcome.phase,
            result: outcome.result.clone(),
        }
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
            | Self::RestoreFrontendRollbackToken { target, .. }
            | Self::AcquireFrontendRuntimeForRollback { target, .. }
            | Self::DiscardFrontendRollbackAuthority { target, .. }
            | Self::QuarantineFrontendAfterRollbackFailure { target, .. }
            | Self::TakeDemuxRollbackTokens { target, .. }
            | Self::RestoreBoundDemuxes { target, .. }
            | Self::RestoreBoundDemuxStep { target, .. }
            | Self::CompleteReplacement { target, .. }
            | Self::CompleteStopObject { target, .. }
            | Self::CloseOwnedLnb { target, .. }
            | Self::CloseFrontendWorkersAndLiveData { target, .. } => *target,
        }
    }

    pub fn frontend_id(&self) -> i32 {
        self.target().frontend_id()
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
            Self::RestoreFrontendRollbackToken { .. } => {
                FrontendWorkerCleanupStep::RestoreFrontendRollbackToken
            }
            Self::AcquireFrontendRuntimeForRollback { .. } => {
                FrontendWorkerCleanupStep::AcquireFrontendRuntimeForRollback
            }
            Self::DiscardFrontendRollbackAuthority { .. } => {
                FrontendWorkerCleanupStep::DiscardFrontendRollbackAuthority
            }
            Self::QuarantineFrontendAfterRollbackFailure { .. } => {
                FrontendWorkerCleanupStep::QuarantineFrontendAfterRollbackFailure
            }
            Self::TakeDemuxRollbackTokens { .. } => {
                FrontendWorkerCleanupStep::TakeDemuxRollbackTokens
            }
            Self::RestoreBoundDemuxes { .. } => FrontendWorkerCleanupStep::RestoreBoundDemuxes,
            Self::RestoreBoundDemuxStep {
                rollback_target,
                phase,
                ..
            } => FrontendWorkerCleanupStep::RestoreBoundDemuxStep(*rollback_target, *phase),
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
            | Self::RestoreFrontendRollbackToken { result, .. }
            | Self::AcquireFrontendRuntimeForRollback { result, .. }
            | Self::DiscardFrontendRollbackAuthority { result, .. }
            | Self::QuarantineFrontendAfterRollbackFailure { result, .. }
            | Self::TakeDemuxRollbackTokens { result, .. }
            | Self::RestoreBoundDemuxes { result, .. }
            | Self::RestoreBoundDemuxStep { result, .. }
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
            | Self::RestoreFrontendRollbackToken { result, .. }
            | Self::AcquireFrontendRuntimeForRollback { result, .. }
            | Self::DiscardFrontendRollbackAuthority { result, .. }
            | Self::QuarantineFrontendAfterRollbackFailure { result, .. }
            | Self::TakeDemuxRollbackTokens { result, .. }
            | Self::RestoreBoundDemuxes { result, .. }
            | Self::RestoreBoundDemuxStep { result, .. }
            | Self::CompleteReplacement { result, .. }
            | Self::CompleteStopObject { result, .. }
            | Self::CloseOwnedLnb { result, .. }
            | Self::CloseFrontendWorkersAndLiveData { result, .. } => result,
        }
    }
}


fn with_demux_rollback_tokens<T>(
    tokens: &SharedDemuxRollbackTokenList,
    operation: impl FnOnce(&DemuxRollbackTokenList) -> Result<T, HalError>,
) -> Result<T, HalError> {
    let guard = tokens.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "demux rollback token list lock poisoned",
        )
    })?;
    let tokens = guard.as_ref().ok_or_else(|| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "demux rollback token list was already consumed",
        )
    })?;
    operation(tokens)
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupPublicOutcome {
    NoPublicError,
    PublicError(HalError),
}

impl FrontendWorkerCleanupPublicOutcome {
    fn from_optional_error(error: Option<HalError>) -> Self {
        match error {
            Some(error) => Self::PublicError(error),
            None => Self::NoPublicError,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FrontendWorkerCleanupDiagnosticRecord {
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    report: FrontendWorkerCleanupExecutionReport,
    public_outcome: FrontendWorkerCleanupPublicOutcome,
}

impl FrontendWorkerCleanupDiagnosticRecord {
    pub fn new(
        kind: FrontendWorkerCleanupDiagnosticKind,
        target: FrontendWorkerCleanupTarget,
        report: FrontendWorkerCleanupExecutionReport,
        public_outcome: FrontendWorkerCleanupPublicOutcome,
    ) -> Self {
        Self {
            kind,
            target,
            report,
            public_outcome,
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

    pub fn report(&self) -> &FrontendWorkerCleanupExecutionReport {
        &self.report
    }

    pub fn public_outcome(&self) -> &FrontendWorkerCleanupPublicOutcome {
        &self.public_outcome
    }
}

pub type FrontendWorkerCleanupDiagnosticSnapshot =
    CleanupExecutionDiagnosticSnapshot<FrontendWorkerCleanupDiagnosticRecord>;
pub type SharedFrontendWorkerCleanupDiagnostics =
    SharedCleanupDiagnostics<FrontendWorkerCleanupDiagnosticRecord>;

type BoundDemuxGenerationSnapshot = Vec<(crate::registry::DemuxRuntimeId, u64)>;

fn share_frontend_rollback_token(
    token: FrontendRuntimeRollbackToken,
) -> SharedFrontendRuntimeRollbackToken {
    Arc::new(Mutex::new(Some(token)))
}

fn with_frontend_rollback_token_mut<T>(
    token: &SharedFrontendRuntimeRollbackToken,
    context: &'static str,
    operation: impl FnOnce(&mut FrontendRuntimeRollbackToken) -> Result<T, HalError>,
) -> Result<T, HalError> {
    let mut guard = token.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend rollback token lock poisoned",
        )
    })?;
    let token = guard
        .as_mut()
        .ok_or_else(|| HalError::internal(HalInternalKind::InvariantViolation, context))?;
    let result = operation(token);
    if result.is_ok() {
        guard.take();
    }
    result
}

fn commit_shared_frontend_tune_rollback_expected_post_state(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    token: &SharedFrontendRuntimeRollbackToken,
    generation: u64,
    request: FrontendTuneRequest,
) -> Result<(), HalError> {
    let token_guard = token.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend rollback token lock poisoned while recording tune expected post state",
        )
    })?;
    let token = token_guard.as_ref().ok_or_else(|| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend rollback token missing while recording tune expected post state",
        )
    })?;
    guard
        .frontend_txn()
        .commit_frontend_tune_rollback_expected_post_state(
            frontend_id,
            token,
            generation,
            request,
        )
}

fn begin_shared_frontend_scan_rollback_expected_post_state(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    token: &SharedFrontendRuntimeRollbackToken,
    generation: u64,
    fingerprint: String,
    candidates: Vec<FrontendTuneRequest>,
) -> Result<(), HalError> {
    let token_guard = token.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend rollback token lock poisoned while beginning scan expected post state",
        )
    })?;
    let token = token_guard.as_ref().ok_or_else(|| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend rollback token missing while beginning scan expected post state",
        )
    })?;
    guard
        .frontend_txn()
        .begin_frontend_scan_rollback_expected_post_state(
            frontend_id,
            token,
            generation,
            fingerprint,
            candidates,
        )
}

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

fn discard_frontend_rollback_token_without_runtime(
    token: &SharedFrontendRuntimeRollbackToken,
) -> Result<(), HalError> {
    let mut guard = token.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend rollback token lock poisoned while discarding authority",
        )
    })?;
    let token = guard.take().ok_or_else(|| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend rollback token was already consumed while discarding authority",
        )
    })?;
    token.discard_without_runtime()
}

fn discard_demux_rollback_tokens_without_runtime(
    tokens: &SharedDemuxRollbackTokenList,
) -> BoundDemuxRollbackExecutionReport {
    let mut report = BoundDemuxRollbackExecutionReport::new();
    match take_demux_rollback_tokens(
        tokens,
        "demux rollback token list was already consumed while discarding authority",
    ) {
        Ok(tokens) => {
            for (demux_id, token) in tokens {
                let result = token
                    .discard_without_runtime()
                    .map_err(|error| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            format!("bound demux rollback authority discard failed: demux_id={}, error={error:?}", demux_id.0),
                        )
                    });
                report.push(BoundDemuxRollbackStepOutcome {
                    target: BoundDemuxRollbackTarget::Demux(demux_id.0),
                    phase: BoundDemuxRollbackPhase::DiscardAuthority,
                    result,
                });
            }
        }
        Err(error) => report.push(BoundDemuxRollbackStepOutcome {
            target: BoundDemuxRollbackTarget::TokenList,
            phase: BoundDemuxRollbackPhase::DiscardAuthority,
            result: Err(error),
        }),
    }
    report
}

struct FrontendWorkerReplacementTicket {
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    stopped_worker_generation: Option<u64>,
    new_worker_generation: u64,
    frontend_rollback_token: FrontendRuntimeRollbackToken,
    previous_tune_request: Option<FrontendTuneRequest>,
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
    frontend_rollback_token: FrontendRuntimeRollbackToken,
    demux_rollback_tokens: DemuxRollbackTokenList,
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

fn frontend_live_data_expectation(
    outcome: &FrontendWorkerStopOutcome,
    kind: FrontendWorkerKind,
) -> Option<(u64, FrontendWorkerKind)> {
    frontend_worker_stop_outcome_generation(outcome).map(|generation| (generation, kind))
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

fn ensure_frontend_join_rollback_token_still_matches(
    guard: &TunerServiceRuntime,
    frontend_id: i32,
    expected_frontend: &FrontendRuntimeRollbackToken,
    expected_demux_tokens: &DemuxRollbackTokenList,
    expected_demux_generations: &BoundDemuxGenerationSnapshot,
) -> Result<(), HalError> {
    if !guard
        .query()
        .frontend_runtime_matches_rollback_token(frontend_id, expected_frontend)?
    {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker ticket rollback token no longer matches runtime after external join",
        ));
    }
    if !guard.bound_demux_runtime_rollback_tokens_match(expected_demux_tokens)? {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker ticket bound demux rollback token no longer matches runtime after external join",
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

fn append_bound_demux_rollback_report(
    report: &mut FrontendWorkerCleanupExecutionReport,
    target: FrontendWorkerCleanupTarget,
    demux_report: &BoundDemuxRollbackExecutionReport,
) {
    for outcome in demux_report.outcomes() {
        report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demux_step(
            target, outcome,
        ));
    }
}

fn record_bound_demux_rollback_preparation_failure(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    diagnostic_kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    failure: BoundDemuxRollbackPreparationFailure,
    context: &'static str,
) -> HalError {
    let BoundDemuxRollbackPreparationFailure { error, report: demux_report } = failure;
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    append_bound_demux_rollback_report(&mut report, target, &demux_report);
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        diagnostic_kind,
        target,
        report,
        FrontendWorkerCleanupPublicOutcome::PublicError(error.clone()),
    );
    match sink.record(record) {
        Ok(()) => error,
        Err(record_error) => compose_frontend_worker_cleanup_record_failure(
            context,
            error,
            record_error,
        ),
    }
}

fn record_frontend_and_demux_rollback_authority_discard_failure(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    diagnostic_kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    failure: BoundDemuxRollbackPreparationFailure,
    frontend_discard_result: Result<(), HalError>,
    context: &'static str,
) -> HalError {
    let BoundDemuxRollbackPreparationFailure { error, report: demux_report } = failure;
    let public_error = match frontend_discard_result.clone() {
        Ok(()) => error,
        Err(discard_error) => compose_primary_cleanup_failure(
            "frontend rollback authority preparation failed and frontend authority discard failed",
            error,
            discard_error,
        ),
    };
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    append_bound_demux_rollback_report(&mut report, target, &demux_report);
    report.push(FrontendWorkerCleanupStepOutcome::discard_frontend_rollback_authority(
        target,
        frontend_discard_result,
    ));
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        diagnostic_kind,
        target,
        report,
        FrontendWorkerCleanupPublicOutcome::PublicError(public_error.clone()),
    );
    match sink.record(record) {
        Ok(()) => public_error,
        Err(record_error) => compose_frontend_worker_cleanup_record_failure(
            context,
            public_error,
            record_error,
        ),
    }
}


fn discard_owned_frontend_and_demux_rollback_authorities(
    report: &mut FrontendWorkerCleanupExecutionReport,
    target: FrontendWorkerCleanupTarget,
    frontend_token: FrontendRuntimeRollbackToken,
    demux_tokens: DemuxRollbackTokenList,
    primary: HalError,
) -> HalError {
    let mut combined = primary;
    for (demux_id, token) in demux_tokens {
        let result = token.discard_without_runtime().map_err(|error| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!(
                    "bound demux rollback authority discard failed: demux_id={}, error={error:?}",
                    demux_id.0
                ),
            )
        });
        if let Err(error) = result.clone() {
            combined = compose_primary_cleanup_failure(
                "frontend worker stop failed and bound demux authority discard failed",
                combined,
                error,
            );
        }
        let outcome = BoundDemuxRollbackStepOutcome {
            target: BoundDemuxRollbackTarget::Demux(demux_id.0),
            phase: BoundDemuxRollbackPhase::DiscardAuthority,
            result,
        };
        report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demux_step(
            target,
            &outcome,
        ));
    }
    let frontend_discard_result = frontend_token.discard_without_runtime();
    if let Err(error) = frontend_discard_result.clone() {
        combined = compose_primary_cleanup_failure(
            "frontend worker stop failed and frontend authority discard failed",
            combined,
            error,
        );
    }
    report.push(FrontendWorkerCleanupStepOutcome::discard_frontend_rollback_authority(
        target,
        frontend_discard_result,
    ));
    combined
}

fn recover_frontend_ticket_after_validation_failure(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    frontend_rollback_token: &mut FrontendRuntimeRollbackToken,
    demux_rollback_tokens: Option<DemuxRollbackTokenList>,
    primary: HalError,
) -> (
    BoundDemuxRollbackExecutionReport,
    Result<(), HalError>,
    HalError,
) {
    let demux_restore_report = match demux_rollback_tokens {
        Some(tokens) => guard
            .frontend_txn()
            .restore_bound_demux_runtime_rollback_tokens(tokens),
        None => BoundDemuxRollbackExecutionReport::new(),
    };
    let demux_restore_result = demux_restore_report.result();
    let frontend_quarantine_result = guard
        .frontend_txn()
        .quarantine_frontend_after_rollback_failure(
            frontend_id,
            frontend_rollback_token,
            primary.clone(),
        );
    let mut combined = primary;
    if let Err(error) = demux_restore_result.clone() {
        combined = compose_primary_cleanup_failure(
            "frontend ticket validation failed and bound demux rollback failed",
            combined,
            error,
        );
    }
    if let Err(error) = frontend_quarantine_result.clone() {
        combined = compose_primary_cleanup_failure(
            "frontend ticket validation failed and frontend quarantine failed",
            combined,
            error,
        );
    }
    (demux_restore_report, frontend_quarantine_result, combined)
}

fn record_frontend_replacement_validation_failure(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    diagnostic_kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    kind: FrontendWorkerKind,
    stop_outcome: &FrontendWorkerStopOutcome,
    stopped_worker_generation: Option<u64>,
    new_worker_generation: u64,
    demux_restore_report: BoundDemuxRollbackExecutionReport,
    frontend_quarantine_result: Result<(), HalError>,
    primary: HalError,
) -> HalError {
    let mut report = build_frontend_worker_replacement_stop_report(
        target,
        kind,
        stop_outcome,
        None,
    );
    append_bound_demux_rollback_report(&mut report, target, &demux_restore_report);
    report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demuxes(
        target,
        demux_restore_report.result(),
    ));
    report.push(FrontendWorkerCleanupStepOutcome::quarantine_frontend_after_rollback_failure(
        target,
        frontend_quarantine_result,
    ));
    report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
        target,
        kind,
        stopped_worker_generation,
        new_worker_generation,
        Err(primary.clone()),
    ));
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        diagnostic_kind,
        target,
        report,
        FrontendWorkerCleanupPublicOutcome::PublicError(primary.clone()),
    );
    match sink.record(record) {
        Ok(()) => primary,
        Err(record_error) => compose_frontend_worker_cleanup_record_failure(
            "frontend replacement recovery diagnostic record failed",
            primary,
            record_error,
        ),
    }
}

fn record_frontend_stop_object_validation_failure(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    diagnostic_kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    kind: FrontendWorkerKind,
    stop_outcome: &FrontendWorkerStopOutcome,
    demux_restore_report: BoundDemuxRollbackExecutionReport,
    frontend_quarantine_result: Result<(), HalError>,
    primary: HalError,
) -> HalError {
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
        target,
        kind,
        frontend_worker_stop_outcome_generation(stop_outcome),
        frontend_worker_stop_result_from_outcome(stop_outcome),
    ));
    append_bound_demux_rollback_report(&mut report, target, &demux_restore_report);
    report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demuxes(
        target,
        demux_restore_report.result(),
    ));
    report.push(FrontendWorkerCleanupStepOutcome::quarantine_frontend_after_rollback_failure(
        target,
        frontend_quarantine_result,
    ));
    report.push(FrontendWorkerCleanupStepOutcome::complete_stop_object(
        target,
        kind,
        frontend_worker_stop_outcome_generation(stop_outcome),
        Err(primary.clone()),
    ));
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        diagnostic_kind,
        target,
        report,
        FrontendWorkerCleanupPublicOutcome::PublicError(primary.clone()),
    );
    match sink.record(record) {
        Ok(()) => primary,
        Err(record_error) => compose_frontend_worker_cleanup_record_failure(
            "frontend stop-object recovery diagnostic record failed",
            primary,
            record_error,
        ),
    }
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
        FrontendRuntimeRollbackToken,
        Option<FrontendTuneRequest>,
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
        mut frontend_rollback_token,
        previous_tune_request,
        demux_rollback_tokens,
        bound_demux_generations,
        stop_ticket,
    } = ticket;
    let mut demux_rollback_tokens = Some(demux_rollback_tokens);
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
        match &stop_outcome {
            FrontendWorkerStopOutcome::StopRequestFailed { .. } => {
                let mut report = build_frontend_worker_replacement_stop_report(
                    target,
                    kind,
                    &stop_outcome,
                    None,
                );
                let error = discard_owned_frontend_and_demux_rollback_authorities(
                    &mut report,
                    target,
                    frontend_rollback_token,
                    demux_rollback_tokens.take().unwrap_or_default(),
                    error,
                );
                report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
                    target,
                    kind,
                    stopped_worker_generation,
                    new_worker_generation,
                    Err(error.clone()),
                ));
                cleanup_diagnostic_sink.record_nonblocking(
                    FrontendWorkerCleanupDiagnosticRecord::new(
                        diagnostic_kind,
                        target,
                        report,
                        FrontendWorkerCleanupPublicOutcome::PublicError(error.clone()),
                    ),
                );
                return Err(error);
            }
            FrontendWorkerStopOutcome::Completed { .. } => {
                let (mut guard, lock_failure) = lock_runtime_for_cleanup(runtime, context);
                let primary = lock_failure
                    .map(|lock_error| {
                        compose_primary_cleanup_failure(
                            "frontend worker stop failed and runtime cleanup lock was poisoned",
                            error.clone(),
                            lock_error,
                        )
                    })
                    .unwrap_or(error);
                let (demux_restore_report, frontend_quarantine_result, combined) =
                    recover_frontend_ticket_after_validation_failure(
                        &mut guard,
                        frontend_id,
                        &mut frontend_rollback_token,
                        demux_rollback_tokens.take(),
                        primary,
                    );
                return Err(record_frontend_replacement_validation_failure(
                    cleanup_diagnostic_sink.clone(),
                    diagnostic_kind,
                    target,
                    kind,
                    &stop_outcome,
                    stopped_worker_generation,
                    new_worker_generation,
                    demux_restore_report,
                    frontend_quarantine_result,
                    combined,
                ));
            }
            FrontendWorkerStopOutcome::NotRunning
            | FrontendWorkerStopOutcome::CancelRequested { .. } => {
                return Err(record_stop_outcome_for_failure(error));
            }
        }
    }
    let (mut guard, lock_failure) = lock_runtime_for_cleanup(runtime, context);
    if let Some(error) = lock_failure {
        let (demux_restore_result, frontend_quarantine_result, error) =
            recover_frontend_ticket_after_validation_failure(
                &mut guard,
                frontend_id,
                &mut frontend_rollback_token,
                demux_rollback_tokens.take(),
                error,
            );
        return Err(record_frontend_replacement_validation_failure(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            stopped_worker_generation,
            new_worker_generation,
            demux_restore_result,
            frontend_quarantine_result,
            error,
        ));
    }
    if let Err(error) = ensure_frontend_ticket_still_targets_object(
        &guard,
        object_id,
        object_generation,
        frontend_id,
    ) {
        let (demux_restore_result, frontend_quarantine_result, error) =
            recover_frontend_ticket_after_validation_failure(
                &mut guard,
                frontend_id,
                &mut frontend_rollback_token,
                demux_rollback_tokens.take(),
                error,
            );
        return Err(record_frontend_replacement_validation_failure(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            stopped_worker_generation,
            new_worker_generation,
            demux_restore_result,
            frontend_quarantine_result,
            error,
        ));
    }
    if let Err(error) = ensure_frontend_join_rollback_token_still_matches(
        &guard,
        frontend_id,
        &frontend_rollback_token,
        demux_rollback_tokens.as_ref().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "bound demux rollback tokens missing during external join validation",
            )
        })?,
        &bound_demux_generations,
    ) {
        let (demux_restore_result, frontend_quarantine_result, error) =
            recover_frontend_ticket_after_validation_failure(
                &mut guard,
                frontend_id,
                &mut frontend_rollback_token,
                demux_rollback_tokens.take(),
                error,
            );
        return Err(record_frontend_replacement_validation_failure(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            stopped_worker_generation,
            new_worker_generation,
            demux_restore_result,
            frontend_quarantine_result,
            error,
        ));
    }
    if frontend_worker_stop_outcome_generation(&stop_outcome) != stopped_worker_generation {
        let error = HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend worker replacement ticket generation mismatch",
        );
        let (demux_restore_result, frontend_quarantine_result, error) =
            recover_frontend_ticket_after_validation_failure(
                &mut guard,
                frontend_id,
                &mut frontend_rollback_token,
                demux_rollback_tokens.take(),
                error,
            );
        return Err(record_frontend_replacement_validation_failure(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            stopped_worker_generation,
            new_worker_generation,
            demux_restore_result,
            frontend_quarantine_result,
            error,
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
                    let error = HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend worker replacement ticket kind mismatch",
                    );
                    let (demux_restore_result, frontend_quarantine_result, error) =
                        recover_frontend_ticket_after_validation_failure(
                            &mut guard,
                            frontend_id,
                            &mut frontend_rollback_token,
                            demux_rollback_tokens.take(),
                            error,
                        );
                    return Err(record_frontend_replacement_validation_failure(
                        cleanup_diagnostic_sink.clone(),
                        diagnostic_kind,
                        target,
                        kind,
                        &stop_outcome,
                        stopped_worker_generation,
                        new_worker_generation,
                        demux_restore_result,
                        frontend_quarantine_result,
                        error,
                    ));
                }
            }
            FrontendWorkerStopOutcome::NotRunning
            | FrontendWorkerStopOutcome::StopRequestFailed { .. } => {}
        }
    }
    let Some(demux_rollback_tokens) = demux_rollback_tokens else {
        return Err(record_stop_outcome_for_failure(HalError::internal(
            HalInternalKind::InvariantViolation,
            "demux rollback tokens consumed during successful replacement validation",
        )));
    };
    Ok((
        guard,
        frontend_id,
        new_worker_generation,
        stop_outcome,
        frontend_rollback_token,
        previous_tune_request,
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
    let target = FrontendWorkerCleanupTarget::object(
        frontend_id,
        object_id,
        object_generation,
    );
    let diagnostic_kind = match kind {
        FrontendWorkerKind::Tune => FrontendWorkerCleanupDiagnosticKind::StopTuneObject,
        FrontendWorkerKind::Scan => FrontendWorkerCleanupDiagnosticKind::StopScanObject,
    };
    let cleanup_diagnostic_sink = runtime.frontend_worker_cleanup_diagnostic_sink();
    let demux_preparation = match runtime.prepare_bound_demux_runtime_rollback_tokens(frontend_id) {
        Ok(preparation) => preparation,
        Err(failure) => {
            return Err(record_bound_demux_rollback_preparation_failure(
                cleanup_diagnostic_sink,
                diagnostic_kind,
                target,
                failure,
                "frontend stop-object demux rollback preparation diagnostic record failed",
            ));
        }
    };
    let frontend_rollback_token = match runtime
        .frontend_txn()
        .prepare_frontend_runtime_rollback_capture(frontend_id)
    {
        Ok(capture) => capture.into_token(),
        Err(error) => {
            let failure = runtime.discard_bound_demux_rollback_preparation(
                demux_preparation,
                error,
            );
            return Err(record_bound_demux_rollback_preparation_failure(
                cleanup_diagnostic_sink,
                diagnostic_kind,
                target,
                failure,
                "frontend stop-object rollback authority discard diagnostic record failed",
            ));
        }
    };
    let (demux_rollback_tokens, _preparation_report) = demux_preparation.into_parts();
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
        frontend_rollback_token,
        demux_rollback_tokens,
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
        mut frontend_rollback_token,
        demux_rollback_tokens,
        bound_demux_generations,
        stop_ticket,
    } = ticket;
    let mut demux_rollback_tokens = Some(demux_rollback_tokens);
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
                FrontendWorkerCleanupPublicOutcome::PublicError(primary.clone()),
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
        let mut report = FrontendWorkerCleanupExecutionReport::new();
        report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            kind,
            frontend_worker_stop_outcome_generation(&stop_outcome),
            frontend_worker_stop_result_from_outcome(&stop_outcome),
        ));
        let error = discard_owned_frontend_and_demux_rollback_authorities(
            &mut report,
            target,
            frontend_rollback_token,
            demux_rollback_tokens.take().unwrap_or_default(),
            error,
        );
        report.push(FrontendWorkerCleanupStepOutcome::complete_stop_object(
            target,
            kind,
            frontend_worker_stop_outcome_generation(&stop_outcome),
            Err(error.clone()),
        ));
        cleanup_diagnostic_sink.record_nonblocking(
            FrontendWorkerCleanupDiagnosticRecord::new(
                diagnostic_kind,
                target,
                report,
                FrontendWorkerCleanupPublicOutcome::PublicError(error.clone()),
            ),
        );
        return Err(error);
    }
    let (mut guard, lock_failure) = lock_runtime_for_cleanup(runtime, context);
    if let Some(error) = lock_failure {
        let (demux_restore_result, frontend_quarantine_result, error) =
            recover_frontend_ticket_after_validation_failure(
                &mut guard,
                frontend_id,
                &mut frontend_rollback_token,
                demux_rollback_tokens.take(),
                error,
            );
        return Err(record_frontend_stop_object_validation_failure(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            demux_restore_result,
            frontend_quarantine_result,
            error,
        ));
    }
    if let Err(error) = ensure_frontend_ticket_still_targets_object(
        &guard,
        object_id,
        object_generation,
        frontend_id,
    ) {
        let (demux_restore_result, frontend_quarantine_result, error) =
            recover_frontend_ticket_after_validation_failure(
                &mut guard,
                frontend_id,
                &mut frontend_rollback_token,
                demux_rollback_tokens.take(),
                error,
            );
        return Err(record_frontend_stop_object_validation_failure(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            demux_restore_result,
            frontend_quarantine_result,
            error,
        ));
    }
    if let Err(error) = ensure_frontend_join_rollback_token_still_matches(
        &guard,
        frontend_id,
        &frontend_rollback_token,
        demux_rollback_tokens.as_ref().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "bound demux rollback tokens missing during external join validation",
            )
        })?,
        &bound_demux_generations,
    ) {
        let (demux_restore_result, frontend_quarantine_result, error) =
            recover_frontend_ticket_after_validation_failure(
                &mut guard,
                frontend_id,
                &mut frontend_rollback_token,
                demux_rollback_tokens.take(),
                error,
            );
        return Err(record_frontend_stop_object_validation_failure(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            demux_restore_result,
            frontend_quarantine_result,
            error,
        ));
    }
    if frontend_worker_stop_outcome_generation(&stop_outcome) != worker_generation {
        let error = HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend worker stop ticket generation mismatch",
        );
        let (demux_restore_result, frontend_quarantine_result, error) =
            recover_frontend_ticket_after_validation_failure(
                &mut guard,
                frontend_id,
                &mut frontend_rollback_token,
                demux_rollback_tokens.take(),
                error,
            );
        return Err(record_frontend_stop_object_validation_failure(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            demux_restore_result,
            frontend_quarantine_result,
            error,
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
                    let error = HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend worker stop ticket kind mismatch",
                    );
                    let (demux_restore_result, frontend_quarantine_result, error) =
                        recover_frontend_ticket_after_validation_failure(
                            &mut guard,
                            frontend_id,
                            &mut frontend_rollback_token,
                            demux_rollback_tokens.take(),
                            error,
                        );
                    return Err(record_frontend_stop_object_validation_failure(
                        cleanup_diagnostic_sink.clone(),
                        diagnostic_kind,
                        target,
                        kind,
                        &stop_outcome,
                        demux_restore_result,
                        frontend_quarantine_result,
                        error,
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

fn lock_runtime_for_cleanup<'a>(
    runtime: &'a SharedRuntime,
    context: &'static str,
) -> (
    std::sync::MutexGuard<'a, TunerServiceRuntime>,
    Option<HalError>,
) {
    match runtime.lock() {
        Ok(guard) => (guard, None),
        Err(poisoned) => (
            poisoned.into_inner(),
            Some(HalError::internal(
                HalInternalKind::InvariantViolation,
                context,
            )),
        ),
    }
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
        FrontendWorkerCleanupPublicOutcome::PublicError(public_error.clone()),
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

fn restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    frontend_rollback_token: &SharedFrontendRuntimeRollbackToken,
    demux_rollback_tokens: &SharedDemuxRollbackTokenList,
    primary: HalError,
    context: &'static str,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> HalError {
    let sink = Ok(guard.frontend_worker_cleanup_diagnostic_sink());
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    let stopped_previous_worker = replacement_context
        .is_some_and(|context| context.stopped_worker_generation.is_some());
    if let Some(replacement_context) = replacement_context {
        report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
            target,
            replacement_context.worker_kind,
            replacement_context.stopped_worker_generation,
            replacement_context.new_worker_generation,
            Err(primary.clone()),
        ));
    }

    let demux_restore_result = match take_demux_rollback_tokens(
        demux_rollback_tokens,
        "demux rollback token list was already consumed",
    ) {
        Ok(tokens) => {
            report.push(FrontendWorkerCleanupStepOutcome::take_demux_rollback_tokens(
                target,
                Ok(()),
            ));
            let demux_report = guard
                .frontend_txn()
                .restore_bound_demux_runtime_rollback_tokens(tokens);
            append_bound_demux_rollback_report(&mut report, target, &demux_report);
            demux_report.result()
        }
        Err(error) => {
            report.push(FrontendWorkerCleanupStepOutcome::take_demux_rollback_tokens(
                target,
                Err(error.clone()),
            ));
            Err(error)
        }
    };
    report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demuxes(
        target,
        demux_restore_result.clone(),
    ));

    if stopped_previous_worker {
        let quarantine_reason = demux_restore_result
            .clone()
            .err()
            .unwrap_or_else(|| primary.clone());
        let quarantine_result = with_frontend_rollback_token_mut(
            frontend_rollback_token,
            "frontend rollback token was already consumed while fail-closing stopped replacement",
            |token| {
                guard.frontend_txn().quarantine_frontend_after_rollback_failure(
                    frontend_id,
                    token,
                    quarantine_reason.clone(),
                )
            },
        );
        report.push(
            FrontendWorkerCleanupStepOutcome::quarantine_frontend_after_rollback_failure(
                target,
                quarantine_result,
            ),
        );
    } else {
        match demux_restore_result {
            Ok(()) => {
                let restore_result = with_frontend_rollback_token_mut(
                    frontend_rollback_token,
                    "frontend rollback token was already consumed",
                    |token| {
                        guard
                            .frontend_txn()
                            .restore_frontend_runtime_rollback_token(frontend_id, token)
                    },
                );
                report.push(FrontendWorkerCleanupStepOutcome::restore_frontend_rollback_token(
                    target,
                    restore_result.clone(),
                ));
                if let Err(restore_error) = restore_result {
                    let quarantine_result = with_frontend_rollback_token_mut(
                        frontend_rollback_token,
                        "frontend rollback token was already consumed while quarantining failed frontend restore",
                        |token| {
                            guard.frontend_txn().quarantine_frontend_after_rollback_failure(
                                frontend_id,
                                token,
                                restore_error.clone(),
                            )
                        },
                    );
                    report.push(
                        FrontendWorkerCleanupStepOutcome::quarantine_frontend_after_rollback_failure(
                            target,
                            quarantine_result,
                        ),
                    );
                }
            }
            Err(demux_error) => {
                let quarantine_result = with_frontend_rollback_token_mut(
                    frontend_rollback_token,
                    "frontend rollback token was already consumed while quarantining split rollback",
                    |token| {
                        guard.frontend_txn().quarantine_frontend_after_rollback_failure(
                            frontend_id,
                            token,
                            demux_error.clone(),
                        )
                    },
                );
                report.push(
                    FrontendWorkerCleanupStepOutcome::quarantine_frontend_after_rollback_failure(
                        target,
                        quarantine_result,
                    ),
                );
            }
        }
    }

    finish_frontend_worker_rollback_report(sink, kind, target, report, primary, context)
}

fn finish_frontend_state_restore_lock_failure_report(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    frontend_rollback_token: &SharedFrontendRuntimeRollbackToken,
    demux_rollback_tokens: &SharedDemuxRollbackTokenList,
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
            Err(primary.clone()),
        ));
    }
    let demux_discard_report =
        discard_demux_rollback_tokens_without_runtime(demux_rollback_tokens);
    append_bound_demux_rollback_report(&mut report, target, &demux_discard_report);
    report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demuxes(
        target,
        demux_discard_report.result(),
    ));
    report.push(FrontendWorkerCleanupStepOutcome::acquire_frontend_runtime_for_rollback(
        target,
        Err(lock_error),
    ));
    let frontend_discard_result = discard_frontend_rollback_token_without_runtime(
        frontend_rollback_token,
    );
    report.push(FrontendWorkerCleanupStepOutcome::discard_frontend_rollback_authority(
        target,
        frontend_discard_result,
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
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        kind,
        target,
        report,
        FrontendWorkerCleanupPublicOutcome::from_optional_error(public_error),
    );
    sink.record(record)
}

fn record_frontend_worker_replacement_stop_report(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    stop_outcome: &FrontendWorkerStopOutcome,
    scan_cancel_result: Option<Result<(), HalError>>,
) {
    let report = build_frontend_worker_replacement_stop_report(
        target,
        worker_kind,
        stop_outcome,
        scan_cancel_result,
    );
    let public_error = report.clone().into_result().err();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        kind,
        target,
        report,
        FrontendWorkerCleanupPublicOutcome::from_optional_error(public_error),
    );
    sink.record_nonblocking(record);
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
    let target = FrontendWorkerCleanupTarget::object(
        frontend_id,
        object_id,
        object_generation,
    );
    let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
    let demux_preparation = match guard.prepare_bound_demux_runtime_rollback_tokens(frontend_id) {
        Ok(preparation) => preparation,
        Err(failure) => {
            return Err(record_bound_demux_rollback_preparation_failure(
                cleanup_diagnostic_sink,
                FrontendWorkerCleanupDiagnosticKind::TuneStartRollback,
                target,
                failure,
                "tune demux rollback preparation diagnostic record failed",
            ));
        }
    };
    let (frontend_rollback_token, previous_tune_request) = match guard
        .frontend_txn()
        .prepare_frontend_runtime_rollback_capture(frontend_id)
    {
        Ok(capture) => capture.into_replacement_parts(),
        Err(error) => {
            let failure = guard.discard_bound_demux_rollback_preparation(
                demux_preparation,
                error,
            );
            return Err(record_bound_demux_rollback_preparation_failure(
                cleanup_diagnostic_sink,
                FrontendWorkerCleanupDiagnosticKind::TuneStartRollback,
                target,
                failure,
                "tune rollback authority discard diagnostic record failed",
            ));
        }
    };
    let generation = match guard
        .frontend_txn()
        .prepare_frontend_worker_replacement_generation(frontend_id, kind)
    {
        Ok(generation) => generation,
        Err(error) => {
            let demux_failure = guard.discard_bound_demux_rollback_preparation(
                demux_preparation,
                error,
            );
            let frontend_discard_result = frontend_rollback_token.discard_without_runtime();
            return Err(record_frontend_and_demux_rollback_authority_discard_failure(
                cleanup_diagnostic_sink,
                FrontendWorkerCleanupDiagnosticKind::TuneStartRollback,
                target,
                demux_failure,
                frontend_discard_result,
                "tune rollback authority discard diagnostic record failed",
            ));
        }
    };
    let (demux_rollback_tokens, _preparation_report) = demux_preparation.into_parts();
    let bound_demux_generations = bound_demux_generation_snapshot(&demux_rollback_tokens);
    let stop_ticket = request_tune_worker_replacement_stop(&mut guard, frontend_id);
    let replacement_ticket = FrontendWorkerReplacementTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        stopped_worker_generation: stop_ticket.worker_generation(),
        new_worker_generation: generation,
        frontend_rollback_token,
        previous_tune_request,
        demux_rollback_tokens,
        bound_demux_generations,
        stop_ticket,
    };
    drop(guard);
    let (
        mut guard,
        frontend_id,
        generation,
        stop_outcome,
        frontend_rollback_token,
        previous_tune_for_worker,
        demux_rollback_tokens,
    ) = complete_frontend_worker_replacement_ticket(
        &runtime,
        replacement_ticket,
        cleanup_diagnostic_sink.clone(),
        FrontendWorkerCleanupDiagnosticKind::TuneReplacementStop,
        "service runtime lock poisoned after tune worker join",
    )?;
    let frontend_rollback_token = share_frontend_rollback_token(frontend_rollback_token);
    let demux_rollback_tokens = share_demux_rollback_tokens(demux_rollback_tokens);
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
    );
    if let Err(error) = with_demux_rollback_tokens(&demux_rollback_tokens, |tokens| {
        guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id, tokens)
    }) {
        return Err(
            restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                &mut guard,
                frontend_id,
                &frontend_rollback_token,
                &demux_rollback_tokens,
                error,
                "frontend tune start reset rollback",
                FrontendWorkerCleanupDiagnosticKind::TuneStartRollback,
                target,
                replacement_context,
            ),
        );
    }
    if let Err(error) = commit_shared_frontend_tune_rollback_expected_post_state(
        &mut guard,
        frontend_id,
        &frontend_rollback_token,
        generation,
        request.clone(),
    ) {
        return Err(restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
            &mut guard,
            frontend_id,
            &frontend_rollback_token,
            &demux_rollback_tokens,
            error,
            "frontend tune expected post state record rollback",
            FrontendWorkerCleanupDiagnosticKind::TuneStartRollback,
            target,
            replacement_context,
        ));
    }
    let plan = FrontendBackendTunePlan::new(
        frontend_id,
        generation,
        entry.backend,
        FrontendDevicePath::new(entry.device_path.clone()),
        request.clone(),
    );
    let target_for_worker = target;
    let replacement_context_for_worker = replacement_context;
    let frontend_rollback_token_for_worker = Arc::clone(&frontend_rollback_token);
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
                            &frontend_rollback_token_for_worker,
                            &demux_rollback_tokens_for_worker,
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
                    &frontend_rollback_token_for_worker,
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
                            &frontend_rollback_token_for_worker,
                            &demux_rollback_tokens_for_worker,
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
        drop(frontend_rollback_token_for_worker);
        drop(demux_rollback_tokens_for_worker);
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
            &frontend_rollback_token,
            &demux_rollback_tokens,
            primary,
            "frontend tune worker start rollback",
            FrontendWorkerCleanupDiagnosticKind::TuneWorkerStartRollback,
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
    frontend_rollback_token: SharedFrontendRuntimeRollbackToken,
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
                            &frontend_rollback_token,
                            &demux_rollback_tokens,
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
                        &frontend_rollback_token,
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
                            &frontend_rollback_token,
                            &demux_rollback_tokens,
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
    let target = FrontendWorkerCleanupTarget::object(
        frontend_id,
        object_id,
        object_generation,
    );
    let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
    let demux_preparation = match guard.prepare_bound_demux_runtime_rollback_tokens(frontend_id) {
        Ok(preparation) => preparation,
        Err(failure) => {
            return Err(record_bound_demux_rollback_preparation_failure(
                cleanup_diagnostic_sink,
                FrontendWorkerCleanupDiagnosticKind::ScanStartRollback,
                target,
                failure,
                "scan demux rollback preparation diagnostic record failed",
            ));
        }
    };
    let (frontend_rollback_token, previous_tune_request) = match guard
        .frontend_txn()
        .prepare_frontend_runtime_rollback_capture(frontend_id)
    {
        Ok(capture) => capture.into_replacement_parts(),
        Err(error) => {
            let failure = guard.discard_bound_demux_rollback_preparation(
                demux_preparation,
                error,
            );
            return Err(record_bound_demux_rollback_preparation_failure(
                cleanup_diagnostic_sink,
                FrontendWorkerCleanupDiagnosticKind::ScanStartRollback,
                target,
                failure,
                "scan rollback authority discard diagnostic record failed",
            ));
        }
    };
    let generation = match guard
        .frontend_txn()
        .prepare_frontend_worker_replacement_generation(frontend_id, FrontendWorkerKind::Scan)
    {
        Ok(generation) => generation,
        Err(error) => {
            let demux_failure = guard.discard_bound_demux_rollback_preparation(
                demux_preparation,
                error,
            );
            let frontend_discard_result = frontend_rollback_token.discard_without_runtime();
            return Err(record_frontend_and_demux_rollback_authority_discard_failure(
                cleanup_diagnostic_sink,
                FrontendWorkerCleanupDiagnosticKind::ScanStartRollback,
                target,
                demux_failure,
                frontend_discard_result,
                "scan rollback authority discard diagnostic record failed",
            ));
        }
    };
    let (demux_rollback_tokens, _preparation_report) = demux_preparation.into_parts();
    let bound_demux_generations = bound_demux_generation_snapshot(&demux_rollback_tokens);
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
        frontend_rollback_token,
        previous_tune_request,
        demux_rollback_tokens,
        bound_demux_generations,
        stop_ticket,
    };
    drop(guard);
    let (
        mut guard,
        frontend_id,
        generation,
        stop_outcome,
        frontend_rollback_token,
        previous_tune_for_worker,
        demux_rollback_tokens,
    ) = complete_frontend_worker_replacement_ticket(
        &runtime,
        replacement_ticket,
        cleanup_diagnostic_sink.clone(),
        FrontendWorkerCleanupDiagnosticKind::ScanReplacementStop,
        "service runtime lock poisoned after scan worker join",
    )?;
    let frontend_rollback_token = share_frontend_rollback_token(frontend_rollback_token);
    let demux_rollback_tokens = share_demux_rollback_tokens(demux_rollback_tokens);
    let scan_cancel_result = record_scan_cancelled_from_stop_outcome_locked(
        &mut guard,
        frontend_id,
        &stop_outcome,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    );
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
        Some(scan_cancel_result.clone()),
    );
    if let Err(error) = scan_cancel_result {
        return Err(restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
            &mut guard,
            frontend_id,
            &frontend_rollback_token,
            &demux_rollback_tokens,
            error,
            "frontend scan replacement cancel rollback",
            FrontendWorkerCleanupDiagnosticKind::ScanStartRollback,
            target,
            replacement_context,
        ));
    }
    if let Err(error) = with_demux_rollback_tokens(&demux_rollback_tokens, |tokens| {
        guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id, tokens)
    }) {
        return Err(
            restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
                &mut guard,
                frontend_id,
                &frontend_rollback_token,
                &demux_rollback_tokens,
                error,
                "frontend scan start reset rollback",
                FrontendWorkerCleanupDiagnosticKind::ScanStartRollback,
                target,
                replacement_context,
            ),
        );
    }
    if let Err(error) = begin_shared_frontend_scan_rollback_expected_post_state(
        &mut guard,
        frontend_id,
        &frontend_rollback_token,
        generation,
        fingerprint.clone(),
        candidates.clone(),
    ) {
        return Err(restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
            &mut guard,
            frontend_id,
            &frontend_rollback_token,
            &demux_rollback_tokens,
            error,
            "frontend scan session/expected-post commit rollback",
            FrontendWorkerCleanupDiagnosticKind::ScanStartRollback,
            target,
            replacement_context,
        ));
    }
    let target_for_worker = target;
    let replacement_context_for_worker = replacement_context;
    let frontend_rollback_token_for_worker = Arc::clone(&frontend_rollback_token);
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
                frontend_rollback_token_for_worker,
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
                &frontend_rollback_token,
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
        .stop_frontend_live_data_and_unbind(
            frontend_id,
            frontend_live_data_expectation(&outcome, FrontendWorkerKind::Tune),
        )
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
        FrontendWorkerCleanupPublicOutcome::from_optional_error(public_error),
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
            .clear_frontend_live_reader_descriptor_and_idle(
                frontend_id,
                frontend_live_data_expectation(&outcome, FrontendWorkerKind::Scan),
            );
        report.push(
            FrontendWorkerCleanupStepOutcome::clear_live_reader_descriptor(target, clear_result),
        );
    }
    let public_error = report.first_error();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::StopScanObject,
        target,
        report,
        FrontendWorkerCleanupPublicOutcome::from_optional_error(public_error),
    );
    finish_frontend_worker_cleanup_report(Ok(cleanup_diagnostic_sink), record)
}

pub fn close_frontend_live_data_and_unbind(
    runtime: SharedRuntime,
    frontend_id: i32,
    expected_worker: Option<(u64, FrontendWorkerKind)>,
) -> Result<(), HalError> {
    lock_runtime(&runtime, "service runtime lock poisoned")?
        .frontend_txn()
        .close_frontend_live_data_and_unbind(frontend_id, expected_worker)
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
        FrontendWorkerCleanupPublicOutcome::from_optional_error(public_error),
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
pub(crate) fn close_frontend_workers_and_live_data(
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
    let tune_expectation = tune_outcome
        .as_ref()
        .ok()
        .and_then(|outcome| frontend_live_data_expectation(outcome, FrontendWorkerKind::Tune));
    let scan_expectation = scan_outcome
        .as_ref()
        .ok()
        .and_then(|outcome| frontend_live_data_expectation(outcome, FrontendWorkerKind::Scan));
    let close_result = match (tune_expectation, scan_expectation) {
        (Some(_), Some(_)) => Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "tune and scan workers both reported live-data ownership during close",
        )),
        (Some(expected), None) | (None, Some(expected)) => close_frontend_live_data_and_unbind(
            Arc::clone(&runtime),
            frontend_id,
            Some(expected),
        ),
        (None, None) => close_frontend_live_data_and_unbind(
            Arc::clone(&runtime),
            frontend_id,
            None,
        ),
    };
    report.push(FrontendWorkerCleanupStepOutcome::close_live_data_and_unbind(target, close_result));
    let public_error = report.first_error();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::FrontendClose,
        target,
        report,
        FrontendWorkerCleanupPublicOutcome::from_optional_error(public_error),
    );
    finish_frontend_worker_cleanup_report(cleanup_diagnostic_sink, record)
}
