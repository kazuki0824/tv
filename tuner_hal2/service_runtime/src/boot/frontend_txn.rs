use super::{
    live_reader_descriptor_for_frontend_entry, DemuxRuntimeId, DemuxRuntimeRollbackToken,
    FrontendLivePumpReport, FrontendRuntimeSnapshot, FrontendSignalState, FrontendTuneRequest,
    FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind,
    FrontendWorkerStartError, FrontendWorkerStopOutcome, StreamBoundaryReport, HalError,
    HalInternalKind, HalInvalidStateKind, PipelineBoundaryReason, TunerServiceRuntime,
};
use maleicacid_tuner_hal2_demux::{
    DemuxRuntimeRollbackCommitRequest, DemuxRuntimeRollbackRestoreRequest,
};
use maleicacid_tuner_hal2_device::FrontendWorkerStopTicket;

impl TunerServiceRuntime {
    pub(crate) fn mark_frontend_scan_session_callback_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        self.frontend_txn()
            .mark_frontend_scan_session_callback_failed(frontend_id, generation)
    }

    fn transact_record_live_pump_report(
        &mut self,
        frontend_id: i32,
        generation: u64,
        report: FrontendLivePumpReport,
        cancel_reason: Option<FrontendWorkerCancelReason>,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.record_live_pump_report(generation, report, cancel_reason)
    }

    fn transact_stop_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<StreamBoundaryReport>, HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.clear_live_reader_and_mark_idle();
        self.transact_reset_and_unbind_bound_demuxes_for_frontend(
            frontend_id,
            PipelineBoundaryReason::FrontendUnbind,
        )
    }

    fn transact_close_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<StreamBoundaryReport>, HalError> {
        let frontend_key = crate::registry::FrontendRuntimeId(frontend_id);
        let runtime = self
            .registry
            .frontend_runtime_mut(frontend_key)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.clear_live_reader_and_mark_closing();
        self.transact_reset_and_unbind_bound_demuxes_for_frontend(
            frontend_id,
            PipelineBoundaryReason::FrontendClose,
        )
    }

    fn transact_begin_frontend_scan_session(
        &mut self,
        frontend_id: i32,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.begin_scan_session(generation, fingerprint, candidates)
    }

    fn transact_cancel_frontend_scan_session(
        &mut self,
        frontend_id: i32,
        generation: u64,
        reason: FrontendWorkerCancelReason,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.cancel_scan_session(generation, reason)
    }

    fn transact_advance_frontend_scan_session_after_candidate(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<bool, HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.advance_scan_session_after_candidate(generation)
    }

    fn transact_mark_frontend_scan_session_locked_reported(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.mark_scan_session_locked_reported(generation)
    }

    fn transact_complete_locked_frontend_scan_continuation(
        &mut self,
        frontend_id: i32,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.complete_locked_scan_continuation(generation, fingerprint, candidates)
    }

    fn transact_mark_frontend_tune_worker_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        {
            let runtime = self
                .registry
                .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend runtime is missing for advertised frontend",
                    )
                })?;
            runtime.mark_tune_worker_failed(generation, error)?;
        }
        self.registry
            .quarantine_bound_demuxes_for_frontend(crate::registry::FrontendRuntimeId(frontend_id));
        Ok(())
    }

    fn transact_mark_frontend_tune_submit_rejected_after_boundary(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.mark_tune_submit_rejected_after_boundary(generation, error)
    }

    fn transact_mark_frontend_tune_no_signal(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.mark_tune_no_signal(generation)
    }

    fn transact_mark_frontend_scan_session_backend_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        {
            let runtime = self
                .registry
                .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend runtime is missing for advertised frontend",
                    )
                })?;
            runtime.mark_scan_session_backend_failed(generation)?;
        }
        self.registry
            .quarantine_bound_demuxes_for_frontend(crate::registry::FrontendRuntimeId(frontend_id));
        Ok(())
    }

    fn transact_mark_frontend_scan_submit_rejected_after_boundary(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.mark_scan_submit_rejected_after_boundary(generation, error)
    }
}

pub(crate) struct FrontendTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn frontend_txn(&mut self) -> FrontendTxn<'_> {
        FrontendTxn { runtime: self }
    }
}

