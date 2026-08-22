use std::sync::Arc;

use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FrontendScanMode, FrontendTuneRequest, HalError,
    HalInternalKind,
};
use maleicacid_tuner_hal2_device::{
    FrontendRuntimeState, FrontendWorkerCancelReason, FrontendWorkerKind,
};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId};

use crate::frontend_ops::{
    FrontendOperationEvent, FrontendOperationEventAcceptance, FrontendWorkerTerminalEvent,
    FrontendWorkerTerminalEventAcceptance, SharedFrontendRuntime,
};
use crate::frontend_worker_termination_use_case::FrontendWorkerTerminationUseCase;
use crate::frontend_worker_txn::{
    start_frontend_backend_scan_session_worker, start_frontend_backend_tune_worker,
    stop_frontend_scan_object, stop_frontend_tune_object, FrontendScanNotifier,
    FrontendTuneNotifier,
};
use crate::object_method_use_case::ObjectMethodExecutionToken;
use crate::registry::FrontendRuntimeId;

/// Private per-call context for the six canonical `FrontendTuneScanTxn`
/// entry roles. It never survives an entry call and owns no generation.
pub(crate) struct FrontendTuneScanContext<'a> {
    runtime: &'a SharedFrontendRuntime,
}

impl<'a> FrontendTuneScanContext<'a> {
    pub(crate) const fn new(runtime: &'a SharedFrontendRuntime) -> Self {
        Self { runtime }
    }

    pub(crate) fn begin_tune(
        self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        request: FrontendTuneRequest,
        kind: FrontendWorkerKind,
        notifier: FrontendTuneNotifier,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        self.preflight_begin(object_id, object_generation, &request, None)?;
        let fixed_power = crate::lnb_ops::ensure_frontend_fixed_power_for_object(
            self.runtime,
            object_id,
            object_generation,
        )?;
        let result = start_frontend_backend_tune_worker(
            Arc::clone(self.runtime),
            object_id,
            object_generation,
            request,
            kind,
            notifier,
            dispatch,
        );
        self.rollback_new_fixed_power_after_begin_failure(fixed_power, result)
    }

    pub(crate) fn begin_scan(
        self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        request: FrontendTuneRequest,
        scan_mode: FrontendScanMode,
        notifier: FrontendScanNotifier,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        self.preflight_begin(
            object_id,
            object_generation,
            &request,
            Some(scan_mode),
        )?;
        let fixed_power = crate::lnb_ops::ensure_frontend_fixed_power_for_object(
            self.runtime,
            object_id,
            object_generation,
        )?;
        let result = start_frontend_backend_scan_session_worker(
            Arc::clone(self.runtime),
            object_id,
            object_generation,
            request,
            scan_mode,
            notifier,
            dispatch,
        );
        self.rollback_new_fixed_power_after_begin_failure(fixed_power, result)
    }

    pub(crate) fn stop_tune(
        self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        reason: FrontendWorkerCancelReason,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        stop_frontend_tune_object(
            Arc::clone(self.runtime),
            object_id,
            object_generation,
            reason,
            dispatch,
        )
    }

    pub(crate) fn stop_scan(
        self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        reason: FrontendWorkerCancelReason,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        stop_frontend_scan_object(
            Arc::clone(self.runtime),
            object_id,
            object_generation,
            reason,
            dispatch,
        )
    }

    pub(crate) fn accept_operation_event(
        self,
        frontend_id: i32,
        operation_generation: u64,
        event: FrontendOperationEvent,
    ) -> Result<FrontendOperationEventAcceptance, HalError> {
        let is_current = self
            .runtime
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

        match event {
            FrontendOperationEvent::Tune {
                notifier,
                notification,
            } => {
                let _ = notifier(frontend_id, operation_generation, notification);
            }
            FrontendOperationEvent::Scan {
                notifier,
                notification,
            } => {
                let _ = notifier(frontend_id, operation_generation, notification);
            }
        }
        Ok(FrontendOperationEventAcceptance::Accepted)
    }

    pub(crate) fn accept_worker_terminal(
        self,
        event: FrontendWorkerTerminalEvent,
    ) -> Result<FrontendWorkerTerminalEventAcceptance, HalError> {
        let frontend_id = FrontendRuntimeId(event.frontend_id());
        let acceptance = {
            let mut runtime = self.runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while accepting a frontend worker terminal",
                )
            })?;
            FrontendWorkerTerminationUseCase::accept_worker_terminal(&mut runtime, event)?
        };
        // A close or replacement may fence the worker generation before its
        // terminal report is collected.  A stale report must not mutate the
        // operation owner, but it may still prove that the physical worker is
        // gone.  The current frontend state remains the authority for whether
        // fixed power can be released.
        self.release_fixed_power_if_operation_terminal(frontend_id)?;
        Ok(acceptance)
    }

    fn preflight_begin(
        &self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        request: &FrontendTuneRequest,
        scan_mode: Option<FrontendScanMode>,
    ) -> Result<(), HalError> {
        let guard = self.runtime.lock().map_err(|_| {
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
        &self,
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
            self.runtime,
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
        &self,
        frontend_id: FrontendRuntimeId,
    ) -> Result<(), HalError> {
        let terminal = self
            .runtime
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
            crate::lnb_ops::release_frontend_fixed_power_after_operation(
                self.runtime,
                frontend_id,
            )?;
        }
        Ok(())
    }
}
