use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IFrontendCallback::IFrontendCallback;
use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnbCallback::ILnbCallback;
use binder::{Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlFailureSource, AidlMethodCall, AidlStatusMapper, TunerStatusCode,
};
use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};
use maleicacid_tuner_hal2_device::FrontendWorkerCancelReason;
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
use maleicacid_tuner_hal2_service_runtime::{
    close_frontend_object_cleanup_use_case, close_object_use_case,
    execute_object_method_call_after_live, execute_object_query_call_after_live,
    execute_object_query_call_after_live_with_aidl_input_conversion,
    execute_shared_object_method_call_after_live, finish_object_close_use_case,
    preflight_object_method_after_live_plan_only, CallbackArtifactCleanupResult,
    CallbackArtifactRuntimeSplitDiagnosticRecord, CallbackArtifactRuntimeSplitOutcome,
    CallbackArtifactRuntimeSplitPhase, CallbackRegistrationArtifactOutcome,
    ObjectArtifactCleanupCommand, ObjectArtifactCleanupExecutor, ObjectCleanupDiagnosticRecord,
    ObjectCleanupExecutionReport, ObjectCloseCleanupFailure, ObjectCloseRuntimeExecutor,
    ObjectCloseUseCasePlan, ObjectDomainCleanupCommand, ObjectDomainCleanupExecutor,
    ObjectMethodExecutionToken, ObjectMethodTxnBuildError, ObjectQueryRequest, ObjectQueryResponse,
    ObjectRuntimeCleanupCommand, OwnerCallbackCleanupArtifactCommand,
    OwnerCallbackCleanupUseCaseOutcome, TunerServiceRuntime,
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

fn callback_artifact_runtime_finish_lock_failure_error(
    context: &SharedAidlServiceContext,
    phase: CallbackArtifactRuntimeSplitPhase,
    command: OwnerCallbackCleanupArtifactCommand,
    artifact_result: &Result<CallbackArtifactCleanupResult, HalError>,
    runtime_error: HalError,
) -> HalError {
    let artifact_error = artifact_result.as_ref().err().cloned();
    let mut runtime_or_record_error = runtime_error.clone();
    if let Some(outcome) = CallbackArtifactRuntimeSplitOutcome::from_results(
        artifact_error.clone(),
        Some(runtime_error),
    ) {
        if let Err(record_error) = context
            .record_callback_artifact_runtime_split_finish_lock_failure(
                CallbackArtifactRuntimeSplitDiagnosticRecord::owner(
                    phase,
                    command.owner_kind(),
                    command.owner_id(),
                    command.owner_generation(),
                    outcome,
                ),
            )
        {
            runtime_or_record_error = compose_primary_cleanup_failure(
                "callback artifact/runtime split diagnostic record failed after runtime finish lock failure",
                runtime_or_record_error,
                record_error,
            );
        }
    }
    match artifact_error {
        Some(artifact_error) => compose_primary_cleanup_failure(
            command.cleanup_failure_message(),
            artifact_error,
            runtime_or_record_error,
        ),
        None => runtime_or_record_error,
    }
}

fn callback_artifact_registration_runtime_lock_failure_error(
    context: &SharedAidlServiceContext,
    command: OwnerCallbackCleanupArtifactCommand,
    artifact_result: &Result<(), HalError>,
    runtime_error: HalError,
) -> HalError {
    let artifact_error = artifact_result.as_ref().err().cloned();
    let mut runtime_or_record_error = runtime_error.clone();
    let mut rollback_cleanup_error = None;
    if artifact_error.is_none() {
        if let Err(cleanup_error) = context.clear_owner_callback_artifacts_bridge(&command) {
            runtime_or_record_error = compose_primary_cleanup_failure(
                command.cleanup_failure_message(),
                runtime_or_record_error,
                cleanup_error.clone(),
            );
            rollback_cleanup_error = Some(cleanup_error);
        }
    }
    let split_outcome = match rollback_cleanup_error.clone() {
        Some(cleanup_error) => Some(
            CallbackArtifactRuntimeSplitOutcome::runtime_finish_and_artifact_cleanup_failure(
                runtime_error.clone(),
                cleanup_error,
            ),
        ),
        None => CallbackArtifactRuntimeSplitOutcome::from_results(
            artifact_error.clone(),
            Some(runtime_error.clone()),
        ),
    };
    if let Some(outcome) = split_outcome {
        if let Err(record_error) = context
            .record_callback_artifact_runtime_split_finish_lock_failure(
                CallbackArtifactRuntimeSplitDiagnosticRecord::owner(
                    CallbackArtifactRuntimeSplitPhase::RegistrationRollbackFinish,
                    command.owner_kind(),
                    command.owner_id(),
                    command.owner_generation(),
                    outcome,
                ),
            )
        {
            runtime_or_record_error = compose_primary_cleanup_failure(
                "callback artifact/runtime split diagnostic record failed after registration finish lock failure",
                runtime_or_record_error,
                record_error,
            );
        }
    }
    match artifact_error {
        Some(artifact_error) => compose_primary_cleanup_failure(
            "callback artifact retain failed before runtime registration finish lock failed",
            artifact_error,
            runtime_or_record_error,
        ),
        None => runtime_or_record_error,
    }
}

fn callback_registration_finish_runtime_lock_failure_error(
    context: &SharedAidlServiceContext,
    command: OwnerCallbackCleanupArtifactCommand,
    primary_error: Option<HalError>,
    runtime_error: HalError,
) -> HalError {
    let mut runtime_or_record_error = runtime_error.clone();
    if let Some(outcome) = CallbackArtifactRuntimeSplitOutcome::from_results(
        primary_error.clone(),
        Some(runtime_error),
    ) {
        if let Err(record_error) = context
            .record_callback_artifact_runtime_split_finish_lock_failure(
                CallbackArtifactRuntimeSplitDiagnosticRecord::owner(
                    CallbackArtifactRuntimeSplitPhase::RegistrationRollbackFinish,
                    command.owner_kind(),
                    command.owner_id(),
                    command.owner_generation(),
                    outcome,
                ),
            )
        {
            runtime_or_record_error = compose_primary_cleanup_failure(
                "callback artifact/runtime split diagnostic record failed after registration finish lock failure",
                runtime_or_record_error,
                record_error,
            );
        }
    }
    match primary_error {
        Some(primary_error) => compose_primary_cleanup_failure(
            "callback artifact registration finish failed after artifact failure",
            primary_error,
            runtime_or_record_error,
        ),
        None => runtime_or_record_error,
    }
}

pub(crate) fn finish_owner_callback_cleanup_outcome<T>(
    context: &SharedAidlServiceContext,
    outcome: OwnerCallbackCleanupUseCaseOutcome<T>,
) -> Result<T, HalError> {
    let command = *outcome.command();
    let artifact_cleanup_result = context.clear_owner_callback_artifacts_bridge(&command);
    let runtime = context.runtime();
    let mut guard = match lock_runtime(&runtime) {
        Ok(guard) => guard,
        Err(runtime_error) => {
            return Err(callback_artifact_runtime_finish_lock_failure_error(
                context,
                CallbackArtifactRuntimeSplitPhase::OwnerCleanupFinish,
                command,
                &artifact_cleanup_result,
                runtime_error,
            ));
        }
    };
    guard.finish_owner_callback_cleanup_outcome(outcome, artifact_cleanup_result)
}

fn finish_callback_registration_artifact_outcome(
    context: &SharedAidlServiceContext,
    outcome: CallbackRegistrationArtifactOutcome,
) -> Result<(), HalError> {
    if !outcome.requires_runtime_finish() {
        return outcome.into_primary_result();
    }

    let finish_lock_failure_command = outcome.finish_lock_failure_command();
    let primary_error = outcome.primary_error().cloned();
    let rollback_command = outcome.rollback_command().copied();
    let rollback_result = rollback_command
        .as_ref()
        .map(|command| context.clear_owner_callback_artifacts_bridge(command));
    let runtime = context.runtime();
    let mut guard = match lock_runtime(&runtime) {
        Ok(guard) => guard,
        Err(runtime_error) => {
            if let (Some(command), Some(ref artifact_result)) =
                (rollback_command, rollback_result.as_ref())
            {
                return Err(callback_artifact_runtime_finish_lock_failure_error(
                    context,
                    CallbackArtifactRuntimeSplitPhase::RegistrationRollbackFinish,
                    command,
                    artifact_result,
                    runtime_error,
                ));
            }
            return Err(callback_registration_finish_runtime_lock_failure_error(
                context,
                finish_lock_failure_command,
                primary_error,
                runtime_error,
            ));
        }
    };
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

pub(crate) fn execute_object_runtime_use_case<T, F>(
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

pub(crate) fn execute_shared_object_runtime_use_case<T, F>(
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

pub(crate) fn execute_object_query_use_case(
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

pub(crate) fn execute_object_query_use_case_with_aidl_input_conversion<Build>(
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

pub(crate) fn execute_object_runtime_use_case_with_request_builder<T, B, Build, Execute>(
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

pub(crate) fn execute_shared_object_runtime_use_case_with_request_builder<T, B, Build, Execute>(
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

pub(crate) fn plan_unavailable_object_method_use_case<Build>(
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
        |runtime, token, ()| {
            Ok(runtime.execute_callback_unregistration_for_object_use_case(
                handle.object_kind(),
                handle.object_id(),
                handle.generation(),
                api,
                token,
            ))
        },
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
    let registration_finish_lock_failure_command = {
        let guard = lock_runtime(&runtime).map_err(status_from_hal_error)?;
        guard.plan_callback_registration_runtime_finish_lock_failure_cleanup_command(
            handle.object_kind(),
            handle.object_id(),
            handle.generation(),
            api,
        )
    };
    let outcome = execute_shared_object_method_call_after_live(
        &runtime,
        handle.object_id(),
        handle.generation(),
        handle.object_kind(),
        || {
            Ok((
                method.clone(),
                (
                    artifact_retain_bridge,
                    registration_finish_lock_failure_command,
                ),
            ))
        },
        |runtime, token, (artifact_retain_bridge, registration_finish_lock_failure_command)| {
            let artifact_retain_result = artifact_retain_bridge.retain(context, handle);
            let mut guard = match lock_runtime(&runtime) {
                Ok(guard) => guard,
                Err(runtime_error) => {
                    return Err(callback_artifact_registration_runtime_lock_failure_error(
                        context,
                        registration_finish_lock_failure_command,
                        &artifact_retain_result,
                        runtime_error,
                    ));
                }
            };
            Ok(
                guard.execute_callback_registration_after_artifact_result_for_object_use_case(
                    handle.object_kind(),
                    handle.object_id(),
                    handle.generation(),
                    api,
                    artifact_retain_result,
                    token,
                ),
            )
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
        AidlObjectHandle::new(
            command.object_kind(),
            command.object_id(),
            command.generation(),
        )
    }

    fn cleanup_frontend(&self, command: ObjectDomainCleanupCommand) -> Result<(), HalError> {
        let handle = Self::handle_from_domain_command(command);
        close_frontend_object_cleanup_use_case(
            self.context.runtime(),
            handle.object_id(),
            handle.generation(),
            FrontendWorkerCancelReason::FrontendClosing,
        )?
        .cleanup_result
    }

    fn cleanup_lnb(&self, command: ObjectDomainCleanupCommand) -> Result<(), HalError> {
        let runtime = self.context.runtime();
        let mut guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned during LNB domain cleanup",
            )
        })?;
        guard.close_lnb_explicit_after_object_close_begin(command.object_id(), command.generation())
    }

    fn record_lnb_drop_leak(&self, command: ObjectDomainCleanupCommand) -> Result<(), HalError> {
        let runtime = self.context.runtime();
        let mut guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned during LNB drop-leak domain cleanup",
            )
        })?;
        guard.record_lnb_drop_leak_after_domain_cleanup_command(command)
    }
}

impl<'a> ObjectDomainCleanupExecutor for AidlObjectDomainCleanupExecutor<'a> {
    fn execute_frontend_cleanup(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        self.cleanup_frontend(command)
    }

    fn execute_lnb_cleanup(&mut self, command: ObjectDomainCleanupCommand) -> Result<(), HalError> {
        self.cleanup_lnb(command)
    }

    fn execute_lnb_drop_leak_record(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        self.record_lnb_drop_leak(command)
    }
}

fn handle_from_artifact_cleanup_command(
    command: &ObjectArtifactCleanupCommand,
) -> AidlObjectHandle {
    AidlObjectHandle::new(
        command.object_kind(),
        command.object_id(),
        command.generation(),
    )
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
        let artifact_result = self
            .context
            .clear_owner_callback_artifacts_bridge(&owner_command);
        let mut guard = match lock_runtime(&runtime) {
            Ok(guard) => guard,
            Err(runtime_error) => {
                let error = callback_artifact_runtime_finish_lock_failure_error(
                    self.context,
                    CallbackArtifactRuntimeSplitPhase::ObjectCloseCleanupFinish,
                    owner_command,
                    &artifact_result,
                    runtime_error,
                );
                return Err(ObjectCloseCleanupFailure::new(step, error));
            }
        };
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
) -> ObjectCleanupExecutionReport {
    let mut runtime_executor = AidlObjectCloseRuntimeExecutor::new(context);
    let mut domain_executor = AidlObjectDomainCleanupExecutor::new(context);
    let mut artifact_executor = AidlObjectArtifactCleanupExecutor::new(context);
    plan.execute_cleanup_report_with_executor(
        &mut runtime_executor,
        &mut domain_executor,
        &mut artifact_executor,
    )
}

fn finish_object_close_plan(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    cleanup_report: ObjectCleanupExecutionReport,
) -> BinderResult<()> {
    let cleanup_result = cleanup_report.clone().into_result();
    let public_error = cleanup_result
        .clone()
        .err()
        .map(ObjectCloseCleanupFailure::into_error);
    let record_result =
        context.record_object_cleanup_diagnostic_fallback(ObjectCleanupDiagnosticRecord::close(
            handle.object_id(),
            handle.generation(),
            cleanup_report,
            public_error,
        ));
    let runtime = context.runtime();
    let finish_result = match runtime.lock() {
        Ok(mut guard) => finish_object_close_use_case(
            &mut guard,
            handle.object_id(),
            handle.generation(),
            cleanup_result,
        ),
        Err(_) => Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while finishing object close",
        )),
    };
    let result = match (finish_result, record_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(primary), Err(cleanup)) => Err(compose_primary_cleanup_failure(
            "object close diagnostic record failed after finish failure",
            primary,
            cleanup,
        )),
    };
    result.map_err(status_from_hal_error)
}

