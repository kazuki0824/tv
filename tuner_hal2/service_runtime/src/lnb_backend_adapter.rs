use maleicacid_tuner_hal2_lnb::{
    LnbBackendOps, LnbDiseqcMessage, LnbElectricalState, LnbFailureKind, LnbTone, LnbVoltage,
};

use crate::registry::{LnbRegistryProfile, LnbRuntimeId, RuntimeRegistry};

pub(crate) struct ServiceRuntimeLnbProfileAdapter<'a> {
    registry: &'a RuntimeRegistry,
    target_lnb_id: LnbRuntimeId,
}

impl<'a> ServiceRuntimeLnbProfileAdapter<'a> {
    pub(crate) fn new(registry: &'a RuntimeRegistry, target_lnb_id: LnbRuntimeId) -> Self {
        Self {
            registry,
            target_lnb_id,
        }
    }
}

impl LnbBackendOps for ServiceRuntimeLnbProfileAdapter<'_> {
    fn apply_lnb_state(
        &mut self,
        lnb_id: i32,
        state: LnbElectricalState,
    ) -> Result<(), LnbFailureKind> {
        if lnb_id != self.target_lnb_id.0 {
            return Err(LnbFailureKind::BackendApplyFailed);
        }
        let Some(entry) = self.registry.lnb(self.target_lnb_id) else {
            return Err(LnbFailureKind::BackendApplyFailed);
        };
        if !profile_accepts_state(entry.profile, state) {
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

    fn send_diseqc_message(
        &mut self,
        lnb_id: i32,
        _message: &LnbDiseqcMessage,
    ) -> Result<(), LnbFailureKind> {
        if lnb_id != self.target_lnb_id.0 {
            return Err(LnbFailureKind::BackendApplyFailed);
        }
        if self.registry.lnb(self.target_lnb_id).is_none() {
            return Err(LnbFailureKind::BackendApplyFailed);
        }
        Err(LnbFailureKind::DiseqcUnsupported)
    }
}

fn profile_accepts_state(profile: LnbRegistryProfile, state: LnbElectricalState) -> bool {
    match profile {
        LnbRegistryProfile::Px4Device15VOnly => {
            matches!(state.voltage, LnbVoltage::None | LnbVoltage::Voltage15V)
                && state.tone == LnbTone::Off
                && state.satellite_position.is_none()
        }
        LnbRegistryProfile::EarthPt1FixedLnb => {
            state.tone == LnbTone::Off && state.satellite_position.is_none()
        }
        LnbRegistryProfile::NoPower => state == LnbElectricalState::safe(),
    }
}
