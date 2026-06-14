use std::ffi::CString;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvr::{BnDvr, IDvr},
    IDvrCallback::IDvrCallback,
    IFilter::{BnFilter, IFilter},
    IFilterCallback::IFilterCallback,
    Result::Result as TunerResult,
};
use binder::{BinderFeatures, Result as BinderResult, Status, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlObjectKind, AidlStatusMapper, OpenDvrRequest, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_demux::config::OpenFilterRequest;
use maleicacid_tuner_hal2_service_runtime::RuntimeOwnerRelation;

use crate::callback_store::{clear_owner_callbacks, retain_dvr_callback, retain_filter_callback};
use crate::dvr_object::DvrAidlObject;
use crate::filter_object::FilterAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::{record_callback_registration, SharedTunerRuntime};

fn service_error(code: i32, message: &str) -> Status {
    match CString::new(message) {
        Ok(detail) => Status::new_service_specific_error(code, Some(detail.as_c_str())),
        Err(_) => Status::new_service_specific_error(code, None),
    }
}

fn status_unknown_error(message: &str) -> Status {
    service_error(TunerResult::UNKNOWN_ERROR.0, message)
}

fn status_from_tuner_status(status: TunerStatusCode, message: &str) -> Status {
    match status {
        TunerStatusCode::Ok => service_error(
            TunerResult::UNKNOWN_ERROR.0,
            "unexpected OK status while building error",
        ),
        TunerStatusCode::InvalidArgument => service_error(TunerResult::INVALID_ARGUMENT.0, message),
        TunerStatusCode::InvalidState => service_error(TunerResult::INVALID_STATE.0, message),
        TunerStatusCode::Unavailable => service_error(TunerResult::UNAVAILABLE.0, message),
        TunerStatusCode::UnknownError => service_error(TunerResult::UNKNOWN_ERROR.0, message),
    }
}

fn status_from_hal_error(error: HalError) -> Status {
    let status = AidlStatusMapper::map_error(&error);
    status_from_tuner_status(status, &error.to_string())
}

fn register_child_aidl_object(
    runtime: &SharedTunerRuntime,
    kind: AidlObjectKind,
    public_runtime_id: i32,
    owner: RuntimeOwnerRelation,
) -> BinderResult<AidlObjectHandle> {
    let entry = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .register_aidl_object_for_runtime_auto_generation(kind, i64::from(public_runtime_id), owner)
        .map_err(|_| {
            service_error(
                TunerResult::INVALID_STATE.0,
                "AIDL child object registration failed",
            )
        })?;
    Ok(AidlObjectHandle::new(
        entry.object_kind,
        entry.object_id,
        entry.generation,
    ))
}

fn rollback_child_aidl_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .unregister_aidl_object_after_registration_failure(handle.object_id(), handle.generation())
        .map(|_| ())
        .map_err(|_| {
            service_error(
                TunerResult::UNKNOWN_ERROR.0,
                "AIDL child object rollback failed",
            )
        })
}

fn unregister_child_public_runtime(
    runtime: &SharedTunerRuntime,
    kind: AidlObjectKind,
    public_runtime_id: i32,
) -> BinderResult<()> {
    let mut runtime = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    match kind {
        AidlObjectKind::Filter => {
            runtime.unregister_filter_runtime(public_runtime_id);
        }
        AidlObjectKind::Dvr => {
            runtime.unregister_dvr_runtime(public_runtime_id);
        }
        AidlObjectKind::Descrambler => {
            runtime.unregister_descrambler_runtime(public_runtime_id);
        }
        _ => {}
    }
    Ok(())
}

fn allocate_filter_public_runtime(
    runtime: &SharedTunerRuntime,
    owner_demux_id: i32,
) -> BinderResult<i32> {
    let entry = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .allocate_filter_runtime(owner_demux_id)
        .map_err(|_| status_unknown_error("filter runtime allocation failed"))?;
    Ok(entry.id.0)
}

fn allocate_dvr_public_runtime(
    runtime: &SharedTunerRuntime,
    owner_demux_id: i32,
) -> BinderResult<i32> {
    let entry = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .allocate_dvr_runtime(owner_demux_id)
        .map_err(|_| status_unknown_error("DVR runtime allocation failed"))?;
    Ok(entry.id.0)
}

fn rollback_retained_child_callback(handle: AidlObjectHandle) -> BinderResult<()> {
    clear_owner_callbacks(handle)
        .map_err(|_| status_unknown_error("child callback rollback failed"))
}

