use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_lnb::{LnbBackendOps, LnbElectricalState, LnbFailureKind, LnbRuntime};

use crate::boot::TunerServiceRuntime;
use crate::registry::{LnbRuntimeId, RuntimeRegistry};

pub(crate) struct ServiceRuntimeLnbBackend<'a> {
    registry: &'a RuntimeRegistry,
    target_lnb_id: LnbRuntimeId,
}

impl<'a> ServiceRuntimeLnbBackend<'a> {
    pub(crate) fn new(registry: &'a RuntimeRegistry, target_lnb_id: LnbRuntimeId) -> Self {
        Self {
            registry,
            target_lnb_id,
        }
    }
}

impl LnbBackendOps for ServiceRuntimeLnbBackend<'_> {
    fn apply_lnb_state(
        &mut self,
        lnb_id: i32,
        _state: LnbElectricalState,
    ) -> Result<(), LnbFailureKind> {
        if lnb_id != self.target_lnb_id.0 {
            return Err(LnbFailureKind::BackendApplyFailed);
        }
        if self.registry.lnb(self.target_lnb_id).is_none() {
            return Err(LnbFailureKind::BackendApplyFailed);
        }
        for frontend_id in self.registry.selected_frontends_for_lnb(self.target_lnb_id) {
            let Some(frontend_lnb) = self.registry.selected_lnb_for_frontend(frontend_id) else {
                return Err(LnbFailureKind::BackendApplyFailed);
            };
            if frontend_lnb != self.target_lnb_id {
                return Err(LnbFailureKind::BackendApplyFailed);
            }
        }
        Ok(())
    }

    fn clear_lnb_callback(&mut self, _lnb_id: i32) -> Result<(), LnbFailureKind> {
        Ok(())
    }
}

pub(crate) fn store_lnb_runtime(
    runtime: &mut TunerServiceRuntime,
    lnb_key: LnbRuntimeId,
    lnb_runtime: LnbRuntime,
) -> Result<(), HalError> {
    let Some(slot) = runtime.registry_mut().lnb_runtime_mut(lnb_key) else {
        return Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "LNB runtime disappeared during lifecycle transaction",
        ));
    };
    *slot = lnb_runtime;
    Ok(())
}
