use crate::method_dispatch::plan_object_method_dispatch;
use crate::object_lifecycle::aidl_public_runtime_id_for_close_cleanup;
use crate::object_method_txn::ObjectMethodDispatchPreflight;
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_domain_request::{
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest,
};

use crate::boot::TunerServiceRuntime;

impl TunerServiceRuntime {
    pub fn set_frontend_lnb(&mut self, frontend_id: i32, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().set_frontend_lnb(frontend_id, lnb_id)
    }

    pub fn apply_lnb_voltage(
        &mut self,
        lnb_id: i32,
        request: LnbVoltageRequest,
    ) -> Result<(), HalError> {
        self.lnb_txn().apply_lnb_voltage(lnb_id, request)
    }

    pub fn apply_lnb_tone(&mut self, lnb_id: i32, request: LnbToneRequest) -> Result<(), HalError> {
        self.lnb_txn().apply_lnb_tone(lnb_id, request)
    }

    pub fn apply_lnb_satellite_position(
        &mut self,
        lnb_id: i32,
        request: LnbSetSatellitePositionRequest,
    ) -> Result<(), HalError> {
        self.lnb_txn().apply_lnb_satellite_position(lnb_id, request)
    }

    pub fn send_lnb_diseqc(&mut self, lnb_id: i32, payload: &[u8]) -> Result<(), HalError> {
        self.lnb_txn().send_lnb_diseqc(lnb_id, payload)
    }

    pub fn open_lnb_for_public_id(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().open_lnb_for_public_id(lnb_id)
    }

    pub fn commit_lnb_callback_registration(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().commit_lnb_callback_registration(lnb_id)
    }

    pub fn close_lnb_explicit(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().close_lnb_explicit(lnb_id)
    }

    pub fn close_lnb_from_frontend_owner_loss(
        &mut self,
        frontend_id: i32,
    ) -> (Vec<i32>, Result<(), HalError>) {
        self.lnb_txn()
            .close_lnb_from_frontend_owner_loss(frontend_id)
    }

    pub fn record_lnb_drop_leak(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().record_lnb_drop_leak(lnb_id)
    }
}

#[cfg(test)]
mod wp_r11_lnb_apply_tests {
    use crate::boot::TunerServiceRuntime;
    use crate::registry::{
        FrontendRegistryEntry, FrontendRuntimeId, LnbRegistryEntry, LnbRegistryProfile,
        LnbRuntimeId,
    };
    use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem, HalError};
    use maleicacid_tuner_hal2_domain_request::LnbVoltageRequest;
    use maleicacid_tuner_hal2_lnb::LnbElectricalState;

    fn runtime_with_lnb(profile: LnbRegistryProfile) -> TunerServiceRuntime {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .registry_mut_for_test()
            .register_frontend(FrontendRegistryEntry {
                id: FrontendRuntimeId(1),
                backend: FrontendBackendKind::Px4CharDevice,
                system: FrontendSystem::IsdbS,
                device_path: "/dev/null".into(),
                lnb_profile: Some(profile),
            })
            .unwrap();
        runtime
            .registry_mut_for_test()
            .register_lnb(LnbRegistryEntry {
                id: LnbRuntimeId(10001),
                name: Some("test-lnb".to_string()),
                owner_frontend_id: FrontendRuntimeId(1),
                profile,
            })
            .unwrap();
        runtime
    }

    #[test]
    fn diseqc_empty_payload_is_invalid_argument() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        let err = runtime.send_lnb_diseqc(10001, &[]).unwrap_err();
        assert!(matches!(err, HalError::InvalidArgument { .. }));
    }

    #[test]
    fn diseqc_valid_payload_is_profile_unsupported_not_success() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        let err = runtime
            .send_lnb_diseqc(10001, &[0xe0, 0x10, 0x5a])
            .unwrap_err();
        assert_eq!(
            err,
            HalError::Unsupported("DiSEqC is unavailable for this LNB profile")
        );
    }

    #[test]
    fn px4_lnb_profile_rejects_11v_before_registry_commit() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        let err = runtime
            .apply_lnb_voltage(10001, LnbVoltageRequest::Voltage11V)
            .unwrap_err();
        assert_eq!(
            err,
            HalError::Unsupported("LNB voltage is unavailable for this fixed profile")
        );
        let lnb = runtime.registry().lnb_runtime(LnbRuntimeId(10001)).unwrap();
        assert_eq!(lnb.registry_state(), LnbElectricalState::safe());
    }
}

impl TunerServiceRuntime {
    pub fn apply_lnb_voltage_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: LnbVoltageRequest,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        dispatch.plan(self)?;
        self.apply_lnb_voltage(lnb_id, request)
    }

    pub fn apply_lnb_tone_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: LnbToneRequest,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        dispatch.plan(self)?;
        self.apply_lnb_tone(lnb_id, request)
    }

    pub fn apply_lnb_satellite_position_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: LnbSetSatellitePositionRequest,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        dispatch.plan(self)?;
        self.apply_lnb_satellite_position(lnb_id, request)
    }

    pub fn send_lnb_diseqc_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        payload: &[u8],
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.send_lnb_diseqc(lnb_id, payload)
    }

    pub fn commit_lnb_callback_registration_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        dispatch.plan(self)?;
        self.commit_lnb_callback_registration(lnb_id)
    }

    pub fn close_lnb_explicit_after_object_close_begin(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
    ) -> Result<(), HalError> {
        let lnb_id = aidl_public_runtime_id_for_close_cleanup(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        self.close_lnb_explicit(lnb_id)
    }
}
