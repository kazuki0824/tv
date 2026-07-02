use super::support::unsupported_public_api_call;
use super::{
    aidl_frontend_settings_to_request, aidl_scan_type_to_mode, close_object_after_close_preflight,
    execute_object_query_use_case, execute_shared_object_runtime_use_case,
    execute_shared_object_runtime_use_case_with_request_builder,
    plan_unavailable_object_method_use_case, scan_end_notifier, set_frontend_lnb_object_use_case,
    start_frontend_scan_use_case, start_frontend_tune_use_case, status_from_hal_error,
    status_unknown_error, stop_frontend_scan_use_case, stop_frontend_tune_use_case, AidlApi,
    AidlMethodCall, AidlObjectKind, BinderResult, FrontendAidlObject, FrontendScanType,
    FrontendSettings, FrontendStatus, FrontendStatusReadiness, FrontendStatusType, IFrontend,
    IFrontendCallback, ObjectFrontendStatusReadinessValue, ObjectFrontendStatusType,
    ObjectFrontendStatusValue, ObjectQueryRequest, ObjectQueryResponse, Strong,
};
use maleicacid_tuner_hal2_device::{FrontendWorkerCancelReason, FrontendWorkerKind};

fn object_frontend_status_type_from_aidl(
    status_type: FrontendStatusType,
) -> ObjectFrontendStatusType {
    match status_type {
        FrontendStatusType::DEMOD_LOCK => ObjectFrontendStatusType::DemodLock,
        FrontendStatusType::LNB_VOLTAGE => ObjectFrontendStatusType::LnbVoltage,
        _ => ObjectFrontendStatusType::Unsupported,
    }
}

fn frontend_status_from_query_value(value: ObjectFrontendStatusValue) -> FrontendStatus {
    match value {
        ObjectFrontendStatusValue::DemodLocked(locked) => FrontendStatus::IsDemodLocked(locked),
        ObjectFrontendStatusValue::LnbVoltageNone => {
            FrontendStatus::LnbVoltage(super::LnbVoltage::NONE)
        }
    }
}

fn frontend_readiness_from_query_value(
    value: ObjectFrontendStatusReadinessValue,
) -> FrontendStatusReadiness {
    match value {
        ObjectFrontendStatusReadinessValue::Stable => FrontendStatusReadiness::STABLE,
        ObjectFrontendStatusReadinessValue::Unstable => FrontendStatusReadiness::UNSTABLE,
        ObjectFrontendStatusReadinessValue::Unavailable => FrontendStatusReadiness::UNAVAILABLE,
        ObjectFrontendStatusReadinessValue::Unsupported => FrontendStatusReadiness::UNSUPPORTED,
    }
}

