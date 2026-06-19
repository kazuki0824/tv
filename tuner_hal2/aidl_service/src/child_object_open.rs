use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvr::{BnDvr, IDvr},
    IDvrCallback::IDvrCallback,
    IFilter::{BnFilter, IFilter},
    IFilterCallback::IFilterCallback,
};
use binder::{BinderFeatures, Result as BinderResult, Status, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlMethodCall, OpenDvrRequest, RuntimeExecutableRequest,
};
use maleicacid_tuner_hal2_demux::config::OpenFilterRequest;
use maleicacid_tuner_hal2_common::{FirstErrorCollector, HalError, HalInternalKind};
use maleicacid_tuner_hal2_service_runtime::error_mapping::object_table_error_to_hal;

use crate::callback_store::{retain_dvr_callback, retain_filter_callback};
use crate::error_bridge::{status_from_hal_error, status_unknown_error};
use crate::dvr_object::DvrAidlObject;
use crate::filter_object::FilterAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::{
    clear_owner_callback_registration, clear_owner_callback_registration_hal, execute_shared_object_runtime_use_case_with_request_builder,
    register_callback_artifact_after_owner_ready, SharedTunerRuntime,
};

fn handle_from_runtime_entry(
    entry: maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry,
) -> AidlObjectHandle {
    AidlObjectHandle::new(entry.object_kind, entry.object_id, entry.generation)
}

fn status_debug_to_hal(status: &Status, context: &'static str) -> HalError {
    HalError::internal(
        HalInternalKind::InvariantViolation,
        format!("{context}: {status:?}"),
    )
}

fn compose_binder_status(
    context: &'static str,
    primary: Status,
    cleanup: Status,
) -> Status {
    status_from_hal_error(HalError::composed_failure(
        context,
        status_debug_to_hal(&primary, "primary status"),
        status_debug_to_hal(&cleanup, "cleanup status"),
    ))
}

fn finish_binder_cleanup_after_primary<T>(
    context: &'static str,
    primary: Status,
    cleanup: BinderResult<()>,
) -> BinderResult<T> {
    match cleanup {
        Ok(()) => Err(primary),
        Err(cleanup_status) => Err(compose_binder_status(context, primary, cleanup_status)),
    }
}

fn finish_hal_cleanup_after_primary<T>(
    context: &'static str,
    primary: HalError,
    cleanup: Result<(), HalError>,
) -> BinderResult<T> {
    match cleanup {
        Ok(()) => Err(status_from_hal_error(primary)),
        Err(cleanup_error) => Err(status_from_hal_error(HalError::composed_failure(
            context,
            primary,
            cleanup_error,
        ))),
    }
}

fn finish_status_primary_hal_cleanup<T>(
    context: &'static str,
    primary: Status,
    cleanup: Result<(), HalError>,
) -> BinderResult<T> {
    match cleanup {
        Ok(()) => Err(primary),
        Err(cleanup_error) => Err(status_from_hal_error(HalError::composed_failure(
            context,
            status_debug_to_hal(&primary, "primary status"),
            cleanup_error,
        ))),
    }
}


fn rollback_child_object_registration_after_runtime_id_conversion_failure(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    _context: &'static str,
) -> Result<(), HalError> {
    runtime
        .lock()
        .map_err(|_| HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned",
        ))?
        .unregister_aidl_object_after_registration_failure(handle.object_id(), handle.generation())
        .map(|_| ())
        .map_err(object_table_error_to_hal)
}

fn rollback_filter_child_open(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    filter_id: i32,
) -> BinderResult<()> {
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .rollback_filter_child_open_after_aidl_failure(
            handle.object_id(),
            handle.generation(),
            filter_id,
        )
        .map_err(status_from_hal_error)
}

fn rollback_dvr_child_open(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    dvr_id: i32,
) -> BinderResult<()> {
    runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?
        .rollback_dvr_child_open_after_aidl_failure(
            handle.object_id(),
            handle.generation(),
            dvr_id,
        )
        .map_err(status_from_hal_error)
}

