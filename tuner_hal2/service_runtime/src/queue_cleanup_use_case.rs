use crate::boot::{
    attach_diagnostic_detail_to_public_error, format_dvr_queue_cleanup_report,
    format_filter_runtime_operation_report, TunerServiceRuntime,
};
use crate::diagnostics::DemuxTransactionDiagnosticRecord;
use crate::registry::{DemuxRuntimeId, DvrRuntimeId, FilterRuntimeId};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidStateKind};
use maleicacid_tuner_hal2_demux::{
    DemuxRuntime, DemuxRuntimeError, DemuxRuntimeErrorKind, DvrKind,
    DvrQueueCleanupCommitError, DvrQueueCleanupOutcome, DvrQueueCleanupReport,
    DvrQueueCleanupSkipReason, DvrQueueCleanupStep, DvrRuntimeOperationRequest,
    FilterRuntimeOperationKind, FilterRuntimeOperationOutcome, FilterRuntimeOperationReport,
    FilterRuntimeOperationRequest, FilterRuntimeOperationSkipReason, FilterRuntimeOperationStep,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueCleanupTarget {
    Filter { filter_id: i32 },
    Dvr { dvr_id: i32 },
}

type DvrQueueCleanupFailure = (DvrQueueCleanupStep, DemuxRuntimeError);
type DvrDemuxCleanupResult = Result<(DvrKind, Option<DvrQueueCleanupFailure>), DemuxRuntimeError>;

pub(crate) struct QueueCleanupUseCase<'a> {
    runtime: &'a mut TunerServiceRuntime,
    target: QueueCleanupTarget,
}

impl<'a> QueueCleanupUseCase<'a> {
    pub(crate) fn filter(runtime: &'a mut TunerServiceRuntime, filter_id: i32) -> Self {
        Self {
            runtime,
            target: QueueCleanupTarget::Filter { filter_id },
        }
    }

    pub(crate) fn dvr(runtime: &'a mut TunerServiceRuntime, dvr_id: i32) -> Self {
        Self {
            runtime,
            target: QueueCleanupTarget::Dvr { dvr_id },
        }
    }

    pub(crate) fn execute(self) -> Result<(), HalError> {
        match self.target {
            QueueCleanupTarget::Filter { filter_id } => self.execute_filter(filter_id),
            QueueCleanupTarget::Dvr { dvr_id } => self.execute_dvr(dvr_id),
        }
    }

