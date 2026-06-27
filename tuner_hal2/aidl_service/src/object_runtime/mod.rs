use binder::Result as BinderResult;
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlFailureSource, AidlMethodCall, AidlObjectKind, AidlStatusMapper, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FirstErrorCollector, HalError, HalInternalKind,
};
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
use maleicacid_tuner_hal2_service_runtime::{
    object_close_txn::{
        commit_object_close_cascade, mark_object_close_cleanup_failed_cascade,
        object_close_cascade_entries, plan_and_begin_object_close_method_call_dispatch,
        quarantine_object_cascade,
    },
    object_lifecycle::{
        aidl_object_live, lnb_public_id_for_live_object_result, AidlObjectCloseability,
    },
    object_method_txn::{
        execute_object_method_call_after_live, execute_object_query_call_after_live,
        execute_object_query_call_after_live_with_aidl_input_conversion,
        execute_shared_object_method_call_after_live, preflight_object_method_after_live_plan_only,
        ObjectMethodExecutionToken, ObjectMethodTxnBuildError, ObjectQueryRequest,
        ObjectQueryResponse,
    },
    CallbackRegistryUpdate, TunerServiceRuntime,
};

use crate::callback_store::AidlCallbackStoreError;
use crate::dvr_callback_delivery::stop_dvr_status_notifier;
use crate::error_bridge::{status_from_hal_error, status_from_tuner_status, status_unknown_error};
use crate::object_handle::AidlObjectHandle;
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};

fn callback_store_error_to_hal(error: AidlCallbackStoreError, context: &'static str) -> HalError {
    error.into_hal_error(context)
}

fn lock_runtime<'a>(
    runtime: &'a SharedTunerRuntime,
) -> Result<std::sync::MutexGuard<'a, TunerServiceRuntime>, HalError> {
    runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned",
        )
    })
}

fn clear_runtime_callback_owner_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let mut runtime = lock_runtime(runtime)?;
    match runtime.clear_callback_registration_owner(handle.object_id(), handle.generation()) {
        CallbackRegistryUpdate::Updated => Ok(()),
        CallbackRegistryUpdate::Missing => Err(HalError::cleanup_failed(
            "callback registry owner clear",
            "callback registry owner missing while clearing callback registration",
        )),
    }
}

fn clear_retained_callback_artifact_hal(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    failure_message: &'static str,
) -> Result<(), HalError> {
    context
        .clear_owner_callbacks(handle)
        .map(|_| ())
        .map_err(|error| callback_store_error_to_hal(error, failure_message))
}

fn mark_runtime_callback_unhealthy_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    api: AidlApi,
) -> Result<(), HalError> {
    let mut runtime = lock_runtime(runtime)?;
    match runtime.mark_callback_registration_unhealthy(
        handle.object_kind(),
        handle.object_id(),
        handle.generation(),
        api,
    ) {
        CallbackRegistryUpdate::Updated => Ok(()),
        CallbackRegistryUpdate::Missing => Err(HalError::cleanup_failed(
            "callback registry unhealthy marking",
            "callback registry entry missing while marking unhealthy",
        )),
    }
}

fn mark_runtime_callback_owner_unhealthy_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let mut runtime = lock_runtime(runtime)?;
    match runtime
        .mark_callback_registration_owner_unhealthy(handle.object_id(), handle.generation())
    {
        CallbackRegistryUpdate::Updated => Ok(()),
        CallbackRegistryUpdate::Missing => Err(HalError::cleanup_failed(
            "callback registry owner unhealthy marking",
            "callback registry owner missing while marking unhealthy",
        )),
    }
}

pub(crate) fn clear_owner_callback_registration_hal(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    api: Option<AidlApi>,
    failure_message: &'static str,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    match context.clear_owner_callbacks(handle) {
        Ok(_) => clear_runtime_callback_owner_hal(&runtime, handle),
        Err(error) => {
            let cleanup_error = callback_store_error_to_hal(error, failure_message);
            match api {
                Some(api) => {
                    if let Err(mark_error) =
                        mark_runtime_callback_unhealthy_hal(&runtime, handle, api)
                    {
                        return Err(compose_primary_cleanup_failure(
                            "callback cleanup failed and unhealthy marking failed",
                            cleanup_error,
                            mark_error,
                        ));
                    }
                }
                None => {
                    if let Err(mark_error) =
                        mark_runtime_callback_owner_unhealthy_hal(&runtime, handle)
                    {
                        return Err(compose_primary_cleanup_failure(
                            "callback cleanup failed and owner unhealthy marking failed",
                            cleanup_error,
                            mark_error,
                        ));
                    }
                }
            }
            Err(cleanup_error)
        }
    }
}

pub(crate) fn register_callback_artifact_after_owner_ready_hal<Retain>(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    api: AidlApi,
    retain: Retain,
) -> Result<(), HalError>
where
    Retain: FnOnce() -> Result<(), HalError>,
{
    let runtime = context.runtime();
    retain()?;
    if let Err(primary_error) = record_callback_registration(&runtime, handle, api) {
        if let Err(rollback_error) = clear_retained_callback_artifact_hal(
            context,
            handle,
            "callback artifact rollback failed before runtime registration",
        ) {
            return Err(compose_primary_cleanup_failure(
                "callback artifact registration rollback failed",
                primary_error,
                rollback_error,
            ));
        }
        return Err(primary_error);
    }
    Ok(())
}

