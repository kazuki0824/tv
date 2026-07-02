use super::{
    status_from_hal_error, status_unknown_error, AidlObjectArtifactCleanupExecutor,
    AidlObjectCloseRuntimeExecutor, AidlObjectDomainCleanupExecutor, AidlObjectHandle,
    BinderResult, ObjectCloseCleanupFailure,
};
use crate::service_context::SharedAidlServiceContext;
use maleicacid_tuner_hal2_common::FirstErrorCollector;
use maleicacid_tuner_hal2_service_runtime::quarantine_object_drop_leak_use_case;

pub(super) fn drop_leak_object(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    let runtime_handle = context.runtime();
    let quarantine_result = {
        let mut runtime = runtime_handle.lock().map_err(|_| {
            status_unknown_error("service runtime lock poisoned during drop leak quarantine")
        })?;
        quarantine_object_drop_leak_use_case(&mut runtime, handle.object_id(), handle.generation())
    };

    let mut error_collector = FirstErrorCollector::new();
    if let Ok(plan) = &quarantine_result {
        let terminalization_result = {
            let mut runtime_executor = AidlObjectCloseRuntimeExecutor::new(context);
            let mut domain_executor = AidlObjectDomainCleanupExecutor::new(context);
            let mut artifact_executor = AidlObjectArtifactCleanupExecutor::new(context);
            plan.execute_terminalization_with_executor(
                &mut runtime_executor,
                &mut domain_executor,
                &mut artifact_executor,
            )
            .map_err(ObjectCloseCleanupFailure::into_error)
        };
        error_collector.push_result(terminalization_result);
    } else if let Err(error) = &quarantine_result {
        error_collector.push_error(error.clone());
    }

    match error_collector.into_result() {
        Err(error) => Err(status_from_hal_error(error)),
        Ok(()) => Ok(()),
    }
}

pub(crate) fn drop_leak_object_from_drop(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) {
    if let Err(status) = drop_leak_object(context, handle) {
        context.record_drop_leak_error(handle, &status);
    }
}