impl<'a> FrontendTxn<'a> {
    pub(crate) fn is_stable_locked_tune_reentry(
        &mut self,
        frontend_id: i32,
        request: &FrontendTuneRequest,
    ) -> Result<bool, HalError> {
        let frontend_key = crate::registry::FrontendRuntimeId(frontend_id);
        if self
            .runtime
            .registry
            .frontend(frontend_key)
            .is_some_and(|entry| entry.backend == FrontendBackendKind::Px4CharDevice)
        {
            // px4のPTX_SET_CHANNEL成功は一回限りの証跡であり、
            // 過去generationのcurrent lock継続を証明しない。
            return Ok(false);
        }
        let snapshot = self
            .runtime
            .registry
            .frontend_runtime(frontend_key)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?
            .snapshot();
        let generation = match snapshot.state {
            maleicacid_tuner_hal2_device::FrontendRuntimeState::Tuning { generation }
                if snapshot.signal_state == FrontendSignalState::Locked
                    && snapshot.active_tune_request.as_ref() == Some(request) => generation,
            _ => return Ok(false),
        };
        if self
            .runtime
            .frontend_workers
            .running_generation(frontend_id, FrontendWorkerKind::Tune)
            != Some(generation)
        {
            return Ok(false);
        }
        Ok(self
            .runtime
            .registry
            .frontend_bound_demux_ids(frontend_key)
            .into_iter()
            .all(|demux_id| {
                self.runtime
                    .registry
                    .demux_runtime(demux_id)
                    .is_some_and(|demux| {
                        demux.state() == maleicacid_tuner_hal2_demux::DemuxRuntimeState::Open
                    })
            }))
    }

