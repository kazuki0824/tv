use super::{
    build_lnb_satellite_position_request, build_lnb_tone_request, build_lnb_voltage_request,
    close_object_after_close_preflight_with_domain_cleanup, execute_object_runtime_use_case,
    execute_object_runtime_use_case_with_request_builder, status_from_hal_error,
    status_unknown_error, AidlMethodCall, BinderResult, ILnb, ILnbCallback, LnbAidlObject,
    LnbPosition, LnbTone, LnbVoltage, Strong,
};

impl ILnb for LnbAidlObject {
    fn setCallback(&self, callback: &Strong<dyn ILnbCallback>) -> BinderResult<()> {
        self.set_callback_transaction(callback)
    }

    fn setVoltage(&self, voltage: LnbVoltage) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let request = build_lnb_voltage_request(voltage).map_err(status_from_hal_error)?;
                Ok((AidlMethodCall::LnbSetVoltage(request.clone()), request))
            },
            |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
                runtime.apply_lnb_voltage_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_preflight,
                )
            },
        )
    }

    fn setTone(&self, tone: LnbTone) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let request = build_lnb_tone_request(tone).map_err(status_from_hal_error)?;
                Ok((AidlMethodCall::LnbSetTone(request.clone()), request))
            },
            |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
                runtime.apply_lnb_tone_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_preflight,
                )
            },
        )
    }

    fn setSatellitePosition(&self, position: LnbPosition) -> BinderResult<()> {
        execute_object_runtime_use_case_with_request_builder(
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
            |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
                runtime.apply_lnb_satellite_position_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_preflight,
                )
            },
        )
    }

    fn sendDiseqcMessage(&self, diseqc_message: &[u8]) -> BinderResult<()> {
        execute_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::LnbSendDiseqc(diseqc_message.to_vec()),
            |runtime, handle, command_plan, executable_request| {
                runtime.send_lnb_diseqc_for_object(
                    handle.object_id(),
                    handle.generation(),
                    diseqc_message,
                    command_plan,
                    executable_request,
                )
            },
        )
    }

    fn close(&self) -> BinderResult<()> {
        let runtime_for_close = self.runtime();
        let runtime_for_cleanup = runtime_for_close.clone();
        let handle = self.handle();
        close_object_after_close_preflight_with_domain_cleanup(
            &runtime_for_close,
            handle,
            AidlMethodCall::LnbClose,
            || {
                let mut runtime = runtime_for_cleanup
                    .lock()
                    .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
                runtime
                    .close_lnb_explicit_after_object_close_begin(
                        handle.object_id(),
                        handle.generation(),
                    )
                    .map_err(status_from_hal_error)
            },
        )
    }
}
