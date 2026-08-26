use super::{
    build_lnb_satellite_position_request, build_lnb_tone_request, build_lnb_voltage_request,
    close_object_after_close_preflight, execute_shared_object_runtime_use_case,
    execute_shared_object_runtime_use_case_with_request_builder, status_from_hal_error,
    AidlMethodCall, BinderResult, ILnb, ILnbCallback, LnbAidlObject, LnbPosition, LnbTone,
    LnbVoltage, Strong,
};

impl ILnb for LnbAidlObject {
    fn setCallback(&self, callback: Option<&Strong<dyn ILnbCallback>>) -> BinderResult<()> {
        self.set_callback_nullable_for_aidl(callback)
    }

    fn setVoltage(&self, voltage: LnbVoltage) -> BinderResult<()> {
        execute_shared_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let request = build_lnb_voltage_request(voltage).map_err(status_from_hal_error)?;
                Ok((AidlMethodCall::LnbSetVoltage(request.clone()), request))
            },
            |runtime, handle, dispatch_proof, request| {
                super::apply_lnb_voltage_object_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_proof,
                )
            },
        )
    }

    fn setTone(&self, tone: LnbTone) -> BinderResult<()> {
        execute_shared_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let request = build_lnb_tone_request(tone).map_err(status_from_hal_error)?;
                Ok((AidlMethodCall::LnbSetTone(request.clone()), request))
            },
            |runtime, handle, dispatch_proof, request| {
                super::apply_lnb_tone_object_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_proof,
                )
            },
        )
    }

    fn setSatellitePosition(&self, position: LnbPosition) -> BinderResult<()> {
        execute_shared_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let request = build_lnb_satellite_position_request(position)
                    .map_err(status_from_hal_error)?;
                Ok((
                    AidlMethodCall::LnbSetSatellitePosition(request.clone()),
                    request,
                ))
            },
            |runtime, handle, dispatch_proof, request| {
                super::apply_lnb_satellite_position_object_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_proof,
                )
            },
        )
    }

    fn sendDiseqcMessage(&self, diseqc_message: &[u8]) -> BinderResult<()> {
        execute_shared_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::LnbSendDiseqc(diseqc_message.to_vec()),
            |runtime, handle, dispatch_proof| {
                super::send_lnb_diseqc_object_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    diseqc_message.to_vec(),
                    dispatch_proof,
                )
            },
        )
    }

    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(&self.context(), self.handle(), AidlMethodCall::LnbClose)
    }
}