    fn execute_filter(self, filter_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self
            .runtime
            .registry()
            .filter(FilterRuntimeId(filter_id))
            .map(|entry| entry.owner_demux_id)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter registry entry is missing",
                )
            })?;
        let (report, result) = execute_filter_cleanup_protocol(
            self.runtime
                .registry_mut()
                .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
                .ok_or_else(|| {
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "owner demux runtime is missing",
                    )
                })?,
            filter_id,
        );
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let primary = TunerServiceRuntime::map_filter_runtime_error(error);
                let diagnostic_id = self.runtime.allocate_demux_transaction_diagnostic_id();
                self.runtime.record_demux_transaction_diagnostic(
                    DemuxTransactionDiagnosticRecord::filter_runtime_operation(
                        diagnostic_id,
                        owner_demux_id,
                        filter_id,
                        report.clone(),
                        primary.clone(),
                    ),
                );
                Err(attach_diagnostic_detail_to_public_error(
                    primary,
                    format_filter_runtime_operation_report(diagnostic_id, &report),
                ))
            }
        }
    }

    fn execute_dvr(self, dvr_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self
            .runtime
            .registry()
            .dvr(DvrRuntimeId(dvr_id))
            .map(|entry| entry.owner_demux_id)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "DVR registry entry is missing",
                )
            })?;
        let (mut report, demux_result) = execute_dvr_demux_cleanup_protocol(
            self.runtime
                .registry_mut()
                .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
                .ok_or_else(|| {
                    HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "owner demux runtime is missing",
                    )
                })?,
            dvr_id,
        );
        let (kind, demux_failure) = match demux_result {
            Ok(committed) => committed,
            Err(error) => {
                let primary = TunerServiceRuntime::map_dvr_runtime_error(error);
                return Err(record_dvr_queue_cleanup_failure(
                    self.runtime,
                    owner_demux_id,
                    dvr_id,
                    report,
                    primary,
                ));
            }
        };
        let mut primary_failure = demux_failure.map(|(step, error)| {
            (
                step,
                TunerServiceRuntime::map_dvr_runtime_error(error),
            )
        });

        if kind == DvrKind::Record {
            report.skipped(
                DvrQueueCleanupStep::PlaybackResidualDiscard,
                DvrQueueCleanupSkipReason::PlaybackOnly,
            );
            report.skipped(
                DvrQueueCleanupStep::PlaybackDiscardDiagnosticCommit,
                DvrQueueCleanupSkipReason::PlaybackOnly,
            );
            return finish_dvr_queue_cleanup(
                self.runtime,
                owner_demux_id,
                dvr_id,
                report,
                primary_failure,
            );
        }

        let dropped_bytes = self
            .runtime
            .discard_playback_consume_for_queue_cleanup(dvr_id);
        report.succeeded(DvrQueueCleanupStep::PlaybackResidualDiscard);
        if dropped_bytes == 0 {
            report.skipped(
                DvrQueueCleanupStep::PlaybackDiscardDiagnosticCommit,
                DvrQueueCleanupSkipReason::NoRetainedPlaybackBytes,
            );
            return finish_dvr_queue_cleanup(
                self.runtime,
                owner_demux_id,
                dvr_id,
                report,
                primary_failure,
            );
        }

        let diagnostic_result = self
            .runtime
            .registry_mut()
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                (
                    DemuxRuntimeErrorKind::DvrMissing,
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "owner demux runtime disappeared after playback flush",
                    ),
                )
            })
            .and_then(|demux| {
                demux
                    .note_playback_consume_boundary_discard(dvr_id, dropped_bytes)
                    .map_err(|error| {
                        (
                            error.kind,
                            TunerServiceRuntime::map_dvr_runtime_error(error),
                        )
                    })
            });
        match diagnostic_result {
            Ok(()) => {
                report.succeeded(DvrQueueCleanupStep::PlaybackDiscardDiagnosticCommit);
            }
            Err((error_kind, primary)) => {
                report.failed(
                    DvrQueueCleanupStep::PlaybackDiscardDiagnosticCommit,
                    error_kind,
                );
                if primary_failure.is_none() {
                    primary_failure = Some((
                        DvrQueueCleanupStep::PlaybackDiscardDiagnosticCommit,
                        primary,
                    ));
                }
            }
        }
        finish_dvr_queue_cleanup(
            self.runtime,
            owner_demux_id,
            dvr_id,
            report,
            primary_failure,
        )
    }
}

fn execute_dvr_demux_cleanup_protocol(
    demux: &mut DemuxRuntime,
    dvr_id: i32,
) -> (DvrQueueCleanupReport, DvrDemuxCleanupResult) {
    let mut report = DvrQueueCleanupReport::new(dvr_id);
    let plan = match demux.prepare_dvr_queue_cleanup(DvrRuntimeOperationRequest::new(dvr_id)) {
        Ok(plan) => {
            report.succeeded(DvrQueueCleanupStep::Prepare);
            plan
        }
        Err(error) => {
            report.failed(DvrQueueCleanupStep::Prepare, error.kind);
            skip_dvr_queue_cleanup_steps(
                &mut report,
                &[
                    DvrQueueCleanupStep::QueueClear,
                    DvrQueueCleanupStep::QueueEpochCommit,
                    DvrQueueCleanupStep::RuntimeStateCommit,
                    DvrQueueCleanupStep::PlaybackPipelineReset,
                    DvrQueueCleanupStep::PcrAnchorInvalidate,
                    DvrQueueCleanupStep::RecordIndexReset,
                    DvrQueueCleanupStep::PlaybackResidualDiscard,
                    DvrQueueCleanupStep::PlaybackDiscardDiagnosticCommit,
                ],
            );
            let outcome = if matches!(
                error.kind,
                DemuxRuntimeErrorKind::GenerationExhausted
                    | DemuxRuntimeErrorKind::QueueRuntimeFailure
            ) {
                DvrQueueCleanupOutcome::Isolated {
                    failed_step: DvrQueueCleanupStep::Prepare,
                }
            } else {
                DvrQueueCleanupOutcome::Failed {
                    failed_step: DvrQueueCleanupStep::Prepare,
                }
            };
            report.finish(outcome);
            return (report, Err(error));
        }
    };
    let kind = plan.kind();

    let committed = match demux.commit_dvr_queue_boundary_for_queue_cleanup(plan) {
        Ok(committed) => {
            report.succeeded(DvrQueueCleanupStep::QueueClear);
            report.succeeded(DvrQueueCleanupStep::QueueEpochCommit);
            committed
        }
        Err(error) => {
            record_dvr_queue_boundary_failure(&mut report, error);
            report.finish(DvrQueueCleanupOutcome::Isolated {
                failed_step: error.failed_step(),
            });
            return (report, Err(error.error()));
        }
    };

    let runtime_state_result = demux.commit_dvr_runtime_state_for_queue_cleanup(&committed);
    let playback_pipeline_reset =
        demux.reset_dvr_playback_pipeline_for_queue_cleanup(&committed);
    let playback_pcr_invalidated =
        demux.invalidate_dvr_playback_pcr_for_queue_cleanup(&committed);
    let record_index_reset = demux.reset_dvr_record_index_for_queue_cleanup(&committed);
    let first_failure = record_dvr_post_commit_demux_results(
        &mut report,
        runtime_state_result,
        playback_pipeline_reset,
        playback_pcr_invalidated,
        record_index_reset,
    );

    (report, Ok((kind, first_failure)))
}

