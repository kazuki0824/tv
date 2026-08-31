use crate::boot::TunerServiceRuntime;
use crate::frontend_worker_termination_use_case::FrontendWorkerTerminationUseCase;
use crate::frontend_worker_txn::{
    start_frontend_backend_scan_session_worker, start_frontend_backend_tune_worker,
    stop_frontend_scan_object, stop_frontend_tune_object, FrontendScanNotification,
    FrontendScanNotifier, FrontendTuneNotification, FrontendTuneNotifier,
};
use crate::object_method_use_case::ObjectMethodExecutionToken;
use crate::registry::FrontendRuntimeId;
use crate::worker_runtime::WorkerTerminalResult;
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FrontendScanMode, FrontendTuneRequest, HalError,
    HalInternalKind,
};
use maleicacid_tuner_hal2_device::{
    FrontendRuntimeState, FrontendWorkerCancelReason, FrontendWorkerKind,
    FrontendWorkerStopOutcome,
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
    AcceptedCallbackFailure,
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
        Self::preflight_begin(&runtime, object_id, object_generation, &request, None)?;
        let fixed_power = crate::lnb_ops::ensure_frontend_fixed_power_for_object(
            &runtime,
            object_id,
            object_generation,
        )?;
        let result = start_frontend_backend_tune_worker(
            std::sync::Arc::clone(&runtime),
            object_id,
            object_generation,
            request,
            kind,
            notifier,
            dispatch,
        );
        Self::rollback_new_fixed_power_after_begin_failure(&runtime, fixed_power, result)
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
        Self::preflight_begin(
            &runtime,
            object_id,
            object_generation,
            &request,
            Some(scan_mode),
        )?;
        let fixed_power = crate::lnb_ops::ensure_frontend_fixed_power_for_object(
            &runtime,
            object_id,
            object_generation,
        )?;
        let result = start_frontend_backend_scan_session_worker(
            std::sync::Arc::clone(&runtime),
            object_id,
            object_generation,
            request,
            scan_mode,
            notifier,
            dispatch,
        );
        Self::rollback_new_fixed_power_after_begin_failure(&runtime, fixed_power, result)
    }

    pub fn stop_tune(
        runtime: SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        reason: FrontendWorkerCancelReason,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        {
            let mut guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while checking stopTune during scan",
                )
            })?;
            let frontend_id = guard
                .frontend_entry_for_aidl_object(object_id, object_generation)?
                .id
                .0;
            let state = guard.query().frontend_runtime_snapshot(frontend_id)?.state;
            if state == FrontendRuntimeState::Scanning {
                // AOSP T-AOSP-35: stopTune() is an idempotent success while a scan owns
                // the frontend. Consume the public method authority, but do not fence the
                // scan generation, stop a worker, clear live data, or advance any demux
                // stream boundary.
                dispatch.consume_for_object(
                    &mut guard,
                    object_id,
                    object_generation,
                    AidlObjectKind::Frontend,
                )?;
                return Ok(());
            }
        }
        stop_frontend_tune_object(
            std::sync::Arc::clone(&runtime),
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
        stop_frontend_scan_object(
            std::sync::Arc::clone(&runtime),
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
        let is_current = runtime
            .lock()
            .map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while validating a frontend operation event",
                )
            })?
            .query()
            .frontend_runtime_snapshot(frontend_id)?
            .generation
            == operation_generation;
        if !is_current {
            return Ok(FrontendOperationEventAcceptance::DiscardedStale);
        }
        let delivery = match event {
            FrontendOperationEvent::Tune {
                notifier,
                notification,
            } => notifier(frontend_id, operation_generation, notification),
            FrontendOperationEvent::Scan {
                notifier,
                notification,
            } => notifier(frontend_id, operation_generation, notification),
        };
        Ok(if delivery.is_ok() {
            FrontendOperationEventAcceptance::Accepted
        } else {
            // The AIDL notifier already commits the classified post-commit callback
            // failure through WorkerFailureClassifier -> PostCommitCallbackFailureTxn.
            // Preserve the committed tune/scan operation and expose that delivery
            // outcome explicitly instead of silently discarding it.
            FrontendOperationEventAcceptance::AcceptedCallbackFailure
        })
    }

    pub fn accept_worker_terminal(
        runtime: &SharedFrontendRuntime,
        event: FrontendWorkerTerminalEvent,
    ) -> Result<FrontendWorkerTerminalEventAcceptance, HalError> {
        let frontend_id = FrontendRuntimeId(event.frontend_id());
        let acceptance = {
            let mut guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while accepting a frontend worker terminal",
                )
            })?;
            FrontendWorkerTerminationUseCase::accept_worker_terminal(&mut guard, event)?
        };
        Self::release_fixed_power_if_operation_terminal(runtime, frontend_id)?;
        Ok(acceptance)
    }

    fn preflight_begin(
        runtime: &SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        request: &FrontendTuneRequest,
        scan_mode: Option<FrontendScanMode>,
    ) -> Result<(), HalError> {
        let guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned during frontend begin preflight",
            )
        })?;
        let entry = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
        let normalized = request.clone().normalized_for_non_blind_operation();
        let validated = guard.validate_frontend_request_for_id(entry.id.0, &normalized)?;
        if let Some(scan_mode) = scan_mode {
            guard.scan_candidates_for_frontend_entry(&validated, &normalized, scan_mode)?;
        }
        Ok(())
    }

    fn rollback_new_fixed_power_after_begin_failure(
        runtime: &SharedFrontendRuntime,
        preparation: crate::lnb_ops::FrontendFixedPowerPreparation,
        result: Result<(), HalError>,
    ) -> Result<(), HalError> {
        let Err(primary) = result else {
            return Ok(());
        };
        if !preparation.newly_retained() {
            return Err(primary);
        }
        match crate::lnb_ops::release_frontend_fixed_power_after_operation(
            runtime,
            preparation.frontend_id(),
        ) {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(compose_primary_cleanup_failure(
                "frontend begin failed and fixed LNB power rollback failed",
                primary,
                cleanup,
            )),
        }
    }

    fn release_fixed_power_if_operation_terminal(
        runtime: &SharedFrontendRuntime,
        frontend_id: FrontendRuntimeId,
    ) -> Result<(), HalError> {
        let terminal = runtime
            .lock()
            .map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while checking fixed-power release",
                )
            })?
            .query()
            .frontend_runtime_snapshot(frontend_id.0)
            .map(|snapshot| {
                matches!(
                    snapshot.state,
                    FrontendRuntimeState::Idle
                        | FrontendRuntimeState::Closing
                        | FrontendRuntimeState::Failed
                )
            })?;
        if terminal {
            crate::lnb_ops::release_frontend_fixed_power_after_operation(runtime, frontend_id)?;
        }
        Ok(())
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
            .lnb_mutation_context()
            .prepare_frontend_lnb_assignment(self.frontend_id, self.lnb_id)
    }

    pub(crate) fn finish(
        runtime: &mut TunerServiceRuntime,
        executed: crate::boot::lnb_txn::ExecutedFrontendLnbAssignment,
    ) -> Result<(), HalError> {
        runtime
            .lnb_mutation_context()
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
