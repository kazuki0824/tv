use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    DemuxPid::DemuxPid, IFilter::{BnFilter, IFilter}, Result::Result as TunerResult,
};
use binder::binder_impl::Binder;
use binder::{Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlFailureSource, AidlMethodAdapter, AidlMethodCall, AidlMethodPlan,
    AidlObjectKind, AidlStatusMapper, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};
use maleicacid_tuner_hal2_service_runtime::{
    RuntimeObjectQueryError, RuntimeOwnerRelation,
};

use crate::error_bridge::{
    service_error, status_from_hal_error, status_from_tuner_status, status_unknown_error,
};
use crate::filter_object::FilterAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::SharedTunerRuntime;

use super::TunerAidlService;

fn map_object_query_error(
    error: RuntimeObjectQueryError,
    mismatch_is_invalid_argument: bool,
    invalid_argument_message: &'static str,
    invalid_state_message: &'static str,
) -> binder::Status {
    match error {
        RuntimeObjectQueryError::KindOrOwnerMismatch if mismatch_is_invalid_argument => {
            service_error(TunerResult::INVALID_ARGUMENT.0, invalid_argument_message)
        }
        RuntimeObjectQueryError::KindOrOwnerMismatch | RuntimeObjectQueryError::NotLive => {
            service_error(TunerResult::INVALID_STATE.0, invalid_state_message)
        }
        RuntimeObjectQueryError::PublicIdOutOfRange => {
            status_unknown_error("public runtime id out of i32 range")
        }
    }
}

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

pub(super) fn unavailable_after_method_plan(
    plan: BinderResult<AidlMethodPlan>,
    message: &'static str,
) -> BinderResult<()> {
    let plan = plan?;
    let failure = AidlFailureSource::RuntimeDispatch(HalError::Unsupported(message));
    let failures = [failure];
    let status =
        AidlStatusMapper::resolve_failure_by_precedence(plan.command_plan.api, &failures, false)
            .unwrap_or(TunerStatusCode::UnknownError);
    Err(status_from_tuner_status(status, message))
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
    let method_plan = AidlMethodAdapter::plan(method);
    service
        .plan_from_method_plan(&method_plan)
        .map_err(|err| status_from_hal_error(err.into_hal_error()))?;
    let failure = AidlFailureSource::RuntimeDispatch(HalError::Unsupported(message));
    let failures = [failure];
    let status = AidlStatusMapper::resolve_failure_by_precedence(
        method_plan.command_plan.api,
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
    let method_plan = AidlMethodAdapter::plan(method);
    service
        .plan_from_method_plan(&method_plan)
        .map_err(|err| status_from_hal_error(err.into_hal_error()))?;
    Ok(())
}

pub(super) fn unavailable_after_object_public_api_plan(
    plan: BinderResult<AidlMethodPlan>,
    message: &'static str,
) -> BinderResult<()> {
    unavailable_after_method_plan(plan, message)
}

pub(super) fn runtime_entry_public_id(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    expected_kind: AidlObjectKind,
) -> BinderResult<i32> {
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .public_runtime_id_for_aidl_object(
            handle.object_id(),
            handle.generation(),
            expected_kind,
        )
        .map_err(|error| {
            map_object_query_error(
                error,
                false,
                "AIDL object lookup failed",
                "AIDL object lookup failed",
            )
        })
}

pub(super) fn current_filter_open_type(
    filter: &FilterAidlObject,
) -> BinderResult<maleicacid_tuner_hal2_demux::FilterOpenType> {
    let runtime = filter.runtime();
    let public_id = runtime_entry_public_id(&runtime, filter.handle(), AidlObjectKind::Filter)?;
    let open_type = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .filter_open_type(public_id)
        .ok_or_else(|| service_error(TunerResult::INVALID_STATE.0, "filter runtime is missing"))?;
    Ok(open_type)
}

pub(super) fn local_filter_handle_from_strong(filter: &Strong<dyn IFilter>) -> BinderResult<AidlObjectHandle> {
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

pub(super) fn filter_entry_public_id_and_owner(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<(i32, RuntimeOwnerRelation)> {
    let entry = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .public_entry_for_aidl_object(
            handle.object_id(),
            handle.generation(),
            AidlObjectKind::Filter,
        )
        .map_err(|error| {
            map_object_query_error(
                error,
                true,
                "source filter owner or kind mismatch",
                "source filter is not live",
            )
        })?;
    Ok((entry.public_id(), entry.owner()))
}


pub(super) fn demux_public_id_for_owner(
    runtime: &SharedTunerRuntime,
    owner_object_id: maleicacid_tuner_hal2_binder_adapter::AidlObjectId,
    owner_generation: maleicacid_tuner_hal2_binder_adapter::AidlObjectGeneration,
) -> BinderResult<i32> {
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .public_runtime_id_for_aidl_object(
            owner_object_id,
            owner_generation,
            AidlObjectKind::Demux,
        )
        .map_err(|error| {
            map_object_query_error(
                error,
                false,
                "owner demux is not live",
                "owner demux is not live",
            )
        })
}
