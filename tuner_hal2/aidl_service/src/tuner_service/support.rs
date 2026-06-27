use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    DemuxPid::DemuxPid,
    IFilter::{BnFilter, IFilter},
};
use binder::binder_impl::Binder;
use binder::{Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlMethodCall, AidlObjectKind, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidArgumentKind};

use crate::error_bridge::status_from_tuner_status;
use crate::filter_object::FilterAidlObject;
use crate::object_handle::AidlObjectHandle;

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

pub(super) fn local_filter_handle_from_strong(
    filter: &Strong<dyn IFilter>,
) -> BinderResult<AidlObjectHandle> {
    let binder_native: Binder<BnFilter> = filter.as_binder().try_into().map_err(|_| {
        status_from_tuner_status(
            TunerStatusCode::InvalidArgument,
            "source filter is not a local HAL filter",
        )
    })?;
    let Some(local_filter) = binder_native.downcast_binder::<FilterAidlObject>() else {
        return Err(status_from_tuner_status(
            TunerStatusCode::InvalidArgument,
            "source filter is not a local HAL filter",
        ));
    };
    Ok(local_filter.handle())
}
