use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IFrontendCallback::IFrontendCallback;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnbCallback::ILnbCallback;
use binder::{Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlFailureSource, AidlMethodCall, AidlStatusMapper, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
use maleicacid_tuner_hal2_device::FrontendWorkerCancelReason;
use maleicacid_tuner_hal2_service_runtime::{
    close_frontend_object_cleanup_use_case,
    object_close_txn::{
        close_object_use_case, finish_object_close_use_case, ObjectArtifactCleanupCommand,
        ObjectArtifactCleanupExecutor, ObjectCloseRuntimeExecutor,
        ObjectCloseCleanupFailure, ObjectCloseUseCasePlan, ObjectRuntimeCleanupCommand,
    },
    object_domain_cleanup::{ObjectDomainCleanupCommand, ObjectDomainCleanupExecutor},
    object_lifecycle::lnb_public_id_for_live_object_result,
    object_method_txn::{
        execute_object_method_call_after_live, execute_object_query_call_after_live,
        execute_object_query_call_after_live_with_aidl_input_conversion,
        execute_shared_object_method_call_after_live, preflight_object_method_after_live_plan_only,
        ObjectMethodExecutionToken, ObjectMethodTxnBuildError, ObjectQueryRequest,
        ObjectQueryResponse,
    },
    CallbackRegistrationArtifactOutcome, OwnerCallbackCleanupUseCaseOutcome,
    TunerServiceRuntime,
};

use crate::dvr_callback_delivery::stop_dvr_status_notifier;
use crate::error_bridge::{status_from_hal_error, status_from_tuner_status, status_unknown_error};
use crate::object_handle::AidlObjectHandle;
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};

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

pub(crate) fn finish_owner_callback_cleanup_outcome<T>(
    context: &SharedAidlServiceContext,
    outcome: OwnerCallbackCleanupUseCaseOutcome<T>,
) -> Result<T, HalError> {
    let runtime = context.runtime();
    let mut guard = lock_runtime(&runtime)?;
    let command = *outcome.command();
    let artifact_cleanup_result = context.clear_owner_callback_artifacts_bridge(&command);
    guard.finish_owner_callback_cleanup_outcome(outcome, artifact_cleanup_result)
}

fn finish_callback_registration_artifact_outcome(
    context: &SharedAidlServiceContext,
    outcome: CallbackRegistrationArtifactOutcome,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let mut guard = lock_runtime(&runtime)?;
    let rollback_result = outcome
        .rollback_command()
        .map(|command| context.clear_owner_callback_artifacts_bridge(command));
    guard.finish_callback_registration_after_artifact_result_use_case(outcome, rollback_result)
}

pub(crate) fn finish_callback_artifact_registration_after_owner_ready_hal(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    api: AidlApi,
    artifact_retain_result: Result<(), HalError>,
) -> Result<(), HalError> {
    let outcome = {
        let runtime = context.runtime();
        let mut guard = lock_runtime(&runtime)?;
        guard.record_callback_artifact_after_owner_ready_use_case(
            handle.object_kind(),
            handle.object_id(),
            handle.generation(),
            api,
            artifact_retain_result,
        )
    };
    finish_callback_registration_artifact_outcome(context, outcome)
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


pub fn execute_callback_unregistration_runtime_use_case(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
) -> BinderResult<()> {
    let runtime = context.runtime();
    let api = method.api();
    let outcome = execute_object_method_call_after_live(
        &runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        || Ok((method, ())),
        |runtime, token, ()| Ok(runtime.execute_callback_unregistration_for_object_use_case(
            handle.object_kind(),
            handle.object_id(),
            handle.generation(),
            api,
            token,
        )),
    )
    .map_err(|error| match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(status) => status,
    })?;
    finish_owner_callback_cleanup_outcome(context, outcome).map_err(status_from_hal_error)
}

