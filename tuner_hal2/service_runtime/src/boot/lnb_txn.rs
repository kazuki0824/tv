use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalInternalKind, HalInvalidArgumentKind,
    HalInvalidStateKind,
};
use maleicacid_tuner_hal2_domain_request::{
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest,
};
use maleicacid_tuner_hal2_lnb::{
    apply_lnb_state_with_txn, close_lnb_lifecycle, record_lnb_drop_leak_lifecycle, LnbBackendOps,
    LnbDiseqcMessage, LnbElectricalState, LnbFailureKind, LnbFailureRecord, LnbLifecycleReason,
    LnbRuntime, LnbRuntimeState, LnbTone as RuntimeLnbTone, LnbVoltage as RuntimeLnbVoltage,
};

use super::TunerServiceRuntime;
use crate::error_mapping::registry_commit_error_to_hal;
use crate::lnb_backend_adapter::ServiceRuntimeLnbProfileAdapter;
use crate::registry::{
    FrontendRuntimeId, LnbRegistryProfile, LnbRuntimeId, PreparedLnbAssignmentLease,
    RegistryCommitError,
};

pub(crate) enum PreparedFrontendLnbAssignment {
    Unchanged,
    Apply {
        prepared_lease: PreparedLnbAssignmentLease,
        prepared_runtime: LnbRuntime,
    },
}

pub(crate) struct LnbTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn lnb_txn(&mut self) -> LnbTxn<'_> {
        LnbTxn { runtime: self }
    }
}