fn record_dvr_queue_boundary_failure(
    report: &mut DvrQueueCleanupReport,
    error: DvrQueueCleanupCommitError,
) {
    match error.failed_step() {
        DvrQueueCleanupStep::QueueClear => {
            report.failed(DvrQueueCleanupStep::QueueClear, error.error().kind);
            skip_dvr_queue_cleanup_steps(report, &[DvrQueueCleanupStep::QueueEpochCommit]);
        }
        DvrQueueCleanupStep::QueueEpochCommit => {
            skip_dvr_queue_cleanup_steps(report, &[DvrQueueCleanupStep::QueueClear]);
            report.failed(
                DvrQueueCleanupStep::QueueEpochCommit,
                error.error().kind,
            );
        }
        failed_step => report.failed(failed_step, error.error().kind),
    }
    skip_dvr_queue_cleanup_steps(
        report,
        &[
            DvrQueueCleanupStep::RuntimeStateCommit,
            DvrQueueCleanupStep::PlaybackPipelineReset,
            DvrQueueCleanupStep::PcrAnchorInvalidate,
            DvrQueueCleanupStep::RecordIndexReset,
            DvrQueueCleanupStep::PlaybackResidualDiscard,
            DvrQueueCleanupStep::PlaybackDiscardDiagnosticCommit,
        ],
    );
}

fn skip_dvr_queue_cleanup_steps(
    report: &mut DvrQueueCleanupReport,
    steps: &[DvrQueueCleanupStep],
) {
    for step in steps {
        report.skipped(*step, DvrQueueCleanupSkipReason::PrerequisiteFailed);
    }
}

fn record_dvr_post_commit_demux_results(
    report: &mut DvrQueueCleanupReport,
    runtime_state_result: Result<(), DemuxRuntimeError>,
    playback_pipeline_reset: bool,
    playback_pcr_invalidated: bool,
    record_index_reset: bool,
) -> Option<DvrQueueCleanupFailure> {
    let first_failure = match runtime_state_result {
        Ok(()) => {
            report.succeeded(DvrQueueCleanupStep::RuntimeStateCommit);
            None
        }
        Err(error) => {
            report.failed(DvrQueueCleanupStep::RuntimeStateCommit, error.kind);
            Some((DvrQueueCleanupStep::RuntimeStateCommit, error))
        }
    };

    if playback_pipeline_reset {
        report.succeeded(DvrQueueCleanupStep::PlaybackPipelineReset);
    } else {
        report.skipped(
            DvrQueueCleanupStep::PlaybackPipelineReset,
            DvrQueueCleanupSkipReason::PlaybackOnly,
        );
    }
    if playback_pcr_invalidated {
        report.succeeded(DvrQueueCleanupStep::PcrAnchorInvalidate);
    } else {
        report.skipped(
            DvrQueueCleanupStep::PcrAnchorInvalidate,
            DvrQueueCleanupSkipReason::PlaybackOnly,
        );
    }
    if record_index_reset {
        report.succeeded(DvrQueueCleanupStep::RecordIndexReset);
    } else {
        report.skipped(
            DvrQueueCleanupStep::RecordIndexReset,
            DvrQueueCleanupSkipReason::RecordOnly,
        );
    }

    first_failure
}

