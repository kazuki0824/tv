use super::{
    execute_close_after_preflight_once, status_from_hal_error, AidlObjectArtifactCleanupExecutor,
    AidlObjectCloseRuntimeExecutor, AidlObjectDomainCleanupExecutor, AidlObjectHandle,
    BinderResult, ObjectCloseCleanupFailure,
};
use crate::service_context::SharedAidlServiceContext;
use maleicacid_tuner_hal2_common::compose_primary_cleanup_failure;
use maleicacid_tuner_hal2_domain_request::{AidlMethodCall, AidlObjectKind};
use maleicacid_tuner_hal2_service_runtime::{
    quarantine_object_drop_leak_use_case, ObjectCleanupDiagnosticRecord, TunerServiceRuntime,
};

pub(crate) fn drop_leak_object(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    if context
        .cleanup_is_terminal_for_handle(handle)
        .map_err(status_from_hal_error)?
    {
        return Ok(());
    }
    let method = close_method_for_drop(handle.object_kind()).map_err(status_from_hal_error)?;
    let result =
        execute_close_after_preflight_once(context, handle, method).map_err(status_from_hal_error);
    if result.is_err() {
        context
            .enqueue_cleanup_retry(handle)
            .map_err(status_from_hal_error)?;
    }
    result
}

fn close_method_for_drop(
    kind: AidlObjectKind,
) -> Result<AidlMethodCall, maleicacid_tuner_hal2_common::HalError> {
    match kind {
        AidlObjectKind::Frontend => Ok(AidlMethodCall::FrontendClose),
        AidlObjectKind::Demux => Ok(AidlMethodCall::DemuxClose),
        AidlObjectKind::Filter => Ok(AidlMethodCall::FilterClose),
        AidlObjectKind::Dvr => Ok(AidlMethodCall::DvrClose),
        AidlObjectKind::Descrambler => Ok(AidlMethodCall::DescramblerClose),
        AidlObjectKind::Lnb => Ok(AidlMethodCall::LnbClose),
        AidlObjectKind::Tuner => Err(maleicacid_tuner_hal2_common::HalError::internal(
            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
            "root tuner object reached object Drop cleanup",
        )),
    }
}

/// 論理隔離後の物理cleanupと診断書き込みを、別々の型付き結果として保持する。
pub(crate) struct DropLeakQuarantineOutcome {
    pub(crate) cleanup_result: Result<(), maleicacid_tuner_hal2_common::HalError>,
    pub(crate) diagnostic_result: Result<(), maleicacid_tuner_hal2_common::HalError>,
    pub(crate) uncontained_failure: bool,
}

impl DropLeakQuarantineOutcome {
    fn into_result(self) -> Result<(), maleicacid_tuner_hal2_common::HalError> {
        match (self.cleanup_result, self.diagnostic_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(primary), Err(cleanup)) => Err(compose_primary_cleanup_failure(
                "drop-leak cleanup diagnostic record failed after terminalization failure",
                primary,
                cleanup,
            )),
        }
    }
}

pub(crate) fn quarantine_drop_leak_object_with_evidence(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<DropLeakQuarantineOutcome, maleicacid_tuner_hal2_common::HalError> {
    let runtime_handle = context.runtime();
    let plan = {
        let mut runtime = TunerServiceRuntime::lock_shared(&runtime_handle, "Drop漏れ隔離")?;
        quarantine_object_drop_leak_use_case(&mut runtime, handle.object_id(), handle.generation())?
    };
    let mut runtime_executor = AidlObjectCloseRuntimeExecutor::new(context);
    let mut domain_executor = AidlObjectDomainCleanupExecutor::new(context);
    let mut artifact_executor = AidlObjectArtifactCleanupExecutor::new(context);
    let report = plan.execute_terminalization_report_with_executor(
        &mut runtime_executor,
        &mut domain_executor,
        &mut artifact_executor,
    );
    let uncontained_failure = maleicacid_tuner_hal2_service_runtime::drop_leak_cleanup_has_uncontained_failure(&report);
    let cleanup_result = report
        .clone()
        .into_result()
        .map_err(ObjectCloseCleanupFailure::into_error);
    let diagnostic_result = context.record_object_cleanup_diagnostic_fallback(
        ObjectCleanupDiagnosticRecord::drop_leak_terminalization(
            handle.object_id(),
            handle.generation(),
            report,
            cleanup_result.clone().err(),
        ),
    );
    Ok(DropLeakQuarantineOutcome {
        cleanup_result,
        diagnostic_result,
        uncontained_failure,
    })
}

pub(crate) fn quarantine_drop_leak_object(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    quarantine_drop_leak_object_with_evidence(context, handle)
        .and_then(DropLeakQuarantineOutcome::into_result)
        .map_err(status_from_hal_error)
}

pub(crate) fn drop_leak_object_from_drop(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) {
    if handle.object_kind() == AidlObjectKind::Filter {
        let runtime_handle = context.runtime();
        let result = TunerServiceRuntime::lock_shared(&runtime_handle, "最終AV参照終了")
            .map_err(status_from_hal_error)
            .and_then(|mut runtime| {
                runtime
                    .finalize_filter_av_release_state_after_last_reference(
                        handle.object_id(),
                        handle.generation(),
                    )
                    .map_err(status_from_hal_error)
            });
        if let Err(status) = result {
            context.record_drop_leak_error(handle, &status);
        }
    }
    if let Err(status) = drop_leak_object(context, handle) {
        context.record_drop_leak_error(handle, &status);
    }
}