fn retain_filter_child_callback(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    callback: &Strong<dyn IFilterCallback>,
) -> BinderResult<()> {
    retain_filter_callback(handle, callback)
        .map_err(|_| Status::new_service_specific_error(TunerResult::UNKNOWN_ERROR.0, None))?;
    if let Err(status) = record_callback_registration(runtime, handle, AidlApi::DemuxOpenFilter) {
        rollback_retained_child_callback(handle)?;
        return Err(status);
    }
    Ok(())
}

fn retain_dvr_child_callback(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    callback: &Strong<dyn IDvrCallback>,
) -> BinderResult<()> {
    retain_dvr_callback(handle, callback)
        .map_err(|_| Status::new_service_specific_error(TunerResult::UNKNOWN_ERROR.0, None))?;
    if let Err(status) = record_callback_registration(runtime, handle, AidlApi::DemuxOpenDvr) {
        rollback_retained_child_callback(handle)?;
        return Err(status);
    }
    Ok(())
}

pub fn open_filter_child_after_plan(
    runtime: &SharedTunerRuntime,
    owner_handle: AidlObjectHandle,
    owner_demux_id: i32,
    request: OpenFilterRequest,
    callback: &Strong<dyn IFilterCallback>,
) -> BinderResult<Strong<dyn IFilter>> {
    let filter_id = allocate_filter_public_runtime(runtime, owner_demux_id)?;
    if let Err(error) = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .register_demux_filter_runtime(owner_demux_id, filter_id, &request)
    {
        unregister_child_public_runtime(runtime, AidlObjectKind::Filter, filter_id)?;
        return Err(status_from_hal_error(error));
    }
    let owner = RuntimeOwnerRelation::Demux {
        demux: owner_handle.object_id(),
        generation: owner_handle.generation(),
    };
    let child_handle =
        match register_child_aidl_object(runtime, AidlObjectKind::Filter, filter_id, owner) {
            Ok(handle) => handle,
            Err(status) => {
                unregister_child_public_runtime(runtime, AidlObjectKind::Filter, filter_id)?;
                return Err(status);
            }
        };
    match FilterAidlObject::new(child_handle, runtime.clone()) {
        Ok(object) => {
            if let Err(status) = retain_filter_child_callback(runtime, child_handle, callback) {
                rollback_child_aidl_object(runtime, child_handle)?;
                unregister_child_public_runtime(runtime, AidlObjectKind::Filter, filter_id)?;
                return Err(status);
            }
            Ok(BnFilter::new_binder(object, BinderFeatures::default()))
        }
        Err(_) => {
            rollback_child_aidl_object(runtime, child_handle)?;
            unregister_child_public_runtime(runtime, AidlObjectKind::Filter, filter_id)?;
            Err(status_unknown_error("filter object kind mismatch"))
        }
    }
}

pub fn open_dvr_child_after_plan(
    runtime: &SharedTunerRuntime,
    owner_handle: AidlObjectHandle,
    owner_demux_id: i32,
    request: OpenDvrRequest,
    callback: &Strong<dyn IDvrCallback>,
) -> BinderResult<Strong<dyn IDvr>> {
    let dvr_id = allocate_dvr_public_runtime(runtime, owner_demux_id)?;
    if let Err(error) = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .register_demux_dvr_runtime(owner_demux_id, dvr_id, &request, true)
    {
        unregister_child_public_runtime(runtime, AidlObjectKind::Dvr, dvr_id)?;
        return Err(status_from_hal_error(error));
    }
    let owner = RuntimeOwnerRelation::Demux {
        demux: owner_handle.object_id(),
        generation: owner_handle.generation(),
    };
    let child_handle = match register_child_aidl_object(runtime, AidlObjectKind::Dvr, dvr_id, owner)
    {
        Ok(handle) => handle,
        Err(status) => {
            unregister_child_public_runtime(runtime, AidlObjectKind::Dvr, dvr_id)?;
            return Err(status);
        }
    };
    match DvrAidlObject::new(child_handle, runtime.clone()) {
        Ok(object) => {
            if let Err(status) = retain_dvr_child_callback(runtime, child_handle, callback) {
                rollback_child_aidl_object(runtime, child_handle)?;
                unregister_child_public_runtime(runtime, AidlObjectKind::Dvr, dvr_id)?;
                return Err(status);
            }
            Ok(BnDvr::new_binder(object, BinderFeatures::default()))
        }
        Err(_) => {
            rollback_child_aidl_object(runtime, child_handle)?;
            unregister_child_public_runtime(runtime, AidlObjectKind::Dvr, dvr_id)?;
            Err(status_unknown_error("DVR object kind mismatch"))
        }
    }
}