fn finish_dvr_queue_cleanup(
    runtime: &mut TunerServiceRuntime,
    owner_demux_id: i32,
    dvr_id: i32,
    mut report: DvrQueueCleanupReport,
    primary_failure: Option<(DvrQueueCleanupStep, HalError)>,
) -> Result<(), HalError> {
    match primary_failure {
        Some((failed_step, primary)) => {
            report.finish(DvrQueueCleanupOutcome::Isolated { failed_step });
            Err(record_dvr_queue_cleanup_failure(
                runtime,
                owner_demux_id,
                dvr_id,
                report,
                primary,
            ))
        }
        None => {
            report.finish(DvrQueueCleanupOutcome::Committed);
            Ok(())
        }
    }
}

fn record_dvr_queue_cleanup_failure(
    runtime: &mut TunerServiceRuntime,
    owner_demux_id: i32,
    dvr_id: i32,
    report: DvrQueueCleanupReport,
    primary: HalError,
) -> HalError {
    let diagnostic_id = runtime.allocate_demux_transaction_diagnostic_id();
    runtime.record_demux_transaction_diagnostic(
        DemuxTransactionDiagnosticRecord::dvr_queue_cleanup(
            diagnostic_id,
            owner_demux_id,
            dvr_id,
            report.clone(),
            primary.clone(),
        ),
    );
    attach_diagnostic_detail_to_public_error(
        primary,
        format_dvr_queue_cleanup_report(diagnostic_id, &report),
    )
}

