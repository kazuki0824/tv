use crate::boot::TunerServiceRuntime;
use crate::frontend_worker_termination_use_case::FrontendWorkerTerminationUseCase;
use crate::frontend_worker_txn::{
    start_frontend_backend_scan_session_worker, start_frontend_backend_tune_worker,
    stop_frontend_scan_object, stop_frontend_tune_object, FrontendScanNotification,
    FrontendScanNotifier, FrontendTuneNotification, FrontendTuneNotifier,
};
use crate::object_method_use_case::ObjectMethodExecutionToken;
use crate::registry::{FrontendRuntimeId, LnbRuntimeId, SatellitePowerTopology};
use crate::worker_runtime::WorkerTerminalResult;
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FrontendScanMode, FrontendTuneRequest, HalError,
    HalInternalKind, LnbVoltageRequest,
};
use maleicacid_tuner_hal2_device::{
    FrontendRuntimeState, FrontendWorkerCancelReason, FrontendWorkerKind, FrontendWorkerStopOutcome,
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
    StreamIdList {
        stream_ids: Vec<i32>,
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

/// frontend tune/scan orchestrationを所有する唯一のcall-local owner。
/// 以下の6 methodが正規entry-roleの完全な集合である。
pub struct FrontendTuneScanTxn;

#[derive(Debug, Eq, PartialEq)]
#[must_use = "frontend固定電源の準備値は完了またはrollbackで消費する必要があります"]
struct FrontendFixedPowerPreparation {
    frontend_id: FrontendRuntimeId,
    newly_retained: bool,
}

impl FrontendFixedPowerPreparation {
    const fn frontend_id(&self) -> FrontendRuntimeId {
        self.frontend_id
    }

    const fn newly_retained(&self) -> bool {
        self.newly_retained
    }
}

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
        let fixed_power =
            Self::ensure_frontend_fixed_power_for_object(&runtime, object_id, object_generation)?;
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
        let fixed_power =
            Self::ensure_frontend_fixed_power_for_object(&runtime, object_id, object_generation)?;
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
                // AOSP T-AOSP-35: scanがfrontendを所有中のstopTune()は冪等成功とする。
                // public method権限は消費するが、scan generationのfence、worker停止、
                // live data clear、demux stream boundary更新は行わない。
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
        if let FrontendOperationEvent::StreamIdList { stream_ids } = event {
            let mut guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while accepting TMCC stream IDs",
                )
            })?;
            if guard.query().frontend_runtime_snapshot(frontend_id)?.generation
                != operation_generation
            {
                return Ok(FrontendOperationEventAcceptance::DiscardedStale);
            }
            guard.frontend_txn().record_frontend_stream_id_list(
                frontend_id,
                operation_generation,
                stream_ids,
            )?;
            return Ok(FrontendOperationEventAcceptance::Accepted);
        }

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
            FrontendOperationEvent::StreamIdList { .. } => {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "TMCC stream-ID event escaped its canonical state-commit path",
                ));
            }
        };
        Ok(if delivery.is_ok() {
            FrontendOperationEventAcceptance::Accepted
        } else {
            // AIDL notifierは分類済みpost-commit callback failureを
            // WorkerFailureClassifier -> PostCommitCallbackFailureTxn経由で既にcommitしている。
            // commit済みtune/scan operationを維持し、delivery outcomeを黙って破棄せず明示する。
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

    fn restore_fixed_power_lease_after_failure(
        runtime: &mut TunerServiceRuntime,
        frontend_id: FrontendRuntimeId,
        lnb_id: LnbRuntimeId,
        primary: HalError,
    ) -> HalError {
        match runtime
            .registry_mut()
            .retain_frontend_fixed_power_lease(frontend_id, lnb_id)
        {
            Ok(_) => primary,
            Err(cleanup) => compose_primary_cleanup_failure(
                "fixed LNB power failure and rail lease restoration both failed",
                primary,
                cleanup,
            ),
        }
    }

    fn rollback_new_fixed_power_lease(
        runtime: &mut TunerServiceRuntime,
        frontend_id: FrontendRuntimeId,
        newly_retained: bool,
        primary: HalError,
    ) -> HalError {
        if !newly_retained {
            return primary;
        }
        match runtime
            .registry_mut()
            .release_frontend_fixed_power_lease(frontend_id)
        {
            Ok(Some(_)) => primary,
            Ok(None) => compose_primary_cleanup_failure(
                "fixed LNB power preparation failed after its rail lease disappeared",
                primary,
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "new fixed-power lease was missing during rollback",
                ),
            ),
            Err(cleanup) => compose_primary_cleanup_failure(
                "fixed LNB power preparation and rail lease rollback both failed",
                primary,
                cleanup,
            ),
        }
    }

    fn ensure_frontend_fixed_power_for_object(
        runtime: &SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
    ) -> Result<FrontendFixedPowerPreparation, HalError> {
        let (frontend_id, lnb_id, authority) = {
            let guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned during fixed-power preflight",
                )
            })?;
            let frontend = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
            let frontend_id = frontend.id;
            if frontend.satellite_power_topology != SatellitePowerTopology::InternalFixed15V {
                return Ok(FrontendFixedPowerPreparation {
                    frontend_id,
                    newly_retained: false,
                });
            }
            let lnb_id = guard
                .registry()
                .lnb_for_frontend(frontend_id)
                .map(|entry| entry.id)
                .ok_or(HalError::Unsupported(
                    "internal fixed-15V frontend has no registered LNB rail",
                ))?;
            let authority = guard
                .registry()
                .lnb_physical_io_authority(lnb_id)
                .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
            (frontend_id, lnb_id, authority)
        };

        authority.execute(|permit| {
            let (prepared, newly_retained) = {
                let mut guard = runtime.lock().map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned during fixed-power preparation",
                    )
                })?;
                let current = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
                if current.id != frontend_id
                    || current.satellite_power_topology != SatellitePowerTopology::InternalFixed15V
                {
                    return Err(HalError::invalid_state(
                        maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                        "frontend fixed-power topology changed before rail preparation",
                    ));
                }
                let newly_retained = guard
                    .registry_mut()
                    .retain_frontend_fixed_power_lease(frontend_id, lnb_id)?;
                let already_applied = guard.registry().lnb_runtime(lnb_id).is_some_and(|lnb| {
                    lnb.state() == maleicacid_tuner_hal2_lnb::LnbRuntimeState::Open
                        && lnb.registry_state().voltage
                            == maleicacid_tuner_hal2_lnb::LnbVoltage::Voltage15V
                });
                if already_applied {
                    return Ok(FrontendFixedPowerPreparation {
                        frontend_id,
                        newly_retained,
                    });
                }
                if guard.registry().lnb_runtime(lnb_id).is_some_and(|lnb| {
                    lnb.state() == maleicacid_tuner_hal2_lnb::LnbRuntimeState::Closed
                }) {
                    if let Err(error) = guard
                        .registry_mut()
                        .reopen_lnb(lnb_id)
                        .map_err(crate::boot::lnb_txn::map_lnb_failure)
                    {
                        return Err(Self::rollback_new_fixed_power_lease(
                            &mut guard,
                            frontend_id,
                            newly_retained,
                            error,
                        ));
                    }
                }
                let prepared = match guard.lnb_control_txn().prepare_internal_fixed_15v(lnb_id.0) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        return Err(Self::rollback_new_fixed_power_lease(
                            &mut guard,
                            frontend_id,
                            newly_retained,
                            error,
                        ));
                    }
                };
                (prepared, newly_retained)
            };

            let completed = prepared.execute(&permit);
            let backend_result = completed.backend_result();
            let finish_result = runtime
                .lock()
                .map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while finishing fixed power",
                    )
                })?
                .lnb_control_txn()
                .finish(completed);
            match finish_result {
                Ok(()) => Ok(FrontendFixedPowerPreparation {
                    frontend_id,
                    newly_retained,
                }),
                Err(error)
                    if matches!(
                        backend_result,
                        maleicacid_tuner_hal2_lnb::LnbBackendApplyOutcome::Rejected(_)
                    ) =>
                {
                    let mut guard = runtime.lock().map_err(|_| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "service runtime lock poisoned while rolling back fixed power",
                        )
                    })?;
                    Err(Self::rollback_new_fixed_power_lease(
                        &mut guard,
                        frontend_id,
                        newly_retained,
                        error,
                    ))
                }
                Err(error) => Err(error),
            }
        })
    }

    fn release_frontend_fixed_power_after_operation(
        runtime: &SharedFrontendRuntime,
        frontend_id: FrontendRuntimeId,
    ) -> Result<(), HalError> {
        let (lnb_id, authority) = {
            let guard = runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned during fixed-power release preflight",
                )
            })?;
            let Some(lnb_id) = guard.registry().frontend_fixed_power_lnb(frontend_id) else {
                return Ok(());
            };
            let authority = guard
                .registry()
                .lnb_physical_io_authority(lnb_id)
                .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
            (lnb_id, authority)
        };

        authority.execute(|permit| {
            let prepared = {
                let mut guard = runtime.lock().map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned during fixed-power release",
                    )
                })?;
                if guard.registry().frontend_fixed_power_lnb(frontend_id) != Some(lnb_id) {
                    return Ok(());
                }
                let operation_is_terminal = guard
                    .registry()
                    .frontend_runtime(frontend_id)
                    .map(|frontend| {
                        matches!(
                            frontend.snapshot().state,
                            FrontendRuntimeState::Idle
                                | FrontendRuntimeState::Closing
                                | FrontendRuntimeState::Failed
                        )
                    })
                    .unwrap_or(true);
                if !operation_is_terminal {
                    return Ok(());
                }
                let state_is_safe = guard.registry().lnb_runtime(lnb_id).is_some_and(|lnb| {
                    lnb.registry_state() == maleicacid_tuner_hal2_lnb::LnbElectricalState::safe()
                });
                let remaining = match guard
                    .registry_mut()
                    .release_frontend_fixed_power_lease(frontend_id)?
                {
                    Some((released_lnb_id, remaining)) if released_lnb_id == lnb_id => remaining,
                    Some(_) => {
                        return Err(HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "fixed-power release changed physical LNB identity",
                        ));
                    }
                    None => return Ok(()),
                };
                if remaining != 0 || state_is_safe {
                    return Ok(());
                }
                match guard
                    .lnb_control_txn()
                    .prepare_voltage(lnb_id.0, LnbVoltageRequest::None)
                {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        return Err(Self::restore_fixed_power_lease_after_failure(
                            &mut guard,
                            frontend_id,
                            lnb_id,
                            error,
                        ));
                    }
                }
            };

            let completed = prepared.execute(&permit);
            match runtime
                .lock()
                .map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned while finishing fixed-power release",
                    )
                })?
                .lnb_control_txn()
                .finish(completed)
            {
                Ok(()) => Ok(()),
                Err(error) => {
                    let mut guard = runtime.lock().map_err(|_| {
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "service runtime lock poisoned while restoring fixed-power lease",
                        )
                    })?;
                    Err(Self::restore_fixed_power_lease_after_failure(
                        &mut guard,
                        frontend_id,
                        lnb_id,
                        error,
                    ))
                }
            }
        })
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
        preparation: FrontendFixedPowerPreparation,
        result: Result<(), HalError>,
    ) -> Result<(), HalError> {
        let Err(primary) = result else {
            return Ok(());
        };
        if !preparation.newly_retained() {
            return Err(primary);
        }
        match Self::release_frontend_fixed_power_after_operation(runtime, preparation.frontend_id())
        {
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
            Self::release_frontend_fixed_power_after_operation(runtime, frontend_id)?;
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
