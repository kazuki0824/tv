use super::{
    AidlApi, AidlMethodCall, AidlObjectKind, BinderResult, FrontendScanType,
    FrontendSettings, FrontendStatus, FrontendStatusReadiness, FrontendStatusType,
    FrontendWorkerCancelReason, FrontendWorkerKind, IFrontend, IFrontendCallback, FrontendAidlObject,
    Strong, TunerResult, aidl_frontend_settings_to_request, aidl_scan_type_to_mode,
    close_frontend_workers_and_live_data_use_case, scan_end_notifier,
    start_frontend_scan_use_case, start_frontend_tune_use_case,
    status_from_hal_error, status_unknown_error, service_error, stop_frontend_live_data_use_case,
    stop_frontend_scan_use_case, stop_frontend_tune_use_case, runtime_frontend_entry_for_object, frontend_signal_state_for_object, frontend_status_for_types, frontend_runtime_state_for_object, frontend_readiness_for_types
};
use super::support::{
    public_api_call, runtime_entry_public_id, unavailable_after_object_public_api_plan,
    unsupported_public_api_call
};

impl IFrontend for FrontendAidlObject {
    fn setCallback(&self, callback: &Strong<dyn IFrontendCallback>) -> BinderResult<()> {
        self.plan_method(AidlMethodCall::FrontendSetCallback)?;
        self.retain_callback(callback)
    }
    fn tune(&self, settings: &FrontendSettings) -> BinderResult<()> {
        self.ensure_open()?;
        let request = aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        let entry = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .validate_frontend_request_for_id(frontend_id, &request)
            .map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FrontendTune(request.clone()))?;
        start_frontend_tune_use_case(
            runtime,
            frontend_id,
            entry,
            request,
            FrontendWorkerKind::Tune,
        )
        .map_err(status_from_hal_error)
    }
    fn stopTune(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FrontendStopTune)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        stop_frontend_tune_use_case(
            runtime.clone(),
            frontend_id,
            FrontendWorkerCancelReason::StopRequested,
        )
        .map_err(status_from_hal_error)?;
        stop_frontend_live_data_use_case(runtime, frontend_id).map_err(status_from_hal_error)
    }
    fn close(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FrontendClose)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        let closed_lnb_ids = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .close_lnb_from_frontend_owner_loss(frontend_id)
            .map_err(status_from_hal_error)?;
        for lnb_id in closed_lnb_ids {
            clear_live_lnb_callback_for_public_id(&runtime, lnb_id)?;
        }
        close_frontend_workers_and_live_data_use_case(
            runtime,
            frontend_id,
            FrontendWorkerCancelReason::FrontendClosing,
        )
        .map_err(status_from_hal_error)?;
        self.close_object()
    }
    fn scan(&self, settings: &FrontendSettings, scan_type: FrontendScanType) -> BinderResult<()> {
        self.ensure_open()?;
        let scan_mode = aidl_scan_type_to_mode(scan_type).map_err(status_from_hal_error)?;
        let request = aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        let entry = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .validate_frontend_request_for_id(frontend_id, &request)
            .map_err(status_from_hal_error)?;
        let candidates = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .scan_candidates_for_frontend_entry(&entry, &request, scan_mode)
            .map_err(status_from_hal_error)?;
        self.plan_method(AidlMethodCall::FrontendScan(request.clone()))?;
        start_frontend_scan_use_case(
            runtime.clone(),
            frontend_id,
            entry,
            request,
            scan_mode,
            candidates,
            scan_end_notifier(runtime, self.handle()),
        )
        .map_err(status_from_hal_error)
    }
    fn stopScan(&self) -> BinderResult<()> {
        self.ensure_open()?;
        self.plan_method(AidlMethodCall::FrontendStopScan)?;
        let runtime = self.runtime();
        let frontend_id =
            runtime_entry_public_id(&runtime, self.handle(), AidlObjectKind::Frontend)?;
        stop_frontend_scan_use_case(
            runtime,
            frontend_id,
            FrontendWorkerCancelReason::StopRequested,
        )
        .map_err(status_from_hal_error)
    }
    fn getStatus(&self, status_types: &[FrontendStatusType]) -> BinderResult<Vec<FrontendStatus>> {
        self.ensure_open()?;
        self.plan_method(public_api_call(
            AidlObjectKind::Frontend,
            AidlApi::FrontendGetStatus,
            None,
        ))?;
        let entry = runtime_frontend_entry_for_object(&self.runtime(), self.handle())?;
        let signal_state = frontend_signal_state_for_object(&self.runtime(), self.handle())?;
        frontend_status_for_types(&entry, signal_state, status_types)
    }
    fn setLnb(&self, lnb_id: i32) -> BinderResult<()> {
        self.ensure_open()?;
        let runtime = self.runtime();
        let entry = runtime_frontend_entry_for_object(&runtime, self.handle())?;
        let frontend_id = entry.id.0;
        let exported_lnb_id = {
            let guard = runtime
                .lock()
                .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
            guard
                .lnb_for_frontend_id(frontend_id)
                .ok_or_else(|| {
                    service_error(TunerResult::UNAVAILABLE.0, "frontend has no exported LNB")
                })?
                .id
                .0
        };
        if exported_lnb_id != lnb_id {
            return Err(service_error(
                TunerResult::INVALID_ARGUMENT.0,
                "LNB does not belong to this frontend",
            ));
        }
        self.plan_method(AidlMethodCall::FrontendSetLnb { lnb_id })?;
        let result = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
            .set_frontend_lnb(frontend_id, lnb_id)
            .map_err(status_from_hal_error);
        result
    }
    fn linkCiCam(&self, _ci_cam_id: i32) -> BinderResult<i32> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendLinkCiCam,
                None,
            )),
            "CI CAM is unsupported",
        )?;
        Err(status_unknown_error(
            "linkCiCam unavailable path unexpectedly returned success",
        ))
    }
    fn unlinkCiCam(&self, _ci_cam_id: i32) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendUnlinkCiCam,
                None,
            )),
            "CI CAM is unsupported",
        )
    }
    fn getHardwareInfo(&self) -> BinderResult<String> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendGetHardwareInfo,
                None,
            )),
            "frontend backend is not probed",
        )?;
        Err(status_unknown_error(
            "getHardwareInfo unavailable path unexpectedly returned success",
        ))
    }
    fn removeOutputPid(&self, _pid: i32) -> BinderResult<()> {
        unavailable_after_object_public_api_plan(
            self.plan_method(unsupported_public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendRemoveOutputPid,
                None,
            )),
            "frontend output PID removal is unsupported",
        )
    }
    fn getFrontendStatusReadiness(
        &self,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<Vec<FrontendStatusReadiness>> {
        self.ensure_open()?;
        self.plan_method(public_api_call(
            AidlObjectKind::Frontend,
            AidlApi::FrontendGetFrontendStatusReadiness,
            None,
        ))?;
        let entry = runtime_frontend_entry_for_object(&self.runtime(), self.handle())?;
        let runtime_state = frontend_runtime_state_for_object(&self.runtime(), self.handle())?;
        let signal_state = frontend_signal_state_for_object(&self.runtime(), self.handle())?;
        Ok(frontend_readiness_for_types(
            &entry,
            runtime_state,
            signal_state,
            status_types,
        ))
    }
}
