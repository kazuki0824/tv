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
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_demux::config::OpenFilterRequest;
use maleicacid_tuner_hal2_service_runtime::{
    ObjectMethodUseCase, ObjectMethodUseCaseBuildError,
};

use crate::dvr_object::DvrAidlObject;
use crate::error_bridge::status_from_hal_error;
use crate::filter_object::FilterAidlObject;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::{
    finish_callback_artifact_registration_after_owner_ready_hal,
    finish_owner_callback_cleanup_outcome,
};
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};

fn handle_from_runtime_entry(
    entry: maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry,
) -> AidlObjectHandle {
    AidlObjectHandle::new(entry.object_kind(), entry.object_id(), entry.generation())
}

fn child_open_txn_error<E>(error: ObjectMethodUseCaseBuildError<E>) -> binder::Status
where
    E: Into<HalError>,
{
    match error {
        ObjectMethodUseCaseBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodUseCaseBuildError::Builder(error) => status_from_hal_error(error.into()),
    }
}

fn finish_filter_child_open_artifact_retain_failure(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    filter_id: i32,
    primary_error: HalError,
) -> BinderResult<Strong<dyn IFilter>> {
    runtime
        .lock()
        .map_err(|_| {
            status_from_hal_error(HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned",
            ))
        })?
        .finish_filter_child_open_artifact_retain_failure_use_case(
            handle.object_id(),
            handle.generation(),
            filter_id,
            primary_error,
        )
        .map_err(status_from_hal_error)?;
    Err(status_from_hal_error(HalError::internal(
        HalInternalKind::InvariantViolation,
        "filter child artifact retain failure unexpectedly returned success",
    )))
}

fn finish_dvr_child_open_artifact_retain_failure(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    dvr_id: i32,
    primary_error: HalError,
) -> BinderResult<Strong<dyn IDvr>> {
    runtime
        .lock()
        .map_err(|_| {
            status_from_hal_error(HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned",
            ))
        })?
        .finish_dvr_child_open_artifact_retain_failure_use_case(
            handle.object_id(),
            handle.generation(),
            dvr_id,
            primary_error,
        )
        .map_err(status_from_hal_error)?;
    Err(status_from_hal_error(HalError::internal(
        HalInternalKind::InvariantViolation,
        "DVR child artifact retain failure unexpectedly returned success",
    )))
}

fn finish_filter_child_object_construction_failure(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    filter_id: i32,
    primary_error: HalError,
) -> BinderResult<Strong<dyn IFilter>> {
    let cleanup =
        cleanup_filter_child_open_after_object_failure(context, runtime, handle, filter_id);
    runtime
        .lock()
        .map_err(|_| {
            status_from_hal_error(HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned",
            ))
        })?
        .finish_filter_child_open_object_construction_failure_use_case(primary_error, cleanup)
        .map_err(status_from_hal_error)?;
    Err(status_from_hal_error(HalError::internal(
        HalInternalKind::InvariantViolation,
        "filter object construction failure unexpectedly returned success",
    )))
}

fn finish_dvr_child_object_construction_failure(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    dvr_id: i32,
    primary_error: HalError,
) -> BinderResult<Strong<dyn IDvr>> {
    let cleanup = cleanup_dvr_child_open_after_object_failure(context, runtime, handle, dvr_id);
    runtime
        .lock()
        .map_err(|_| {
            status_from_hal_error(HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned",
            ))
        })?
        .finish_dvr_child_open_object_construction_failure_use_case(primary_error, cleanup)
        .map_err(status_from_hal_error)?;
    Err(status_from_hal_error(HalError::internal(
        HalInternalKind::InvariantViolation,
        "DVR object construction failure unexpectedly returned success",
    )))
}

fn cleanup_filter_child_open_after_object_failure(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    filter_id: i32,
) -> Result<(), HalError> {
    let outcome = runtime
        .lock()
        .map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned",
            )
        })?
        .begin_filter_child_open_object_failure_cleanup_use_case(
            handle.object_id(),
            handle.generation(),
            filter_id,
        );
    finish_owner_callback_cleanup_outcome(context, outcome)
}

fn cleanup_dvr_child_open_after_object_failure(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    dvr_id: i32,
) -> Result<(), HalError> {
    let outcome = runtime
        .lock()
        .map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned",
            )
        })?
        .begin_dvr_child_open_object_failure_cleanup_use_case(
            handle.object_id(),
            handle.generation(),
            dvr_id,
        );
    finish_owner_callback_cleanup_outcome(context, outcome)
}

fn retain_filter_child_callback(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    callback: &Strong<dyn IFilterCallback>,
) -> Result<(), HalError> {
    let retain_result = context
        .retain_filter_callback(handle, callback)
        .map_err(|error| error.into_hal_error("filter callback store retain failed"));
    finish_callback_artifact_registration_after_owner_ready_hal(
        context,
        handle,
        AidlApi::DemuxOpenFilter,
        retain_result,
    )
}

