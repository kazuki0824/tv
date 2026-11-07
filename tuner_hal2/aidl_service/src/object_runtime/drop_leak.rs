use super::{
    execute_close_after_preflight_once, status_from_hal_error, AidlObjectArtifactCleanupExecutor,
    AidlObjectCloseRuntimeExecutor, AidlObjectDomainCleanupExecutor, AidlObjectHandle,
    BinderResult, ObjectCloseCleanupFailure,
};
use crate::service_context::SharedAidlServiceContext;
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

/// 対象objectの隔離結果。Supervisor移管は物理cleanupの完了とは異なる。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DropLeakQuarantineOutcome {
    Completed,
    SupervisedPending,
    OwnedByExistingCleanup,
}

pub(crate) fn quarantine_drop_leak_object(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> BinderResult<DropLeakQuarantineOutcome> {
    let runtime_handle = context.runtime();
    let plan = {
        let mut runtime = TunerServiceRuntime::lock_shared(&runtime_handle, "Drop漏れ隔離")
            .map_err(status_from_hal_error)?;
        quarantine_object_drop_leak_use_case(&mut runtime, handle.object_id(), handle.generation())
            .map_err(status_from_hal_error)?
    };
    let Some(plan) = plan else {
        return Ok(DropLeakQuarantineOutcome::OwnedByExistingCleanup);
    };
    let report = plan.execute_terminalization_report_with_executor(
        &mut AidlObjectCloseRuntimeExecutor::new(context),
        &mut AidlObjectDomainCleanupExecutor::new(context),
        &mut AidlObjectArtifactCleanupExecutor::new(context),
    );
    let diagnostic = report.report().clone();
    let public_error = diagnostic.clone().into_result().err().map(ObjectCloseCleanupFailure::into_error);
    if let Err(error) = context.record_object_cleanup_diagnostic_fallback(
        ObjectCleanupDiagnosticRecord::drop_leak_terminalization(
            handle.object_id(),
            handle.generation(),
            diagnostic,
            public_error,
        ),
    ) {
        log::error!("drop-leak cleanupの診断記録に失敗しました: handle={handle:?} error={error:?}");
    }
    let mut runtime = TunerServiceRuntime::lock_shared(&runtime_handle, "Drop漏れ後片付け完了確定")
        .map_err(status_from_hal_error)?;
    let pending = report.finish(&mut runtime).map_err(status_from_hal_error)?;
    Ok(if pending {
        DropLeakQuarantineOutcome::SupervisedPending
    } else {
        DropLeakQuarantineOutcome::Completed
    })
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