mod drop_leak;
pub(crate) use drop_leak::drop_leak_object_from_drop;

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
    let cleanup_report = execute_close_cleanup_plan_with_executor(context, close_plan);
    finish_object_close_plan(context, handle, cleanup_report)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::drop_leak::drop_leak_object;
    use super::*;
    use crate::service_context::AidlServiceContext;
    use maleicacid_tuner_hal2_binder_adapter::{
        AidlApi, AidlMethodCall, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
        RuntimeExecutableRequest,
    };
    use maleicacid_tuner_hal2_service_runtime::RuntimeOwnerRelation;

    fn shared_runtime_with_live_object(
        kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        public_runtime_id: i64,
    ) -> SharedTunerRuntime {
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        let ledger_id = match kind {
            AidlObjectKind::Demux => {
                let mut guard = runtime.lock().unwrap();
                let entry = guard
                    .open_demux_root_object(AidlMethodCall::PublicApi {
                        object: AidlObjectKind::Tuner,
                        api: AidlApi::TunerOpenDemux,
                    })
                    .unwrap();
                guard
                    .unregister_aidl_object_after_registration_failure(
                        entry.object_id(),
                        entry.generation(),
                    )
                    .unwrap();
                entry.public_runtime_id().0
            }
            AidlObjectKind::Filter => {
                let owner = {
                    let mut guard = runtime.lock().unwrap();
                    guard
                        .open_demux_root_object(AidlMethodCall::PublicApi {
                            object: AidlObjectKind::Tuner,
                            api: AidlApi::TunerOpenDemux,
                        })
                        .unwrap()
                };
                let runtime_open = execute_object_method_call_after_live(
                    &runtime,
                    owner.object_id(),
                    owner.generation(),
                    AidlObjectKind::Demux,
                    || -> Result<_, maleicacid_tuner_hal2_common::HalError> {
                        let request = maleicacid_tuner_hal2_demux::OpenFilterRequest {
                            open_type: maleicacid_tuner_hal2_demux::FilterOpenType::TsRaw,
                            buffer_size: 4096,
                            callback_present: true,
                        };
                        Ok((
                            AidlMethodCall::DemuxOpenFilter(RuntimeExecutableRequest::OpenFilter(
                                request.clone(),
                            )),
                            request,
                        ))
                    },
                    |runtime, dispatch, request| {
                        runtime.open_filter_child_runtime_for_demux_object(
                            owner.object_id(),
                            owner.generation(),
                            &request,
                            dispatch,
                        )
                    },
                )
                .unwrap();
                let mut guard = runtime.lock().unwrap();
                guard
                    .unregister_aidl_object_after_registration_failure(
                        runtime_open.runtime_entry.object_id(),
                        runtime_open.runtime_entry.generation(),
                    )
                    .unwrap();
                guard
                    .register_aidl_object_for_runtime(
                        kind,
                        object_id,
                        generation,
                        i64::from(runtime_open.filter_id),
                        RuntimeOwnerRelation::Demux {
                            demux: owner.object_id(),
                            generation: owner.generation(),
                        },
                    )
                    .unwrap();
                drop(guard);
                return runtime;
            }
            AidlObjectKind::Dvr => {
                let owner = {
                    let mut guard = runtime.lock().unwrap();
                    guard
                        .open_demux_root_object(AidlMethodCall::PublicApi {
                            object: AidlObjectKind::Tuner,
                            api: AidlApi::TunerOpenDemux,
                        })
                        .unwrap()
                };
                let runtime_open = execute_object_method_call_after_live(
                    &runtime,
                    owner.object_id(),
                    owner.generation(),
                    AidlObjectKind::Demux,
                    || -> Result<_, maleicacid_tuner_hal2_common::HalError> {
                        let request = maleicacid_tuner_hal2_binder_adapter::OpenDvrRequest {
                            kind: maleicacid_tuner_hal2_binder_adapter::DvrOpenKind::Record,
                            buffer_size: 4096,
                        };
                        Ok((AidlMethodCall::DemuxOpenDvr(request.clone()), request))
                    },
                    |runtime, dispatch, request| {
                        runtime.open_dvr_child_runtime_for_demux_object(
                            owner.object_id(),
                            owner.generation(),
                            request,
                            dispatch,
                        )
                    },
                )
                .unwrap();
                let mut guard = runtime.lock().unwrap();
                guard
                    .unregister_aidl_object_after_registration_failure(
                        runtime_open.runtime_entry.object_id(),
                        runtime_open.runtime_entry.generation(),
                    )
                    .unwrap();
                guard
                    .register_aidl_object_for_runtime(
                        kind,
                        object_id,
                        generation,
                        i64::from(runtime_open.dvr_id),
                        RuntimeOwnerRelation::Demux {
                            demux: owner.object_id(),
                            generation: owner.generation(),
                        },
                    )
                    .unwrap();
                drop(guard);
                return runtime;
            }
            AidlObjectKind::Descrambler => {
                let mut guard = runtime.lock().unwrap();
                let entry = guard
                    .open_descrambler_root_object(AidlMethodCall::PublicApi {
                        object: AidlObjectKind::Tuner,
                        api: AidlApi::TunerOpenDescrambler,
                    })
                    .unwrap();
                guard
                    .unregister_aidl_object_after_registration_failure(
                        entry.object_id(),
                        entry.generation(),
                    )
                    .unwrap();
                entry.public_runtime_id().0
            }
            AidlObjectKind::Frontend | AidlObjectKind::Lnb | AidlObjectKind::Tuner => {
                public_runtime_id
            }
        };
        let mut guard = runtime.lock().unwrap();
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

        assert!(!context
            .has_callback_for_owner(handle, AidlApi::DemuxOpenFilter)
            .unwrap());
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
        assert!(!context
            .has_callback_for_owner(filter_handle, AidlApi::DemuxOpenFilter)
            .unwrap());
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

        assert!(drop_leak_object(&context, handle).is_err());

        assert!(!context
            .has_callback_for_owner(handle, AidlApi::LnbSetCallback)
            .unwrap());
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
        {
            let mut guard = runtime.lock().unwrap();
            close_object_use_case(
                &mut guard,
                handle.object_id(),
                handle.generation(),
                handle.object_kind(),
                AidlMethodCall::FilterClose,
            )
            .expect("public close use-case begins close");
            finish_object_close_use_case(
                &mut guard,
                handle.object_id(),
                handle.generation(),
                Ok(()),
            )
            .expect("public close use-case commits close");
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
        let retain_result =
            retain_test_callback_marker_as_hal(&context, handle, AidlApi::DemuxOpenFilter);
        {
            let mut guard = runtime.lock().unwrap();
            close_object_use_case(
                &mut guard,
                handle.object_id(),
                handle.generation(),
                handle.object_kind(),
                AidlMethodCall::FilterClose,
            )
            .expect("public close use-case begins close");
            finish_object_close_use_case(
                &mut guard,
                handle.object_id(),
                handle.generation(),
                Ok(()),
            )
            .expect("public close use-case commits close");
        }
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
        {
            let mut guard = runtime.lock().unwrap();
            close_object_use_case(
                &mut guard,
                handle.object_id(),
                handle.generation(),
                handle.object_kind(),
                AidlMethodCall::LnbClose,
            )
            .expect("public close use-case begins close");
            finish_object_close_use_case(
                &mut guard,
                handle.object_id(),
                handle.generation(),
                Ok(()),
            )
            .expect("public close use-case commits close");
        }
        let retain_result =
            retain_test_callback_marker_as_hal(&context, handle, AidlApi::LnbSetCallback);
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
        let retain_result =
            retain_test_callback_marker_as_hal(&context, handle, AidlApi::FrontendSetCallback);
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
        assert!(!context
            .has_callback_for_owner(handle, AidlApi::FrontendSetCallback)
            .unwrap());
    }

    #[test]
    fn close_object_after_close_preflight_retries_cleanup_failed_object() {
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

        let second = close_object_after_close_preflight(
            &context_for_runtime(&runtime),
            handle,
            AidlMethodCall::DemuxClose,
        );

        assert!(second.is_err());
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
    }
}
