use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_domain_request::{
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest,
};
use maleicacid_tuner_hal2_lnb::{
    LnbBackendApplyOutcome, LnbBackendOps, PreparedLnbStateApply,
};

use crate::boot::lnb_txn::{
    map_lnb_failure, missing_lnb_error, validate_position_for_profile,
    validate_tone_for_profile, validate_voltage_for_profile,
};
use crate::boot::TunerServiceRuntime;
use crate::lnb_backend_adapter::{
    ServiceRuntimeLnbBackendSnapshot, ServiceRuntimeLnbProfileAdapter,
};
use crate::registry::{LnbPhysicalIoPermit, LnbRuntimeId};

pub(crate) struct PreparedLnbControlTxn {
    lnb_key: LnbRuntimeId,
    runtime_apply: PreparedLnbStateApply,
    backend: ServiceRuntimeLnbBackendSnapshot,
}

pub(crate) struct CompletedLnbControlTxn {
    lnb_key: LnbRuntimeId,
    runtime_apply: PreparedLnbStateApply,
    backend_result: LnbBackendApplyOutcome,
}

impl PreparedLnbControlTxn {
    pub(crate) fn execute(
        self,
        permit: &LnbPhysicalIoPermit<'_>,
    ) -> CompletedLnbControlTxn {
        let mut backend = ServiceRuntimeLnbProfileAdapter::new(self.backend, permit);
        let backend_result = backend.apply_lnb_state(
            self.runtime_apply.lnb_id(),
            self.runtime_apply.target_state(),
        );
        CompletedLnbControlTxn {
            lnb_key: self.lnb_key,
            runtime_apply: self.runtime_apply,
            backend_result,
        }
    }
}

impl CompletedLnbControlTxn {
    pub(crate) const fn backend_result(&self) -> LnbBackendApplyOutcome {
        self.backend_result
    }
}

pub(crate) struct LnbControlTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn lnb_control_txn(&mut self) -> LnbControlTxn<'_> {
        LnbControlTxn { runtime: self }
    }
}

impl LnbControlTxn<'_> {
    pub(crate) fn prepare_voltage(
        &mut self,
        lnb_id: i32,
        request: LnbVoltageRequest,
    ) -> Result<PreparedLnbControlTxn, HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        let entry = self
            .runtime
            .registry()
            .lnb(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        let mut candidate = self.current_state(lnb_key)?;
        candidate.voltage = validate_voltage_for_profile(entry.profile, request)?;
        self.prepare_candidate(lnb_key, candidate)
    }

    pub(crate) fn prepare_internal_fixed_15v(
        &mut self,
        lnb_id: i32,
    ) -> Result<PreparedLnbControlTxn, HalError> {
        self.prepare_voltage(lnb_id, LnbVoltageRequest::Voltage15V)
    }

    pub(crate) fn prepare_tone(
        &mut self,
        lnb_id: i32,
        request: LnbToneRequest,
    ) -> Result<PreparedLnbControlTxn, HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        let mut candidate = self.current_state(lnb_key)?;
        candidate.tone = validate_tone_for_profile(request)?;
        self.prepare_candidate(lnb_key, candidate)
    }

    pub(crate) fn prepare_satellite_position(
        &mut self,
        lnb_id: i32,
        request: LnbSetSatellitePositionRequest,
    ) -> Result<PreparedLnbControlTxn, HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        let mut candidate = self.current_state(lnb_key)?;
        candidate.satellite_position = validate_position_for_profile(request)?;
        self.prepare_candidate(lnb_key, candidate)
    }

    pub(crate) fn finish(
        &mut self,
        completed: CompletedLnbControlTxn,
    ) -> Result<(), HalError> {
        self.runtime
            .registry_mut()
            .finish_lnb_state_apply(
                completed.lnb_key,
                completed.runtime_apply,
                completed.backend_result,
            )
        .map(|_| ())
        .map_err(map_lnb_failure)
    }

    fn current_state(
        &self,
        lnb_key: LnbRuntimeId,
    ) -> Result<maleicacid_tuner_hal2_lnb::LnbElectricalState, HalError> {
        self.runtime
            .registry()
            .lnb_runtime(lnb_key)
            .map(|runtime| runtime.registry_state())
            .ok_or_else(missing_lnb_error)
    }

    fn prepare_candidate(
        &mut self,
        lnb_key: LnbRuntimeId,
        candidate: maleicacid_tuner_hal2_lnb::LnbElectricalState,
    ) -> Result<PreparedLnbControlTxn, HalError> {
        let backend = ServiceRuntimeLnbBackendSnapshot::new(self.runtime.registry(), lnb_key)
            .map_err(|kind| {
                map_lnb_failure(maleicacid_tuner_hal2_lnb::LnbFailureRecord {
                    lnb_id: lnb_key.0,
                    kind,
                    step: maleicacid_tuner_hal2_lnb::LnbFailureStep::ApplyBackend,
                })
            })?;
        let runtime_apply = self
            .runtime
            .registry_mut()
            .prepare_lnb_state_apply(lnb_key, candidate)
            .map_err(map_lnb_failure)?;
        Ok(PreparedLnbControlTxn {
            lnb_key,
            runtime_apply,
            backend,
        })
    }
}