impl IFrontend for FrontendAidlObject {
    fn setCallback(&self, callback: &Strong<dyn IFrontendCallback>) -> BinderResult<()> {
        self.set_callback_nullable_for_aidl(Some(callback))
    }
    fn tune(&self, settings: &FrontendSettings) -> BinderResult<()> {
        execute_shared_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let request =
                    aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
                Ok((AidlMethodCall::FrontendTune(request.clone()), request))
            },
            |runtime, handle, dispatch_proof, request| {
                start_frontend_tune_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    request,
                    FrontendWorkerKind::Tune,
                    dispatch_proof,
                )
            },
        )
    }
    fn stopTune(&self) -> BinderResult<()> {
        execute_shared_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FrontendStopTune,
            |runtime, handle, dispatch_proof| {
                stop_frontend_tune_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    FrontendWorkerCancelReason::StopRequested,
                    dispatch_proof,
                )
            },
        )
    }
    fn close(&self) -> BinderResult<()> {
        close_object_after_close_preflight(
            &self.context(),
            self.handle(),
            AidlMethodCall::FrontendClose,
        )
    }
    fn scan(&self, settings: &FrontendSettings, scan_type: FrontendScanType) -> BinderResult<()> {
        execute_shared_object_runtime_use_case_with_request_builder(
            &self.runtime(),
            self.handle(),
            || {
                let scan_mode = aidl_scan_type_to_mode(scan_type).map_err(status_from_hal_error)?;
                let request =
                    aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
                Ok((
                    AidlMethodCall::FrontendScan(request.clone()),
                    (request, scan_mode),
                ))
            },
            |runtime, handle, dispatch_proof, (request, scan_mode)| {
                start_frontend_scan_use_case(
                    runtime.clone(),
                    handle.object_id(),
                    handle.generation(),
                    request,
                    scan_mode,
                    scan_end_notifier(self.context(), handle),
                    dispatch_proof,
                )
            },
        )
    }
    fn stopScan(&self) -> BinderResult<()> {
        execute_shared_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FrontendStopScan,
            |runtime, handle, dispatch_proof| {
                stop_frontend_scan_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    FrontendWorkerCancelReason::StopRequested,
                    dispatch_proof,
                )
            },
        )
    }
    fn getStatus(&self, status_types: &[FrontendStatusType]) -> BinderResult<Vec<FrontendStatus>> {
        match execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            ObjectQueryRequest::FrontendGetStatus {
                status_types: status_types
                    .iter()
                    .copied()
                    .map(object_frontend_status_type_from_aidl)
                    .collect(),
            },
        )? {
            ObjectQueryResponse::FrontendStatus(values) => Ok(values
                .into_iter()
                .map(frontend_status_from_query_value)
                .collect()),
            _ => Err(status_unknown_error(
                "unexpected object query response for Frontend.getStatus",
            )),
        }
    }
    fn setLnb(&self, lnb_id: i32) -> BinderResult<()> {
        execute_shared_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FrontendSetLnb { lnb_id },
            |runtime, handle, dispatch_proof| {
                set_frontend_lnb_object_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    lnb_id,
                    dispatch_proof,
                )
            },
        )
    }
    fn linkCiCam(&self, _ci_cam_id: i32) -> BinderResult<i32> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(unsupported_public_api_call(
                    AidlObjectKind::Frontend,
                    AidlApi::FrontendLinkCiCam,
                    None,
                ))
            },
            "CI CAM is unsupported",
        )?;
        Err(status_unknown_error(
            "linkCiCam unavailable path unexpectedly returned success",
        ))
    }
    fn unlinkCiCam(&self, _ci_cam_id: i32) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(unsupported_public_api_call(
                    AidlObjectKind::Frontend,
                    AidlApi::FrontendUnlinkCiCam,
                    None,
                ))
            },
            "CI CAM is unsupported",
        )
    }
    fn getHardwareInfo(&self) -> BinderResult<String> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(unsupported_public_api_call(
                    AidlObjectKind::Frontend,
                    AidlApi::FrontendGetHardwareInfo,
                    None,
                ))
            },
            "frontend backend is not probed",
        )?;
        Err(status_unknown_error(
            "getHardwareInfo unavailable path unexpectedly returned success",
        ))
    }
    fn removeOutputPid(&self, _pid: i32) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                Ok(unsupported_public_api_call(
                    AidlObjectKind::Frontend,
                    AidlApi::FrontendRemoveOutputPid,
                    None,
                ))
            },
            "frontend output PID removal is unsupported",
        )
    }
    fn getFrontendStatusReadiness(
        &self,
        status_types: &[FrontendStatusType],
    ) -> BinderResult<Vec<FrontendStatusReadiness>> {
        match execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            ObjectQueryRequest::FrontendGetFrontendStatusReadiness {
                status_types: status_types
                    .iter()
                    .copied()
                    .map(object_frontend_status_type_from_aidl)
                    .collect(),
            },
        )? {
            ObjectQueryResponse::FrontendStatusReadiness(values) => Ok(values
                .into_iter()
                .map(frontend_readiness_from_query_value)
                .collect()),
            _ => Err(status_unknown_error(
                "unexpected object query response for Frontend.getFrontendStatusReadiness",
            )),
        }
    }
}