#[cfg(test)]
pub fn register_callback_artifact_after_owner_ready<Retain>(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    api: AidlApi,
    retain: Retain,
) -> BinderResult<()>
where
    Retain: FnOnce() -> Result<(), HalError>,
{
    register_callback_artifact_after_owner_ready_hal(context, handle, api, retain)
        .map_err(status_from_hal_error)
}

pub fn execute_object_runtime_use_case<T, F>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
    execute: F,
) -> BinderResult<T>
where
    F: FnOnce(
        &mut TunerServiceRuntime,
        AidlObjectHandle,
        ObjectMethodExecutionToken,
    ) -> Result<T, HalError>,
{
    execute_object_method_call_after_live(
        runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        || Ok((method, ())),
        |runtime, token, ()| execute(runtime, handle, token),
    )
    .map_err(|error| match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(status) => status,
    })
}

pub fn execute_shared_object_runtime_use_case<T, F>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
    execute: F,
) -> BinderResult<T>
where
    F: FnOnce(
        SharedTunerRuntime,
        AidlObjectHandle,
        ObjectMethodExecutionToken,
    ) -> Result<T, HalError>,
{
    execute_shared_object_method_call_after_live(
        runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        || Ok((method, ())),
        |runtime, token, ()| execute(runtime, handle, token),
    )
    .map_err(|error| match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(status) => status,
    })
}

pub fn execute_object_query_use_case(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    request: ObjectQueryRequest,
) -> BinderResult<ObjectQueryResponse> {
    execute_object_query_call_after_live(
        runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        request,
    )
    .map_err(status_from_hal_error)
}

pub fn execute_object_query_use_case_with_aidl_input_conversion<Build>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
    build: Build,
) -> BinderResult<ObjectQueryResponse>
where
    Build: FnOnce() -> BinderResult<ObjectQueryRequest>,
{
    execute_object_query_call_after_live_with_aidl_input_conversion(
        runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        method,
        build,
    )
    .map_err(|error| match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(status) => status,
    })
}

pub fn execute_object_runtime_use_case_with_request_builder<T, B, Build, Execute>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    build: Build,
    execute: Execute,
) -> BinderResult<T>
where
    Build: FnOnce() -> BinderResult<(AidlMethodCall, B)>,
    Execute: FnOnce(
        &mut TunerServiceRuntime,
        AidlObjectHandle,
        ObjectMethodExecutionToken,
        B,
    ) -> Result<T, HalError>,
{
    execute_object_method_call_after_live(
        runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        || build(),
        |runtime, token, built_request| execute(runtime, handle, token, built_request),
    )
    .map_err(|error| match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(status) => status,
    })
}

pub fn execute_shared_object_runtime_use_case_with_request_builder<T, B, Build, Execute>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    build: Build,
    execute: Execute,
) -> BinderResult<T>
where
    Build: FnOnce() -> BinderResult<(AidlMethodCall, B)>,
    Execute: FnOnce(
        SharedTunerRuntime,
        AidlObjectHandle,
        ObjectMethodExecutionToken,
        B,
    ) -> Result<T, HalError>,
{
    execute_shared_object_method_call_after_live(
        runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        || build(),
        |runtime, token, built_request| execute(runtime, handle, token, built_request),
    )
    .map_err(|error| match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(status) => status,
    })
}

pub fn plan_unavailable_object_method_use_case<Build>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    build: Build,
    message: &'static str,
) -> BinderResult<()>
where
    Build: FnOnce() -> BinderResult<AidlMethodCall>,
{
    let api = preflight_object_method_after_live_plan_only(
        runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        || build(),
    )
    .map_err(|error| match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(status) => status,
    })?;
    let failure = AidlFailureSource::RuntimeDispatch(HalError::Unsupported(message));
    let failures = [failure];
    let status = AidlStatusMapper::resolve_failure_by_precedence(api, &failures, false)
        .unwrap_or(TunerStatusCode::UnknownError);
    Err(status_from_tuner_status(status, message))
}

pub fn execute_callback_registration_runtime_use_case<T, Retain, Rollback, Execute>(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
    retain: Retain,
    rollback: Rollback,
    execute: Execute,
) -> BinderResult<T>
where
    Retain: FnOnce() -> Result<(), HalError>,
    Rollback: FnMut() -> Result<(), HalError>,
    Execute: FnOnce(
        &mut TunerServiceRuntime,
        AidlObjectHandle,
        ObjectMethodExecutionToken,
    ) -> Result<T, HalError>,
{
    let runtime = context.runtime();
    let api = method.api();
    let mut callback_retained = false;
    let mut rollback = rollback;
    let result = execute_object_method_call_after_live(
        &runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        || Ok((method, ())),
        |runtime, dispatch_proof, ()| {
            retain()?;
            callback_retained = true;
            runtime.record_callback_registration_for_object(
                handle.object_kind(),
                handle.object_id(),
                handle.generation(),
                api,
            );
            execute(runtime, handle, dispatch_proof)
        },
    );
    match result {
        Ok(value) => Ok(value),
        Err(ObjectMethodTxnBuildError::Builder(status)) => Err(status),
        Err(ObjectMethodTxnBuildError::Runtime(primary_error)) if callback_retained => {
            if let Err(rollback_error) = rollback() {
                let cleanup_error = match mark_runtime_callback_unhealthy_hal(&runtime, handle, api)
                {
                    Ok(()) => rollback_error,
                    Err(mark_error) => compose_primary_cleanup_failure(
                        "callback rollback unhealthy marking failed",
                        rollback_error,
                        mark_error,
                    ),
                };
                return Err(status_from_hal_error(compose_primary_cleanup_failure(
                    "callback registration domain failure rollback failed",
                    primary_error,
                    cleanup_error,
                )));
            }
            Err(status_from_hal_error(primary_error))
        }
        Err(ObjectMethodTxnBuildError::Runtime(error)) => Err(status_from_hal_error(error)),
    }
}