impl<'a> LnbTxn<'a> {
    pub(crate) fn prepare_frontend_lnb_assignment(
        &mut self,
        frontend_id: i32,
        lnb_id: i32,
    ) -> Result<PreparedFrontendLnbAssignment, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().frontend(frontend_key).is_none() {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "frontend id is missing for LNB binding",
            ));
        }
        let Some(entry) = self.runtime.registry().lnb(lnb_key) else {
            return Err(missing_lnb_error());
        };
        if entry.owner_frontend_id != frontend_key {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "LNB does not belong to this frontend",
            ));
        }
        let runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(&runtime)?;
        let Some(prepared_lease) = self
            .runtime
            .registry_mut()
            .prepare_lnb_assignment_lease(frontend_key, lnb_key)?
        else {
            return Ok(PreparedFrontendLnbAssignment::Unchanged);
        };
        let prepared_runtime = match self.prepare_lnb_state_for_pending_frontend(
            lnb_key,
            frontend_key,
            runtime,
        ) {
            Ok(runtime) => runtime,
            Err(error) => {
                if self
                    .runtime
                    .registry_mut()
                    .abort_prepared_lnb_assignment_lease(prepared_lease)
                {
                    return Err(error);
                }
                return Err(compose_primary_cleanup_failure(
                    "LNB assignment backend prepare failed and prepared lease abort failed",
                    error,
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "prepared LNB assignment lease disappeared before abort",
                    ),
                ));
            }
        };
        Ok(PreparedFrontendLnbAssignment::Apply {
            prepared_lease,
            prepared_runtime,
        })
    }

    pub(crate) fn commit_frontend_lnb_assignment(
        &mut self,
        prepared: PreparedFrontendLnbAssignment,
    ) -> Result<(), HalError> {
        let PreparedFrontendLnbAssignment::Apply {
            prepared_lease,
            prepared_runtime,
        } = prepared
        else {
            return Ok(());
        };
        let cleanup = match self
            .runtime
            .registry_mut()
            .commit_prepared_lnb_assignment(prepared_lease, prepared_runtime)
        {
            Ok(cleanup) => cleanup,
            Err(error) => {
                if self
                    .runtime
                    .registry_mut()
                    .abort_prepared_lnb_assignment_lease(prepared_lease)
                {
                    return Err(error);
                }
                return Err(compose_primary_cleanup_failure(
                    "LNB assignment composite commit failed and prepared lease abort failed",
                    error,
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "prepared LNB assignment lease disappeared after commit failure",
                    ),
                ));
            }
        };
        if let Some(cleanup) = cleanup {
            self.runtime
                .registry_mut()
                .complete_lnb_assignment_cleanup(cleanup)?;
        }
        Ok(())
    }

    pub(crate) fn send_lnb_diseqc(&mut self, lnb_id: i32, payload: &[u8]) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(runtime)?;
        let message = LnbDiseqcMessage::new(lnb_id, payload).map_err(map_lnb_failure)?;
        let mut backend = ServiceRuntimeLnbProfileAdapter::new(self.runtime.registry(), lnb_key);
        backend
            .send_diseqc_message(lnb_id, &message)
            .map_err(|kind| {
                map_lnb_failure(LnbFailureRecord {
                    lnb_id,
                    kind,
                    step: maleicacid_tuner_hal2_lnb::LnbFailureStep::SendDiseqc,
                })
            })
    }

    pub(crate) fn open_lnb_for_public_id(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let mut runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        runtime
            .reopen_after_public_open()
            .map_err(map_lnb_failure)?;
        self.store_lnb_runtime(lnb_key, runtime)
    }

    pub(crate) fn commit_lnb_callback_registration(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let mut runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(&runtime)?;
        runtime.set_callback_registered(true);
        self.store_lnb_runtime(lnb_key, runtime)
    }

    pub(crate) fn clear_lnb_callback_registration(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let mut runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(&runtime)?;
        runtime.set_callback_registered(false);
        self.store_lnb_runtime(lnb_key, runtime)
    }

    pub(crate) fn close_lnb_explicit(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        self.close_lnb_with_reason(lnb_key, LnbLifecycleReason::PublicClose)?;
        for frontend_id in self.runtime.registry().selected_frontends_for_lnb(lnb_key) {
            crate::frontend_ops::FrontendLnbRelationTxn::release(
                self.runtime,
                frontend_id.0,
            )?;
        }
        Ok(())
    }

    pub(crate) fn close_lnb_from_frontend_owner_loss_report(
        &mut self,
        frontend_id: i32,
    ) -> Vec<(i32, Result<(), HalError>)> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let owned_lnb_ids: Vec<LnbRuntimeId> = self
            .runtime
            .registry()
            .lnb_ids()
            .into_iter()
            .filter(|lnb_id| {
                self.runtime
                    .registry()
                    .lnb(*lnb_id)
                    .map(|entry| entry.owner_frontend_id == frontend_key)
                    .unwrap_or(false)
            })
            .collect();
        let outcomes = owned_lnb_ids
            .into_iter()
            .map(|lnb_key| {
                let result = self.close_lnb_with_reason(lnb_key, LnbLifecycleReason::OwnerLoss);
                (lnb_key.0, result)
            })
            .collect::<Vec<_>>();
        if outcomes.iter().all(|(_, result)| result.is_ok()) {
            if let Err(error) =
                crate::frontend_ops::FrontendLnbRelationTxn::release(self.runtime, frontend_id)
            {
                return vec![(frontend_id, Err(error))];
            }
        }
        outcomes
    }

    pub(crate) fn record_lnb_drop_leak(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let mut runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        if runtime.state() == LnbRuntimeState::Closed {
            return Ok(());
        }
        let outcome = record_lnb_drop_leak_lifecycle(&mut runtime);
        let store_result = self.store_lnb_runtime(lnb_key, runtime);
        match (outcome.result, store_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(store_error)) => Err(store_error),
            (Err(record), Ok(())) if record.kind == LnbFailureKind::DropWithoutClose => Ok(()),
            (Err(record), Err(store_error)) if record.kind == LnbFailureKind::DropWithoutClose => {
                Err(store_error)
            }
            (Err(record), Ok(())) => Err(map_lnb_failure(record)),
            (Err(record), Err(store_error)) => Err(compose_primary_cleanup_failure(
                "LNB drop-leak transaction failed and runtime store failed",
                map_lnb_failure(record),
                store_error,
            )),
        }
    }

    fn prepare_lnb_state_for_pending_frontend(
        &mut self,
        lnb_key: LnbRuntimeId,
        frontend_key: FrontendRuntimeId,
        mut runtime: LnbRuntime,
    ) -> Result<LnbRuntime, HalError> {
        let target = runtime.registry_state();
        let outcome = {
            let mut backend = ServiceRuntimeLnbProfileAdapter::new_with_pending_frontend(
                self.runtime.registry(),
                lnb_key,
                frontend_key,
            );
            apply_lnb_state_with_txn(&mut runtime, &mut backend, target)
        };
        match outcome.result {
            Ok(_) => Ok(runtime),
            Err(record) => match self.store_lnb_runtime(lnb_key, runtime) {
                Ok(()) => Err(map_lnb_failure(record)),
                Err(store_error) => Err(compose_primary_cleanup_failure(
                    "LNB setLnb backend apply transaction failed and runtime store failed",
                    map_lnb_failure(record),
                    store_error,
                )),
            },
        }
    }

    fn close_lnb_with_reason(
        &mut self,
        lnb_key: LnbRuntimeId,
        reason: LnbLifecycleReason,
    ) -> Result<(), HalError> {
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let mut runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        let outcome = {
            let mut backend =
                ServiceRuntimeLnbProfileAdapter::new(self.runtime.registry(), lnb_key);
            close_lnb_lifecycle(&mut runtime, &mut backend, reason)
        };
        let store_result = self.store_lnb_runtime(lnb_key, runtime);
        match (outcome.result, store_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(store_error)) => Err(store_error),
            (Err(record), Ok(())) => Err(map_lnb_failure(record)),
            (Err(record), Err(store_error)) => Err(compose_primary_cleanup_failure(
                "LNB close transaction failed and runtime store failed",
                map_lnb_failure(record),
                store_error,
            )),
        }
    }

    fn store_lnb_runtime(
        &mut self,
        lnb_key: LnbRuntimeId,
        lnb_runtime: LnbRuntime,
    ) -> Result<(), HalError> {
        let Some(slot) = self.runtime.registry_mut().lnb_runtime_mut(lnb_key) else {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "LNB runtime disappeared during lifecycle transaction",
            ));
        };
        *slot = lnb_runtime;
        Ok(())
    }
}

