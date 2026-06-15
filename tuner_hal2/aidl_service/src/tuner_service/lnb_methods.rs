use super::{
    AidlMethodCall, AidlObjectKind, BinderResult, ILnb, ILnbCallback, LnbAidlObject, LnbPosition, LnbTone,
    LnbVoltage, Strong, build_lnb_satellite_position_request, build_lnb_tone_request,
    build_lnb_voltage_request, status_from_hal_error, status_unknown_error
};
use super::support::{
    runtime_entry_public_id
};

impl ILnb for LnbAidlObject {
    fn setCallback(&self, callback: &Strong<dyn ILnbCallback>) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::LnbSetCallback)?;
        self.retain_callback(callback)?;
        let runtime = self.runtime();
        let lnb_id = match runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb) {
            Ok(id) => id,
            Err(status) => {
                self.rollback_callback_registration()?;
                return Err(status);
            }
        };
        let result = match runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .commit_lnb_callback_registration(lnb_id)
        {
            Ok(()) => Ok(()),
            Err(error) => {
                self.rollback_callback_registration()?;
                Err(status_from_hal_error(error))
            }
        };
        result
    }
    fn setVoltage(&self, voltage: LnbVoltage) -> BinderResult<()> {
        self.ensure_open()?;
        let request = build_lnb_voltage_request(voltage).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::LnbSetVoltage(request))?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .apply_lnb_voltage(lnb_id, request)
            .map_err(status_from_hal_error);
        result
    }
    fn setTone(&self, tone: LnbTone) -> BinderResult<()> {
        self.ensure_open()?;
        let request = build_lnb_tone_request(tone).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::LnbSetTone(request))?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .apply_lnb_tone(lnb_id, request)
            .map_err(status_from_hal_error);
        result
    }
    fn setSatellitePosition(&self, position: LnbPosition) -> BinderResult<()> {
        self.ensure_open()?;
        let request =
            build_lnb_satellite_position_request(position).map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::LnbSetSatellitePosition(request))?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .apply_lnb_satellite_position(lnb_id, request)
            .map_err(status_from_hal_error);
        result
    }
    fn sendDiseqcMessage(&self, diseqc_message: &[u8]) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::LnbSendDiseqc(diseqc_message.to_vec()))?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .send_lnb_diseqc(lnb_id, diseqc_message)
            .map_err(status_from_hal_error);
        result
    }
    fn close(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::LnbClose)?;
        let runtime = self.runtime();
        let lnb_id = runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Lnb)?;
        runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .close_lnb_explicit(lnb_id)
            .map_err(status_from_hal_error)?;
        self.close_object()
    }
}
