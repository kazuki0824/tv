use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvr::{BnDvr, IDvr},
    IDvrCallback::IDvrCallback,
    IFilter::{BnFilter, IFilter},
    IFilterCallback::IFilterCallback,
};
use binder::{BinderFeatures, Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlMethodCall, OpenDvrRequest, RuntimeExecutableRequest,
};
use maleicacid_tuner_hal2_common::{
    fail_after_cleanup, FirstErrorCollector, HalError, HalInternalKind,
};
use maleicacid_tuner_hal2_demux::config::OpenFilterRequest;
use maleicacid_tuner_hal2_service_runtime::object_method_txn::{
    execute_object_method_call_after_live, ObjectMethodTxnBuildError,
};

use crate::dvr_object::DvrAidlObject;
use crate::error_bridge::status_from_hal_error;
use crate::filter_object::FilterAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::{
    clear_owner_callback_registration_hal, register_callback_artifact_after_owner_ready_hal,
};
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};

fn handle_from_runtime_entry(
    entry: maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry,
) -> AidlObjectHandle {
    AidlObjectHandle::new(entry.object_kind, entry.object_id, entry.generation)
}

fn child_open_txn_error<E>(error: ObjectMethodTxnBuildError<E>) -> binder::Status
where
    E: Into<HalError>,
{
    match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(error) => status_from_hal_error(error.into()),
    }
}

fn finish_hal_cleanup_after_primary<T>(
    context: &'static str,
    primary: HalError,
    cleanup: Result<(), HalError>,
) -> BinderResult<T> {
    fail_after_cleanup(context, primary, cleanup).map_err(status_from_hal_error)
}

fn rollback_filter_child_open_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    filter_id: i32,
) -> Result<(), HalError> {
    runtime
        .lock()
        .map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned",
            )
        })?
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
        .map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned",
            )
        })?
        .rollback_dvr_child_open_after_aidl_failure(handle.object_id(), handle.generation(), dvr_id)
}

fn cleanup_filter_child_open_after_object_failure(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    filter_id: i32,
) -> Result<(), HalError> {
    let mut cleanup_collector = FirstErrorCollector::new();
    cleanup_collector.push_result(clear_owner_callback_registration_hal(
        context,
        handle,
        Some(AidlApi::DemuxOpenFilter),
        "filter child callback rollback failed",
    ));
    cleanup_collector.push_result(rollback_filter_child_open_hal(runtime, handle, filter_id));
    cleanup_collector.into_result()
}

fn cleanup_dvr_child_open_after_object_failure(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    dvr_id: i32,
) -> Result<(), HalError> {
    let mut cleanup_collector = FirstErrorCollector::new();
    cleanup_collector.push_result(clear_owner_callback_registration_hal(
        context,
        handle,
        Some(AidlApi::DemuxOpenDvr),
        "DVR child callback rollback failed",
    ));
    cleanup_collector.push_result(rollback_dvr_child_open_hal(runtime, handle, dvr_id));
    cleanup_collector.into_result()
}

fn retain_filter_child_callback(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    callback: &Strong<dyn IFilterCallback>,
) -> Result<(), HalError> {
    register_callback_artifact_after_owner_ready_hal(
        context,
        handle,
        AidlApi::DemuxOpenFilter,
        || {
            context
                .retain_filter_callback(handle, callback)
                .map_err(|error| error.into_hal_error("filter callback store retain failed"))
        },
    )
}

fn retain_dvr_child_callback(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    callback: &Strong<dyn IDvrCallback>,
) -> Result<(), HalError> {
    register_callback_artifact_after_owner_ready_hal(context, handle, AidlApi::DemuxOpenDvr, || {
        context
            .retain_dvr_callback(handle, callback)
            .map_err(|error| error.into_hal_error("DVR callback store retain failed"))
    })
}

