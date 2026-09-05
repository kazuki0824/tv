use super::support::unsupported_public_api_call;
use super::{
    aidl_frontend_settings_to_request, aidl_scan_type_to_mode, close_object_after_close_preflight,
    execute_object_query_use_case, execute_shared_object_runtime_use_case,
    execute_shared_object_runtime_use_case_with_request_builder,
    plan_unavailable_object_method_use_case, scan_notifier, set_frontend_lnb_object_use_case,
    status_from_hal_error, status_unknown_error, tune_notifier, AidlApi, AidlMethodCall,
    AidlObjectKind, BinderResult, FrontendAidlObject, FrontendProfileRequirement,
    FrontendScanType, FrontendSettings, FrontendStatus, FrontendStatusReadiness,
    FrontendStatusType, FrontendTuneScanTxn, IFrontend, IFrontendCallback,
    ObjectFrontendStatusReadinessValue, ObjectFrontendStatusType, ObjectFrontendStatusValue,
    ObjectQueryRequest, ObjectQueryResponse, Strong,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};
use maleicacid_tuner_hal2_device::{FrontendWorkerCancelReason, FrontendWorkerKind};

fn object_frontend_status_type_from_aidl(
    status_type: FrontendStatusType,
) -> ObjectFrontendStatusType {
    match status_type {
        FrontendStatusType::DEMOD_LOCK => ObjectFrontendStatusType::DemodLock,
        FrontendStatusType::RF_LOCK => ObjectFrontendStatusType::RfLock,
        FrontendStatusType::LNB_VOLTAGE => ObjectFrontendStatusType::LnbVoltage,
        FrontendStatusType::STREAM_ID_LIST => ObjectFrontendStatusType::StreamIdList,
        _ => ObjectFrontendStatusType::Unsupported,
    }
}

fn frontend_status_from_query_value(value: ObjectFrontendStatusValue) -> FrontendStatus {
    match value {
        ObjectFrontendStatusValue::DemodLocked(locked) => FrontendStatus::IsDemodLocked(locked),
        ObjectFrontendStatusValue::RfLocked(locked) => FrontendStatus::IsRfLocked(locked),
        ObjectFrontendStatusValue::LnbVoltageNone => {
            FrontendStatus::LnbVoltage(super::LnbVoltage::NONE)
        }
        ObjectFrontendStatusValue::LnbVoltage11V => {
            FrontendStatus::LnbVoltage(super::LnbVoltage::VOLTAGE_11V)
        }
        ObjectFrontendStatusValue::LnbVoltage15V => {
            FrontendStatus::LnbVoltage(super::LnbVoltage::VOLTAGE_15V)
        }
        ObjectFrontendStatusValue::StreamIdList(stream_ids) => {
            FrontendStatus::StreamIdList(stream_ids)
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

fn enforce_frontend_product_profile(
    requirements: &[FrontendProfileRequirement],
) -> Result<(), HalError> {
    let Some(requirement) = requirements.first() else {
        return Ok(());
    };
    let (feature, detail) = match requirement {
        FrontendProfileRequirement::IsdbtExplicitMode => {
            ("isdbt.mode", "explicit ISDB-T mode is not supported")
        }
        FrontendProfileRequirement::IsdbtExplicitInversion => (
            "isdbt.inversion",
            "explicit ISDB-T spectral inversion is not supported",
        ),
        FrontendProfileRequirement::IsdbtExplicitGuardInterval => (
            "isdbt.guardInterval",
            "explicit ISDB-T guard interval is not supported",
        ),
        FrontendProfileRequirement::IsdbtServiceAreaId => (
            "isdbt.serviceAreaId",
            "explicit ISDB-T serviceAreaId is not supported",
        ),
        FrontendProfileRequirement::IsdbtPartialReceptionAuto => (
            "isdbt.partialReceptionFlag",
            "ISDB-T partial reception AUTO is not supported",
        ),
        FrontendProfileRequirement::IsdbtLayerModulation => (
            "isdbt.layer.modulation",
            "explicit ISDB-T layer modulation is not supported",
        ),
        FrontendProfileRequirement::IsdbtLayerCoderate => (
            "isdbt.layer.coderate",
            "explicit ISDB-T layer coderate is not supported",
        ),
        FrontendProfileRequirement::IsdbtLayerTimeInterleave => (
            "isdbt.layer.timeInterleave",
            "explicit ISDB-T layer time interleave is not supported",
        ),
        FrontendProfileRequirement::IsdbtExplicitSegmentCount { .. } => (
            "isdbt.layer.numOfSegment",
            "explicit ISDB-T segment count is not supported",
        ),
        FrontendProfileRequirement::IsdbsExplicitModulation => (
            "isdbs.modulation",
            "explicit ISDB-S modulation is not supported",
        ),
        FrontendProfileRequirement::IsdbsExplicitCoderate => (
            "isdbs.coderate",
            "explicit ISDB-S coderate is not supported",
        ),
        FrontendProfileRequirement::IsdbsExplicitRolloff => {
            ("isdbs.rolloff", "explicit ISDB-S rolloff is not supported")
        }
    };
    Err(HalError::unsupported_detail(feature, detail))
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
                let converted =
                    aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
                enforce_frontend_product_profile(&converted.profile_requirements)
                    .map_err(status_from_hal_error)?;
                Ok((
                    AidlMethodCall::FrontendTune(converted.request.clone()),
                    converted.request,
                ))
            },
            |runtime, handle, dispatch_proof, request| {
                FrontendTuneScanTxn::begin_tune(
                    runtime,
                    handle.object_id(),
                    handle.generation(),
                    request,
                    FrontendWorkerKind::Tune,
                    tune_notifier(self.context(), handle),
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
                FrontendTuneScanTxn::stop_tune(
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
                let converted =
                    aidl_frontend_settings_to_request(settings).map_err(status_from_hal_error)?;
                enforce_frontend_product_profile(&converted.profile_requirements)
                    .map_err(status_from_hal_error)?;
                Ok((
                    AidlMethodCall::FrontendScan(converted.request.clone()),
                    (converted.request, scan_mode),
                ))
            },
            |runtime, handle, dispatch_proof, (request, scan_mode)| {
                FrontendTuneScanTxn::begin_scan(
                    runtime.clone(),
                    handle.object_id(),
                    handle.generation(),
                    request,
                    scan_mode,
                    scan_notifier(self.context(), handle),
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
                FrontendTuneScanTxn::stop_scan(
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
    fn linkCiCam(&self, ci_cam_id: i32) -> BinderResult<i32> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                if ci_cam_id < 0 {
                    return Err(status_from_hal_error(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "CI CAM id must be non-negative",
                    )));
                }
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
    fn unlinkCiCam(&self, ci_cam_id: i32) -> BinderResult<()> {
        plan_unavailable_object_method_use_case(
            &self.runtime(),
            self.handle(),
            || {
                if ci_cam_id < 0 {
                    return Err(status_from_hal_error(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "CI CAM id must be non-negative",
                    )));
                }
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
        match execute_object_query_use_case(
            &self.runtime(),
            self.handle(),
            ObjectQueryRequest::FrontendGetHardwareInfo,
        )? {
            ObjectQueryResponse::FrontendHardwareInfo(hardware_info) => Ok(hardware_info),
            _ => Err(status_unknown_error(
                "unexpected object query response for Frontend.getHardwareInfo",
            )),
        }
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
