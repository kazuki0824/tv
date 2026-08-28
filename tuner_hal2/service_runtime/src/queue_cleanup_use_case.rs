use crate::boot::{
    attach_diagnostic_detail_to_public_error, format_filter_runtime_operation_report,
    TunerServiceRuntime,
};
use crate::diagnostics::DemuxTransactionDiagnosticRecord;
use crate::registry::{DemuxRuntimeId, DvrRuntimeId, FilterRuntimeId};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind, HalInvalidStateKind};
use maleicacid_tuner_hal2_demux::{DvrRuntimeOperationRequest, FilterRuntimeOperationRequest};

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
        let (report, result) = self
            .runtime
            .registry_mut()
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "owner demux runtime is missing",
                )
            })?
            .flush_filter_runtime_with_typed_request(FilterRuntimeOperationRequest::new(
                filter_id,
            ));
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
