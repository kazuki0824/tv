use crate::object_domain_cleanup::ObjectDomainCleanupCommand;
use crate::object_lifecycle::{
    aidl_public_runtime_id_for_close_cleanup, lnb_public_id_for_live_object_result,
};
use crate::object_method_txn::ObjectMethodExecutionToken;
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_domain_request::{
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest,
};

use crate::boot::TunerServiceRuntime;

impl TunerServiceRuntime {
    pub(crate) fn set_frontend_lnb(
        &mut self,
        frontend_id: i32,
        lnb_id: i32,
    ) -> Result<(), HalError> {
        crate::frontend_ops::FrontendLnbRelationTxn::new(frontend_id, lnb_id).execute(self)
    }

    pub(crate) fn apply_lnb_voltage(
        &mut self,
        lnb_id: i32,
        request: LnbVoltageRequest,
    ) -> Result<(), HalError> {
        self.lnb_control_txn().apply_voltage(lnb_id, request)
    }

    pub(crate) fn apply_lnb_tone(
        &mut self,
        lnb_id: i32,
        request: LnbToneRequest,
    ) -> Result<(), HalError> {
        self.lnb_control_txn().apply_tone(lnb_id, request)
    }

    pub(crate) fn apply_lnb_satellite_position(
        &mut self,
        lnb_id: i32,
        request: LnbSetSatellitePositionRequest,
    ) -> Result<(), HalError> {
        self.lnb_control_txn()
            .apply_satellite_position(lnb_id, request)
    }

    pub(crate) fn send_lnb_diseqc(&mut self, lnb_id: i32, payload: &[u8]) -> Result<(), HalError> {
        self.lnb_txn().send_lnb_diseqc(lnb_id, payload)
    }

    pub(crate) fn open_lnb_for_public_id(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().open_lnb_for_public_id(lnb_id)
    }

    pub(crate) fn commit_lnb_callback_registration(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().commit_lnb_callback_registration(lnb_id)
    }

    pub(crate) fn clear_lnb_callback_registration(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().clear_lnb_callback_registration(lnb_id)
    }

    pub(crate) fn close_lnb_explicit(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().close_lnb_explicit(lnb_id)
    }

    pub(crate) fn close_lnb_from_frontend_owner_loss_report(
        &mut self,
        frontend_id: i32,
    ) -> Vec<(i32, Result<(), HalError>)> {
        self.lnb_txn()
            .close_lnb_from_frontend_owner_loss_report(frontend_id)
    }

    pub(crate) fn record_lnb_drop_leak(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_txn().record_lnb_drop_leak(lnb_id)
    }

    pub fn record_lnb_drop_leak_after_domain_cleanup_command(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        let lnb_id =
            lnb_public_id_for_live_object_result(self, command.object_id(), command.generation())?;
        self.record_lnb_drop_leak(lnb_id)
    }
}

#[cfg(test)]
mod wp_r11_lnb_apply_tests {
    use crate::boot::TunerServiceRuntime;
    use crate::registry::{
        FrontendCapabilitySnapshot, FrontendRegistryEntry, FrontendRuntimeId,
        FrontendScalarCapability, LnbRegistryEntry, LnbRegistryProfile, LnbRuntimeId,
    };
    use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem, HalError};
    use maleicacid_tuner_hal2_domain_request::LnbVoltageRequest;
    use maleicacid_tuner_hal2_lnb::{LnbElectricalState, LnbRuntimeState};

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
                capability: FrontendCapabilitySnapshot {
                    scalar: FrontendScalarCapability {
                        min_frequency_hz: 1_049_480_000,
                        max_frequency_hz: 2_053_000_000,
                        min_symbol_rate: 28_860_000,
                        max_symbol_rate: 28_860_000,
                        acquire_range_hz: 0,
                    },
                    exclusive_group_id: 0x1000_0001,
                    isdbt_segment: None,
                },
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

    #[test]
    fn selected_lnb_backend_failure_keeps_registry_state() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        runtime
            .registry_mut_for_test()
            .bind_lnb_to_frontend(FrontendRuntimeId(1), LnbRuntimeId(10001))
            .unwrap();

        let err = runtime
            .apply_lnb_voltage(10001, LnbVoltageRequest::Voltage15V)
            .unwrap_err();

        assert!(matches!(err, HalError::Internal { .. }));
        let lnb = runtime.registry().lnb_runtime(LnbRuntimeId(10001)).unwrap();
        assert_eq!(lnb.registry_state(), LnbElectricalState::safe());
        assert_eq!(lnb.state(), LnbRuntimeState::Failed);
    }

    #[test]
    fn set_frontend_lnb_backend_failure_does_not_commit_binding() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);

        let err = runtime.set_frontend_lnb(1, 10001).unwrap_err();

        assert!(matches!(err, HalError::Internal { .. }));
        assert_eq!(
            runtime
                .registry()
                .selected_lnb_for_frontend(FrontendRuntimeId(1)),
            None
        );
        let lnb = runtime.registry().lnb_runtime(LnbRuntimeId(10001)).unwrap();
        assert_eq!(lnb.registry_state(), LnbElectricalState::safe());
        assert_eq!(lnb.state(), LnbRuntimeState::Failed);
    }

    #[test]
    fn frontend_lnb_relation_commits_and_releases_with_assignment_lease() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::NoPower);

        runtime.set_frontend_lnb(1, 10001).unwrap();
        runtime.set_frontend_lnb(1, 10001).unwrap();

        assert_eq!(
            runtime
                .registry()
                .selected_lnb_for_frontend(FrontendRuntimeId(1)),
            Some(LnbRuntimeId(10001))
        );
        crate::frontend_ops::FrontendLnbRelationTxn::release(&mut runtime, 1).unwrap();
        assert_eq!(
            runtime
                .registry()
                .selected_lnb_for_frontend(FrontendRuntimeId(1)),
            None
        );
    }
}

impl TunerServiceRuntime {
    pub fn apply_lnb_voltage_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: LnbVoltageRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;

        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        self.apply_lnb_voltage(lnb_id, request)
    }

    pub fn apply_lnb_tone_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: LnbToneRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;

        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        self.apply_lnb_tone(lnb_id, request)
    }

    pub fn apply_lnb_satellite_position_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: LnbSetSatellitePositionRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;

        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        self.apply_lnb_satellite_position(lnb_id, request)
    }

    pub fn send_lnb_diseqc_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        payload: &[u8],
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;

        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        self.send_lnb_diseqc(lnb_id, payload)
    }

    pub fn commit_lnb_callback_registration_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;

        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        self.commit_lnb_callback_registration(lnb_id)
    }

    pub fn clear_lnb_callback_registration_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;

        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        self.clear_lnb_callback_registration(lnb_id)
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
