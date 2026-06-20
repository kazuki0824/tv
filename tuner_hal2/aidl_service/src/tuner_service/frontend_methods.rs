use super::support::{public_api_call, unsupported_public_api_call};
use super::{
    aidl_frontend_settings_to_request, aidl_scan_type_to_mode,
    close_frontend_object_cleanup_use_case, close_object_after_close_preflight_with_domain_cleanup,
    execute_object_query_use_case, execute_shared_object_runtime_use_case,
    execute_shared_object_runtime_use_case_with_request_builder, frontend_readiness_for_types,
    frontend_status_for_types, plan_unavailable_object_method_use_case, scan_end_notifier,
    set_frontend_lnb_object_use_case, start_frontend_scan_use_case, start_frontend_tune_use_case,
    status_from_hal_error, status_unknown_error, stop_frontend_scan_use_case,
    stop_frontend_tune_use_case, AidlApi, AidlMethodCall, AidlObjectKind, BinderResult,
    FrontendAidlObject, FrontendScanType, FrontendSettings, FrontendStatus,
    FrontendStatusReadiness, FrontendStatusType, IFrontend, IFrontendCallback, Strong,
};
use crate::object_runtime::clear_live_lnb_callback_for_public_id_hal;
use maleicacid_tuner_hal2_common::FirstErrorCollector;
use maleicacid_tuner_hal2_device::{FrontendWorkerCancelReason, FrontendWorkerKind};

impl IFrontend for FrontendAidlObject {
    fn setCallback(&self, callback: &Strong<dyn IFrontendCallback>) -> BinderResult<()> {
        self.set_callback_transaction(callback)
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
            |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
                start_frontend_tune_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    request,
                    FrontendWorkerKind::Tune,
                    dispatch_preflight,
                )
            },
        )
    }
    fn stopTune(&self) -> BinderResult<()> {
        execute_shared_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FrontendStopTune,
            |runtime, handle, command_plan, executable_request| {
                stop_frontend_tune_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    FrontendWorkerCancelReason::StopRequested,
                    command_plan,
                    executable_request,
                )
            },
        )
    }
    fn close(&self) -> BinderResult<()> {
        let runtime_for_cleanup = self.runtime();
        let handle = self.handle();
        close_object_after_close_preflight_with_domain_cleanup(
            &runtime_for_cleanup,
            handle,
            AidlMethodCall::FrontendClose,
            || {
                let mut cleanup_collector = FirstErrorCollector::new();
                match close_frontend_object_cleanup_use_case(
                    runtime_for_cleanup.clone(),
                    handle.object_id(),
                    handle.generation(),
                    FrontendWorkerCancelReason::FrontendClosing,
                ) {
                    Ok(report) => {
                        cleanup_collector.push_result(report.cleanup_result);
                        for lnb_id in report.closed_lnb_ids {
                            cleanup_collector.push_result(
                                clear_live_lnb_callback_for_public_id_hal(
                                    &runtime_for_cleanup,
                                    lnb_id,
                                ),
                            );
                        }
                    }
                    Err(error) => cleanup_collector.push_error(error),
                }
                cleanup_collector.into_result()
            },
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
            |runtime,
             handle,
             _command_plan,
             _executable_request,
             dispatch_preflight,
             (request, scan_mode)| {
                start_frontend_scan_use_case(
                    runtime.clone(),
                    handle.object_id(),
                    handle.generation(),
                    request,
                    scan_mode,
                    scan_end_notifier(runtime, handle),
                    dispatch_preflight,
                )
            },
        )
    }
    fn stopScan(&self) -> BinderResult<()> {
        execute_shared_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FrontendStopScan,
            |runtime, handle, command_plan, executable_request| {
                stop_frontend_scan_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    FrontendWorkerCancelReason::StopRequested,
                    command_plan,
                    executable_request,
                )
            },
        )
    }
    fn getStatus(&self, status_types: &[FrontendStatusType]) -> BinderResult<Vec<FrontendStatus>> {
        let (entry, _runtime_state, signal_state) = execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            public_api_call(AidlObjectKind::Frontend, AidlApi::FrontendGetStatus, None),
            |runtime, handle| {
                runtime
                    .frontend_status_query_for_aidl_object(handle.object_id(), handle.generation())
            },
        )?;
        frontend_status_for_types(&entry, signal_state, status_types)
    }
    fn setLnb(&self, lnb_id: i32) -> BinderResult<()> {
        execute_shared_object_runtime_use_case(
            &self.runtime(),
            self.handle(),
            AidlMethodCall::FrontendSetLnb { lnb_id },
            |runtime, handle, command_plan, executable_request| {
                set_frontend_lnb_object_use_case(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    lnb_id,
                    command_plan,
                    executable_request,
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
        let (entry, runtime_state, signal_state) = execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            public_api_call(
                AidlObjectKind::Frontend,
                AidlApi::FrontendGetFrontendStatusReadiness,
                None,
            ),
            |runtime, handle| {
                runtime
                    .frontend_status_query_for_aidl_object(handle.object_id(), handle.generation())
            },
        )?;
        Ok(frontend_readiness_for_types(
            &entry,
            runtime_state,
            signal_state,
            status_types,
        ))
    }
}
