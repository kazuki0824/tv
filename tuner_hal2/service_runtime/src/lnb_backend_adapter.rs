use maleicacid_tuner_hal2_common::FrontendDevicePath;
use maleicacid_tuner_hal2_device::{
    apply_frontend_backend_lnb_voltage, FrontendBackendLnbApplyPlan, FrontendLnbVoltage,
};
use maleicacid_tuner_hal2_lnb::{
    LnbBackendOps, LnbDiseqcMessage, LnbElectricalState, LnbFailureKind, LnbTone, LnbVoltage,
};

use crate::registry::{FrontendRuntimeId, LnbRegistryProfile, LnbRuntimeId, RuntimeRegistry};

pub(crate) struct ServiceRuntimeLnbProfileAdapter<'a> {
    registry: &'a RuntimeRegistry,
    target_lnb_id: LnbRuntimeId,
    pending_frontend_id: Option<FrontendRuntimeId>,
}

impl<'a> ServiceRuntimeLnbProfileAdapter<'a> {
    pub(crate) fn new(registry: &'a RuntimeRegistry, target_lnb_id: LnbRuntimeId) -> Self {
        Self {
            registry,
            target_lnb_id,
            pending_frontend_id: None,
        }
    }

    pub(crate) fn new_with_pending_frontend(
        registry: &'a RuntimeRegistry,
        target_lnb_id: LnbRuntimeId,
        pending_frontend_id: FrontendRuntimeId,
    ) -> Self {
        Self {
            registry,
            target_lnb_id,
            pending_frontend_id: Some(pending_frontend_id),
        }
    }

    fn target_frontend_ids(&self) -> Vec<FrontendRuntimeId> {
        let mut frontend_ids = self.registry.selected_frontends_for_lnb(self.target_lnb_id);
        if let Some(frontend_id) = self.pending_frontend_id {
            if !frontend_ids.contains(&frontend_id) {
                frontend_ids.push(frontend_id);
            }
        }
        frontend_ids
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
        let profile = entry.profile;
        if !profile_accepts_state(profile, state) {
            return Err(LnbFailureKind::BackendApplyFailed);
        }
        for frontend_id in self.target_frontend_ids() {
            match self.registry.selected_lnb_for_frontend(frontend_id) {
                Some(frontend_lnb) if frontend_lnb != self.target_lnb_id => {
                    return Err(LnbFailureKind::BackendApplyFailed);
                }
                Some(_) => {}
                None if self.pending_frontend_id != Some(frontend_id) => {
                    return Err(LnbFailureKind::BackendApplyFailed);
                }
                None => {}
            }
            let Some(frontend_entry) = self.registry.frontend(frontend_id) else {
                return Err(LnbFailureKind::BackendApplyFailed);
            };
            if entry.owner_frontend_id != frontend_id {
                return Err(LnbFailureKind::BackendApplyFailed);
            }
            if frontend_entry.lnb_profile != Some(profile) {
                return Err(LnbFailureKind::BackendApplyFailed);
            }
            if profile == LnbRegistryProfile::NoPower {
                continue;
            }
            let plan = FrontendBackendLnbApplyPlan::new(
                frontend_id.0,
                frontend_entry.backend,
                FrontendDevicePath::new(frontend_entry.device_path.clone()),
                device_voltage(state.voltage),
            );
            apply_frontend_backend_lnb_voltage(&plan)
                .map_err(|_| LnbFailureKind::BackendApplyFailed)?;
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

fn device_voltage(voltage: LnbVoltage) -> FrontendLnbVoltage {
    match voltage {
        LnbVoltage::None => FrontendLnbVoltage::None,
        LnbVoltage::Voltage11V => FrontendLnbVoltage::Voltage11V,
        LnbVoltage::Voltage15V => FrontendLnbVoltage::Voltage15V,
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