fn retain_dvr_child_callback(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    callback: &Strong<dyn IDvrCallback>,
) -> Result<(), HalError> {
    let retain_result = context
        .retain_dvr_callback(handle, callback)
        .map_err(|error| error.into_hal_error("DVR callback store retain failed"));
    finish_callback_artifact_registration_after_owner_ready_hal(
        context,
        handle,
        AidlApi::DemuxOpenDvr,
        retain_result,
    )
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
    let runtime_entry = ObjectMethodUseCase::execute_after_live(
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
            runtime.child_open_txn().open_filter_child_runtime_for_demux_object(
                owner_handle.object_id(),
                owner_handle.generation(),
                &request,
                dispatch_proof,
            )
        },
    )
    .map_err(child_open_txn_error::<maleicacid_tuner_hal2_common::HalError>)?;
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
    let runtime_entry = ObjectMethodUseCase::execute_after_live(
        &runtime,
        owner_handle.object_id(),
        owner_handle.generation(),
        owner_handle.object_kind(),
        || {
            let request = build_request()?;
            Ok((AidlMethodCall::DemuxOpenDvr(request.clone()), request))
        },
        |runtime, dispatch_proof, request| {
            runtime.child_open_txn().open_dvr_child_runtime_for_demux_object(
                owner_handle.object_id(),
                owner_handle.generation(),
                request,
                dispatch_proof,
            )
        },
    )
    .map_err(child_open_txn_error::<maleicacid_tuner_hal2_common::HalError>)?;
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
    let artifact = match context.prepare_filter_callback_artifact(child_handle, callback) {
        Ok(token) => token,
        Err(primary_error) => {
            return finish_filter_child_open_artifact_retain_failure(
                runtime, child_handle, filter_id, primary_error,
            )
        }
    };
    let object = match FilterAidlObject::new(child_handle, context.clone()) {
        Ok(object) => BnFilter::new_binder(object, BinderFeatures::default()),
        Err(_) => {
            let _ = context.abort_child_callback_artifact(child_handle, AidlApi::DemuxOpenFilter, artifact);
            return finish_filter_child_object_construction_failure(
                context,
                runtime,
                child_handle,
                filter_id,
                HalError::internal(HalInternalKind::InvariantViolation, "filter object kind mismatch"),
            );
        }
    };
    if let Err(primary) = context.commit_child_callback_artifact(
        child_handle,
        AidlApi::DemuxOpenFilter,
        artifact,
    ) {
        return finish_filter_child_open_artifact_retain_failure(runtime, child_handle, filter_id, primary);
    }
    if let Err(primary) = runtime
        .lock()
        .map_err(|_| status_from_hal_error(HalError::internal(HalInternalKind::InvariantViolation, "service runtime lock poisoned")))?
        .commit_prepared_child_object(child_handle.object_id(), child_handle.generation())
    {
        let _ = context.clear_owner_callbacks(child_handle);
        return finish_filter_child_object_construction_failure(
            context, runtime, child_handle, filter_id, primary,
        );
    }
    Ok(object)
}

fn finish_dvr_child_open(
    context: &SharedAidlServiceContext,
    runtime: &SharedTunerRuntime,
    runtime_open: maleicacid_tuner_hal2_service_runtime::DvrChildRuntimeOpen,
    callback: &Strong<dyn IDvrCallback>,
) -> BinderResult<Strong<dyn IDvr>> {
    let child_handle = handle_from_runtime_entry(runtime_open.runtime_entry);
    let dvr_id = runtime_open.dvr_id;
    let artifact = match context.prepare_dvr_callback_artifact(child_handle, callback) {
        Ok(token) => token,
        Err(primary_error) => {
            return finish_dvr_child_open_artifact_retain_failure(runtime, child_handle, dvr_id, primary_error)
        }
    };
    let object = match DvrAidlObject::new(child_handle, context.clone()) {
        Ok(object) => BnDvr::new_binder(object, BinderFeatures::default()),
        Err(_) => {
            let _ = context.abort_child_callback_artifact(child_handle, AidlApi::DemuxOpenDvr, artifact);
            return finish_dvr_child_object_construction_failure(
                context,
                runtime,
                child_handle,
                dvr_id,
                HalError::internal(HalInternalKind::InvariantViolation, "DVR object kind mismatch"),
            );
        }
    };
    if let Err(primary) = context.commit_child_callback_artifact(
        child_handle,
        AidlApi::DemuxOpenDvr,
        artifact,
    ) {
        return finish_dvr_child_open_artifact_retain_failure(runtime, child_handle, dvr_id, primary);
    }
    if let Err(primary) = runtime
        .lock()
        .map_err(|_| status_from_hal_error(HalError::internal(HalInternalKind::InvariantViolation, "service runtime lock poisoned")))?
        .commit_prepared_child_object(child_handle.object_id(), child_handle.generation())
    {
        let _ = context.clear_owner_callbacks(child_handle);
        return finish_dvr_child_object_construction_failure(context, runtime, child_handle, dvr_id, primary);
    }
    Ok(object)
}

