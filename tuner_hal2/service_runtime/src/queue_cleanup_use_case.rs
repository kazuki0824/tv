use crate::boot::{
    attach_diagnostic_detail_to_public_error, format_filter_runtime_operation_report,
    TunerServiceRuntime,
};
use crate::diagnostics::DemuxTransactionDiagnosticRecord;
use crate::registry::{DemuxRuntimeId, DvrRuntimeId, FilterRuntimeId};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidStateKind};
use maleicacid_tuner_hal2_demux::{
    DemuxRuntime, DemuxRuntimeError, DemuxRuntimeErrorKind, DvrRuntimeOperationRequest,
    FilterRuntimeOperationKind, FilterRuntimeOperationOutcome, FilterRuntimeOperationReport,
    FilterRuntimeOperationRequest, FilterRuntimeOperationSkipReason, FilterRuntimeOperationStep,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueCleanupTarget {
    Filter { filter_id: i32 },
    Dvr { dvr_id: i32 },
}

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
        self.runtime
            .registry_mut()
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "owner demux runtime is missing",
                )
            })?
            .flush_dvr_runtime_from_typed_request(DvrRuntimeOperationRequest::new(dvr_id))
            .map_err(TunerServiceRuntime::map_dvr_runtime_error)?;

        let dropped_bytes = self
            .runtime
            .discard_playback_consume_for_queue_cleanup(dvr_id);
        if dropped_bytes == 0 {
            return Ok(());
        }
        self.runtime
            .registry_mut()
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "owner demux runtime disappeared after playback flush",
                )
            })?
            .note_playback_consume_boundary_discard(dvr_id, dropped_bytes)
            .map_err(TunerServiceRuntime::map_dvr_runtime_error)
    }
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
