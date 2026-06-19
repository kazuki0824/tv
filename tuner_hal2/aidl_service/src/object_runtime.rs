use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};

use binder::{Result as BinderResult, Status};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlFailureSource, AidlMethodAdapter, AidlMethodCall, AidlObjectKind,
    AidlStatusMapper, CommandPlan, RuntimeExecutableRequest, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::{
    FirstErrorCollector, HalError, HalInternalKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
use maleicacid_tuner_hal2_service_runtime::{
    object_close_txn::{
        begin_object_close_cascade, commit_object_close_cascade,
        mark_object_close_cleanup_failed_cascade, plan_and_begin_object_close_method_dispatch,
        quarantine_object_cascade,
    },
    object_lifecycle::{
        aidl_object_for_close_cleanup_runtime, aidl_object_live, lnb_public_id_for_live_object,
    },
    object_method_txn::{
        build_and_plan_object_method_request_after_live, ObjectMethodDispatchPreflight,
        ObjectMethodTxnBuildError, ObjectMethodTxnPlan, ObjectMethodTxnTarget,
    },
    CallbackRegistryUpdate, TunerServiceRuntime,
};

use crate::callback_store::{clear_owner_callbacks, AidlCallbackStoreError};
use crate::error_bridge::{status_from_hal_error, status_from_tuner_status, status_unknown_error};
use crate::object_handle::AidlObjectHandle;

pub type SharedTunerRuntime = Arc<Mutex<TunerServiceRuntime>>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct DropLeakErrorRecord {
    object_kind: AidlObjectKind,
    object_id: maleicacid_tuner_hal2_binder_adapter::AidlObjectId,
    generation: maleicacid_tuner_hal2_binder_adapter::AidlObjectGeneration,
    status_debug: String,
}

static DROP_LEAK_ERROR_RECORDS: OnceLock<Mutex<Vec<DropLeakErrorRecord>>> = OnceLock::new();
static DROP_LEAK_ERROR_RECORD_FAILURES: AtomicUsize = AtomicUsize::new(0);

fn callback_store_error_to_hal(error: AidlCallbackStoreError, context: &'static str) -> HalError {
    error.into_hal_error(context)
}

fn status_debug_to_hal(status: &Status, context: &'static str) -> HalError {
    HalError::internal(
        HalInternalKind::InvariantViolation,
        format!("{context}: {status:?}"),
    )
}

fn drop_leak_error_records() -> &'static Mutex<Vec<DropLeakErrorRecord>> {
    DROP_LEAK_ERROR_RECORDS.get_or_init(|| Mutex::new(Vec::new()))
}