pub fn open_filter_child_for_owner_object_with_request_builder<Build>(
    context: &SharedAidlServiceContext,
    owner_handle: AidlObjectHandle,
    build_request: Build,
    callback: &Strong<dyn IFilterCallback>,
) -> BinderResult<Strong<dyn IFilter>>
where
    Build: FnOnce() -> Result<OpenFilterRequest, maleicacid_tuner_hal2_common::HalError>,
{
    let runtime = context.runtime();
    let runtime_entry = execute_object_method_call_after_live(
        &runtime,
        owner_handle.object_id(),
        owner_handle.generation(),
        owner_handle.object_kind(),
        || {
            let request = build_request()?;
            Ok((
                AidlMethodCall::DemuxOpenFilter(RuntimeExecutableRequest::OpenFilter(
                    request.clone(),
                )),
                request,
            ))
        },
        |runtime, dispatch_proof, request| {
            runtime.open_filter_child_runtime_for_demux_object(
                owner_handle.object_id(),
                owner_handle.generation(),
                &request,
                dispatch_proof,
            )
        },
    )
    .map_err(child_open_txn_error)?;
    finish_filter_child_open(context, &runtime, runtime_entry, callback)
}

pub fn open_dvr_child_for_owner_object_with_request_builder<Build>(
    context: &SharedAidlServiceContext,
    owner_handle: AidlObjectHandle,
    build_request: Build,
    callback: &Strong<dyn IDvrCallback>,
) -> BinderResult<Strong<dyn IDvr>>
where
    Build: FnOnce() -> Result<OpenDvrRequest, maleicacid_tuner_hal2_common::HalError>,
{
    let runtime = context.runtime();
    let runtime_entry = execute_object_method_call_after_live(
        &runtime,
        owner_handle.object_id(),
        owner_handle.generation(),
        owner_handle.object_kind(),
        || {
            let request = build_request()?;
            Ok((AidlMethodCall::DemuxOpenDvr(request.clone()), request))
        },
        |runtime, dispatch_proof, request| {
            runtime.open_dvr_child_runtime_for_demux_object(
                owner_handle.object_id(),
                owner_handle.generation(),
                request,
                dispatch_proof,
            )
        },
    )
    .map_err(child_open_txn_error)?;
    finish_dvr_child_open(context, &runtime, runtime_entry, callback)
}

fn finish_filter_child_open(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    runtime_open: maleicacid_tuner_hal2_service_runtime::FilterChildRuntimeOpen,
    callback: &Strong<dyn IFilterCallback>,
) -> BinderResult<Strong<dyn IFilter>> {
    let child_handle = handle_from_runtime_entry(runtime_open.runtime_entry);
    let filter_id = runtime_open.filter_id;
    if let Err(primary_error) = retain_filter_child_callback(context, child_handle, callback) {
        return finish_hal_cleanup_after_primary(
            "filter child callback retain failure rollback failed",
            primary_error,
            rollback_filter_child_open_hal(runtime, child_handle, filter_id),
        );
    }
    match FilterAidlObject::new(child_handle, context.clone()) {
        Ok(object) => Ok(BnFilter::new_binder(object, BinderFeatures::default())),
        Err(_) => finish_hal_cleanup_after_primary(
            "filter object construction failure cleanup failed",
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter object kind mismatch",
            ),
            cleanup_filter_child_open_after_object_failure(
                context,
                runtime,
                child_handle,
                filter_id,
            ),
        ),
    }
}

fn finish_dvr_child_open(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    runtime_open: maleicacid_tuner_hal2_service_runtime::DvrChildRuntimeOpen,
    callback: &Strong<dyn IDvrCallback>,
) -> BinderResult<Strong<dyn IDvr>> {
    let child_handle = handle_from_runtime_entry(runtime_open.runtime_entry);
    let dvr_id = runtime_open.dvr_id;
    if let Err(primary_error) = retain_dvr_child_callback(context, child_handle, callback) {
        return finish_hal_cleanup_after_primary(
            "DVR child callback retain failure rollback failed",
            primary_error,
            rollback_dvr_child_open_hal(runtime, child_handle, dvr_id),
        );
    }
    match DvrAidlObject::new(child_handle, context.clone()) {
        Ok(object) => Ok(BnDvr::new_binder(object, BinderFeatures::default())),
        Err(_) => finish_hal_cleanup_after_primary(
            "DVR object construction failure cleanup failed",
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR object kind mismatch",
            ),
            cleanup_dvr_child_open_after_object_failure(context, runtime, child_handle, dvr_id),
        ),
    }
}