fn entry_requires_public_runtime_unregister(
    entry: &maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry,
) -> bool {
    matches!(
        entry.object_kind,
        AidlObjectKind::Demux
            | AidlObjectKind::Filter
            | AidlObjectKind::Dvr
            | AidlObjectKind::Descrambler
    )
}

fn unregister_public_runtime_entries(
    runtime: &mut TunerServiceRuntime,
    entries: &[maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry],
) -> Result<(), HalError> {
    let public_runtime_entries = entries
        .iter()
        .filter(|entry| entry_requires_public_runtime_unregister(entry))
        .collect::<Vec<_>>();
    let mut preflight_collector = FirstErrorCollector::new();
    for entry in &public_runtime_entries {
        preflight_collector
            .push_result(runtime.validate_public_runtime_for_closed_aidl_entry(entry));
    }
    preflight_collector.into_result()?;

    let mut cleanup_collector = FirstErrorCollector::new();
    for entry in public_runtime_entries {
        cleanup_collector
            .push_result(runtime.unregister_public_runtime_for_closed_aidl_entry(entry));
    }
    cleanup_collector.into_result()
}

fn unregister_quarantined_public_runtime_entries(
    runtime: &mut TunerServiceRuntime,
    entries: &[maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry],
) -> Result<(), HalError> {
    let public_runtime_entries = entries
        .iter()
        .filter(|entry| entry_requires_public_runtime_unregister(entry))
        .collect::<Vec<_>>();
    let mut preflight_collector = FirstErrorCollector::new();
    for entry in &public_runtime_entries {
        preflight_collector
            .push_result(runtime.validate_public_runtime_for_drop_leak_aidl_entry(entry));
    }
    preflight_collector.into_result()?;

    let mut cleanup_collector = FirstErrorCollector::new();
    for entry in public_runtime_entries {
        cleanup_collector
            .push_result(runtime.unregister_public_runtime_for_drop_leak_aidl_entry(entry));
    }
    cleanup_collector.into_result()
}

fn handle_from_runtime_entry(
    entry: &maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry,
) -> AidlObjectHandle {
    AidlObjectHandle::new(entry.object_kind, entry.object_id, entry.generation)
}

fn run_pre_finalization_close_cleanup<F>(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    domain_cleanup: F,
) -> Result<(), (CleanupStep, HalError)>
where
    F: FnOnce() -> Result<(), HalError>,
{
    let mut cleanup_collector = FirstErrorCollector::new();
    if let Err(error) = clear_owner_callback_registration_hal(
        context,
        handle,
        None,
        "callback store cleanup failed during AIDL object close",
    ) {
        cleanup_collector.push_error((CleanupStep::UnregisterRuntime, error));
    }
    if let Err(error) = domain_cleanup() {
        cleanup_collector.push_error((CleanupStep::ReleaseBackend, error));
    }
    cleanup_collector.into_result()
}

fn cleanup_cascade_descendant_owner_artifacts(
    context: &SharedAidlServiceContext,
    root_handle: AidlObjectHandle,
    entries: &[maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry],
) -> Result<(), (CleanupStep, HalError)> {
    let mut cleanup_collector = FirstErrorCollector::new();
    for entry in entries {
        if entry.object_id == root_handle.object_id()
            && entry.generation == root_handle.generation()
        {
            continue;
        }
        let handle = handle_from_runtime_entry(entry);
        if let Err(error) = clear_owner_callback_registration_hal(
            context,
            handle,
            None,
            "callback store cleanup failed during AIDL close cascade",
        ) {
            cleanup_collector.push_error((CleanupStep::UnregisterRuntime, error));
        }
        if entry.object_kind == AidlObjectKind::Dvr {
            if let Err(error) = stop_dvr_status_notifier(context, handle) {
                cleanup_collector.push_error((CleanupStep::StopWorker, error));
            }
        }
    }
    cleanup_collector.into_result()
}

fn mark_close_cleanup_failed_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    step: CleanupStep,
    detail: &'static str,
) -> Result<(), HalError> {
    let mut guard = lock_runtime(runtime)?;
    mark_object_close_cleanup_failed_cascade(
        &mut guard,
        handle.object_id(),
        handle.generation(),
        step,
        detail,
    )
}

