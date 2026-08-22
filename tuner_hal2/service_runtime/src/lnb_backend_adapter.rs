use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendDevicePath};
use maleicacid_tuner_hal2_device::{
    apply_frontend_backend_lnb_voltage_classified, FrontendBackendLnbApplyOutcome,
    FrontendBackendLnbApplyPlan, FrontendLnbVoltage,
};
use maleicacid_tuner_hal2_lnb::{
    LnbBackendApplyOutcome, LnbBackendOps, LnbDiseqcMessage, LnbElectricalState, LnbFailureKind,
    LnbTone, LnbVoltage,
};

use crate::registry::{
    FrontendRuntimeId, LnbPhysicalIoPermit, LnbRegistryProfile, LnbRuntimeId, RuntimeRegistry,
};

#[derive(Debug)]
struct LnbFrontendIoSnapshot {
    frontend_id: FrontendRuntimeId,
    backend: FrontendBackendKind,
    device_path: std::path::PathBuf,
}

#[derive(Debug)]
pub(crate) struct ServiceRuntimeLnbBackendSnapshot {
    target_lnb_id: LnbRuntimeId,
    profile: LnbRegistryProfile,
    frontends: Vec<LnbFrontendIoSnapshot>,
}

impl ServiceRuntimeLnbBackendSnapshot {
    pub(crate) fn new(
        registry: &RuntimeRegistry,
        target_lnb_id: LnbRuntimeId,
    ) -> Result<Self, LnbFailureKind> {
        Self::new_with_optional_pending_frontend(registry, target_lnb_id, None)
    }

    pub(crate) fn new_with_pending_frontend(
        registry: &RuntimeRegistry,
        target_lnb_id: LnbRuntimeId,
        pending_frontend_id: FrontendRuntimeId,
    ) -> Result<Self, LnbFailureKind> {
        Self::new_with_optional_pending_frontend(
            registry,
            target_lnb_id,
            Some(pending_frontend_id),
        )
    }

    fn new_with_optional_pending_frontend(
        registry: &RuntimeRegistry,
        target_lnb_id: LnbRuntimeId,
        pending_frontend_id: Option<FrontendRuntimeId>,
    ) -> Result<Self, LnbFailureKind> {
        let entry = registry
            .lnb(target_lnb_id)
            .ok_or(LnbFailureKind::BackendApplyFailed)?;
        let mut frontend_ids = registry.selected_frontends_for_lnb(target_lnb_id);
        if !frontend_ids.contains(&entry.owner_frontend_id) {
            frontend_ids.push(entry.owner_frontend_id);
        }
        if let Some(frontend_id) = pending_frontend_id {
            if !frontend_ids.contains(&frontend_id) {
                frontend_ids.push(frontend_id);
            }
        }
        let mut frontends = Vec::with_capacity(frontend_ids.len());
        for frontend_id in frontend_ids {
            match registry.selected_lnb_for_frontend(frontend_id) {
                Some(frontend_lnb) if frontend_lnb != target_lnb_id => {
                    return Err(LnbFailureKind::BackendApplyFailed);
                }
                Some(_) => {}
                None
                    if pending_frontend_id != Some(frontend_id)
                        && frontend_id != entry.owner_frontend_id =>
                {
                    return Err(LnbFailureKind::BackendApplyFailed);
                }
                None => {}
            }
            let frontend_entry = registry
                .frontend(frontend_id)
                .ok_or(LnbFailureKind::BackendApplyFailed)?;
            if entry.owner_frontend_id != frontend_id
                || frontend_entry.lnb_profile != Some(entry.profile)
            {
                return Err(LnbFailureKind::BackendApplyFailed);
            }
            frontends.push(LnbFrontendIoSnapshot {
                frontend_id,
                backend: frontend_entry.backend,
                device_path: frontend_entry.device_path.clone(),
            });
        }
        Ok(Self {
            target_lnb_id,
            profile: entry.profile,
            frontends,
        })
    }
}

pub(crate) struct ServiceRuntimeLnbProfileAdapter<'permit, 'gate> {
    snapshot: ServiceRuntimeLnbBackendSnapshot,
    _permit: &'permit LnbPhysicalIoPermit<'gate>,
}

impl<'permit, 'gate> ServiceRuntimeLnbProfileAdapter<'permit, 'gate> {
    pub(crate) fn new(
        snapshot: ServiceRuntimeLnbBackendSnapshot,
        permit: &'permit LnbPhysicalIoPermit<'gate>,
    ) -> Self {
        Self {
            snapshot,
            _permit: permit,
        }
    }
}

impl LnbBackendOps for ServiceRuntimeLnbProfileAdapter<'_, '_> {
    fn apply_lnb_state(
        &mut self,
        lnb_id: i32,
        state: LnbElectricalState,
    ) -> LnbBackendApplyOutcome {
        if lnb_id != self.snapshot.target_lnb_id.0 {
            return LnbBackendApplyOutcome::Rejected(LnbFailureKind::BackendApplyFailed);
        }
        let profile = self.snapshot.profile;
        if !profile_accepts_state(profile, state) {
            return LnbBackendApplyOutcome::Rejected(LnbFailureKind::BackendApplyFailed);
        }
        for frontend in &self.snapshot.frontends {
            if profile == LnbRegistryProfile::NoPower {
                continue;
            }
            let plan = FrontendBackendLnbApplyPlan::new(
                frontend.frontend_id.0,
                frontend.backend,
                FrontendDevicePath::new(frontend.device_path.clone()),
                device_voltage(state.voltage),
            );
            match apply_frontend_backend_lnb_voltage_classified(&plan) {
                FrontendBackendLnbApplyOutcome::Applied => {}
                FrontendBackendLnbApplyOutcome::Rejected(_) => {
                    return LnbBackendApplyOutcome::Rejected(
                        LnbFailureKind::BackendApplyFailed,
                    );
                }
                FrontendBackendLnbApplyOutcome::Indeterminate(_) => {
                    return LnbBackendApplyOutcome::Indeterminate(
                        LnbFailureKind::BackendApplyFailed,
                    );
                }
            }
        }
        LnbBackendApplyOutcome::Applied
    }

    fn send_diseqc_message(
        &mut self,
        lnb_id: i32,
        _message: &LnbDiseqcMessage,
    ) -> LnbBackendApplyOutcome {
        if lnb_id != self.snapshot.target_lnb_id.0 {
            return LnbBackendApplyOutcome::Rejected(LnbFailureKind::BackendApplyFailed);
        }
        LnbBackendApplyOutcome::Rejected(LnbFailureKind::DiseqcUnsupported)
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
