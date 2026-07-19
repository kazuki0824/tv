use super::{
    live_reader_descriptor_for_frontend_entry, DemuxRuntimeId, DemuxRuntimeRollbackToken,
    FrontendLiveDataCompletion, FrontendLiveDataCompletionRequest, FrontendLivePumpCompletionRequest, FrontendLivePumpReport,
    FrontendRollbackFailureRequest, FrontendRuntimeRollbackCapture, FrontendRuntimeRollbackToken, FrontendScanStartRequest,
    FrontendScanTransitionOutcome, FrontendScanTransitionRequest, FrontendSignalRecordRequest,
    FrontendSignalState,
    FrontendTuneCommitRequest, FrontendTuneRequest, FrontendTuneWorkerFailureRequest,
    FrontendWorkerInstallRequest,
    FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind,
    FrontendWorkerStartError, FrontendWorkerStopOutcome, GenerationBoundaryReport, HalError,
    HalInternalKind, HalInvalidStateKind, PipelineBoundaryReason, TunerServiceRuntime,
};
use maleicacid_tuner_hal2_common::compose_primary_cleanup_failure;
use maleicacid_tuner_hal2_demux::{
    DemuxRuntimeQuarantineRequest, DemuxRuntimeRollbackCommitRequest,
    DemuxRuntimeRollbackRestoreRequest,
};
use maleicacid_tuner_hal2_device::FrontendWorkerStopTicket;
use crate::frontend_worker_txn::{
    BoundDemuxRollbackExecutionReport, BoundDemuxRollbackPhase,
    BoundDemuxRollbackStepOutcome,
};