fn finish_object_close_after_begin_with_domain_cleanup<F>(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    domain_cleanup: F,
) -> BinderResult<()>
where
    F: FnOnce() -> Result<(), HalError>,
{
    let runtime = context.runtime();
    if let Err((cleanup_step, cleanup_error)) =
        run_pre_finalization_close_cleanup(context, handle, domain_cleanup)
    {
        return match mark_close_cleanup_failed_hal(
            &runtime,
            handle,
            cleanup_step,
            "AIDL object close cleanup failure could not be recorded",
        ) {
            Ok(()) => Err(status_from_hal_error(cleanup_error)),
            Err(mark_error) => Err(status_from_hal_error(compose_primary_cleanup_failure(
                "AIDL object close cleanup failed and cleanup-failed marking failed",
                cleanup_error,
                mark_error,
            ))),
        };
    }

    let closing_entries = {
        let mut guard = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
        match object_close_cascade_entries(&guard, handle.object_id(), handle.generation()) {
            Ok(entries) => entries,
            Err(cleanup_error) => {
                return mark_close_finalization_failed(
                    &mut guard,
                    handle,
                    CleanupStep::ReleaseLedger,
                    cleanup_error,
                );
            }
        }
    };

    if let Err((cleanup_step, cleanup_error)) =
        cleanup_cascade_descendant_owner_artifacts(context, handle, &closing_entries)
    {
        return match mark_close_cleanup_failed_hal(
            &runtime,
            handle,
            cleanup_step,
            "AIDL close cascade descendant cleanup failure could not be recorded",
        ) {
            Ok(()) => Err(status_from_hal_error(cleanup_error)),
            Err(mark_error) => Err(status_from_hal_error(compose_primary_cleanup_failure(
                "AIDL close cascade descendant cleanup failed and cleanup-failed marking failed",
                cleanup_error,
                mark_error,
            ))),
        };
    }

    let mut guard = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    if let Err(cleanup_error) = unregister_public_runtime_entries(&mut guard, &closing_entries) {
        return mark_close_finalization_failed(
            &mut guard,
            handle,
            CleanupStep::UnregisterRuntime,
            cleanup_error,
        );
    }
    if let Err(cleanup_error) =
        commit_object_close_cascade(&mut guard, handle.object_id(), handle.generation()).map(|_| ())
    {
        return mark_close_finalization_failed(
            &mut guard,
            handle,
            CleanupStep::ReleaseLedger,
            cleanup_error,
        );
    }
    Ok(())
}

fn mark_close_finalization_failed(
    runtime: &mut TunerServiceRuntime,
    handle: AidlObjectHandle,
    step: CleanupStep,
    cleanup_error: HalError,
) -> BinderResult<()> {
    match mark_object_close_cleanup_failed_cascade(
        runtime,
        handle.object_id(),
        handle.generation(),
        step,
        "AIDL object close finalization failure could not be recorded",
    ) {
        Ok(()) => Err(status_from_hal_error(cleanup_error)),
        Err(mark_error) => Err(status_from_hal_error(compose_primary_cleanup_failure(
            "AIDL object close finalization failed and cleanup-failed marking failed",
            cleanup_error,
            mark_error,
        ))),
    }
}

mod drop_leak;
pub use drop_leak::{drop_leak_object_from_drop, DropLeakDomainAction};

pub fn record_callback_registration(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    api: AidlApi,
) -> Result<(), HalError> {
    let mut runtime = lock_runtime(runtime)?;
    aidl_object_live(
        &runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
    )?;
    runtime.record_callback_registration_for_object(
        handle.object_kind(),
        handle.object_id(),
        handle.generation(),
        api,
    );
    Ok(())
}

pub fn close_object_after_close_preflight_with_domain_cleanup<F>(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
    domain_cleanup: F,
) -> BinderResult<()>
where
    F: FnOnce() -> Result<(), HalError>,
{
    let runtime = context.runtime();
    {
        let mut runtime = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
        match plan_and_begin_object_close_method_call_dispatch(
            &mut runtime,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            method,
            CleanupStep::ReleaseBackend,
        )
        .map_err(status_from_hal_error)?
        {
            AidlObjectCloseability::BeginClose => {}
            AidlObjectCloseability::AlreadyClosed => return Ok(()),
        }
    }
    finish_object_close_after_begin_with_domain_cleanup(context, handle, domain_cleanup)
}