pub(crate) fn missing_lnb_error() -> HalError {
    HalError::invalid_argument(
        HalInvalidArgumentKind::NumericRange,
        "LNB runtime id is missing",
    )
}

fn lnb_state_error() -> HalError {
    HalError::invalid_state(
        HalInvalidStateKind::InvalidLifecycle,
        "LNB runtime is not open",
    )
}

pub(crate) fn ensure_lnb_open(runtime: &LnbRuntime) -> Result<(), HalError> {
    if runtime.state() == LnbRuntimeState::Open {
        Ok(())
    } else {
        Err(lnb_state_error())
    }
}

fn map_registry_error(error: RegistryCommitError) -> HalError {
    registry_commit_error_to_hal(error, "frontend/LNB binding is invalid")
}

pub(crate) fn map_lnb_failure(record: LnbFailureRecord) -> HalError {
    match record.kind {
        LnbFailureKind::InvalidState => lnb_state_error(),
        LnbFailureKind::GenerationOverflow => HalError::internal(
            HalInternalKind::InvariantViolation,
            "LNB generation overflow",
        ),
        LnbFailureKind::DiseqcInvalidMessage => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "DiSEqC message length is invalid",
        ),
        LnbFailureKind::DiseqcUnsupported => {
            HalError::Unsupported("DiSEqC is unavailable for this LNB profile")
        }
        LnbFailureKind::BackendApplyFailed
        | LnbFailureKind::RegistryCommitFailed
        | LnbFailureKind::DropWithoutClose => HalError::internal(
            HalInternalKind::InvariantViolation,
            "LNB transaction failed",
        ),
    }
}

fn runtime_voltage(request: LnbVoltageRequest) -> RuntimeLnbVoltage {
    match request {
        LnbVoltageRequest::None => RuntimeLnbVoltage::None,
        LnbVoltageRequest::Voltage11V => RuntimeLnbVoltage::Voltage11V,
        LnbVoltageRequest::Voltage15V => RuntimeLnbVoltage::Voltage15V,
    }
}

pub(crate) fn validate_voltage_for_profile(
    profile: LnbRegistryProfile,
    request: LnbVoltageRequest,
) -> Result<RuntimeLnbVoltage, HalError> {
    match (profile, request) {
        (_, LnbVoltageRequest::None)
        | (LnbRegistryProfile::EarthPt1FixedLnb, LnbVoltageRequest::Voltage11V)
        | (LnbRegistryProfile::EarthPt1FixedLnb, LnbVoltageRequest::Voltage15V)
        | (LnbRegistryProfile::Px4Device15VOnly, LnbVoltageRequest::Voltage15V) => {
            Ok(runtime_voltage(request))
        }
        (LnbRegistryProfile::Px4Device15VOnly, LnbVoltageRequest::Voltage11V)
        | (LnbRegistryProfile::NoPower, LnbVoltageRequest::Voltage11V)
        | (LnbRegistryProfile::NoPower, LnbVoltageRequest::Voltage15V) => Err(
            HalError::Unsupported("LNB voltage is unavailable for this fixed profile"),
        ),
    }
}

pub(crate) fn validate_tone_for_profile(
    request: LnbToneRequest,
) -> Result<RuntimeLnbTone, HalError> {
    match request {
        LnbToneRequest::None => Ok(RuntimeLnbTone::Off),
        LnbToneRequest::Continuous => Err(HalError::Unsupported(
            "LNB continuous tone is unavailable for this fixed profile",
        )),
    }
}

pub(crate) fn validate_position_for_profile(
    request: LnbSetSatellitePositionRequest,
) -> Result<Option<i32>, HalError> {
    if request.position == 0 {
        Ok(None)
    } else {
        Err(HalError::Unsupported(
            "LNB satellite position is unavailable for this fixed profile",
        ))
    }
}