fn live_data_completion_request(
    runtime: &TunerServiceRuntime,
    frontend_id: i32,
    expected_worker: Option<(u64, FrontendWorkerKind)>,
    completion: FrontendLiveDataCompletion,
) -> Result<FrontendLiveDataCompletionRequest, HalError> {
    match expected_worker {
        Some((generation, kind)) => Ok(FrontendLiveDataCompletionRequest::worker(
            generation,
            kind,
            completion,
        )),
        None => {
            let snapshot = runtime
                .frontend_runtime(frontend_id)?
                .query()
                .status_snapshot();
            Ok(FrontendLiveDataCompletionRequest::no_worker(
                snapshot.generation(),
                snapshot.state(),
                completion,
            ))
        }
    }
}

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
        runtime.record_live_pump_completion(FrontendLivePumpCompletionRequest::new(
            generation,
            report,
            cancel_reason,
        ))
    }

    fn transact_stop_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
        expected_worker: Option<(u64, FrontendWorkerKind)>,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let request = live_data_completion_request(
            self,
            frontend_id,
            expected_worker,
            FrontendLiveDataCompletion::Idle,
        )?;
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.complete_live_data(request)?;
        self.transact_reset_and_unbind_bound_demuxes_for_frontend(
            frontend_id,
            PipelineBoundaryReason::FrontendUnbind,
        )
    }

    fn transact_close_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
        expected_worker: Option<(u64, FrontendWorkerKind)>,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let request = live_data_completion_request(
            self,
            frontend_id,
            expected_worker,
            FrontendLiveDataCompletion::Closing,
        )?;
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
        runtime.complete_live_data(request)?;
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
        runtime.start_scan(FrontendScanStartRequest::new(
            generation,
            fingerprint,
            candidates,
        ))
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
        runtime
            .apply_scan_transition(FrontendScanTransitionRequest::cancel(generation, reason))
            .map(|_| ())
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
        match runtime
            .apply_scan_transition(FrontendScanTransitionRequest::advance_after_candidate(
                generation,
            ))?
        {
            FrontendScanTransitionOutcome::CandidateAdvanced { has_next } => Ok(has_next),
            FrontendScanTransitionOutcome::Applied => Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend scan advance returned a non-advance outcome",
            )),
        }
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
            runtime
                .record_tune_worker_failure(FrontendTuneWorkerFailureRequest::new(
                    generation,
                    error,
                ))?;
        }
        self.registry
            .quarantine_bound_demuxes_for_frontend(crate::registry::FrontendRuntimeId(frontend_id))?;
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
            runtime
                .apply_scan_transition(FrontendScanTransitionRequest::backend_failed(generation))
                .map(|_| ())?;
        }
        self.registry
            .quarantine_bound_demuxes_for_frontend(crate::registry::FrontendRuntimeId(frontend_id))?;
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
    pub(crate) fn commit_frontend_tune_rollback_expected_post_state(
        &mut self,
        frontend_id: i32,
        token: &FrontendRuntimeRollbackToken,
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
                    "frontend registry entry is missing while committing tune rollback expected post state",
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
                    "frontend runtime is missing while committing tune rollback expected post state",
                )
            })?;
        runtime.commit_tune_worker_rollback_expected_post_state(
            token,
            generation,
            reader,
            request,
        )
    }

    pub(crate) fn begin_frontend_scan_rollback_expected_post_state(
        &mut self,
        frontend_id: i32,
        token: &FrontendRuntimeRollbackToken,
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
                    "frontend registry entry is missing while beginning scan rollback expected post state",
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
                    "frontend runtime is missing while beginning scan rollback expected post state",
                )
            })?;
        runtime.begin_scan_worker_rollback_expected_post_state(
            token,
            generation,
            reader,
            fingerprint,
            candidates,
        )
    }

    pub(crate) fn restore_frontend_runtime_rollback_token(
        &mut self,
        frontend_id: i32,
        token: &mut FrontendRuntimeRollbackToken,
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
        runtime.restore_worker_rollback(token)
    }

    pub(crate) fn restore_bound_demux_runtime_rollback_tokens(
        &mut self,
        tokens: Vec<(DemuxRuntimeId, DemuxRuntimeRollbackToken)>,
    ) -> BoundDemuxRollbackExecutionReport {
        let mut report = BoundDemuxRollbackExecutionReport::new();
        for (demux_id, mut token) in tokens {
            match self.runtime.registry.demux_runtime_mut(demux_id) {
                Some(demux) => match demux.restore_from_rollback_request(
                    DemuxRuntimeRollbackRestoreRequest::new(&mut token),
                ) {
                    Ok(()) => report.push(BoundDemuxRollbackStepOutcome {
                        target: crate::frontend_worker_txn::BoundDemuxRollbackTarget::Demux(demux_id.0),
                        phase: BoundDemuxRollbackPhase::Restore,
                        result: Ok(()),
                    }),
                    Err(restore_error) => {
                        let restore_error = super::demux_runtime_error_to_hal(restore_error);
                        report.push(BoundDemuxRollbackStepOutcome {
                            target: crate::frontend_worker_txn::BoundDemuxRollbackTarget::Demux(demux_id.0),
                            phase: BoundDemuxRollbackPhase::Restore,
                            result: Err(restore_error.clone()),
                        });
                        demux.quarantine_runtime_from_typed_request(
                            DemuxRuntimeQuarantineRequest::new(),
                        );
                        report.push(BoundDemuxRollbackStepOutcome {
                            target: crate::frontend_worker_txn::BoundDemuxRollbackTarget::Demux(demux_id.0),
                            phase: BoundDemuxRollbackPhase::Quarantine,
                            result: Ok(()),
                        });
                        let discard_result = demux
                            .commit_rollback_request(DemuxRuntimeRollbackCommitRequest::new(token))
                            .map_err(super::demux_runtime_error_to_hal);
                        report.push(BoundDemuxRollbackStepOutcome {
                            target: crate::frontend_worker_txn::BoundDemuxRollbackTarget::Demux(demux_id.0),
                            phase: BoundDemuxRollbackPhase::DiscardAuthority,
                            result: discard_result,
                        });
                    }
                },
                None => {
                    report.push(BoundDemuxRollbackStepOutcome {
                        target: crate::frontend_worker_txn::BoundDemuxRollbackTarget::Demux(demux_id.0),
                        phase: BoundDemuxRollbackPhase::Restore,
                        result: Err(HalError::invalid_state(
                            HalInvalidStateKind::InvalidLifecycle,
                            format!(
                                "bound demux runtime is missing while restoring tune rollback token: demux_id={}",
                                demux_id.0
                            ),
                        )),
                    });
                    report.push(BoundDemuxRollbackStepOutcome {
                        target: crate::frontend_worker_txn::BoundDemuxRollbackTarget::Demux(demux_id.0),
                        phase: BoundDemuxRollbackPhase::Quarantine,
                        result: Err(HalError::invalid_state(
                            HalInvalidStateKind::InvalidLifecycle,
                            format!(
                                "bound demux runtime is missing and cannot be quarantined: demux_id={}",
                                demux_id.0
                            ),
                        )),
                    });
                    let discard_result = token
                        .discard_without_runtime()
                        .map_err(super::demux_runtime_error_to_hal);
                    report.push(BoundDemuxRollbackStepOutcome {
                        target: crate::frontend_worker_txn::BoundDemuxRollbackTarget::Demux(demux_id.0),
                        phase: BoundDemuxRollbackPhase::DiscardAuthority,
                        result: discard_result,
                    });
                }
            }
        }
        report
    }

    pub(crate) fn discard_frontend_runtime_rollback_token(
        &mut self,
        frontend_id: i32,
        token: &mut FrontendRuntimeRollbackToken,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing while discarding rollback authority",
                )
            })?;
        runtime.discard_worker_rollback(token)
    }

    pub(crate) fn quarantine_frontend_after_rollback_failure(
        &mut self,
        frontend_id: i32,
        token: &mut FrontendRuntimeRollbackToken,
        error: HalError,
    ) -> Result<(), HalError> {
        let runtime = self
            .runtime
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing while quarantining rollback failure",
                )
            })?;
        runtime.quarantine_rollback_failure(FrontendRollbackFailureRequest::new(error));
        runtime.discard_worker_rollback(token)
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
        runtime
            .commit_tune(FrontendTuneCommitRequest::new(generation, request))
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
        runtime
            .record_signal(FrontendSignalRecordRequest::new(generation, signal_state))
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

    pub(crate) fn prepare_frontend_runtime_rollback_capture(
        &mut self,
        frontend_id: i32,
    ) -> Result<FrontendRuntimeRollbackCapture, HalError> {
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
        runtime.prepare_worker_rollback()
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
        runtime.checked_next_worker_generation()
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
        runtime.install_worker(FrontendWorkerInstallRequest::new(
            generation,
            reader,
            kind,
        ))
    }

    pub(crate) fn clear_frontend_live_reader_descriptor_and_idle(
        &mut self,
        frontend_id: i32,
        expected_worker: Option<(u64, FrontendWorkerKind)>,
    ) -> Result<(), HalError> {
        let request = live_data_completion_request(
            self.runtime,
            frontend_id,
            expected_worker,
            FrontendLiveDataCompletion::Idle,
        )?;
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
        runtime.complete_live_data(request)
    }

    pub(crate) fn stop_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
        expected_worker: Option<(u64, FrontendWorkerKind)>,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.runtime
            .transact_stop_frontend_live_data_and_unbind(frontend_id, expected_worker)
    }

    pub(crate) fn close_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
        expected_worker: Option<(u64, FrontendWorkerKind)>,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.runtime
            .transact_close_frontend_live_data_and_unbind(frontend_id, expected_worker)
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
        runtime
            .apply_scan_transition(FrontendScanTransitionRequest::callback_failed(generation))
            .map(|_| ())
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
