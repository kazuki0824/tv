use super::{
    live_reader_descriptor_for_frontend_entry, DemuxRuntimeId, DemuxRuntimeSnapshot,
    FrontendLivePumpReport, FrontendRuntimeSnapshot, FrontendSignalState, FrontendTuneRequest,
    FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind,
    FrontendWorkerStartError, FrontendWorkerStopOutcome, GenerationBoundaryReport, HalError,
    HalInternalKind, HalInvalidStateKind, PipelineBoundaryReason, TunerServiceRuntime,
};
use maleicacid_tuner_hal2_device::FrontendWorkerStopTicket;

impl TunerServiceRuntime {
    pub fn mark_frontend_scan_session_callback_failed(
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
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.clear_live_reader_descriptor();
        runtime.mark_idle();
        self.transact_reset_and_unbind_bound_demuxes_for_frontend(
            frontend_id,
            PipelineBoundaryReason::FrontendUnbind,
        )
    }

    fn transact_close_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
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
        runtime.clear_live_reader_descriptor();
        runtime.mark_closing();
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
        runtime.restore_snapshot(snapshot);
        Ok(())
    }

    pub(crate) fn restore_bound_demux_runtime_snapshots(
        &mut self,
        snapshots: Vec<(DemuxRuntimeId, DemuxRuntimeSnapshot)>,
    ) -> Result<(), HalError> {
        for (demux_id, snapshot) in snapshots {
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
                .restore(snapshot)
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

    pub(crate) fn prepare_frontend_worker_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
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
        if self
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
        runtime.commit_generation(generation)?;
        runtime.set_live_reader_descriptor(reader);
        match kind {
            FrontendWorkerKind::Tune => runtime.mark_tuning(generation),
            FrontendWorkerKind::Scan => runtime.mark_scanning(generation),
        }
        Ok(())
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
        runtime.clear_live_reader_descriptor();
        runtime.mark_idle();
        Ok(())
    }

    pub(crate) fn stop_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.runtime
            .transact_stop_frontend_live_data_and_unbind(frontend_id)
    }

    pub(crate) fn close_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
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

    pub(crate) fn mark_frontend_tune_worker_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_mark_frontend_tune_worker_failed(frontend_id, generation, error)
    }

    pub(crate) fn mark_frontend_scan_session_backend_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_mark_frontend_scan_session_backend_failed(frontend_id, generation)
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
