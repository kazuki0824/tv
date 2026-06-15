use maleicacid_tuner_hal2_common::{
    HalError, HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_domain_request::{
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest,
};
use maleicacid_tuner_hal2_lnb::{
    LnbApplyTxn, LnbBackendOps, LnbDiseqcMessage, LnbElectricalState, LnbFailureKind,
    LnbFailureRecord, LnbLifecycleReason, LnbLifecycleTxn, LnbRuntime, LnbRuntimeState,
    LnbTone as RuntimeLnbTone, LnbVoltage as RuntimeLnbVoltage,
};

use super::TunerServiceRuntime;
use crate::lnb_backend_adapter::ServiceRuntimeLnbProfileAdapter;
use crate::registry::{FrontendRuntimeId, LnbRegistryProfile, LnbRuntimeId, RegistryCommitError};

pub(crate) struct LnbTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn lnb_txn(&mut self) -> LnbTxn<'_> {
        LnbTxn { runtime: self }
    }
}

impl<'a> LnbTxn<'a> {
    pub(crate) fn set_frontend_lnb(
        &mut self,
        frontend_id: i32,
        lnb_id: i32,
    ) -> Result<(), HalError> {
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
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(runtime)?;
        self.runtime
            .registry_mut()
            .bind_lnb_to_frontend(frontend_key, lnb_key)
            .map_err(map_registry_error)
    }

    pub(crate) fn apply_lnb_voltage(
        &mut self,
        lnb_id: i32,
        request: LnbVoltageRequest,
    ) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        let entry = self
            .runtime
            .registry()
            .lnb(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        let runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(&runtime)?;
        let voltage = validate_voltage_for_profile(entry.profile, request)?;
        let mut target = runtime.registry_state();
        target.voltage = voltage;
        self.apply_lnb_state_with_generation(lnb_key, runtime, target)
    }

    pub(crate) fn apply_lnb_tone(
        &mut self,
        lnb_id: i32,
        request: LnbToneRequest,
    ) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(&runtime)?;
        let tone = validate_tone_for_profile(request)?;
        let mut target = runtime.registry_state();
        target.tone = tone;
        self.apply_lnb_state_with_generation(lnb_key, runtime, target)
    }

    pub(crate) fn apply_lnb_satellite_position(
        &mut self,
        lnb_id: i32,
        request: LnbSetSatellitePositionRequest,
    ) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(&runtime)?;
        let satellite_position = validate_position_for_profile(request)?;
        let mut target = runtime.registry_state();
        target.satellite_position = satellite_position;
        self.apply_lnb_state_with_generation(lnb_key, runtime, target)
    }

    pub(crate) fn send_lnb_diseqc(
        &mut self,
        lnb_id: i32,
        payload: &[u8],
    ) -> Result<(), HalError> {
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

    pub(crate) fn close_lnb_explicit(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.close_lnb_with_reason(LnbRuntimeId(lnb_id), LnbLifecycleReason::PublicClose)
    }

    pub(crate) fn close_lnb_from_frontend_owner_loss(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<i32>, HalError> {
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
        let mut closed = Vec::with_capacity(owned_lnb_ids.len());
        for lnb_key in owned_lnb_ids {
            self.close_lnb_with_reason(lnb_key, LnbLifecycleReason::OwnerLoss)?;
            closed.push(lnb_key.0);
        }
        Ok(closed)
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
        let outcome = {
            let mut backend = ServiceRuntimeLnbProfileAdapter::new(self.runtime.registry(), lnb_key);
            LnbLifecycleTxn::new().close(&mut runtime, &mut backend, LnbLifecycleReason::DropLeak)
        };
        self.store_lnb_runtime(lnb_key, runtime)?;
        match outcome.result {
            Ok(()) => Ok(()),
            Err(record) if record.kind == LnbFailureKind::DropWithoutClose => Ok(()),
            Err(record) => Err(map_lnb_failure(record)),
        }
    }

    fn apply_lnb_state_with_generation(
        &mut self,
        lnb_key: LnbRuntimeId,
        mut runtime: LnbRuntime,
        target: LnbElectricalState,
    ) -> Result<(), HalError> {
        let next_generation = match runtime.checked_next_generation() {
            Ok(next) => next,
            Err(_) => {
                let record = runtime.quarantine_generation_overflow();
                self.store_lnb_runtime(lnb_key, runtime)?;
                return Err(map_lnb_failure(record));
            }
        };
        let outcome = {
            let mut backend = ServiceRuntimeLnbProfileAdapter::new(self.runtime.registry(), lnb_key);
            LnbApplyTxn::new().apply_with_generation(
                &mut runtime,
                &mut backend,
                target,
                next_generation,
            )
        };
        self.store_lnb_runtime(lnb_key, runtime)?;
        outcome.result.map(|_| ()).map_err(map_lnb_failure)
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
            let mut backend = ServiceRuntimeLnbProfileAdapter::new(self.runtime.registry(), lnb_key);
            LnbLifecycleTxn::new().close(&mut runtime, &mut backend, reason)
        };
        self.store_lnb_runtime(lnb_key, runtime)?;
        outcome.result.map_err(map_lnb_failure)
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

fn missing_lnb_error() -> HalError {
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

fn ensure_lnb_open(runtime: &LnbRuntime) -> Result<(), HalError> {
    if runtime.state() == LnbRuntimeState::Open {
        Ok(())
    } else {
        Err(lnb_state_error())
    }
}

fn map_registry_error(error: RegistryCommitError) -> HalError {
    match error {
        RegistryCommitError::MissingFrontendId { .. }
        | RegistryCommitError::MissingLnbId { .. }
        | RegistryCommitError::LnbFrontendMismatch { .. } => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "frontend/LNB binding is invalid",
        ),
        _ => HalError::internal(
            HalInternalKind::InvariantViolation,
            "LNB registry commit failed",
        ),
    }
}

fn map_lnb_failure(record: LnbFailureRecord) -> HalError {
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
        | LnbFailureKind::OperationAlreadyActive
        | LnbFailureKind::OperationLockFailed
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

fn validate_voltage_for_profile(
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

fn validate_tone_for_profile(request: LnbToneRequest) -> Result<RuntimeLnbTone, HalError> {
    match request {
        LnbToneRequest::None => Ok(RuntimeLnbTone::Off),
        LnbToneRequest::Continuous => Err(HalError::Unsupported(
            "LNB continuous tone is unavailable for this fixed profile",
        )),
    }
}

fn validate_position_for_profile(
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