fn record_drop_leak_error(handle: AidlObjectHandle, status: &Status) {
    match drop_leak_error_records().lock() {
        Ok(mut records) => {
            records.push(DropLeakErrorRecord {
                object_kind: handle.object_kind(),
                object_id: handle.object_id(),
                generation: handle.generation(),
                status_debug: format!("{status:?}"),
            });
        }
        Err(_) => {
            DROP_LEAK_ERROR_RECORD_FAILURES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
fn drop_leak_error_record_count() -> usize {
    drop_leak_error_records()
        .lock()
        .map(|records| records.len())
        .unwrap_or(0)
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

fn execute_object_runtime_locked_call<T, H, F>(
    runtime: &SharedTunerRuntime,
    handle: H,
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
    execute: F,
) -> Result<T, HalError>
where
    F: FnOnce(
        &mut TunerServiceRuntime,
        H,
        CommandPlan,
        Option<RuntimeExecutableRequest>,
    ) -> Result<T, HalError>,
{
    let mut runtime = lock_runtime(runtime)?;
    execute(&mut runtime, handle, command_plan, executable_request)
}

fn execute_shared_object_runtime_call<T, H, F>(
    runtime: &SharedTunerRuntime,
    handle: H,
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
    execute: F,
) -> Result<T, HalError>
where
    F: FnOnce(
        SharedTunerRuntime,
        H,
        CommandPlan,
        Option<RuntimeExecutableRequest>,
    ) -> Result<T, HalError>,
{
    execute(runtime.clone(), handle, command_plan, executable_request)
}

fn execute_object_query_runtime_call<T, F>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    object_kind: AidlObjectKind,
    command_plan: CommandPlan,
    executable_request: Option<RuntimeExecutableRequest>,
    execute: F,
) -> Result<T, HalError>
where
    F: FnOnce(&mut TunerServiceRuntime, AidlObjectHandle) -> Result<T, HalError>,
{
    if command_plan.object() != object_kind {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL query method/object kind mismatch",
        ));
    }
    let mut runtime = lock_runtime(runtime)?;
    aidl_object_live(
        &runtime,
        handle.object_id(),
        handle.generation(),
        object_kind,
    )?;
    runtime
        .plan_command_dispatch(command_plan, executable_request)
        .map_err(|error| error.into_hal_error())?;
    execute(&mut runtime, handle)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropLeakDomainAction {
    None,
    RecordLnbDropLeak,
}

fn clear_runtime_callback_owner_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    expected_runtime_entry: bool,
) -> Result<(), HalError> {
    let mut runtime = lock_runtime(runtime)?;
    match runtime
        .callback_registry_mut()
        .clear_owner(handle.object_id(), handle.generation())
    {
        CallbackRegistryUpdate::Updated => Ok(()),
        CallbackRegistryUpdate::Missing if !expected_runtime_entry => Ok(()),
        CallbackRegistryUpdate::Missing => Err(HalError::cleanup_failed(
            "callback registry owner clear",
            "callback registry owner missing while clearing callback registration",
        )),
    }
}

fn clear_retained_callback_artifact_hal(
    handle: AidlObjectHandle,
    failure_message: &'static str,
) -> Result<(), HalError> {
    clear_owner_callbacks(handle)
        .map(|_| ())
        .map_err(|error| callback_store_error_to_hal(error, failure_message))
}

fn mark_runtime_callback_unhealthy_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    api: AidlApi,
) -> Result<(), HalError> {
    let mut runtime = lock_runtime(runtime)?;
    match runtime.callback_registry_mut().mark_unhealthy(
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
        .callback_registry_mut()
        .mark_owner_unhealthy(handle.object_id(), handle.generation())
    {
        CallbackRegistryUpdate::Updated => Ok(()),
        CallbackRegistryUpdate::Missing => Err(HalError::cleanup_failed(
            "callback registry owner unhealthy marking",
            "callback registry owner missing while marking unhealthy",
        )),
    }
}

pub(crate) fn clear_owner_callback_registration_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    api: Option<AidlApi>,
    failure_message: &'static str,
) -> Result<(), HalError> {
    match clear_owner_callbacks(handle) {
        Ok(removed) => clear_runtime_callback_owner_hal(runtime, handle, removed > 0),
        Err(error) => {
            let cleanup_error = callback_store_error_to_hal(error, failure_message);
            match api {
                Some(api) => {
                    if let Err(mark_error) =
                        mark_runtime_callback_unhealthy_hal(runtime, handle, api)
                    {
                        return Err(HalError::composed_failure(
                            "callback cleanup failed and unhealthy marking failed",
                            cleanup_error,
                            mark_error,
                        ));
                    }
                }
                None => {
                    if let Err(mark_error) =
                        mark_runtime_callback_owner_unhealthy_hal(runtime, handle)
                    {
                        return Err(HalError::composed_failure(
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

pub fn clear_owner_callback_registration(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    api: Option<AidlApi>,
    failure_message: &'static str,
) -> BinderResult<()> {
    clear_owner_callback_registration_hal(runtime, handle, api, failure_message)
        .map_err(status_from_hal_error)
}

pub fn register_callback_artifact_after_owner_ready<Retain, Rollback>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    api: AidlApi,
    retain: Retain,
    _rollback: &mut Rollback,
) -> BinderResult<()>
where
    Retain: FnOnce() -> Result<(), HalError>,
    Rollback: FnMut() -> Result<(), HalError>,
{
    retain().map_err(status_from_hal_error)?;
    if let Err(primary_error) = record_callback_registration(runtime, handle, api) {
        if let Err(rollback_error) = clear_retained_callback_artifact_hal(
            handle,
            "callback artifact rollback failed before runtime registration",
        ) {
            return Err(status_from_hal_error(HalError::composed_failure(
                "callback artifact registration rollback failed",
                primary_error,
                rollback_error,
            )));
        }
        return Err(status_from_hal_error(primary_error));
    }
    Ok(())
}

fn object_method_txn_target(handle: AidlObjectHandle) -> ObjectMethodTxnTarget {
    ObjectMethodTxnTarget::new(
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
    )
}

fn object_method_txn_plan(
    method: AidlMethodCall,
) -> BinderResult<(CommandPlan, Option<RuntimeExecutableRequest>)> {
    let method_plan = AidlMethodAdapter::plan(method).map_err(status_from_hal_error)?;
    Ok((
        method_plan.command_plan,
        method_plan.command.runtime_executable_request(),
    ))
}

fn build_and_plan_request_after_live<T, F>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    build: F,
) -> BinderResult<(ObjectMethodTxnPlan, ObjectMethodDispatchPreflight, T)>
where
    F: FnOnce() -> BinderResult<(CommandPlan, Option<RuntimeExecutableRequest>, T)>,
{
    build_and_plan_object_method_request_after_live(
        runtime,
        object_method_txn_target(handle),
        build,
    )
    .map_err(|error| match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(status) => status,
    })
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
        CommandPlan,
        Option<RuntimeExecutableRequest>,
    ) -> Result<T, HalError>,
{
    let method_plan = AidlMethodAdapter::plan(method).map_err(status_from_hal_error)?;
    let executable_request = method_plan.command.runtime_executable_request();
    execute_object_runtime_locked_call(
        runtime,
        handle,
        method_plan.command_plan,
        executable_request,
        execute,
    )
    .map_err(status_from_hal_error)
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
        CommandPlan,
        Option<RuntimeExecutableRequest>,
    ) -> Result<T, HalError>,
{
    let method_plan = AidlMethodAdapter::plan(method).map_err(status_from_hal_error)?;
    let executable_request = method_plan.command.runtime_executable_request();
    execute_shared_object_runtime_call(
        runtime,
        handle,
        method_plan.command_plan,
        executable_request,
        execute,
    )
    .map_err(status_from_hal_error)
}

pub fn execute_object_query_use_case<T, F>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
    execute: F,
) -> BinderResult<T>
where
    F: FnOnce(&mut TunerServiceRuntime, AidlObjectHandle) -> Result<T, HalError>,
{
    let method_plan = AidlMethodAdapter::plan(method).map_err(status_from_hal_error)?;
    let executable_request = method_plan.command.runtime_executable_request();
    execute_object_query_runtime_call(
        runtime,
        handle,
        handle.object_kind(),
        method_plan.command_plan,
        executable_request,
        execute,
    )
    .map_err(status_from_hal_error)
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
        CommandPlan,
        Option<RuntimeExecutableRequest>,
        ObjectMethodDispatchPreflight,
        B,
    ) -> Result<T, HalError>,
{
    let (txn_plan, dispatch_preflight, built_request) =
        build_and_plan_request_after_live(runtime, handle, || {
            let (method, built_request) = build()?;
            let (command_plan, executable_request) = object_method_txn_plan(method)?;
            Ok((command_plan, executable_request, built_request))
        })?;
    execute_object_runtime_locked_call(
        runtime,
        handle,
        txn_plan.command_plan(),
        txn_plan.executable_request(),
        |runtime, handle, command_plan, executable_request| {
            execute(
                runtime,
                handle,
                command_plan,
                executable_request,
                dispatch_preflight,
                built_request,
            )
        },
    )
    .map_err(status_from_hal_error)
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
        CommandPlan,
        Option<RuntimeExecutableRequest>,
        ObjectMethodDispatchPreflight,
        B,
    ) -> Result<T, HalError>,
{
    let (txn_plan, dispatch_preflight, built_request) =
        build_and_plan_request_after_live(runtime, handle, || {
            let (method, built_request) = build()?;
            let (command_plan, executable_request) = object_method_txn_plan(method)?;
            Ok((command_plan, executable_request, built_request))
        })?;
    execute_shared_object_runtime_call(
        runtime,
        handle,
        txn_plan.command_plan(),
        txn_plan.executable_request(),
        |runtime, handle, command_plan, executable_request| {
            execute(
                runtime,
                handle,
                command_plan,
                executable_request,
                dispatch_preflight,
                built_request,
            )
        },
    )
    .map_err(status_from_hal_error)
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
    let (txn_plan, _dispatch_preflight, ()) =
        build_and_plan_request_after_live(runtime, handle, || {
            let method = build()?;
            let (command_plan, executable_request) = object_method_txn_plan(method)?;
            Ok((command_plan, executable_request, ()))
        })?;
    let failure = AidlFailureSource::RuntimeDispatch(HalError::Unsupported(message));
    let failures = [failure];
    let status = AidlStatusMapper::resolve_failure_by_precedence(
        txn_plan.command_plan().api(),
        &failures,
        false,
    )
    .unwrap_or(TunerStatusCode::UnknownError);
    Err(status_from_tuner_status(status, message))
}

pub fn execute_callback_registration_runtime_use_case<T, Retain, Rollback, Execute>(
    runtime: &SharedTunerRuntime,
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
        ObjectMethodDispatchPreflight,
    ) -> Result<T, HalError>,
{
    let api = method.api();
    let (txn_plan, dispatch_preflight, ()) =
        build_and_plan_request_after_live(runtime, handle, || {
            let (command_plan, executable_request) = object_method_txn_plan(method)?;
            Ok((command_plan, executable_request, ()))
        })?;
    let mut rollback = rollback;
    register_callback_artifact_after_owner_ready(runtime, handle, api, retain, &mut rollback)?;
    let result = execute_object_runtime_locked_call(
        runtime,
        handle,
        txn_plan.command_plan(),
        txn_plan.executable_request(),
        |runtime, handle, _command_plan, _executable_request| {
            execute(runtime, handle, dispatch_preflight)
        },
    );
    match result {
        Ok(value) => Ok(value),
        Err(primary_error) => {
            if let Err(rollback_error) = rollback() {
                let cleanup_error = match mark_runtime_callback_unhealthy_hal(runtime, handle, api)
                {
                    Ok(()) => rollback_error,
                    Err(mark_error) => HalError::composed_failure(
                        "callback rollback unhealthy marking failed",
                        rollback_error,
                        mark_error,
                    ),
                };
                return Err(status_from_hal_error(HalError::composed_failure(
                    "callback registration domain failure rollback failed",
                    primary_error,
                    cleanup_error,
                )));
            }
            Err(status_from_hal_error(primary_error))
        }
    }
}

pub fn clear_live_lnb_callback_for_public_id(
    runtime: &SharedTunerRuntime,
    lnb_id: i32,
) -> BinderResult<()> {
    let handle = {
        let runtime = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
        let Some(entry) =
            aidl_object_for_close_cleanup_runtime(&runtime, AidlObjectKind::Lnb, i64::from(lnb_id))
        else {
            return Err(status_from_hal_error(HalError::cleanup_failed(
                "LNB owner-loss callback cleanup",
                format!("LNB AIDL object is missing during owner-loss cleanup: id={lnb_id}"),
            )));
        };
        AidlObjectHandle::new(entry.object_kind, entry.object_id, entry.generation)
    };
    clear_owner_callback_registration(
        runtime,
        handle,
        Some(AidlApi::LnbSetCallback),
        "callback store cleanup failed during LNB owner loss",
    )
}

fn unregister_public_runtime_entries(
    runtime: &mut TunerServiceRuntime,
    entries: &[maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry],
) -> Result<(), HalError> {
    let mut collector = FirstErrorCollector::new();
    for entry in entries {
        collector.push_result(runtime.unregister_public_runtime_for_closed_aidl_entry(entry));
    }
    collector.into_result()
}

fn mark_close_cleanup_failed_hal(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    detail: &'static str,
) -> Result<(), HalError> {
    let mut guard = lock_runtime(runtime)?;
    mark_object_close_cleanup_failed_cascade(
        &mut guard,
        handle.object_id(),
        handle.generation(),
        CleanupStep::UnregisterRuntime,
        detail,
    )
}

fn finish_object_close_after_begin_with_domain_cleanup<F>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    domain_cleanup: F,
) -> BinderResult<()>
where
    F: FnOnce() -> BinderResult<()>,
{
    let mut cleanup_collector = FirstErrorCollector::new();
    cleanup_collector.push_result(clear_owner_callback_registration_hal(
        runtime,
        handle,
        None,
        "callback store cleanup failed during AIDL object close",
    ));
    cleanup_collector.push_result(
        domain_cleanup()
            .map_err(|status| status_debug_to_hal(&status, "AIDL object domain cleanup status")),
    );

    if cleanup_collector.has_error() {
        let cleanup_error = match cleanup_collector.into_result() {
            Err(error) => error,
            Ok(()) => {
                return Err(status_unknown_error(
                    "cleanup collector reported an error but returned success",
                ));
            }
        };
        return match mark_close_cleanup_failed_hal(
            runtime,
            handle,
            "AIDL object close cleanup failure could not be recorded",
        ) {
            Ok(()) => Err(status_from_hal_error(cleanup_error)),
            Err(mark_error) => Err(status_from_hal_error(HalError::composed_failure(
                "AIDL object close cleanup failed and cleanup-failed marking failed",
                cleanup_error,
                mark_error,
            ))),
        };
    }

    let mut guard = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    let closed_entries =
        commit_object_close_cascade(&mut guard, handle.object_id(), handle.generation())
            .map_err(status_from_hal_error)?;
    unregister_public_runtime_entries(&mut guard, &closed_entries).map_err(status_from_hal_error)
}

pub fn close_object_with_domain_cleanup<F>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    domain_cleanup: F,
) -> BinderResult<()>
where
    F: FnOnce() -> BinderResult<()>,
{
    {
        let mut guard = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
        begin_object_close_cascade(
            &mut guard,
            handle.object_id(),
            handle.generation(),
            CleanupStep::StopWorker,
        )
        .map_err(status_from_hal_error)?;
    }
    finish_object_close_after_begin_with_domain_cleanup(runtime, handle, domain_cleanup)
}

pub fn close_object(runtime: &SharedTunerRuntime, handle: AidlObjectHandle) -> BinderResult<()> {
    close_object_with_domain_cleanup(runtime, handle, || Ok(()))
}

fn lnb_public_id_for_drop(runtime: &TunerServiceRuntime, handle: AidlObjectHandle) -> Option<i32> {
    if handle.object_kind() != AidlObjectKind::Lnb {
        return None;
    }
    lnb_public_id_for_live_object(runtime, handle.object_id(), handle.generation())
}

fn record_domain_drop_leak(
    runtime: &mut TunerServiceRuntime,
    handle: AidlObjectHandle,
    action: DropLeakDomainAction,
) -> Result<(), HalError> {
    match action {
        DropLeakDomainAction::None => Ok(()),
        DropLeakDomainAction::RecordLnbDropLeak => {
            let lnb_id = lnb_public_id_for_drop(runtime, handle).ok_or_else(|| {
                HalError::cleanup_failed(
                    "drop leak LNB domain record",
                    "drop leak LNB runtime id is missing",
                )
            })?;
            runtime.record_lnb_drop_leak(lnb_id)
        }
    }
}

pub fn drop_leak_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    action: DropLeakDomainAction,
) -> BinderResult<()> {
    let callback_store_clear = clear_owner_callbacks(handle);
    let mut runtime = runtime.lock().map_err(|_| {
        status_unknown_error("service runtime lock poisoned during drop leak quarantine")
    })?;
    let domain_record = record_domain_drop_leak(&mut runtime, handle, action);
    let mut error_collector = FirstErrorCollector::new();
    let callback_store_removed = match callback_store_clear {
        Ok(removed) => Some(removed),
        Err(error) => {
            error_collector.push_error(callback_store_error_to_hal(
                error,
                "drop leak callback store clear failed",
            ));
            None
        }
    };
    if let Err(error) = domain_record {
        error_collector.push_error(error);
    }

    if !error_collector.has_error() {
        match runtime
            .callback_registry_mut()
            .clear_owner(handle.object_id(), handle.generation())
        {
            CallbackRegistryUpdate::Updated => {}
            CallbackRegistryUpdate::Missing if callback_store_removed == Some(0) => {}
            CallbackRegistryUpdate::Missing => {
                error_collector.push_error(HalError::cleanup_failed(
                    "drop leak callback registry clear",
                    "callback registry owner missing during clear",
                ));
            }
        }
    } else {
        match runtime
            .callback_registry_mut()
            .mark_owner_unhealthy(handle.object_id(), handle.generation())
        {
            CallbackRegistryUpdate::Updated => {}
            CallbackRegistryUpdate::Missing => {
                error_collector.push_error(HalError::cleanup_failed(
                    "drop leak callback registry unhealthy marking",
                    "callback registry owner missing while marking unhealthy",
                ));
            }
        }
    }

    match quarantine_object_cascade(&mut runtime, handle.object_id(), handle.generation()) {
        Ok(entries) => {
            error_collector.push_result(unregister_public_runtime_entries(&mut runtime, &entries))
        }
        Err(error) => error_collector.push_error(error),
    }

    match error_collector.into_result() {
        Err(error) => Err(status_from_hal_error(error)),
        Ok(()) => Ok(()),
    }
}

pub fn drop_leak_object_from_drop(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    action: DropLeakDomainAction,
) {
    if let Err(status) = drop_leak_object(runtime, handle, action) {
        record_drop_leak_error(handle, &status);
    }
}

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
    runtime.callback_registry_mut().record_registration(
        handle.object_kind(),
        handle.object_id(),
        handle.generation(),
        api,
    );
    Ok(())
}

