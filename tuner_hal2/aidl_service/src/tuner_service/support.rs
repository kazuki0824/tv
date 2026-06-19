use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    DemuxPid::DemuxPid,
    IFilter::{BnFilter, IFilter},
    Result::Result as TunerResult,
};
use binder::binder_impl::Binder;
use binder::{Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlFailureSource, AidlMethodAdapter, AidlMethodCall, AidlObjectKind,
    AidlStatusMapper, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};

use crate::error_bridge::{service_error, status_from_hal_error, status_from_tuner_status};
use crate::filter_object::FilterAidlObject;
use crate::object_handle::AidlObjectHandle;

use super::TunerAidlService;

pub(super) fn ts_pid_from_demux_pid(pid: &DemuxPid) -> Result<u16, HalError> {
    match pid {
        DemuxPid::TPid(value) if (0..=0x1ffe).contains(value) => Ok(*value as u16),
        DemuxPid::TPid(_) => Err(HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "TS PID must be in 0..=0x1ffe for descrambler binding",
        )),
        _ => Err(HalError::Unsupported(
            "non-TS descrambler PID is outside the r51 TS-only profile",
        )),
    }
}

pub(super) fn public_api_call(
    object: AidlObjectKind,
    api: AidlApi,
    _input: Option<()>,
) -> AidlMethodCall {
    AidlMethodCall::PublicApi { object, api }
}

pub(super) fn unsupported_public_api_call(
    object: AidlObjectKind,
    api: AidlApi,
    _input: Option<()>,
) -> AidlMethodCall {
    AidlMethodCall::UnsupportedPublicApi { object, api }
}

pub(super) fn unavailable_after_tuner_method_plan(
    service: &TunerAidlService,
    api: AidlApi,
    input: Option<()>,
    message: &'static str,
) -> BinderResult<()> {
    let method = unsupported_public_api_call(AidlObjectKind::Tuner, api, input);
    let method_plan = AidlMethodAdapter::plan(method).map_err(status_from_hal_error)?;
    service
        .plan_from_method_plan(&method_plan)
        .map_err(|err| status_from_hal_error(err.into_hal_error()))?;
    let failure = AidlFailureSource::RuntimeDispatch(HalError::Unsupported(message));
    let failures = [failure];
    let status = AidlStatusMapper::resolve_failure_by_precedence(
        method_plan.command_plan.api(),
        &failures,
        false,
    )
    .unwrap_or(TunerStatusCode::UnknownError);
    Err(status_from_tuner_status(status, message))
}

pub(super) fn plan_tuner_public_api_method(
    service: &TunerAidlService,
    api: AidlApi,
    input: Option<()>,
) -> BinderResult<()> {
    let method = public_api_call(AidlObjectKind::Tuner, api, input);
    let method_plan = AidlMethodAdapter::plan(method).map_err(status_from_hal_error)?;
    service
        .plan_from_method_plan(&method_plan)
        .map_err(|err| status_from_hal_error(err.into_hal_error()))?;
    Ok(())
}

pub(super) fn local_filter_handle_from_strong(
    filter: &Strong<dyn IFilter>,
) -> BinderResult<AidlObjectHandle> {
    let binder_native: Binder<BnFilter> = filter.as_binder().try_into().map_err(|_| {
        service_error(
            TunerResult::INVALID_ARGUMENT.0,
            "source filter is not a local HAL filter",
        )
    })?;
    let Some(local_filter) = binder_native.downcast_binder::<FilterAidlObject>() else {
        return Err(service_error(
            TunerResult::INVALID_ARGUMENT.0,
            "source filter is not a local HAL filter",
        ));
    };
    Ok(local_filter.handle())
}