    pub(crate) fn commit_stable_locked_tune_reentry(
        &mut self,
        frontend_id: i32,
        request: &FrontendTuneRequest,
    ) -> Result<Option<(u64, u64)>, HalError> {
        if !self.is_stable_locked_tune_reentry(frontend_id, request)? {
            return Ok(None);
        }
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing while committing stable tune re-entry",
                )
            })?;
        let generation = runtime.generation();
        let request_sequence = runtime.commit_stable_tune_reentry(generation, request)?;
        Ok(Some((generation, request_sequence)))
    }

    pub(crate) fn restore_frontend_runtime_snapshot(
        &mut self,
        frontend_id: i32,
        snapshot: FrontendRuntimeSnapshot,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.restore_from_rollback_snapshot(snapshot);
        Ok(())
    }

    pub(crate) fn restore_bound_demux_runtime_rollback_tokens(
        &mut self,
        tokens: Vec<(DemuxRuntimeId, DemuxRuntimeRollbackToken)>,
    ) -> Result<(), HalError> {
        for (demux_id, token) in tokens {
            let demux = self
                .runtime
                .registry
                .demux_runtime_mut(demux_id)
                .ok_or_else(|| {
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "bound demux runtime is missing while restoring tune rollback snapshot",
                    )
                })?;
            demux
                .restore_from_rollback_request(DemuxRuntimeRollbackRestoreRequest::new(token))
                .map_err(super::demux_runtime_error_to_hal)?;
        }
        Ok(())
    }

    pub(crate) fn commit_bound_demux_runtime_rollback_tokens(
        &mut self,
        tokens: Vec<(DemuxRuntimeId, DemuxRuntimeRollbackToken)>,
    ) -> Result<(), HalError> {
        for (demux_id, token) in tokens {
            let demux = self
                .runtime
                .registry
                .demux_runtime_mut(demux_id)
                .ok_or_else(|| {
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "bound demux runtime is missing while committing tune boundary",
                    )
                })?;
            demux
                .commit_rollback_request(DemuxRuntimeRollbackCommitRequest::new(token))
                .map_err(super::demux_runtime_error_to_hal)?;
        }
        Ok(())
    }

    pub(crate) fn commit_frontend_active_tune_request(
        &mut self,
        frontend_id: i32,
        generation: u64,
        request: FrontendTuneRequest,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.commit_active_tune_request(generation, request)
    }

    pub(crate) fn record_frontend_signal_state(
        &mut self,
        frontend_id: i32,
        generation: u64,
        signal_state: FrontendSignalState,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.record_signal_state(generation, signal_state)
    }

    pub(crate) fn record_live_pump_report(
        &mut self,
        frontend_id: i32,
        generation: u64,
        report: FrontendLivePumpReport,
        cancel_reason: Option<FrontendWorkerCancelReason>,
    ) -> Result<(), HalError> {
        self.runtime.transact_record_live_pump_report(
            frontend_id,
            generation,
            report,
            cancel_reason,
        )
    }

    fn prepare_frontend_worker_generation_with_running_policy(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        allow_running_replacement: bool,
    ) -> Result<u64, HalError> {
        if let Some(FrontendWorkerStopOutcome::Completed {
            generation,
            result: Err(error),
            ..
        }) = self
            .runtime
            .frontend_workers
            .take_completed(frontend_id, kind)
        {
            match kind {
                FrontendWorkerKind::Tune => self
                    .runtime
                    .transact_mark_frontend_tune_worker_failed(frontend_id, generation, error)?,
                FrontendWorkerKind::Scan => self
                    .runtime
                    .transact_mark_frontend_scan_session_backend_failed(frontend_id, generation)?,
            }
        }
        if !allow_running_replacement
            && self
                .runtime
                .frontend_workers
                .running_generation(frontend_id, kind)
                .is_some()
        {
            return Err(HalError::invalid_state(
                maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                "frontend worker is already running",
            ));
        }
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.checked_next_generation()
    }

    #[cfg(test)]
    pub(crate) fn prepare_frontend_worker_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Result<u64, HalError> {
        self.prepare_frontend_worker_generation_with_running_policy(frontend_id, kind, false)
    }

    pub(crate) fn prepare_frontend_worker_replacement_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Result<u64, HalError> {
        self.prepare_frontend_worker_generation_with_running_policy(frontend_id, kind, true)
    }

    pub(crate) fn install_frontend_live_reader_descriptor_for_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
    ) -> Result<(), HalError> {
        let entry = self
            .runtime
            .registry
            .frontend(crate::registry::FrontendRuntimeId(frontend_id))
            .cloned()
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend registry entry is missing for advertised frontend",
                )
            })?;
        let reader = live_reader_descriptor_for_frontend_entry(&entry)?;
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.install_live_reader_for_worker_generation(generation, reader, kind)
    }

    pub(crate) fn fence_frontend_worker_replacement_generation(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing while fencing a worker generation",
                )
            })?;
        runtime.fence_for_worker_replacement(generation)
    }

    pub(crate) fn mark_frontend_worker_stop_pending_failure(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing while recording pending worker stop",
                )
            })?;
        runtime.mark_worker_stop_pending_failure(generation, error)
    }

    pub(crate) fn install_frontend_live_reader_descriptor_after_fence(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
    ) -> Result<(), HalError> {
        let entry = self
            .runtime
            .registry
            .frontend(crate::registry::FrontendRuntimeId(frontend_id))
            .cloned()
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend registry entry is missing after worker fence",
                )
            })?;
        let reader = live_reader_descriptor_for_frontend_entry(&entry)?;
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing after worker fence",
                )
            })?;
        runtime.install_live_reader_for_fenced_worker_generation(generation, reader, kind)
    }

    pub(crate) fn commit_frontend_tune_after_fence(
        &mut self,
        frontend_id: i32,
        generation: u64,
        request: FrontendTuneRequest,
    ) -> Result<(), HalError> {
        let entry = self
            .runtime
            .registry
            .frontend(crate::registry::FrontendRuntimeId(frontend_id))
            .cloned()
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend registry entry is missing at tune commit",
                )
            })?;
        let reader = live_reader_descriptor_for_frontend_entry(&entry)?;
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing at tune commit",
                )
            })?;
        runtime.commit_tune_after_fence(generation, reader, request)
    }

    pub(crate) fn commit_frontend_scan_after_fence(
        &mut self,
        frontend_id: i32,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        let entry = self
            .runtime
            .registry
            .frontend(crate::registry::FrontendRuntimeId(frontend_id))
            .cloned()
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend registry entry is missing at scan commit",
                )
            })?;
        let reader = live_reader_descriptor_for_frontend_entry(&entry)?;
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing at scan commit",
                )
            })?;
        runtime.commit_scan_after_fence(generation, reader, fingerprint, candidates)
    }

    pub(crate) fn record_frontend_backend_request_failure_after_fence(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
        backend_stopped: bool,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing while recording backend request failure",
                )
            })?;
        runtime.record_backend_request_failure_after_fence(
            generation,
            error,
            backend_stopped,
        )
    }

    pub(crate) fn record_frontend_backend_activation_failure_after_commit(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
        backend_stopped: bool,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing while recording backend activation failure",
                )
            })?;
        runtime.record_backend_activation_failure_after_commit(
            generation,
            error,
            backend_stopped,
        )
    }

    pub(crate) fn clear_frontend_live_reader_descriptor_and_idle(
        &mut self,
        frontend_id: i32,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.clear_live_reader_and_mark_idle();
        Ok(())
    }

    pub(crate) fn stop_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<StreamBoundaryReport>, HalError> {
        self.runtime
            .transact_stop_frontend_live_data_and_unbind(frontend_id)
    }

    pub(crate) fn close_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<StreamBoundaryReport>, HalError> {
        self.runtime
            .transact_close_frontend_live_data_and_unbind(frontend_id)
    }

    pub(crate) fn begin_frontend_scan_session(
        &mut self,
        frontend_id: i32,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        self.runtime.transact_begin_frontend_scan_session(
            frontend_id,
            generation,
            fingerprint,
            candidates,
        )
    }

    pub(crate) fn cancel_frontend_scan_session(
        &mut self,
        frontend_id: i32,
        generation: u64,
        reason: FrontendWorkerCancelReason,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_cancel_frontend_scan_session(frontend_id, generation, reason)
    }

    pub(crate) fn advance_frontend_scan_session_after_candidate(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<bool, HalError> {
        self.runtime
            .transact_advance_frontend_scan_session_after_candidate(frontend_id, generation)
    }

    pub(crate) fn mark_frontend_scan_session_locked_reported(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_mark_frontend_scan_session_locked_reported(frontend_id, generation)
    }

    pub(crate) fn complete_locked_frontend_scan_continuation(
        &mut self,
        frontend_id: i32,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_complete_locked_frontend_scan_continuation(
                frontend_id,
                generation,
                fingerprint,
                candidates,
            )
    }

    pub(crate) fn complete_locked_frontend_scan_continuation_after_fence(
        &mut self,
        frontend_id: i32,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for fenced scan continuation",
                )
            })?;
        runtime.complete_locked_scan_continuation_after_fence(
            generation,
            fingerprint,
            candidates,
        )
    }

    pub(crate) fn mark_frontend_tune_worker_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_mark_frontend_tune_worker_failed(frontend_id, generation, error)
    }

    pub(crate) fn mark_frontend_tune_submit_rejected_after_boundary(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_mark_frontend_tune_submit_rejected_after_boundary(
                frontend_id,
                generation,
                error,
            )
    }

    pub(crate) fn mark_frontend_tune_no_signal(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_mark_frontend_tune_no_signal(frontend_id, generation)
    }

    pub(crate) fn mark_frontend_scan_session_backend_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_mark_frontend_scan_session_backend_failed(frontend_id, generation)
    }

    pub(crate) fn mark_frontend_scan_submit_rejected_after_boundary(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_mark_frontend_scan_submit_rejected_after_boundary(
                frontend_id,
                generation,
                error,
            )
    }

    pub(crate) fn mark_frontend_scan_session_callback_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.mark_scan_session_callback_failed(generation)
    }

    pub(crate) fn start_worker<F>(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        job: F,
    ) -> Result<(), FrontendWorkerStartError>
    where
        F: FnOnce(FrontendWorkerContext) -> Result<(), HalError> + Send + 'static,
    {
        self.runtime
            .frontend_workers
            .start(frontend_id, kind, generation, job)
    }

    pub(crate) fn request_worker_stop_for_join(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopTicket {
        self.runtime
            .frontend_workers
            .request_stop_for_join(frontend_id, kind, reason)
    }
}