enum CallbackArtifactRetainBridge<'a> {
    Frontend(&'a Strong<dyn IFrontendCallback>),
    Lnb(&'a Strong<dyn ILnbCallback>),
}

impl<'a> CallbackArtifactRetainBridge<'a> {
    fn retain(
        self,
        context: &SharedAidlServiceContext,
        handle: AidlObjectHandle,
    ) -> Result<(), HalError> {
        match self {
            Self::Frontend(callback) => context
                .retain_frontend_callback(handle, callback)
                .map_err(|error| error.into_hal_error("frontend callback store retain failed")),
            Self::Lnb(callback) => context
                .retain_lnb_callback(handle, callback)
                .map_err(|error| error.into_hal_error("LNB callback store retain failed")),
        }
    }
}

fn execute_callback_registration_after_artifact_bridge(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
    artifact_retain_bridge: CallbackArtifactRetainBridge<'_>,
) -> BinderResult<()> {
    let runtime = context.runtime();
    let api = method.api();
    let outcome = execute_object_method_call_after_live(
        &runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        || Ok((method.clone(), artifact_retain_bridge)),
        |runtime, token, artifact_retain_bridge| {
            let artifact_retain_result = artifact_retain_bridge.retain(context, handle);
            Ok(runtime.execute_callback_registration_after_artifact_result_for_object_use_case(
                handle.object_kind(),
                handle.object_id(),
                handle.generation(),
                api,
                artifact_retain_result,
                token,
            ))
        },
    )
    .map_err(|error| match error {
        ObjectMethodTxnBuildError::Runtime(error) => status_from_hal_error(error),
        ObjectMethodTxnBuildError::Builder(status) => status,
    })?;
    finish_callback_registration_artifact_outcome(context, outcome).map_err(status_from_hal_error)
}

pub fn execute_frontend_callback_registration_runtime_use_case(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    callback: &Strong<dyn IFrontendCallback>,
) -> BinderResult<()> {
    execute_callback_registration_after_artifact_bridge(
        context,
        handle,
        AidlMethodCall::FrontendSetCallback,
        CallbackArtifactRetainBridge::Frontend(callback),
    )
}

pub fn execute_lnb_callback_registration_runtime_use_case(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    callback: &Strong<dyn ILnbCallback>,
) -> BinderResult<()> {
    execute_callback_registration_after_artifact_bridge(
        context,
        handle,
        AidlMethodCall::LnbSetCallback,
        CallbackArtifactRetainBridge::Lnb(callback),
    )
}


struct AidlObjectCloseRuntimeExecutor<'a> {
    context: &'a SharedAidlServiceContext,
}

impl<'a> AidlObjectCloseRuntimeExecutor<'a> {
    fn new(context: &'a SharedAidlServiceContext) -> Self {
        Self { context }
    }
}

impl<'a> ObjectCloseRuntimeExecutor for AidlObjectCloseRuntimeExecutor<'a> {
    fn execute_runtime_cleanup(
        &mut self,
        command: ObjectRuntimeCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure> {
        let runtime = self.context.runtime();
        let mut guard = runtime.lock().map_err(|_| {
            ObjectCloseCleanupFailure::new(
                CleanupStep::UnregisterRuntime,
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned during object close runtime cleanup command",
                ),
            )
        })?;
        command.execute(&mut guard)
    }
}

struct AidlObjectArtifactCleanupExecutor<'a> {
    context: &'a SharedAidlServiceContext,
}

impl<'a> AidlObjectArtifactCleanupExecutor<'a> {
    fn new(context: &'a SharedAidlServiceContext) -> Self {
        Self { context }
    }
}

struct AidlObjectDomainCleanupExecutor<'a> {
    context: &'a SharedAidlServiceContext,
}

impl<'a> AidlObjectDomainCleanupExecutor<'a> {
    fn new(context: &'a SharedAidlServiceContext) -> Self {
        Self { context }
    }

    fn handle_from_domain_command(command: ObjectDomainCleanupCommand) -> AidlObjectHandle {
        AidlObjectHandle::new(command.object_kind(), command.object_id(), command.generation())
    }

    fn cleanup_frontend(
        &self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        let handle = Self::handle_from_domain_command(command);
        close_frontend_object_cleanup_use_case(
            self.context.runtime(),
            handle.object_id(),
            handle.generation(),
            FrontendWorkerCancelReason::FrontendClosing,
        )?
        .cleanup_result
    }

    fn cleanup_lnb(
        &self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        let runtime = self.context.runtime();
        let mut guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned during LNB domain cleanup",
            )
        })?;
        guard.close_lnb_explicit_after_object_close_begin(
            command.object_id(),
            command.generation(),
        )
    }

    fn record_lnb_drop_leak(
        &self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        let runtime = self.context.runtime();
        let mut guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned during LNB drop-leak domain cleanup",
            )
        })?;
        let lnb_id = lnb_public_id_for_live_object_result(
            &guard,
            command.object_id(),
            command.generation(),
        )?;
        guard.record_lnb_drop_leak(lnb_id)
    }
}

impl<'a> ObjectDomainCleanupExecutor for AidlObjectDomainCleanupExecutor<'a> {
    fn execute_frontend_cleanup(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        self.cleanup_frontend(command)
    }

    fn execute_lnb_cleanup(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        self.cleanup_lnb(command)
    }