fn rollback_filter_child_open_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    filter_id: i32,
) -> Result<(), HalError> {
    runtime
        .lock()
        .map_err(|_| HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned",
        ))?
        .rollback_filter_child_open_after_aidl_failure(
            handle.object_id(),
            handle.generation(),
            filter_id,
        )
}

fn rollback_dvr_child_open_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    dvr_id: i32,
) -> Result<(), HalError> {
    runtime
        .lock()
        .map_err(|_| HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned",
        ))?
        .rollback_dvr_child_open_after_aidl_failure(
            handle.object_id(),
            handle.generation(),
            dvr_id,
        )
}


fn cleanup_filter_child_open_after_object_failure(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    filter_id: i32,
) -> BinderResult<()> {
    let mut cleanup_collector = FirstErrorCollector::new();
    cleanup_collector.push_result(clear_owner_callback_registration(
        runtime,
        handle,
        Some(AidlApi::DemuxOpenFilter),
        "filter child callback rollback failed",
    ));
    cleanup_collector.push_result(rollback_filter_child_open(runtime, handle, filter_id));
    cleanup_collector.into_result()
}

fn cleanup_dvr_child_open_after_object_failure(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    dvr_id: i32,
) -> BinderResult<()> {
    let mut cleanup_collector = FirstErrorCollector::new();
    cleanup_collector.push_result(clear_owner_callback_registration(
        runtime,
        handle,
        Some(AidlApi::DemuxOpenDvr),
        "DVR child callback rollback failed",
    ));
    cleanup_collector.push_result(rollback_dvr_child_open(runtime, handle, dvr_id));
    cleanup_collector.into_result()
}

fn retain_filter_child_callback(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    callback: &Strong<dyn IFilterCallback>,
) -> BinderResult<()> {
    let mut rollback_callback = || {
        clear_owner_callback_registration_hal(
            runtime,
            handle,
            Some(AidlApi::DemuxOpenFilter),
            "child callback rollback failed",
        )
    };
    register_callback_artifact_after_owner_ready(
        runtime,
        handle,
        AidlApi::DemuxOpenFilter,
        || retain_filter_callback(handle, callback).map_err(|error| {
            error.into_hal_error("filter callback store retain failed")
        }),
        &mut rollback_callback,
    )
}

fn retain_dvr_child_callback(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    callback: &Strong<dyn IDvrCallback>,
) -> BinderResult<()> {
    let mut rollback_callback = || {
        clear_owner_callback_registration_hal(
            runtime,
            handle,
            Some(AidlApi::DemuxOpenDvr),
            "child callback rollback failed",
        )
    };
    register_callback_artifact_after_owner_ready(
        runtime,
        handle,
        AidlApi::DemuxOpenDvr,
        || retain_dvr_callback(handle, callback).map_err(|error| {
            error.into_hal_error("DVR callback store retain failed")
        }),
        &mut rollback_callback,
    )
}



pub fn open_filter_child_for_owner_object_with_request_builder<Build>(
    runtime: &SharedTunerRuntime,
    owner_handle: AidlObjectHandle,
    build_request: Build,
    callback: &Strong<dyn IFilterCallback>,
) -> BinderResult<Strong<dyn IFilter>>
where
    Build: FnOnce() -> Result<OpenFilterRequest, maleicacid_tuner_hal2_common::HalError>,
{
    let runtime_entry = execute_shared_object_runtime_use_case_with_request_builder(
        runtime,
        owner_handle,
        || {
            let request = build_request().map_err(status_from_hal_error)?;
            Ok((
                AidlMethodCall::DemuxOpenFilter(RuntimeExecutableRequest::OpenFilter(request.clone())),
                request,
            ))
        },
        |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
            runtime
                .lock()
                .map_err(|_| {
                    maleicacid_tuner_hal2_common::HalError::internal(
                        maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned",
                    )
                })?
                .open_filter_child_runtime_for_demux_object(
                    handle.object_id(),
                    handle.generation(),
                    &request,
                    dispatch_preflight,
                )
        },
    )?;
    finish_filter_child_open(runtime, runtime_entry, callback)
}

