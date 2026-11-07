use super::{
    execute_close_after_preflight_once, status_from_hal_error, AidlObjectArtifactCleanupExecutor,
    AidlObjectCloseRuntimeExecutor, AidlObjectDomainCleanupExecutor, AidlObjectHandle,
    BinderResult, ObjectCloseCleanupFailure,
};
use crate::service_context::SharedAidlServiceContext;
use maleicacid_tuner_hal2_common::FirstErrorCollector;
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
    let quarantine_result = {
        let mut runtime = TunerServiceRuntime::lock_shared(&runtime_handle, "Drop漏れ隔離")
            .map_err(status_from_hal_error)?;
        quarantine_object_drop_leak_use_case(&mut runtime, handle.object_id(), handle.generation())
    };

    let mut error_collector = FirstErrorCollector::new();
    let mut supervised_handoff: Option<
        Vec<(
            maleicacid_tuner_hal2_domain_request::AidlObjectId,
            maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        )>,
    > = None;
    match quarantine_result {
        Ok(None) => {
            // 同一・祖先・子孫のObjectCloseTxnが処理中。新たな物理cleanupは開始しない。
            return Ok(DropLeakQuarantineOutcome::OwnedByExistingCleanup);
        }
        Ok(Some(plan)) => {
            let terminalization_result = {
                let mut runtime_executor = AidlObjectCloseRuntimeExecutor::new(context);
                let mut domain_executor = AidlObjectDomainCleanupExecutor::new(context);
                let mut artifact_executor = AidlObjectArtifactCleanupExecutor::new(context);
                let report = plan.execute_terminalization_report_with_executor(
                    &mut runtime_executor,
                    &mut domain_executor,
                    &mut artifact_executor,
                );
                let supervised_pending =
                    maleicacid_tuner_hal2_service_runtime::drop_leak_cleanup_supervised_targets(
                        &report,
                    );
                let terminalization_result = report
                    .clone()
                    .into_result()
                    .map_err(ObjectCloseCleanupFailure::into_error);
                let public_error = terminalization_result.clone().err();
                let record_result = context.record_object_cleanup_diagnostic_fallback(
                    ObjectCleanupDiagnosticRecord::drop_leak_terminalization(
                        handle.object_id(),
                        handle.generation(),
                        report,
                        public_error,
                    ),
                );
                // cleanupと診断記録は別の確定内容であり、診断失敗で
                // 成功した物理cleanupを再実行しない。
                if let Err(error) = record_result {
                    log::error!("drop-leak cleanupの診断記録に失敗しました: handle={handle:?} error={error:?}");
                }
                // 物理回収の成功とSupervisorへの所有権移管を別の値として返す。
                // 後続reaperはCompletedとSupervisedPendingを混同できない。
                if let Some(pending) = supervised_pending {
                    supervised_handoff = Some(pending);
                    Ok(())
                } else {
                    terminalization_result
                }
            };
            error_collector.push_result(terminalization_result);
        }
        Err(error) => error_collector.push_error(error),
    }

    error_collector
        .into_result()
        .map_err(status_from_hal_error)?;

    // 外部cleanup前にruntime lock下でObjectCloseTxnへ義務を登録済み。
    // 通知workerのjoinと物理cleanup reportは、どちらが先でも許容する。
    {
        let mut runtime =
            TunerServiceRuntime::lock_shared(&runtime_handle, "Drop漏れ後片付け完了確定")
                .map_err(status_from_hal_error)?;
        maleicacid_tuner_hal2_service_runtime::finish_quarantined_cleanup_report_use_case(
            &mut runtime,
            handle.object_id(),
            handle.generation(),
            supervised_handoff.as_deref().unwrap_or(&[]),
        )
        .map_err(status_from_hal_error)?;
    }
    Ok(if supervised_handoff.is_some() {
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