pub fn close_object_after_close_preflight(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
) -> BinderResult<()> {
    close_object_after_close_preflight_with_domain_cleanup(context, handle, method, || Ok(()))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::drop_leak::drop_leak_object;
    use super::*;
    use crate::service_context::AidlServiceContext;
    use maleicacid_tuner_hal2_binder_adapter::{
        AidlApi, AidlMethodCall, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
    };
    use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
    use maleicacid_tuner_hal2_service_runtime::{
        object_close_txn::begin_object_close_cascade, CallbackHealthState, RuntimeObjectLifecycle,
        RuntimeOwnerRelation,
    };

    fn shared_runtime_with_live_object(
        kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        public_runtime_id: i64,
    ) -> SharedTunerRuntime {
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        runtime
            .lock()
            .unwrap()
            .register_aidl_object_for_runtime(
                kind,
                object_id,
                generation,
                public_runtime_id,
                RuntimeOwnerRelation::Root,
            )
            .unwrap();
        runtime
    }

    fn context_for_runtime(runtime: &SharedTunerRuntime) -> SharedAidlServiceContext {
        AidlServiceContext::from_shared_runtime_for_test(runtime.clone())
    }

    fn retain_test_callback_marker_as_hal(
        context: &SharedAidlServiceContext,
        handle: AidlObjectHandle,
        api: AidlApi,
    ) -> Result<(), HalError> {
        context
            .retain_test_callback_marker(handle, api)
            .map_err(|error| error.into_hal_error("test callback marker retain failed"))
    }

    #[test]
    fn query_use_case_rejects_missing_object_before_execute() {
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_010),
            AidlObjectGeneration(1),
        );
        let result =
            execute_object_query_use_case(&runtime, handle, ObjectQueryRequest::FilterGetId);
        assert!(result.is_err());
    }

    #[test]
    fn drop_leak_reports_runtime_lock_poison() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_000),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_000),
            AidlObjectGeneration(1),
            91_000,
        );
        let poisoned = runtime.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poisoned.lock().unwrap();
            panic!("poison runtime for drop leak test");
        }));

        assert!(drop_leak_object(
            &context_for_runtime(&runtime),
            handle,
            DropLeakDomainAction::None
        )
        .is_err());
    }

    #[test]
    fn drop_leak_registry_missing_is_reported_after_quarantine() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_011),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_011),
            AidlObjectGeneration(1),
            91_011,
        );

        let result = drop_leak_object(
            &context_for_runtime(&runtime),
            handle,
            DropLeakDomainAction::RecordLnbDropLeak,
        );

        assert!(result.is_err());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime.aidl_object_lifecycle(AidlObjectId(91_011)).unwrap(),
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn drop_leak_from_drop_records_returned_error() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_012),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_012),
            AidlObjectGeneration(1),
            91_012,
        );
        let context = context_for_runtime(&runtime);
        let before = context.drop_leak_error_record_count();

        drop_leak_object_from_drop(&context, handle, DropLeakDomainAction::RecordLnbDropLeak);

        assert_eq!(context.drop_leak_error_record_count(), before + 1);
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime.aidl_object_lifecycle(AidlObjectId(91_012)).unwrap(),
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn drop_leak_diagnostic_records_are_bounded_and_dropped_count_is_observable() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_013),
            AidlObjectGeneration(1),
        );
        let context = AidlServiceContext::shared(TunerServiceRuntime::new());
        let status = status_unknown_error("drop leak diagnostic bounded store test");

        for _ in 0..70 {
            context.record_drop_leak_error(handle, &status);
        }

        assert_eq!(context.drop_leak_error_record_count(), 64);
        assert!(context.drop_leak_error_records_dropped_count() > 0);
        assert_eq!(context.drop_leak_error_record_failure_count(), 0);
    }

    #[test]
    fn drop_leak_clears_runtime_callback_registration_and_quarantines_object() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_001),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_001),
            AidlObjectGeneration(1),
            91_001,
        );
        let context = context_for_runtime(&runtime);
        context.clear_owner_callbacks(handle).unwrap();
        context
            .retain_test_callback_marker(handle, AidlApi::DemuxOpenFilter)
            .unwrap();
        record_callback_registration(&runtime, handle, AidlApi::DemuxOpenFilter).unwrap();

        drop_leak_object(&context, handle, DropLeakDomainAction::None).unwrap();

        let runtime = runtime.lock().unwrap();
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::DemuxOpenFilter)
            .unwrap());
        assert_eq!(runtime.callback_registration_count(), 0);
        assert_eq!(
            runtime.aidl_object_lifecycle(AidlObjectId(91_001)).unwrap(),
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn drop_leak_demux_clears_descendant_filter_callback_and_quarantines_descendant() {
        let demux_handle = AidlObjectHandle::new(
            AidlObjectKind::Demux,
            AidlObjectId(91_020),
            AidlObjectGeneration(1),
        );
        let filter_handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_021),
            AidlObjectGeneration(1),
        );
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        {
            let mut guard = runtime.lock().unwrap();
            guard
                .register_aidl_object_for_runtime(
                    AidlObjectKind::Demux,
                    demux_handle.object_id(),
                    demux_handle.generation(),
                    91_020,
                    RuntimeOwnerRelation::Root,
                )
                .unwrap();
            guard
                .register_aidl_object_for_runtime(
                    AidlObjectKind::Filter,
                    filter_handle.object_id(),
                    filter_handle.generation(),
                    91_021,
                    RuntimeOwnerRelation::Demux {
                        demux: demux_handle.object_id(),
                        generation: demux_handle.generation(),
                    },
                )
                .unwrap();
        }
        let context = context_for_runtime(&runtime);
        context
            .retain_test_callback_marker(filter_handle, AidlApi::DemuxOpenFilter)
            .unwrap();
        record_callback_registration(&runtime, filter_handle, AidlApi::DemuxOpenFilter).unwrap();

        let result = drop_leak_object(&context, demux_handle, DropLeakDomainAction::None);

        assert!(result.is_err());
        let runtime = runtime.lock().unwrap();
        assert!(!context
            .has_callback_for_owner(filter_handle, AidlApi::DemuxOpenFilter)
            .unwrap());
        assert!(runtime
            .callback_registration_health(
                AidlObjectKind::Filter,
                filter_handle.object_id(),
                filter_handle.generation(),
                AidlApi::DemuxOpenFilter,
            )
            .is_none());
        assert_eq!(
            runtime
                .aidl_object_lifecycle(demux_handle.object_id())
                .unwrap(),
            RuntimeObjectLifecycle::Quarantined
        );
        assert_eq!(
            runtime
                .aidl_object_lifecycle(filter_handle.object_id())
                .unwrap(),
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn drop_leak_returns_error_without_marking_callback_unhealthy_when_domain_drop_record_fails() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_002),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_002),
            AidlObjectGeneration(1),
            91_002,
        );
        let context = context_for_runtime(&runtime);
        context.clear_owner_callbacks(handle).unwrap();
        context
            .retain_test_callback_marker(handle, AidlApi::LnbSetCallback)
            .unwrap();
        record_callback_registration(&runtime, handle, AidlApi::LnbSetCallback).unwrap();

        assert!(
            drop_leak_object(&context, handle, DropLeakDomainAction::RecordLnbDropLeak).is_err()
        );

        let runtime = runtime.lock().unwrap();
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::LnbSetCallback)
            .unwrap());
        assert!(runtime
            .callback_registration_health(
                AidlObjectKind::Lnb,
                AidlObjectId(91_002),
                AidlObjectGeneration(1),
                AidlApi::LnbSetCallback,
            )
            .is_none());
        assert_eq!(
            runtime.aidl_object_lifecycle(AidlObjectId(91_002)).unwrap(),
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn close_object_after_preflight_runs_hook_after_single_begin_and_commits_closed() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Demux,
            AidlObjectId(91_003),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Demux,
            AidlObjectId(91_003),
            AidlObjectGeneration(1),
            91_003,
        );
        let runtime_for_hook = runtime.clone();
        let hook_called = Arc::new(Mutex::new(false));
        let hook_called_for_hook = hook_called.clone();

        close_object_after_close_preflight_with_domain_cleanup(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::DemuxClose,
            || {
                let guard = runtime_for_hook.lock().unwrap();
                assert_eq!(
                    guard.aidl_object_lifecycle(AidlObjectId(91_003)).unwrap(),
                    RuntimeObjectLifecycle::Closing {
                        step: CleanupStep::ReleaseBackend
                    }
                );
                drop(guard);
                *hook_called_for_hook.lock().unwrap() = true;
                Ok(())
            },
        )
        .expect("close succeeds");

        assert!(*hook_called.lock().unwrap());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime.aidl_object_lifecycle(AidlObjectId(91_003)).unwrap(),
            RuntimeObjectLifecycle::Closed
        );
    }

    #[test]
    fn close_object_after_close_preflight_begins_closing_before_domain_cleanup() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_009),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_009),
            AidlObjectGeneration(1),
            91_009,
        );
        let runtime_for_hook = runtime.clone();
        let hook_called = Arc::new(Mutex::new(false));
        let hook_called_for_hook = hook_called.clone();

        close_object_after_close_preflight_with_domain_cleanup(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::FilterClose,
            || {
                let guard = runtime_for_hook.lock().unwrap();
                assert_eq!(
                    guard.aidl_object_lifecycle(AidlObjectId(91_009)).unwrap(),
                    RuntimeObjectLifecycle::Closing {
                        step: CleanupStep::ReleaseBackend
                    }
                );
                drop(guard);
                *hook_called_for_hook.lock().unwrap() = true;
                Ok(())
            },
        )
        .expect("close succeeds");

        assert!(*hook_called.lock().unwrap());
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .aidl_object_lifecycle(AidlObjectId(91_009))
                .unwrap(),
            RuntimeObjectLifecycle::Closed
        );
    }

    #[test]
    fn execute_runtime_request_builder_runs_under_lifecycle_lock() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_010),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_010),
            AidlObjectGeneration(1),
            91_010,
        );
        let runtime_for_builder = runtime.clone();
        let builder_saw_lock = Arc::new(Mutex::new(false));
        let builder_saw_lock_for_builder = builder_saw_lock.clone();

        execute_object_runtime_use_case_with_request_builder(
            &runtime,
            handle,
            || {
                assert!(runtime_for_builder.try_lock().is_err());
                *builder_saw_lock_for_builder.lock().unwrap() = true;
                Ok((AidlMethodCall::FilterGetId, ()))
            },
            |_runtime, _handle, _dispatch_proof, ()| Ok(()),
        )
        .expect("request-builder method succeeds");

        assert!(*builder_saw_lock.lock().unwrap());
    }

    #[test]
    fn execute_runtime_request_builder_checks_lifecycle_before_builder() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_011),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_011),
            AidlObjectGeneration(1),
            91_011,
        );
        {
            let mut guard = runtime.lock().unwrap();
            begin_object_close_cascade(
                &mut guard,
                AidlObjectId(91_011),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .unwrap();
            commit_object_close_cascade(&mut guard, AidlObjectId(91_011), AidlObjectGeneration(1))
                .unwrap();
        }
        let builder_called = Arc::new(Mutex::new(false));
        let builder_called_for_builder = builder_called.clone();

        let result: BinderResult<()> = execute_object_runtime_use_case_with_request_builder(
            &runtime,
            handle,
            || {
                *builder_called_for_builder.lock().unwrap() = true;
                Ok((AidlMethodCall::FilterGetId, ()))
            },
            |_runtime, _handle, _dispatch_proof, ()| Ok(()),
        );

        assert!(result.is_err());
        assert!(!*builder_called.lock().unwrap());
    }

    #[test]
    fn close_object_after_preflight_marks_cleanup_failed_when_domain_cleanup_fails() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_004),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_004),
            AidlObjectGeneration(1),
            91_004,
        );

        let result = close_object_after_close_preflight_with_domain_cleanup(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::FilterClose,
            || {
                Err(HalError::cleanup_failed(
                    "test domain cleanup",
                    "domain cleanup failed for test",
                ))
            },
        );

        assert!(result.is_err());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime.aidl_object_lifecycle(AidlObjectId(91_004)).unwrap(),
            RuntimeObjectLifecycle::CleanupFailed {
                step: CleanupStep::ReleaseBackend
            }
        );
    }

    #[test]
    fn close_object_after_preflight_rejects_domain_cleanup_second_begin() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Descrambler,
            AidlObjectId(91_005),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Descrambler,
            AidlObjectId(91_005),
            AidlObjectGeneration(1),
            91_005,
        );
        let runtime_for_hook = runtime.clone();

        let result = close_object_after_close_preflight_with_domain_cleanup(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::DescramblerClose,
            || {
                let mut guard = runtime_for_hook.lock().unwrap();
                let second_begin = begin_object_close_cascade(
                    &mut guard,
                    AidlObjectId(91_005),
                    AidlObjectGeneration(1),
                    CleanupStep::UnregisterRuntime,
                );
                assert!(second_begin.is_err());
                drop(guard);
                second_begin
            },
        );

        assert!(result.is_err());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime.aidl_object_lifecycle(AidlObjectId(91_005)).unwrap(),
            RuntimeObjectLifecycle::CleanupFailed {
                step: CleanupStep::ReleaseBackend
            }
        );
    }

    #[test]
    fn callback_artifact_registration_core_rolls_back_store_when_registry_record_fails() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_007),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_007),
            AidlObjectGeneration(1),
            91_007,
        );
        let context = context_for_runtime(&runtime);
        context.clear_owner_callbacks(handle).unwrap();
        let runtime_for_retain = runtime.clone();
        let result = register_callback_artifact_after_owner_ready(
            &context,
            handle,
            AidlApi::DemuxOpenFilter,
            || {
                retain_test_callback_marker_as_hal(&context, handle, AidlApi::DemuxOpenFilter)?;
                let mut guard = runtime_for_retain.lock().unwrap();
                begin_object_close_cascade(
                    &mut guard,
                    AidlObjectId(91_007),
                    AidlObjectGeneration(1),
                    CleanupStep::UnregisterRuntime,
                )
                .unwrap();
                commit_object_close_cascade(
                    &mut guard,
                    AidlObjectId(91_007),
                    AidlObjectGeneration(1),
                )
                .unwrap();
                Ok(())
            },
        );

        assert!(result.is_err());
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::DemuxOpenFilter)
            .unwrap());
        assert_eq!(runtime.lock().unwrap().callback_registration_count(), 0);
    }

    #[test]
    fn execute_callback_registration_runtime_use_case_checks_lifecycle_before_retain() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_008),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_008),
            AidlObjectGeneration(1),
            91_008,
        );
        let context = context_for_runtime(&runtime);
        {
            let mut guard = runtime.lock().unwrap();
            begin_object_close_cascade(
                &mut guard,
                AidlObjectId(91_008),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .unwrap();
            commit_object_close_cascade(&mut guard, AidlObjectId(91_008), AidlObjectGeneration(1))
                .unwrap();
        }
        let retain_called = Arc::new(Mutex::new(false));
        let retain_called_for_closure = retain_called.clone();

        let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
            &context,
            handle,
            AidlMethodCall::LnbSetCallback,
            || {
                *retain_called_for_closure.lock().unwrap() = true;
                Ok(())
            },
            || Ok(()),
            |_runtime, _handle, _dispatch_proof| Ok(()),
        );

        assert!(result.is_err());
        assert!(!*retain_called.lock().unwrap());
    }

    #[test]
    fn callback_registration_runtime_use_case_checks_dispatch_before_retain() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_009),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_009),
            AidlObjectGeneration(1),
            91_009,
        );
        let context = context_for_runtime(&runtime);
        let retain_called = Arc::new(Mutex::new(false));
        let retain_called_for_closure = retain_called.clone();

        let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
            &context,
            handle,
            AidlMethodCall::FrontendSetCallback,
            || {
                *retain_called_for_closure.lock().unwrap() = true;
                Ok(())
            },
            || Ok(()),
            |_runtime, _handle, _dispatch_proof| Ok(()),
        );

        assert!(result.is_err());
        assert!(!*retain_called.lock().unwrap());
        assert_eq!(runtime.lock().unwrap().callback_registration_count(), 0);
    }

    #[test]
    fn callback_registration_runtime_use_case_records_registry_after_retain() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_012),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_012),
            AidlObjectGeneration(1),
            91_012,
        );
        let context = context_for_runtime(&runtime);
        context.clear_owner_callbacks(handle).unwrap();

        execute_callback_registration_runtime_use_case(
            &context,
            handle,
            AidlMethodCall::LnbSetCallback,
            || retain_test_callback_marker_as_hal(&context, handle, AidlApi::LnbSetCallback),
            || {
                clear_owner_callback_registration_hal(
                    &context,
                    handle,
                    Some(AidlApi::LnbSetCallback),
                    "callback rollback failed for test",
                )
            },
            |runtime, handle, dispatch_proof| {
                runtime.commit_lnb_callback_registration_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_proof,
                )
            },
        )
        .expect("callback registration succeeds");

        assert!(context
            .has_callback_for_owner(handle, AidlApi::LnbSetCallback)
            .unwrap());
        let runtime = runtime.lock().unwrap();
        assert_eq!(runtime.callback_registration_count(), 1);
        assert_eq!(
            runtime
                .callback_registration_health(
                    AidlObjectKind::Lnb,
                    AidlObjectId(91_012),
                    AidlObjectGeneration(1),
                    AidlApi::LnbSetCallback,
                )
                .expect("registration recorded"),
            CallbackHealthState::Registered
        );
    }

    #[test]
    fn callback_registration_runtime_use_case_rolls_back_store_and_registry_on_domain_failure() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_013),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_013),
            AidlObjectGeneration(1),
            91_013,
        );
        let context = context_for_runtime(&runtime);
        context.clear_owner_callbacks(handle).unwrap();

        let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
            &context,
            handle,
            AidlMethodCall::LnbSetCallback,
            || retain_test_callback_marker_as_hal(&context, handle, AidlApi::LnbSetCallback),
            || {
                clear_owner_callback_registration_hal(
                    &context,
                    handle,
                    Some(AidlApi::LnbSetCallback),
                    "callback rollback failed for test",
                )
            },
            |_runtime, _handle, _dispatch_proof| {
                Err(HalError::Unsupported("domain commit failed for test"))
            },
        );

        assert!(result.is_err());
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::LnbSetCallback)
            .unwrap());
        assert_eq!(runtime.lock().unwrap().callback_registration_count(), 0);
    }

    #[test]
    fn callback_registration_runtime_use_case_marks_unhealthy_when_rollback_fails() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_015),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_015),
            AidlObjectGeneration(1),
            91_015,
        );
        let context = context_for_runtime(&runtime);
        context.clear_owner_callbacks(handle).unwrap();

        let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
            &context,
            handle,
            AidlMethodCall::LnbSetCallback,
            || retain_test_callback_marker_as_hal(&context, handle, AidlApi::LnbSetCallback),
            || {
                Err(HalError::cleanup_failed(
                    "callback rollback test",
                    "callback rollback failed for test",
                ))
            },
            |_runtime, _handle, _dispatch_proof| {
                Err(HalError::Unsupported("domain commit failed for test"))
            },
        );

        assert!(result.is_err());
        assert!(context
            .has_callback_for_owner(handle, AidlApi::LnbSetCallback)
            .unwrap());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime
                .callback_registration_health(
                    AidlObjectKind::Lnb,
                    AidlObjectId(91_015),
                    AidlObjectGeneration(1),
                    AidlApi::LnbSetCallback,
                )
                .expect("registration remains for unhealthy diagnostic"),
            CallbackHealthState::Unhealthy
        );
    }

    #[test]
    fn callback_registration_runtime_use_case_rolls_back_store_when_registry_record_fails() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_014),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_014),
            AidlObjectGeneration(1),
            91_014,
        );
        let context = context_for_runtime(&runtime);
        context.clear_owner_callbacks(handle).unwrap();
        let runtime_for_retain = runtime.clone();
        let rollback_called = Arc::new(Mutex::new(false));
        let rollback_called_for_closure = rollback_called.clone();
        let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
            &context,
            handle,
            AidlMethodCall::LnbSetCallback,
            || {
                retain_test_callback_marker_as_hal(&context, handle, AidlApi::LnbSetCallback)?;
                let mut guard = runtime_for_retain.lock().unwrap();
                begin_object_close_cascade(
                    &mut guard,
                    AidlObjectId(91_014),
                    AidlObjectGeneration(1),
                    CleanupStep::UnregisterRuntime,
                )
                .unwrap();
                commit_object_close_cascade(
                    &mut guard,
                    AidlObjectId(91_014),
                    AidlObjectGeneration(1),
                )
                .unwrap();
                Ok(())
            },
            || {
                *rollback_called_for_closure.lock().unwrap() = true;
                clear_owner_callback_registration_hal(
                    &context,
                    handle,
                    Some(AidlApi::LnbSetCallback),
                    "callback rollback failed for test",
                )
            },
            |_runtime, _handle, _dispatch_proof| Ok(()),
        );

        assert!(result.is_err());
        assert!(*rollback_called.lock().unwrap());
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::LnbSetCallback)
            .unwrap());
        assert_eq!(runtime.lock().unwrap().callback_registration_count(), 0);
    }

    #[test]
    fn callback_artifact_registration_failure_rolls_back_store_without_registry_cleanup_error() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_016),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_016),
            AidlObjectGeneration(1),
            91_016,
        );
        let context = context_for_runtime(&runtime);
        context.clear_owner_callbacks(handle).unwrap();
        {
            let mut guard = runtime.lock().unwrap();
            begin_object_close_cascade(
                &mut guard,
                AidlObjectId(91_016),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            )
            .unwrap();
            commit_object_close_cascade(&mut guard, AidlObjectId(91_016), AidlObjectGeneration(1))
                .unwrap();
        }
        let result = register_callback_artifact_after_owner_ready(
            &context,
            handle,
            AidlApi::LnbSetCallback,
            || retain_test_callback_marker_as_hal(&context, handle, AidlApi::LnbSetCallback),
        );

        assert!(result.is_err());
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::LnbSetCallback)
            .unwrap());
        assert_eq!(runtime.lock().unwrap().callback_registration_count(), 0);
    }

    #[test]
    fn close_object_after_close_preflight_allows_cleanup_failed_retry() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_006),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_006),
            AidlObjectGeneration(1),
            91_006,
        );

        let first = close_object_after_close_preflight_with_domain_cleanup(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::FilterClose,
            || {
                Err(HalError::cleanup_failed(
                    "test domain cleanup retry",
                    "domain cleanup failed for retry test",
                ))
            },
        );
        assert!(first.is_err());
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .aidl_object_lifecycle(AidlObjectId(91_006))
                .unwrap(),
            RuntimeObjectLifecycle::CleanupFailed {
                step: CleanupStep::ReleaseBackend
            }
        );

        close_object_after_close_preflight_with_domain_cleanup(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::FilterClose,
            || Ok(()),
        )
        .expect("close retry from cleanup failed succeeds");

        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .aidl_object_lifecycle(AidlObjectId(91_006))
                .unwrap(),
            RuntimeObjectLifecycle::Closed
        );
    }

    #[test]
    fn close_object_after_close_preflight_is_idempotent_for_closed_object() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_011),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_011),
            AidlObjectGeneration(1),
            91_011,
        );

        close_object_after_close_preflight_with_domain_cleanup(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::FilterClose,
            || Ok(()),
        )
        .expect("first close succeeds");

        close_object_after_close_preflight_with_domain_cleanup(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::FilterClose,
            || panic!("domain cleanup must not run for already closed object"),
        )
        .expect("second close is idempotent");

        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .aidl_object_lifecycle(AidlObjectId(91_011))
                .unwrap(),
            RuntimeObjectLifecycle::Closed
        );
    }
}