pub fn open_dvr_child_for_owner_object_with_request_builder<Build>(
    runtime: &SharedTunerRuntime,
    owner_handle: AidlObjectHandle,
    build_request: Build,
    callback: &Strong<dyn IDvrCallback>,
) -> BinderResult<Strong<dyn IDvr>>
where
    Build: FnOnce() -> Result<OpenDvrRequest, maleicacid_tuner_hal2_common::HalError>,
{
    let runtime_entry = execute_shared_object_runtime_use_case_with_request_builder(
        runtime,
        owner_handle,
        || {
            let request = build_request().map_err(status_from_hal_error)?;
            Ok((AidlMethodCall::DemuxOpenDvr(request), request))
        },
        |runtime, handle, _command_plan, _executable_request, dispatch_preflight, request| {
            runtime
                .lock()
                .map_err(|_| {
                    maleicacid_tuner_hal2_common::HalError::internal(
                        maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned",
                    )
                })?
                .open_dvr_child_runtime_for_demux_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch_preflight,
                )
        },
    )?;
    finish_dvr_child_open(runtime, runtime_entry, callback)
}


fn finish_filter_child_open(
    runtime: &SharedTunerRuntime,
    runtime_entry: maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry,
    callback: &Strong<dyn IFilterCallback>,
) -> BinderResult<Strong<dyn IFilter>> {
    let child_handle = handle_from_runtime_entry(runtime_entry.clone());
    let filter_id = match i32::try_from(runtime_entry.ledger_id.0) {
        Ok(id) => id,
        Err(_) => {
            return finish_hal_cleanup_after_primary(
                "filter runtime id conversion failure rollback failed",
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter runtime id is outside i32 range",
                ),
                rollback_child_object_registration_after_runtime_id_conversion_failure(
                    runtime,
                    child_handle,
                    "filter child object rollback failed after runtime id conversion failure",
                ),
            );
        }
    };
    if let Err(status) = retain_filter_child_callback(runtime, child_handle, callback) {
        return finish_status_primary_hal_cleanup(
            "filter child callback retain failure rollback failed",
            status,
            rollback_filter_child_open_hal(runtime, child_handle, filter_id),
        );
    }
    match FilterAidlObject::new(child_handle, runtime.clone()) {
        Ok(object) => Ok(BnFilter::new_binder(object, BinderFeatures::default())),
        Err(_) => {
            let object_status = status_unknown_error("filter object kind mismatch");
            finish_binder_cleanup_after_primary(
                "filter object construction failure cleanup failed",
                object_status,
                cleanup_filter_child_open_after_object_failure(runtime, child_handle, filter_id),
            )
        }
    }
}

fn finish_dvr_child_open(
    runtime: &SharedTunerRuntime,
    runtime_entry: maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry,
    callback: &Strong<dyn IDvrCallback>,
) -> BinderResult<Strong<dyn IDvr>> {
    let child_handle = handle_from_runtime_entry(runtime_entry.clone());
    let dvr_id = match i32::try_from(runtime_entry.ledger_id.0) {
        Ok(id) => id,
        Err(_) => {
            return finish_hal_cleanup_after_primary(
                "DVR runtime id conversion failure rollback failed",
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "DVR runtime id is outside i32 range",
                ),
                rollback_child_object_registration_after_runtime_id_conversion_failure(
                    runtime,
                    child_handle,
                    "DVR child object rollback failed after runtime id conversion failure",
                ),
            );
        }
    };
    if let Err(status) = retain_dvr_child_callback(runtime, child_handle, callback) {
        return finish_status_primary_hal_cleanup(
            "DVR child callback retain failure rollback failed",
            status,
            rollback_dvr_child_open_hal(runtime, child_handle, dvr_id),
        );
    }
    match DvrAidlObject::new(child_handle, runtime.clone()) {
        Ok(object) => Ok(BnDvr::new_binder(object, BinderFeatures::default())),
        Err(_) => {
            let object_status = status_unknown_error("DVR object kind mismatch");
            finish_binder_cleanup_after_primary(
                "DVR object construction failure cleanup failed",
                object_status,
                cleanup_dvr_child_open_after_object_failure(runtime, child_handle, dvr_id),
            )
        }
    }
}