fn execute_filter_cleanup_protocol(
    demux: &mut DemuxRuntime,
    filter_id: i32,
) -> (FilterRuntimeOperationReport, Result<(), DemuxRuntimeError>) {
    let mut report =
        FilterRuntimeOperationReport::new(FilterRuntimeOperationKind::Flush, filter_id);
    let mut plan = match demux
        .prepare_filter_queue_cleanup(FilterRuntimeOperationRequest::new(filter_id))
    {
        Ok(plan) => {
            report.succeeded(FilterRuntimeOperationStep::ValidateState);
            plan
        }
        Err(error) => {
            let (failed_step, outcome) = match error.kind {
                DemuxRuntimeErrorKind::GenerationExhausted => (
                    FilterRuntimeOperationStep::SourceGenerationRefresh,
                    FilterRuntimeOperationOutcome::Isolated {
                        failed_step: FilterRuntimeOperationStep::SourceGenerationRefresh,
                    },
                ),
                DemuxRuntimeErrorKind::QueueRuntimeFailure => (
                    FilterRuntimeOperationStep::ProducerDrainCommit,
                    FilterRuntimeOperationOutcome::Isolated {
                        failed_step: FilterRuntimeOperationStep::ProducerDrainCommit,
                    },
                ),
                _ => (
                    FilterRuntimeOperationStep::ValidateState,
                    FilterRuntimeOperationOutcome::Failed {
                        failed_step: FilterRuntimeOperationStep::ValidateState,
                    },
                ),
            };
            report.failed(failed_step, error.kind);
            report.finish(outcome);
            return (report, Err(error));
        }
    };

    demux.flush_filter_pipeline_for_queue_cleanup(&plan);
    report.succeeded(FilterRuntimeOperationStep::PipelineFlush);

    match demux.clear_filter_fmq_for_queue_cleanup(&plan) {
        Ok(true) => report.succeeded(FilterRuntimeOperationStep::QueueClear),
        Ok(false) => report.skipped(
            FilterRuntimeOperationStep::QueueClear,
            FilterRuntimeOperationSkipReason::QueueNotPresent,
        ),
        Err(error) => {
            report.failed(FilterRuntimeOperationStep::QueueClear, error.kind);
            report.skipped(
                FilterRuntimeOperationStep::QueuedPayloadClear,
                FilterRuntimeOperationSkipReason::QueueClearFailed,
            );
            report.skipped(
                FilterRuntimeOperationStep::AvBackingFlush,
                FilterRuntimeOperationSkipReason::QueueClearFailed,
            );
            report.finish(FilterRuntimeOperationOutcome::Isolated {
                failed_step: FilterRuntimeOperationStep::QueueClear,
            });
            return (report, Err(error));
        }
    }

    if let Err(error) = demux.discard_filter_pending_events_for_queue_cleanup(&mut plan) {
        report.failed(FilterRuntimeOperationStep::PendingEventDiscard, error.kind);
        report.finish(FilterRuntimeOperationOutcome::Isolated {
            failed_step: FilterRuntimeOperationStep::PendingEventDiscard,
        });
        return (report, Err(error));
    }
    report.succeeded(FilterRuntimeOperationStep::PendingEventDiscard);

    let payload_outcome = demux.clear_filter_payload_state_for_queue_cleanup(&plan);
    if payload_outcome.filter_state_cleared() {
        report.succeeded(FilterRuntimeOperationStep::QueuedPayloadClear);
    } else {
        report.skipped(
            FilterRuntimeOperationStep::QueuedPayloadClear,
            FilterRuntimeOperationSkipReason::FilterMissingForOptionalFlush,
        );
    }

    if demux.flush_filter_av_backing_for_queue_cleanup(&plan) {
        report.succeeded(FilterRuntimeOperationStep::AvBackingFlush);
    } else {
        report.skipped(
            FilterRuntimeOperationStep::AvBackingFlush,
            FilterRuntimeOperationSkipReason::AvBackingNotPresent,
        );
    }
    demux.invalidate_filter_pcr_for_queue_cleanup(&plan);
    report.succeeded(FilterRuntimeOperationStep::PcrAnchorInvalidate);

    let committed = match demux.commit_filter_producer_drain_for_queue_cleanup(plan) {
        Ok(committed) => {
            report.succeeded(FilterRuntimeOperationStep::ProducerDrainCommit);
            committed
        }
        Err(error) => {
            report.failed(FilterRuntimeOperationStep::ProducerDrainCommit, error.kind);
            report.finish(FilterRuntimeOperationOutcome::Isolated {
                failed_step: FilterRuntimeOperationStep::ProducerDrainCommit,
            });
            return (report, Err(error));
        }
    };
    match demux.refresh_filter_source_generation_for_queue_cleanup(committed) {
        Ok(true) => report.succeeded(FilterRuntimeOperationStep::SourceGenerationRefresh),
        Ok(false) => report.skipped(
            FilterRuntimeOperationStep::SourceGenerationRefresh,
            FilterRuntimeOperationSkipReason::NoSourceDownstreams,
        ),
        Err(error) => {
            report.failed(FilterRuntimeOperationStep::SourceGenerationRefresh, error.kind);
            report.finish(FilterRuntimeOperationOutcome::Isolated {
                failed_step: FilterRuntimeOperationStep::SourceGenerationRefresh,
            });
            return (report, Err(error));
        }
    }
    report.finish(FilterRuntimeOperationOutcome::Committed);
    (report, Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_demux::DvrQueueCleanupStepOutcome;

    #[test]
    fn post_commit_failure_still_aggregates_every_later_demux_phase() {
        let mut report = DvrQueueCleanupReport::new(71);
        let runtime_error = DemuxRuntimeError::dvr_missing(71);

        let first_failure = record_dvr_post_commit_demux_results(
            &mut report,
            Err(runtime_error),
            true,
            true,
            false,
        );

        assert_eq!(
            first_failure,
            Some((DvrQueueCleanupStep::RuntimeStateCommit, runtime_error))
        );
        assert_eq!(
            report.steps(),
            &[
                DvrQueueCleanupStepOutcome::Failed {
                    step: DvrQueueCleanupStep::RuntimeStateCommit,
                    error: DemuxRuntimeErrorKind::DvrMissing,
                },
                DvrQueueCleanupStepOutcome::Succeeded(
                    DvrQueueCleanupStep::PlaybackPipelineReset,
                ),
                DvrQueueCleanupStepOutcome::Succeeded(
                    DvrQueueCleanupStep::PcrAnchorInvalidate,
                ),
                DvrQueueCleanupStepOutcome::Skipped {
                    step: DvrQueueCleanupStep::RecordIndexReset,
                    reason: DvrQueueCleanupSkipReason::RecordOnly,
                },
            ]
        );
    }
}