pub fn close_object_after_close_preflight_with_domain_cleanup<F>(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
    domain_cleanup: F,
) -> BinderResult<()>
where
    F: FnOnce() -> BinderResult<()>,
{
    let method_plan = AidlMethodAdapter::plan(method).map_err(status_from_hal_error)?;
    let executable_request = method_plan.command.runtime_executable_request();
    {
        let mut runtime = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
        plan_and_begin_object_close_method_dispatch(
            &mut runtime,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            method_plan.command_plan,
            executable_request,
            CleanupStep::StopWorker,
        )
        .map_err(status_from_hal_error)?;
    }
    finish_object_close_after_begin_with_domain_cleanup(runtime, handle, domain_cleanup)
}

pub fn close_object_after_close_preflight(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
) -> BinderResult<()> {
    close_object_after_close_preflight_with_domain_cleanup(runtime, handle, method, || Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callback_store::{has_callback_for_owner, retain_test_callback_marker};
    use maleicacid_tuner_hal2_binder_adapter::{
        AidlApi, AidlMethodCall, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
    };
    use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
    use maleicacid_tuner_hal2_service_runtime::{
        CallbackHealthState, RuntimeObjectLifecycle, RuntimeOwnerRelation,
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

    fn retain_test_callback_marker_as_hal(
        handle: AidlObjectHandle,
        api: AidlApi,
    ) -> Result<(), HalError> {
        retain_test_callback_marker(handle, api)
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
        let mut executed = false;
        let result = execute_object_query_use_case(
            &runtime,
            handle,
            AidlMethodCall::FilterGetId,
            |_runtime, _handle| {
                executed = true;
                Ok(91_010)
            },
        );
        assert!(result.is_err());
        assert!(!executed);
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

        assert!(drop_leak_object(&runtime, handle, DropLeakDomainAction::None).is_err());
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

        let result = drop_leak_object(&runtime, handle, DropLeakDomainAction::RecordLnbDropLeak);

        assert!(result.is_err());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(91_011))
                .unwrap()
                .lifecycle,
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
        let before = drop_leak_error_record_count();

        drop_leak_object_from_drop(&runtime, handle, DropLeakDomainAction::RecordLnbDropLeak);

        assert_eq!(drop_leak_error_record_count(), before + 1);
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(91_012))
                .unwrap()
                .lifecycle,
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn drop_leak_clears_runtime_callback_registration_and_quarantines_object() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_001),
            AidlObjectGeneration(1),
        );
        clear_owner_callbacks(handle).unwrap();
        retain_test_callback_marker(handle, AidlApi::DemuxOpenFilter).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_001),
            AidlObjectGeneration(1),
            91_001,
        );
        record_callback_registration(&runtime, handle, AidlApi::DemuxOpenFilter).unwrap();

        drop_leak_object(&runtime, handle, DropLeakDomainAction::None).unwrap();

        let runtime = runtime.lock().unwrap();
        assert!(!has_callback_for_owner(handle, AidlApi::DemuxOpenFilter).unwrap());
        assert_eq!(runtime.callback_registry().registration_count(), 0);
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(91_001))
                .unwrap()
                .lifecycle,
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn drop_leak_returns_error_and_marks_callback_unhealthy_when_domain_drop_record_fails() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_002),
            AidlObjectGeneration(1),
        );
        clear_owner_callbacks(handle).unwrap();
        retain_test_callback_marker(handle, AidlApi::LnbSetCallback).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_002),
            AidlObjectGeneration(1),
            91_002,
        );
        record_callback_registration(&runtime, handle, AidlApi::LnbSetCallback).unwrap();

        assert!(
            drop_leak_object(&runtime, handle, DropLeakDomainAction::RecordLnbDropLeak).is_err()
        );

        let runtime = runtime.lock().unwrap();
        assert!(!has_callback_for_owner(handle, AidlApi::LnbSetCallback).unwrap());
        assert_eq!(
            runtime
                .callback_registry()
                .registration_for(
                    AidlObjectKind::Lnb,
                    AidlObjectId(91_002),
                    AidlObjectGeneration(1),
                    AidlApi::LnbSetCallback,
                )
                .unwrap()
                .health,
            CallbackHealthState::Unhealthy
        );
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(91_002))
                .unwrap()
                .lifecycle,
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn close_object_with_domain_cleanup_runs_hook_after_single_begin_and_commits_closed() {
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

        close_object_with_domain_cleanup(&runtime, handle, || {
            let guard = runtime_for_hook.lock().unwrap();
            assert_eq!(
                guard
                    .object_table()
                    .entry(AidlObjectId(91_003))
                    .expect("object remains tracked")
                    .lifecycle,
                RuntimeObjectLifecycle::Closing {
                    step: CleanupStep::StopWorker
                }
            );
            drop(guard);
            *hook_called_for_hook.lock().unwrap() = true;
            Ok(())
        })
        .expect("close succeeds");

        assert!(*hook_called.lock().unwrap());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(91_003))
                .expect("object remains tracked")
                .lifecycle,
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
            &runtime,
            handle,
            AidlMethodCall::FilterClose,
            || {
                let guard = runtime_for_hook.lock().unwrap();
                assert_eq!(
                    guard
                        .object_table()
                        .entry(AidlObjectId(91_009))
                        .expect("object remains tracked")
                        .lifecycle,
                    RuntimeObjectLifecycle::Closing {
                        step: CleanupStep::StopWorker
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
                .object_table()
                .entry(AidlObjectId(91_009))
                .expect("object remains tracked")
                .lifecycle,
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
            |_runtime, _handle, _command_plan, _executable_request, _dispatch_preflight, ()| Ok(()),
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
            |_runtime, _handle, _command_plan, _executable_request, _dispatch_preflight, ()| Ok(()),
        );

        assert!(result.is_err());
        assert!(!*builder_called.lock().unwrap());
    }

    #[test]
    fn close_object_with_domain_cleanup_marks_cleanup_failed_when_domain_cleanup_fails() {
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

        let result = close_object_with_domain_cleanup(&runtime, handle, || {
            Err(status_unknown_error("domain cleanup failed for test"))
        });

        assert!(result.is_err());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(91_004))
                .expect("object remains tracked")
                .lifecycle,
            RuntimeObjectLifecycle::CleanupFailed {
                step: CleanupStep::UnregisterRuntime
            }
        );
    }

    #[test]
    fn close_object_with_domain_cleanup_rejects_domain_cleanup_second_begin() {
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

        let result = close_object_with_domain_cleanup(&runtime, handle, || {
            let mut guard = runtime_for_hook.lock().unwrap();
            let second_begin = begin_object_close_cascade(
                &mut guard,
                AidlObjectId(91_005),
                AidlObjectGeneration(1),
                CleanupStep::UnregisterRuntime,
            );
            assert!(second_begin.is_err());
            drop(guard);
            second_begin.map_err(status_from_hal_error)
        });

        assert!(result.is_err());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(91_005))
                .expect("object remains tracked")
                .lifecycle,
            RuntimeObjectLifecycle::CleanupFailed {
                step: CleanupStep::UnregisterRuntime
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
        clear_owner_callbacks(handle).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_007),
            AidlObjectGeneration(1),
            91_007,
        );
        let runtime_for_retain = runtime.clone();
        let rollback_called = Arc::new(Mutex::new(false));
        let rollback_called_for_closure = rollback_called.clone();
        let mut rollback_callback = || {
            *rollback_called_for_closure.lock().unwrap() = true;
            clear_owner_callback_registration_hal(
                &runtime,
                handle,
                Some(AidlApi::DemuxOpenFilter),
                "callback rollback failed for test",
            )
        };

        let result = register_callback_artifact_after_owner_ready(
            &runtime,
            handle,
            AidlApi::DemuxOpenFilter,
            || {
                retain_test_callback_marker_as_hal(handle, AidlApi::DemuxOpenFilter)?;
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
            &mut rollback_callback,
        );

        assert!(result.is_err());
        assert!(*rollback_called.lock().unwrap());
        assert!(!has_callback_for_owner(handle, AidlApi::DemuxOpenFilter).unwrap());
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .callback_registry()
                .registration_count(),
            0
        );
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
            &runtime,
            handle,
            AidlMethodCall::LnbSetCallback,
            || {
                *retain_called_for_closure.lock().unwrap() = true;
                Ok(())
            },
            || Ok(()),
            |_runtime, _handle, _dispatch_preflight| Ok(()),
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
        let retain_called = Arc::new(Mutex::new(false));
        let retain_called_for_closure = retain_called.clone();

        let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
            &runtime,
            handle,
            AidlMethodCall::FrontendSetCallback,
            || {
                *retain_called_for_closure.lock().unwrap() = true;
                Ok(())
            },
            || Ok(()),
            |_runtime, _handle, _dispatch_preflight| Ok(()),
        );

        assert!(result.is_err());
        assert!(!*retain_called.lock().unwrap());
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .callback_registry()
                .registration_count(),
            0
        );
    }

    #[test]
    fn callback_registration_runtime_use_case_records_registry_after_retain() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_012),
            AidlObjectGeneration(1),
        );
        clear_owner_callbacks(handle).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_012),
            AidlObjectGeneration(1),
            91_012,
        );

        execute_callback_registration_runtime_use_case(
            &runtime,
            handle,
            AidlMethodCall::LnbSetCallback,
            || retain_test_callback_marker_as_hal(handle, AidlApi::LnbSetCallback),
            || {
                clear_owner_callback_registration_hal(
                    &runtime,
                    handle,
                    Some(AidlApi::LnbSetCallback),
                    "callback rollback failed for test",
                )
            },
            |runtime, handle, dispatch_preflight| {
                runtime.commit_lnb_callback_registration_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_preflight,
                )
            },
        )
        .expect("callback registration succeeds");

        assert!(has_callback_for_owner(handle, AidlApi::LnbSetCallback).unwrap());
        let runtime = runtime.lock().unwrap();
        assert_eq!(runtime.callback_registry().registration_count(), 1);
        assert_eq!(
            runtime
                .callback_registry()
                .registration_for(
                    AidlObjectKind::Lnb,
                    AidlObjectId(91_012),
                    AidlObjectGeneration(1),
                    AidlApi::LnbSetCallback,
                )
                .expect("registration recorded")
                .health,
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
        clear_owner_callbacks(handle).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_013),
            AidlObjectGeneration(1),
            91_013,
        );

        let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
            &runtime,
            handle,
            AidlMethodCall::LnbSetCallback,
            || retain_test_callback_marker_as_hal(handle, AidlApi::LnbSetCallback),
            || {
                clear_owner_callback_registration_hal(
                    &runtime,
                    handle,
                    Some(AidlApi::LnbSetCallback),
                    "callback rollback failed for test",
                )
            },
            |_runtime, _handle, _dispatch_preflight| {
                Err(HalError::Unsupported("domain commit failed for test"))
            },
        );

        assert!(result.is_err());
        assert!(!has_callback_for_owner(handle, AidlApi::LnbSetCallback).unwrap());
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .callback_registry()
                .registration_count(),
            0
        );
    }

    #[test]
    fn callback_registration_runtime_use_case_marks_unhealthy_when_rollback_fails() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_015),
            AidlObjectGeneration(1),
        );
        clear_owner_callbacks(handle).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_015),
            AidlObjectGeneration(1),
            91_015,
        );

        let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
            &runtime,
            handle,
            AidlMethodCall::LnbSetCallback,
            || retain_test_callback_marker_as_hal(handle, AidlApi::LnbSetCallback),
            || {
                Err(HalError::cleanup_failed(
                    "callback rollback test",
                    "callback rollback failed for test",
                ))
            },
            |_runtime, _handle, _dispatch_preflight| {
                Err(HalError::Unsupported("domain commit failed for test"))
            },
        );

        assert!(result.is_err());
        assert!(has_callback_for_owner(handle, AidlApi::LnbSetCallback).unwrap());
        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime
                .callback_registry()
                .registration_for(
                    AidlObjectKind::Lnb,
                    AidlObjectId(91_015),
                    AidlObjectGeneration(1),
                    AidlApi::LnbSetCallback,
                )
                .expect("registration remains for unhealthy diagnostic")
                .health,
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
        clear_owner_callbacks(handle).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_014),
            AidlObjectGeneration(1),
            91_014,
        );
        let runtime_for_retain = runtime.clone();
        let rollback_called = Arc::new(Mutex::new(false));
        let rollback_called_for_closure = rollback_called.clone();

        let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
            &runtime,
            handle,
            AidlMethodCall::LnbSetCallback,
            || {
                retain_test_callback_marker_as_hal(handle, AidlApi::LnbSetCallback)?;
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
                    &runtime,
                    handle,
                    Some(AidlApi::LnbSetCallback),
                    "callback rollback failed for test",
                )
            },
            |_runtime, _handle, _dispatch_preflight| Ok(()),
        );

        assert!(result.is_err());
        assert!(*rollback_called.lock().unwrap());
        assert!(!has_callback_for_owner(handle, AidlApi::LnbSetCallback).unwrap());
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .callback_registry()
                .registration_count(),
            0
        );
    }

    #[test]
    fn callback_artifact_registration_failure_rolls_back_store_without_registry_cleanup_error() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_016),
            AidlObjectGeneration(1),
        );
        clear_owner_callbacks(handle).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_016),
            AidlObjectGeneration(1),
            91_016,
        );
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
        let rollback_called = Arc::new(Mutex::new(false));
        let rollback_called_for_closure = rollback_called.clone();

        let result = register_callback_artifact_after_owner_ready(
            &runtime,
            handle,
            AidlApi::LnbSetCallback,
            || retain_test_callback_marker_as_hal(handle, AidlApi::LnbSetCallback),
            &mut || {
                *rollback_called_for_closure.lock().unwrap() = true;
                clear_owner_callback_registration_hal(
                    &runtime,
                    handle,
                    Some(AidlApi::LnbSetCallback),
                    "callback rollback closure should not be used",
                )
            },
        );

        assert!(result.is_err());
        assert!(!*rollback_called.lock().unwrap());
        assert!(!has_callback_for_owner(handle, AidlApi::LnbSetCallback).unwrap());
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .callback_registry()
                .registration_count(),
            0
        );
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
            &runtime,
            handle,
            AidlMethodCall::FilterClose,
            || Err(status_unknown_error("domain cleanup failed for retry test")),
        );
        assert!(first.is_err());
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .object_table()
                .entry(AidlObjectId(91_006))
                .expect("object remains tracked")
                .lifecycle,
            RuntimeObjectLifecycle::CleanupFailed {
                step: CleanupStep::UnregisterRuntime
            }
        );

        close_object_after_close_preflight_with_domain_cleanup(
            &runtime,
            handle,
            AidlMethodCall::FilterClose,
            || Ok(()),
        )
        .expect("close retry from cleanup failed succeeds");

        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .object_table()
                .entry(AidlObjectId(91_006))
                .expect("object remains tracked")
                .lifecycle,
            RuntimeObjectLifecycle::Closed
        );
    }
}
