use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_device::{
    FrontendRuntimeState, FrontendWorkerCancelReason, FrontendWorkerKind,
};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId};

use crate::boot::TunerServiceRuntime;
use crate::frontend_ops::{
    FrontendWorkerTerminalEvent, FrontendWorkerTerminalEventAcceptance, SharedFrontendRuntime,
};
use crate::frontend_worker_txn::{
    cleanup_frontend_object_after_close_begin, FrontendCloseCleanupReport,
};
use crate::worker_failure_classifier::WorkerFailureClassifier;

/// frontend固有terminal処理をまとめるcall-local orchestration。
/// genericなstop、wake、join、reaping stateは`WorkerRuntime`が引き続き所有する。
pub struct FrontendWorkerTerminationUseCase;

impl FrontendWorkerTerminationUseCase {
    pub(crate) fn accept_worker_terminal(
        runtime: &mut TunerServiceRuntime,
        event: FrontendWorkerTerminalEvent,
    ) -> Result<FrontendWorkerTerminalEventAcceptance, HalError> {
        let snapshot = runtime
            .query()
            .frontend_runtime_snapshot(event.frontend_id())?;
        if snapshot.generation != event.owner_generation() {
            return Ok(FrontendWorkerTerminalEventAcceptance::DiscardedStale);
        }

        let frontend_id = event.frontend_id();
        let owner_generation = event.owner_generation();
        let worker_kind = event.worker_kind();
        let terminal_error = WorkerFailureClassifier::classify_terminal(
            event.into_terminal_result(),
            "frontend worker panicked or could not be joined",
        )
        .into_failure()
        .map(|(_, error)| error);
        if matches!(
            snapshot.state,
            FrontendRuntimeState::Tuning { .. } | FrontendRuntimeState::Scanning { .. }
        ) {
            if let Some(error) = terminal_error {
                match worker_kind {
                    FrontendWorkerKind::Tune => runtime
                        .frontend_txn()
                        .mark_frontend_tune_worker_failed(
                            frontend_id,
                            owner_generation,
                            error,
                        )?,
                    FrontendWorkerKind::Scan => runtime
                        .frontend_txn()
                        .mark_frontend_scan_session_backend_failed(
                            frontend_id,
                            owner_generation,
                        )?,
                }
            }
        }
        Ok(FrontendWorkerTerminalEventAcceptance::Accepted)
    }

    pub fn cleanup_after_close_begin(
        runtime: SharedFrontendRuntime,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        reason: FrontendWorkerCancelReason,
    ) -> Result<FrontendCloseCleanupReport, HalError> {
        cleanup_frontend_object_after_close_begin(
            runtime,
            object_id,
            object_generation,
            reason,
        )
    }
}
