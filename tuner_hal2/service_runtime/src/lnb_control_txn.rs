use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalInternalKind,
};
use maleicacid_tuner_hal2_domain_request::{
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest,
};
use maleicacid_tuner_hal2_lnb::{
    apply_lnb_state_with_txn, LnbElectricalState, LnbRuntime,
};

use crate::boot::lnb_txn::{
    ensure_lnb_open, map_lnb_failure, missing_lnb_error, validate_position_for_profile,
    validate_tone_for_profile, validate_voltage_for_profile,
};
use crate::boot::TunerServiceRuntime;
use crate::lnb_backend_adapter::ServiceRuntimeLnbProfileAdapter;
use crate::registry::LnbRuntimeId;

pub(crate) struct LnbControlTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn lnb_control_txn(&mut self) -> LnbControlTxn<'_> {
        LnbControlTxn { runtime: self }
    }
}

impl LnbControlTxn<'_> {
    pub(crate) fn apply_voltage(
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
        let runtime = self.load_open_runtime(lnb_key)?;
        let mut candidate = runtime.registry_state();
        candidate.voltage = validate_voltage_for_profile(entry.profile, request)?;
        self.apply_candidate(lnb_key, runtime, candidate)
    }

    pub(crate) fn apply_tone(
        &mut self,
        lnb_id: i32,
        request: LnbToneRequest,
    ) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        let runtime = self.load_open_runtime(lnb_key)?;
        let mut candidate = runtime.registry_state();
        candidate.tone = validate_tone_for_profile(request)?;
        self.apply_candidate(lnb_key, runtime, candidate)
    }

    pub(crate) fn apply_satellite_position(
        &mut self,
        lnb_id: i32,
        request: LnbSetSatellitePositionRequest,
    ) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        let runtime = self.load_open_runtime(lnb_key)?;
        let mut candidate = runtime.registry_state();
        candidate.satellite_position = validate_position_for_profile(request)?;
        self.apply_candidate(lnb_key, runtime, candidate)
    }

    fn load_open_runtime(&self, lnb_key: LnbRuntimeId) -> Result<LnbRuntime, HalError> {
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
        Ok(runtime)
    }

    fn apply_candidate(
        &mut self,
        lnb_key: LnbRuntimeId,
        mut runtime: LnbRuntime,
        candidate: LnbElectricalState,
    ) -> Result<(), HalError> {
        let outcome = {
            let mut backend =
                ServiceRuntimeLnbProfileAdapter::new(self.runtime.registry(), lnb_key);
            apply_lnb_state_with_txn(&mut runtime, &mut backend, candidate)
        };
        let store_result = self.store_runtime(lnb_key, runtime);
        match (outcome.result.map(|_| ()), store_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(store_error)) => Err(store_error),
            (Err(record), Ok(())) => Err(map_lnb_failure(record)),
            (Err(record), Err(store_error)) => Err(compose_primary_cleanup_failure(
                "LNB control transaction failed and runtime store failed",
                map_lnb_failure(record),
                store_error,
            )),
        }
    }

    fn store_runtime(
        &mut self,
        lnb_key: LnbRuntimeId,
        runtime: LnbRuntime,
    ) -> Result<(), HalError> {
        let Some(slot) = self.runtime.registry_mut().lnb_runtime_mut(lnb_key) else {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "LNB runtime disappeared during control transaction",
            ));
        };
        *slot = runtime;
        Ok(())
    }
}