    fn execute_lnb_drop_leak_record(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        self.record_lnb_drop_leak(command)
    }
}

fn handle_from_artifact_cleanup_command(command: &ObjectArtifactCleanupCommand) -> AidlObjectHandle {
    AidlObjectHandle::new(command.object_kind(), command.object_id(), command.generation())
}

impl<'a> AidlObjectArtifactCleanupExecutor<'a> {
    fn execute_callback_cleanup_command(
        &self,
        command: ObjectArtifactCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure> {
        let step = command.step();
        let owner_command = command.owner_callback_cleanup_command().copied().ok_or_else(|| {
            ObjectCloseCleanupFailure::new(
                step,
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "callback artifact cleanup command missing service_runtime owner callback command",
                ),
            )
        })?;
        let runtime = self.context.runtime();
        let mut guard = lock_runtime(&runtime)
            .map_err(|error| ObjectCloseCleanupFailure::new(step, error))?;
        let artifact_result = self.context.clear_owner_callback_artifacts_bridge(&owner_command);
        guard
            .finish_object_close_callback_cleanup_outcome(owner_command, artifact_result)
            .map(|_| ())
            .map_err(|error| ObjectCloseCleanupFailure::new(step, error))
    }
}

impl<'a> ObjectArtifactCleanupExecutor for AidlObjectArtifactCleanupExecutor<'a> {
    fn execute_owner_callback_cleanup(
        &mut self,
        command: ObjectArtifactCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure> {
        self.execute_callback_cleanup_command(command)
    }

    fn execute_descendant_callback_cleanup(
        &mut self,
        command: ObjectArtifactCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure> {
        self.execute_callback_cleanup_command(command)
    }

    fn execute_lnb_owner_loss_callback_cleanup(
        &mut self,
        command: ObjectArtifactCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure> {
        self.execute_callback_cleanup_command(command)
    }

    fn execute_dvr_status_notifier_cleanup(
        &mut self,
        command: ObjectArtifactCleanupCommand,
    ) -> Result<(), ObjectCloseCleanupFailure> {
        let step = command.step();
        let handle = handle_from_artifact_cleanup_command(&command);
        stop_dvr_status_notifier(self.context, handle)
            .map_err(|error| ObjectCloseCleanupFailure::new(step, error))
    }
}

fn execute_close_cleanup_plan_with_executor(
    context: &SharedAidlServiceContext,
    plan: ObjectCloseUseCasePlan,
) -> Result<(), ObjectCloseCleanupFailure> {
    let mut runtime_executor = AidlObjectCloseRuntimeExecutor::new(context);
    let mut domain_executor = AidlObjectDomainCleanupExecutor::new(context);
    let mut artifact_executor = AidlObjectArtifactCleanupExecutor::new(context);
    plan.execute_cleanup_with_executor(
        &mut runtime_executor,
        &mut domain_executor,
        &mut artifact_executor,
    )
}

fn finish_object_close_plan(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    cleanup_result: Result<(), ObjectCloseCleanupFailure>,
) -> BinderResult<()> {
    let runtime = context.runtime();
    let mut guard = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    finish_object_close_use_case(
        &mut guard,
        handle.object_id(),
        handle.generation(),
        cleanup_result,
    )
    .map_err(status_from_hal_error)
}

mod drop_leak;
pub use drop_leak::drop_leak_object_from_drop;

pub fn close_object_after_close_preflight(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
) -> BinderResult<()> {
    let close_plan = {
        let runtime = context.runtime();
        let mut guard = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
        close_object_use_case(
            &mut guard,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            method,
        )
        .map_err(status_from_hal_error)?
    };
    let cleanup_result = execute_close_cleanup_plan_with_executor(context, close_plan);
    finish_object_close_plan(context, handle, cleanup_result)
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
    use maleicacid_tuner_hal2_service_runtime::{RuntimeObjectLifecycle, RuntimeOwnerRelation};

    fn shared_runtime_with_live_object(
        kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        public_runtime_id: i64,
    ) -> SharedTunerRuntime {
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let mut guard = runtime.lock().unwrap();
        let ledger_id = match kind {
            AidlObjectKind::Demux => guard.allocate_demux_runtime().unwrap().id.0 as i64,
            AidlObjectKind::Filter => {
                let demux = guard.allocate_demux_runtime().unwrap();
                let filter = guard.allocate_filter_runtime(demux.id.0).unwrap();
                guard
                    .register_demux_filter_runtime(
                        demux.id.0,
                        filter.id.0,
                        &maleicacid_tuner_hal2_demux::OpenFilterRequest {
                            open_type: maleicacid_tuner_hal2_demux::FilterOpenType::TsRaw,
                            buffer_size: 4096,
                            callback_present: true,
                        },
                    )
                    .unwrap();
                filter.id.0 as i64
            }
            AidlObjectKind::Dvr => {
                let demux = guard.allocate_demux_runtime().unwrap();
                let dvr = guard.allocate_dvr_runtime(demux.id.0).unwrap();
                guard
                    .register_demux_dvr_runtime(
                        demux.id.0,
                        dvr.id.0,
                        &maleicacid_tuner_hal2_binder_adapter::OpenDvrRequest {
                            kind: maleicacid_tuner_hal2_binder_adapter::DvrOpenKind::Record,
                            buffer_size: 4096,
                        },
                        true,
                    )
                    .unwrap();
                dvr.id.0 as i64
            }
            AidlObjectKind::Descrambler => guard.allocate_descrambler_runtime().unwrap().id.0 as i64,
            AidlObjectKind::Frontend | AidlObjectKind::Lnb | AidlObjectKind::Tuner => {
                public_runtime_id
            }
        };
        guard
            .register_aidl_object_for_runtime(
                kind,
                object_id,
                generation,
                ledger_id,
                RuntimeOwnerRelation::Root,
            )
            .unwrap();
        drop(guard);
        runtime
    }

    fn shared_runtime_with_object_table_only(
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


    fn record_callback_registration_for_test(
        runtime: &SharedTunerRuntime,
        handle: AidlObjectHandle,
        api: AidlApi,
    ) {
        let mut guard = runtime.lock().unwrap();
        let outcome = guard.record_callback_artifact_after_owner_ready_use_case(
            handle.object_kind(),
            handle.object_id(),
            handle.generation(),
            api,
            Ok(()),
        );
        guard
            .finish_callback_registration_after_artifact_result_use_case(outcome, None)
            .unwrap();
    }

    fn close_live_object_for_test(
        runtime: &SharedTunerRuntime,
        handle: AidlObjectHandle,
        method: AidlMethodCall,
    ) {
        let mut guard = runtime.lock().unwrap();
        close_object_use_case(
            &mut guard,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            method,
        )
        .expect("public close use-case begins close");
        finish_object_close_use_case(&mut guard, handle.object_id(), handle.generation(), Ok(()))
            .expect("public close use-case commits close");
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
    fn drop_leak_registry_missing_is_reported_after_quarantine() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_011),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_object_table_only(
            AidlObjectKind::Filter,
            AidlObjectId(91_011),
            AidlObjectGeneration(1),
            91_011,
        );

        let result = drop_leak_object(&context_for_runtime(&runtime), handle);

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
        let runtime = shared_runtime_with_object_table_only(
            AidlObjectKind::Filter,
            AidlObjectId(91_012),
            AidlObjectGeneration(1),
            91_012,
        );
        let context = context_for_runtime(&runtime);
        let before = context.drop_leak_error_record_count();

        drop_leak_object_from_drop(&context, handle);

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
        context.clear_owner_callbacks_for_test(handle).unwrap();
        context
            .retain_test_callback_marker(handle, AidlApi::DemuxOpenFilter)
            .unwrap();
        record_callback_registration_for_test(&runtime, handle, AidlApi::DemuxOpenFilter);

        drop_leak_object(&context, handle).unwrap();

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
        record_callback_registration_for_test(&runtime, filter_handle, AidlApi::DemuxOpenFilter);

        let result = drop_leak_object(&context, demux_handle);

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
        context.clear_owner_callbacks_for_test(handle).unwrap();
        context
            .retain_test_callback_marker(handle, AidlApi::LnbSetCallback)
            .unwrap();
        record_callback_registration_for_test(&runtime, handle, AidlApi::LnbSetCallback);

        assert!(
            drop_leak_object(&context, handle).is_err()
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
    fn close_object_after_close_preflight_closes_live_object_and_commits_closed() {
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

        close_object_after_close_preflight(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::DemuxClose,
        )
        .expect("close succeeds");

        let runtime = runtime.lock().unwrap();
        assert_eq!(
            runtime.aidl_object_lifecycle(AidlObjectId(91_003)).unwrap(),
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
        let runtime = shared_runtime_with_object_table_only(
            AidlObjectKind::Filter,
            AidlObjectId(91_011),
            AidlObjectGeneration(1),
            91_011,
        );
        close_live_object_for_test(&runtime, handle, AidlMethodCall::FilterClose);
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
            AidlObjectKind::Lnb,
            AidlObjectId(91_004),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_004),
            AidlObjectGeneration(1),
            91_004,
        );

        let result = close_object_after_close_preflight(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::LnbClose,
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
        context.clear_owner_callbacks_for_test(handle).unwrap();
        let retain_result = retain_test_callback_marker_as_hal(
            &context,
            handle,
            AidlApi::DemuxOpenFilter,
        );
        close_live_object_for_test(&runtime, handle, AidlMethodCall::FilterClose);
        let result = finish_callback_artifact_registration_after_owner_ready_hal(
            &context,
            handle,
            AidlApi::DemuxOpenFilter,
            retain_result,
        );

        assert!(result.is_err());
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::DemuxOpenFilter)
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
        context.clear_owner_callbacks_for_test(handle).unwrap();
        close_live_object_for_test(&runtime, handle, AidlMethodCall::LnbClose);
        let retain_result = retain_test_callback_marker_as_hal(
            &context,
            handle,
            AidlApi::LnbSetCallback,
        );
        let result = finish_callback_artifact_registration_after_owner_ready_hal(
            &context,
            handle,
            AidlApi::LnbSetCallback,
            retain_result,
        );

        assert!(result.is_err());
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::LnbSetCallback)
            .unwrap());
        assert_eq!(runtime.lock().unwrap().callback_registration_count(), 0);
    }


    #[test]
    fn callback_artifact_bridge_clears_store_without_owning_runtime_registry_policy() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_060),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_060),
            AidlObjectGeneration(1),
            91_060,
        );
        let context = context_for_runtime(&runtime);
        context
            .retain_test_callback_marker(handle, AidlApi::DemuxOpenFilter)
            .unwrap();
        record_callback_registration_for_test(&runtime, handle, AidlApi::DemuxOpenFilter);
        assert!(context
            .has_callback_for_owner(handle, AidlApi::DemuxOpenFilter)
            .unwrap());
        assert_eq!(runtime.lock().unwrap().callback_registration_count(), 1);

        let command = {
            let mut guard = runtime.lock().unwrap();
            *guard
                .begin_filter_child_open_object_failure_cleanup_use_case(
                    handle.object_id(),
                    handle.generation(),
                    91_060,
                )
                .command()
        };

        context
            .clear_owner_callback_artifacts_bridge(&command)
            .expect("bridge should clear callback artifact");

        assert!(!context
            .has_callback_for_owner(handle, AidlApi::DemuxOpenFilter)
            .unwrap());
        assert_eq!(
            runtime.lock().unwrap().callback_registration_count(),
            1,
            "artifact bridge must not own runtime callback registry mutation"
        );
    }

    #[test]
    fn callback_unregistration_success_clears_runtime_and_artifact() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Frontend,
            AidlObjectId(91_020),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Frontend,
            AidlObjectId(91_020),
            AidlObjectGeneration(1),
            91_020,
        );
        let context = context_for_runtime(&runtime);
        let retain_result = retain_test_callback_marker_as_hal(
            &context,
            handle,
            AidlApi::FrontendSetCallback,
        );
        finish_callback_artifact_registration_after_owner_ready_hal(
            &context,
            handle,
            AidlApi::FrontendSetCallback,
            retain_result,
        )
        .expect("callback registration succeeds");

        let result: BinderResult<()> = execute_callback_unregistration_runtime_use_case(
            &context,
            handle,
            AidlMethodCall::FrontendSetCallback,
        );

        assert!(result.is_ok());
        assert_eq!(runtime.lock().unwrap().callback_registration_count(), 0);
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::FrontendSetCallback)
            .unwrap());
    }

    #[test]
    fn close_object_after_close_preflight_allows_cleanup_failed_retry() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Demux,
            AidlObjectId(91_006),
            AidlObjectGeneration(1),
        );
        let runtime = shared_runtime_with_object_table_only(
            AidlObjectKind::Demux,
            AidlObjectId(91_006),
            AidlObjectGeneration(1),
            1,
        );

        let first = close_object_after_close_preflight(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::DemuxClose,
        );
        assert!(first.is_err());
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .aidl_object_lifecycle(AidlObjectId(91_006))
                .unwrap(),
            RuntimeObjectLifecycle::CleanupFailed {
                step: CleanupStep::UnregisterRuntime
            }
        );

        runtime.lock().unwrap().allocate_demux_runtime().unwrap();
        close_object_after_close_preflight(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::DemuxClose,
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
    fn close_object_after_close_preflight_rejects_closed_object() {
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

        close_object_after_close_preflight(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::FilterClose,
        )
        .expect("first close succeeds");

        assert!(close_object_after_close_preflight(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::FilterClose,
        )
        .is_err());

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
