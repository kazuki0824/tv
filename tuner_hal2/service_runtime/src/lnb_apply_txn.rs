use maleicacid_tuner_hal2_common::{
    HalError, HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_domain_request::{
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest,
};
use maleicacid_tuner_hal2_lnb::{
    LnbApplyTxn, LnbElectricalState, LnbFailureKind, LnbFailureRecord, LnbRuntime,
    LnbRuntimeState, LnbTone as RuntimeLnbTone, LnbVoltage as RuntimeLnbVoltage,
};

use crate::boot::TunerServiceRuntime;
use crate::lnb_backend_adapter::{store_lnb_runtime, ServiceRuntimeLnbBackend};
use crate::registry::{FrontendRuntimeId, LnbRegistryProfile, LnbRuntimeId, RegistryCommitError};

fn missing_lnb_error() -> HalError {
    HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "LNB runtime id is missing")
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
        LnbFailureKind::BackendApplyFailed
        | LnbFailureKind::RegistryCommitFailed
        | LnbFailureKind::OperationAlreadyActive
        | LnbFailureKind::OperationLockFailed
        | LnbFailureKind::CallbackClearFailed
        | LnbFailureKind::DropWithoutClose => HalError::internal(
            HalInternalKind::InvariantViolation,
            "LNB apply transaction failed",
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

impl TunerServiceRuntime {
    pub fn set_frontend_lnb(&mut self, frontend_id: i32, lnb_id: i32) -> Result<(), HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.registry().frontend(frontend_key).is_none() {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "frontend id is missing for LNB binding",
            ));
        }
        let Some(entry) = self.registry().lnb(lnb_key) else {
            return Err(missing_lnb_error());
        };
        if entry.owner_frontend_id != frontend_key {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "LNB does not belong to this frontend",
            ));
        }
        if self.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let runtime = self
            .registry()
            .lnb_runtime(lnb_key)
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(runtime)?;
        self.registry_mut()
            .bind_lnb_to_frontend(frontend_key, lnb_key)
            .map_err(map_registry_error)
    }

    pub fn apply_lnb_voltage(
        &mut self,
        lnb_id: i32,
        request: LnbVoltageRequest,
    ) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        let entry = self
            .registry()
            .lnb(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        let runtime = self
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

    pub fn apply_lnb_tone(
        &mut self,
        lnb_id: i32,
        request: LnbToneRequest,
    ) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let runtime = self
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

    pub fn apply_lnb_satellite_position(
        &mut self,
        lnb_id: i32,
        request: LnbSetSatellitePositionRequest,
    ) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let runtime = self
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

    pub fn reject_lnb_diseqc(&mut self, lnb_id: i32, _payload: &[u8]) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let runtime = self
            .registry()
            .lnb_runtime(lnb_key)
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(runtime)?;
        Err(HalError::Unsupported(
            "DiSEqC is permanently unavailable for current LNB profiles",
        ))
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
                store_lnb_runtime(self, lnb_key, runtime)?;
                return Err(map_lnb_failure(record));
            }
        };
        let outcome = {
            let mut backend = ServiceRuntimeLnbBackend::new(self.registry(), lnb_key);
            LnbApplyTxn::new().apply_with_generation(
                &mut runtime,
                &mut backend,
                target,
                next_generation,
            )
        };
        store_lnb_runtime(self, lnb_key, runtime)?;
        outcome.result.map(|_| ()).map_err(map_lnb_failure)
    }

}
