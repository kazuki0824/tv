use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError};
use maleicacid_tuner_hal2_device::{
    FrontendRuntimeState, FrontendWorkerCancelReason, FrontendWorkerKind,
};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId};

use crate::boot::TunerServiceRuntime;
use crate::frontend_ops::{
    FrontendWorkerTerminalEvent, FrontendWorkerTerminalEventAcceptance, SharedFrontendRuntime,
};
use crate::frontend_worker_txn::{
    cleanup_frontend_object_after_close_begin, record_frontend_worker_terminal_failure,
    FrontendCloseCleanupReport,
};
use crate::worker_failure_classifier::WorkerFailureClassifier;

/// frontend固有terminal処理をまとめるcall-local orchestration。
/// genericなstop、wake、join、reaping stateは`WorkerRuntime`が引き続き所有する。
pub struct FrontendWorkerTerminationUseCase;

pub(crate) struct FrontendWorkerTerminalAcceptanceReport {
    state_result: Result<FrontendWorkerTerminalEventAcceptance, HalError>,
    diagnostic_result: Result<(), HalError>,
}

impl FrontendWorkerTerminalAcceptanceReport {
    pub(crate) fn into_parts(
        self,
    ) -> (
        Result<FrontendWorkerTerminalEventAcceptance, HalError>,
        Result<(), HalError>,
    ) {
        (self.state_result, self.diagnostic_result)
    }
}

impl FrontendWorkerTerminationUseCase {
    pub(crate) fn accept_worker_terminal_report(
        runtime: &mut TunerServiceRuntime,
        event: FrontendWorkerTerminalEvent,
    ) -> FrontendWorkerTerminalAcceptanceReport {
        let snapshot = match runtime
            .query()
            .frontend_runtime_snapshot(event.frontend_id())
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return FrontendWorkerTerminalAcceptanceReport {
                    state_result: Err(error),
                    diagnostic_result: Ok(()),
                }
            }
        };
        if snapshot.generation != event.owner_generation() {
            return FrontendWorkerTerminalAcceptanceReport {
                state_result: Ok(FrontendWorkerTerminalEventAcceptance::DiscardedStale),
                diagnostic_result: Ok(()),
            };
        }

        let frontend_id = event.frontend_id();
        let owner_generation = event.owner_generation();
        let worker_kind = event.worker_kind();
        let terminal_failure = WorkerFailureClassifier::classify_terminal(
            event.into_terminal_result(),
            "frontend worker panicked or could not be joined",
        )
        .into_failure();
        let diagnostic_result = match terminal_failure.as_ref() {
            Some((category, error)) => record_frontend_worker_terminal_failure(
                runtime,
                frontend_id,
                worker_kind,
                owner_generation,
                *category,
                error.clone(),
            ),
            None => Ok(()),
        };

        // 診断記録と正本状態へのterminal受理は別結果とする。
        // closeの資源寿命はstate_resultだけを依存条件にし、診断storeの失敗では変更しない。
        let mut state_result = Ok(());
        if matches!(
            snapshot.state,
            FrontendRuntimeState::Tuning { .. } | FrontendRuntimeState::Scanning { .. }
        ) {
            if let Some((_, error)) = terminal_failure {
                state_result = match worker_kind {
                    FrontendWorkerKind::Tune => runtime
                        .frontend_txn()
                        .mark_frontend_tune_worker_failed(frontend_id, owner_generation, error),
                    FrontendWorkerKind::Scan => runtime
                        .frontend_txn()
                        .mark_frontend_scan_session_backend_failed(frontend_id, owner_generation),
                };
            }
        }

        FrontendWorkerTerminalAcceptanceReport {
            state_result: state_result.map(|_| FrontendWorkerTerminalEventAcceptance::Accepted),
            diagnostic_result,
        }
    }

    pub(crate) fn accept_worker_terminal(
        runtime: &mut TunerServiceRuntime,
        event: FrontendWorkerTerminalEvent,
    ) -> Result<FrontendWorkerTerminalEventAcceptance, HalError> {
        let (state_result, diagnostic_result) =
            Self::accept_worker_terminal_report(runtime, event).into_parts();
        match (state_result, diagnostic_result) {
            (Ok(acceptance), Ok(())) => Ok(acceptance),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(primary), Err(cleanup)) => Err(compose_primary_cleanup_failure(
                "frontendワーカー終端遷移と診断がともに失敗しました",
                primary,
                cleanup,
            )),
        }
    }

    pub fn cleanup_after_close_begin(
        runtime: SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        reason: FrontendWorkerCancelReason,
    ) -> Result<FrontendCloseCleanupReport, HalError> {
        cleanup_frontend_object_after_close_begin(runtime, object_id, object_generation, reason)
    }
}
