use crate::boot::{FrontendTuneScanContext, TunerServiceRuntime};
use crate::frontend_worker_txn::{
    FrontendScanNotification, FrontendScanNotifier, FrontendTuneNotification,
    FrontendTuneNotifier,
};
use crate::object_method_use_case::ObjectMethodExecutionToken;
use crate::worker_runtime::WorkerTerminalResult;
use maleicacid_tuner_hal2_common::{
    FrontendScanMode, FrontendTuneRequest, HalError,
};
use maleicacid_tuner_hal2_device::{
    FrontendWorkerCancelReason, FrontendWorkerKind, FrontendWorkerStopOutcome,
};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

pub type SharedFrontendRuntime = std::sync::Arc<std::sync::Mutex<TunerServiceRuntime>>;

pub enum FrontendOperationEvent {
    Tune {
        notifier: FrontendTuneNotifier,
        notification: FrontendTuneNotification,
    },
    Scan {
        notifier: FrontendScanNotifier,
        notification: FrontendScanNotification,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendOperationEventAcceptance {
    Accepted,
    DiscardedStale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendWorkerTerminalEvent {
    frontend_id: i32,
    owner_generation: u64,
    worker_kind: FrontendWorkerKind,
    terminal_result: WorkerTerminalResult<()>,
}

impl FrontendWorkerTerminalEvent {
    pub const fn new(
        frontend_id: i32,
        owner_generation: u64,
        worker_kind: FrontendWorkerKind,
        terminal_result: WorkerTerminalResult<()>,
    ) -> Self {
        Self {
            frontend_id,
            owner_generation,
            worker_kind,
            terminal_result,
        }
    }

    pub const fn frontend_id(&self) -> i32 {
        self.frontend_id
    }

    pub const fn owner_generation(&self) -> u64 {
        self.owner_generation
    }

    pub const fn worker_kind(&self) -> FrontendWorkerKind {
        self.worker_kind
    }

    pub fn into_terminal_result(self) -> WorkerTerminalResult<()> {
        self.terminal_result
    }

    pub fn from_stop_outcome(outcome: &FrontendWorkerStopOutcome) -> Option<Self> {
        match outcome {
            FrontendWorkerStopOutcome::NotRunning => None,
            FrontendWorkerStopOutcome::CancelRequested {
                frontend_id,
                kind,
                generation,
                ..
            } => Some(Self::new(
                *frontend_id,
                *generation,
                *kind,
                WorkerTerminalResult::StopRequested,
            )),
            FrontendWorkerStopOutcome::StopRequestFailed {
                frontend_id,
                kind,
                generation,
                error,
                ..
            } => Some(Self::new(
                *frontend_id,
                *generation,
                *kind,
                WorkerTerminalResult::RuntimeFailure(error.clone()),
            )),
            FrontendWorkerStopOutcome::Completed {
                frontend_id,
                kind,
                generation,
                result,
                ..
            } => Some(Self::new(
                *frontend_id,
                *generation,
                *kind,
                match result {
                    Ok(()) => WorkerTerminalResult::Normal(()),
                    Err(error) => WorkerTerminalResult::RuntimeFailure(error.clone()),
                },
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerTerminalEventAcceptance {
    Accepted,
    DiscardedStale,
}

/// The sole call-local owner for frontend tune/scan orchestration. The six
/// methods below are the complete canonical entry-role set.
pub struct FrontendTuneScanTxn;

impl FrontendTuneScanTxn {
    pub fn begin_tune(
        runtime: SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        request: FrontendTuneRequest,
        kind: FrontendWorkerKind,
        notifier: FrontendTuneNotifier,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        FrontendTuneScanContext::new(&runtime).begin_tune(
            object_id,
            object_generation,
            request,
            kind,
            notifier,
            dispatch,
        )
    }

    pub fn begin_scan(
        runtime: SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        request: FrontendTuneRequest,
        scan_mode: FrontendScanMode,
        notifier: FrontendScanNotifier,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        FrontendTuneScanContext::new(&runtime).begin_scan(
            object_id,
            object_generation,
            request,
            scan_mode,
            notifier,
            dispatch,
        )
    }

    pub fn stop_tune(
        runtime: SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        reason: FrontendWorkerCancelReason,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        FrontendTuneScanContext::new(&runtime).stop_tune(
            object_id,
            object_generation,
            reason,
            dispatch,
        )
    }

    pub fn stop_scan(
        runtime: SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        reason: FrontendWorkerCancelReason,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        FrontendTuneScanContext::new(&runtime).stop_scan(
            object_id,
            object_generation,
            reason,
            dispatch,
        )
    }

    pub fn accept_operation_event(
        runtime: &SharedFrontendRuntime,
        frontend_id: i32,
        operation_generation: u64,
        event: FrontendOperationEvent,
    ) -> Result<FrontendOperationEventAcceptance, HalError> {
        FrontendTuneScanContext::new(runtime).accept_operation_event(
            frontend_id,
            operation_generation,
            event,
        )
    }

    pub fn accept_worker_terminal(
        runtime: &SharedFrontendRuntime,
        event: FrontendWorkerTerminalEvent,
    ) -> Result<FrontendWorkerTerminalEventAcceptance, HalError> {
        FrontendTuneScanContext::new(runtime).accept_worker_terminal(event)
    }
}

pub(crate) struct FrontendLnbRelationTxn {
    frontend_id: i32,
    lnb_id: i32,
}

impl FrontendLnbRelationTxn {
    pub(crate) const fn new(frontend_id: i32, lnb_id: i32) -> Self {
        Self {
            frontend_id,
            lnb_id,
        }
    }

    pub(crate) fn prepare(
        self,
        runtime: &mut TunerServiceRuntime,
    ) -> Result<crate::boot::lnb_txn::PreparedFrontendLnbAssignment, HalError> {
        runtime
            .lnb_txn()
            .prepare_frontend_lnb_assignment(self.frontend_id, self.lnb_id)
    }

    pub(crate) fn finish(
        runtime: &mut TunerServiceRuntime,
        executed: crate::boot::lnb_txn::ExecutedFrontendLnbAssignment,
    ) -> Result<(), HalError> {
        runtime
            .lnb_txn()
            .commit_frontend_lnb_assignment(executed)
    }

    pub(crate) fn release(
        runtime: &mut TunerServiceRuntime,
        frontend_id: i32,
    ) -> Result<(), HalError> {
        runtime
            .registry_mut()
            .release_lnb_assignment(crate::registry::FrontendRuntimeId(frontend_id))
            .map(|_| ())
    }
}

pub fn set_frontend_lnb_object_use_case(
    runtime: SharedFrontendRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    lnb_id: i32,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let (frontend_id, authority) = {
        let guard = runtime.lock().map_err(|_| {
            HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while resolving frontend LNB I/O authority",
            )
        })?;
        let frontend_entry = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
        let frontend_id = frontend_entry.id.0;
        let exported_lnb_id = guard
            .lnb_for_frontend_id(frontend_id)
            .ok_or(HalError::Unsupported("frontend has no exported LNB"))?
            .id
            .0;
        if exported_lnb_id != lnb_id {
            return Err(HalError::invalid_argument(
                maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
                "LNB does not belong to this frontend",
            ));
        }
        let authority = guard
            .registry()
            .lnb_physical_io_authority(crate::registry::LnbRuntimeId(lnb_id))
            .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
        (frontend_id, authority)
    };
    authority.execute(|permit| {
        let prepared = {
            let mut guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while preparing frontend LNB assignment",
                )
            })?;
            dispatch.consume_for_object(
                &mut guard,
                object_id,
                object_generation,
                AidlObjectKind::Frontend,
            )?;
            let current = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
            if current.id.0 != frontend_id {
                return Err(HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "frontend object changed runtime id before LNB assignment preparation",
                ));
            }
            FrontendLnbRelationTxn::new(frontend_id, lnb_id).prepare(&mut guard)?
        };
        let executed = prepared.execute(&permit);
        let mut guard = runtime.lock().map_err(|_| {
            HalError::internal(
                maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while finishing frontend LNB assignment",
            )
        })?;
        FrontendLnbRelationTxn::finish(&mut guard, executed)
    })
}

impl TunerServiceRuntime {
    pub fn commit_frontend_callback_registration_for_object(
        &mut self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        self.public_runtime_id_for_object_method(
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        Ok(())
    }

    pub fn clear_frontend_callback_registration_for_object(
        &mut self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        self.public_runtime_id_for_object_method(
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        Ok(())
    }
}
