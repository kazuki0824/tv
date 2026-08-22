use super::{
    AvStreamKind, AvStreamTypeConfig, DemuxRuntimeError, DemuxRuntimeErrorKind, DemuxRuntimeId,
    DvrChildRuntimeOpen, DvrConfigureKind, DvrConfigureRequest, DvrKind, DvrOpenKind, DvrRuntimeId,
    FilterAvStreamKind, FilterAvStreamTypeRequest, FilterChildRuntimeOpen, FilterConfig,
    FilterDelayHint, FilterDelayHintKind, FilterDelayHintRequest, FilterOpenType, FilterRuntimeId,
    HalError, HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind, OpenDvrRequest,
    OpenFilterRequest, PipelineResetReport, RegistryCommitError, TunerServiceRuntime,
};
#[cfg(test)]
use crate::diagnostics::ChildOpenRollbackKind;
use crate::diagnostics::{
    ChildOpenRollbackDiagnosticRecord, ChildOpenRollbackOutcome, ChildOpenRollbackPhase,
    DemuxTransactionDiagnosticId, DemuxTransactionDiagnosticRecord,
};
use crate::error_mapping::{object_table_error_to_hal, registry_commit_error_to_hal};
use crate::object_method_use_case::ObjectMethodExecutionToken;
use crate::open_rollback::finish_open_rollback;
use maleicacid_tuner_hal2_common::compose_primary_cleanup_failure;
use maleicacid_tuner_hal2_demux::{
    DvrDataFormat as RuntimeDvrDataFormat, FilterRuntimeState, SourceBoundaryReport,
};
use maleicacid_tuner_hal2_domain_request::DvrDataFormat as DomainDvrDataFormat;

const MAX_FILTER_DELAY_MS: i64 = 10_000;
const DVR_PACKET_SIZE_TS_188: i64 = 188;

fn format_filter_configure_report(
    diagnostic_id: DemuxTransactionDiagnosticId,
    report: &maleicacid_tuner_hal2_demux::FilterConfigureReport,
) -> String {
    format!(
        "demux runtime filter configure failed; diagnostic_id={}; outcome={:?}; steps={:?}; source_boundary_report={:?}",
        diagnostic_id.value(),
        report.outcome(),
        report.steps(),
        report.source_boundary_report()
    )
}

fn format_dvr_configure_report(
    diagnostic_id: DemuxTransactionDiagnosticId,
    report: &maleicacid_tuner_hal2_demux::DvrConfigureReport,
) -> String {
    format!(
        "demux runtime DVR configure failed; diagnostic_id={}; outcome={:?}; steps={:?}",
        diagnostic_id.value(),
        report.outcome(),
        report.steps()
    )
}

fn format_filter_runtime_operation_report(
    diagnostic_id: DemuxTransactionDiagnosticId,
    report: &maleicacid_tuner_hal2_demux::FilterRuntimeOperationReport,
) -> String {
    format!(
        "demux runtime filter operation failed; diagnostic_id={}; operation={:?}; filter_id={}; outcome={:?}; steps={:?}",
        diagnostic_id.value(),
        report.operation(),
        report.filter_id(),
        report.outcome(),
        report.steps()
    )
}

fn format_source_boundary_report(
    diagnostic_id: DemuxTransactionDiagnosticId,
    report: &SourceBoundaryReport,
) -> String {
    format!(
        "source boundary failed; diagnostic_id={}; sink_filter_id={}; source_filter_id={:?}; outcome={:?}; steps={:?}; reset_report={:?}",
        diagnostic_id.value(),
        report.sink_filter_id(),
        report.source_filter_id(),
        report.outcome(),
        report.steps(),
        report.reset_report()
    )
}

fn attach_diagnostic_detail_to_public_error(primary: HalError, detail: String) -> HalError {
    match primary {
        HalError::InvalidArgument {
            kind,
            detail: existing,
        } => HalError::invalid_argument(kind, format!("{}; {detail}", existing.detail)),
        HalError::InvalidState {
            kind,
            detail: existing,
        } => HalError::invalid_state(kind, format!("{}; {detail}", existing.detail)),
        HalError::Internal {
            kind,
            detail: existing,
        } => HalError::internal(kind, format!("{}; {detail}", existing.detail)),
        HalError::CleanupFailed {
            resource,
            detail: existing,
        } => HalError::cleanup_failed(resource, format!("{}; {detail}", existing.detail)),
        HalError::Unsupported(feature) => HalError::unsupported_detail(feature, detail),
        HalError::UnsupportedDetail {
            feature,
            detail: existing,
        } => HalError::unsupported_detail(feature, format!("{}; {detail}", existing.detail)),
        other => compose_primary_cleanup_failure(
            "demux transaction diagnostic detail attached through secondary error",
            other,
            HalError::internal(HalInternalKind::InvariantViolation, detail),
        ),
    }
}

impl TunerServiceRuntime {
    fn supported_record_status_mask() -> i32 {
        (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3)
    }

    fn supported_playback_status_mask() -> i32 {
        (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3)
    }

    fn validate_dvr_configure_request(
        buffer_size: i32,
        request: DvrConfigureRequest,
    ) -> Result<(usize, usize), HalError> {
        match request.data_format {
            DomainDvrDataFormat::Ts => {}
        }
        if request.packet_size <= 0 {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR packet size must be positive",
            ));
        }
        if request.packet_size != DVR_PACKET_SIZE_TS_188 {
            return Err(HalError::unsupported_detail(
                "dvr.packetSize",
                "positive DVR packet size other than 188 is unavailable for TS",
            ));
        }
        if request.low_threshold_bytes < 0 || request.high_threshold_bytes < 0 {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR thresholds must be non-negative",
            ));
        }
        if request.low_threshold_bytes > request.high_threshold_bytes {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR low threshold must be less than or equal to high threshold",
            ));
        }
        let capacity = usize::try_from(buffer_size).map_err(|_| {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR buffer size must be positive",
            )
        })?;
        let low_threshold = usize::try_from(request.low_threshold_bytes).map_err(|_| {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR low threshold must fit usize",
            )
        })?;
        let high_threshold = usize::try_from(request.high_threshold_bytes).map_err(|_| {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR high threshold must fit usize",
            )
        })?;
        if low_threshold > capacity || high_threshold > capacity {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR thresholds must not exceed buffer size",
            ));
        }
        let supported_mask = match request.kind {
            DvrConfigureKind::Record => Self::supported_record_status_mask(),
            DvrConfigureKind::Playback => Self::supported_playback_status_mask(),
        };
        if (request.status_mask & !supported_mask) != 0 {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR status mask contains unsupported bits",
            ));
        }
        Ok((low_threshold, high_threshold))
    }

    pub(crate) fn transact_allocate_demux_runtime(
        &mut self,
    ) -> Result<crate::registry::DemuxRegistryEntry, RegistryCommitError> {
        self.registry.allocate_demux()
    }

    pub(crate) fn transact_allocate_demux_runtime_for_public_id(
        &mut self,
        id: i32,
    ) -> Result<crate::registry::DemuxRegistryEntry, RegistryCommitError> {
        let entry = crate::registry::DemuxRegistryEntry {
            id: DemuxRuntimeId(id),
        };
        self.registry.register_demux(entry.clone())?;
        Ok(entry)
    }

    pub(crate) fn transact_unregister_demux_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::DemuxRegistryEntry>, HalError> {
        self.cleanup_descramblers_for_demux_owner_loss(id)?;
        Ok(self.registry.unregister_demux(DemuxRuntimeId(id)))
    }

    pub(crate) fn transact_allocate_filter_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<crate::registry::FilterRegistryEntry, RegistryCommitError> {
        self.registry.allocate_filter(owner_demux_id)
    }

    pub(crate) fn transact_unregister_filter_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::FilterRegistryEntry>, HalError> {
        let entry = self.registry.filter(FilterRuntimeId(id)).cloned();
        let Some(entry_ref) = entry.as_ref() else {
            return Ok(None);
        };
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(entry_ref.owner_demux_id))
        else {
            return Err(HalError::cleanup_failed(
                "filter runtime unregister owner cleanup",
                format!("owner demux runtime is missing while unregistering filter: filter_id={id} owner_demux_id={}", entry_ref.owner_demux_id),
            ));
        };
        let open_type = demux_runtime
            .filter_snapshot(id)
            .map(|snapshot| snapshot.open_type)
            .map_err(Self::map_filter_runtime_error)?;
        let release_only_backing =
            demux_runtime.take_filter_av_backing_for_release_only(id);
        if demux_runtime
            .remove_filter_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterRuntimeOperationRequest::new(id),
            )
            .is_err()
        {
            if let Some(backing) = release_only_backing {
                demux_runtime.restore_filter_av_backing_after_failed_remove(id, backing);
            }
            demux_runtime.quarantine_runtime_from_typed_request(
                maleicacid_tuner_hal2_demux::DemuxRuntimeQuarantineRequest::new(),
            );
            return Err(HalError::cleanup_failed(
                "filter runtime unregister owner cleanup",
                format!("demux runtime rejected filter removal during unregister: filter_id={id} owner_demux_id={}", entry_ref.owner_demux_id),
            ));
        }
        let removed = self.registry.unregister_filter(FilterRuntimeId(id));
        if removed.is_some() {
            if let Some(backing) = release_only_backing {
                if !backing.release_is_complete() {
                    self.release_only_filter_av_backings.insert(id, backing);
                    self.release_only_filter_types.insert(id, open_type);
                } else if let Some(identity) = backing.released_shared_handle_identity() {
                    self.released_filter_av_shared_handle_leases
                        .insert(id, identity);
                }
            }
            self.capacity_ledger.release_filter(id)?;
        }
        Ok(removed)
    }

    pub(crate) fn transact_register_demux_filter_runtime(
        &mut self,
        owner_demux_id: i32,
        filter_id: i32,
        request: &OpenFilterRequest,
    ) -> Result<(), HalError> {
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .register_filter_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterRuntimeRegistrationRequest::new(
                    filter_id,
                    request,
                    self.capability_snapshot
                        .filter_pending_event_capacity_per_filter,
                ),
            )
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter runtime registration failed",
                )
            })
    }

    pub(super) fn map_filter_runtime_error(error: DemuxRuntimeError) -> HalError {
        match error.kind {
            DemuxRuntimeErrorKind::FilterMissing => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter runtime is missing",
            ),
            DemuxRuntimeErrorKind::SourceLifecycle
            | DemuxRuntimeErrorKind::SinkLifecycle
            | DemuxRuntimeErrorKind::InvalidState => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter lifecycle is invalid for requested operation",
            ),
            DemuxRuntimeErrorKind::InvalidSourceSubtype
            | DemuxRuntimeErrorKind::InvalidSinkSubtype => {
                HalError::Unsupported("filter subtype is unsupported for requested operation")
            }
            DemuxRuntimeErrorKind::UnsupportedDvrOperation => {
                HalError::Unsupported("DVR operation is unavailable for this DVR kind")
            }
            DemuxRuntimeErrorKind::PidMismatch => HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "filter PID does not match requested operation",
            ),
            DemuxRuntimeErrorKind::SelfReference => HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "a filter cannot use itself as its data source",
            ),
            DemuxRuntimeErrorKind::GenerationExhausted => HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter generation exhausted",
            ),
            DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed => HalError::cleanup_failed(
                "filter source boundary rollback",
                "demux runtime was quarantined after source boundary rollback failure",
            ),
            DemuxRuntimeErrorKind::PipelineFailed
            | DemuxRuntimeErrorKind::RelationCommitUnknown
            | DemuxRuntimeErrorKind::DvrMissing
            | DemuxRuntimeErrorKind::InvalidDvrFilter
            | DemuxRuntimeErrorKind::QueueMissing
            | DemuxRuntimeErrorKind::QueueRuntimeFailure
            | DemuxRuntimeErrorKind::AvBackingFailure => HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter runtime pipeline operation failed",
            ),
        }
    }

    fn owner_demux_id_for_filter(&self, filter_id: i32) -> Result<i32, HalError> {
        self.registry
            .filter(FilterRuntimeId(filter_id))
            .map(|entry| entry.owner_demux_id)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter registry entry is missing",
                )
            })
    }

    pub(crate) fn transact_configure_filter_runtime_request(
        &mut self,
        filter_id: i32,
        config: FilterConfig,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let (report, result) = demux_runtime.configure_filter_runtime_with_typed_request(
            maleicacid_tuner_hal2_demux::FilterRuntimeConfigureRequest::new(filter_id, config),
        );
        match result {
            Ok(_) => Ok(()),
            Err(error) => {
                let primary = Self::map_filter_runtime_error(error);
                let diagnostic_id = self.allocate_demux_transaction_diagnostic_id();
                self.record_demux_transaction_diagnostic(
                    DemuxTransactionDiagnosticRecord::filter_configure(
                        diagnostic_id,
                        owner_demux_id,
                        filter_id,
                        report.clone(),
                        primary.clone(),
                    ),
                );
                if matches!(
                    report.outcome(),
                    Some(maleicacid_tuner_hal2_demux::FilterConfigureOutcome::Quarantined { .. })
                ) {
                    Err(compose_primary_cleanup_failure(
                        "filter configure failed and rollback failed",
                        primary,
                        HalError::cleanup_failed(
                            "filter configure rollback",
                            format_filter_configure_report(diagnostic_id, &report),
                        ),
                    ))
                } else {
                    Err(attach_diagnostic_detail_to_public_error(
                        primary,
                        format_filter_configure_report(diagnostic_id, &report),
                    ))
                }
            }
        }
    }

    pub(crate) fn transact_start_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .start_filter_runtime_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterRuntimeOperationRequest::new(filter_id),
            )
            .map_err(Self::map_filter_runtime_error)
    }

    pub(crate) fn transact_stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let (report, result) = demux_runtime.stop_filter_runtime_with_typed_request(
            maleicacid_tuner_hal2_demux::FilterRuntimeOperationRequest::new(filter_id),
        );
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let primary = Self::map_filter_runtime_error(error);
                let diagnostic_id = self.allocate_demux_transaction_diagnostic_id();
                self.record_demux_transaction_diagnostic(
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

    pub(crate) fn transact_flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let (report, result) = demux_runtime.flush_filter_runtime_with_typed_request(
            maleicacid_tuner_hal2_demux::FilterRuntimeOperationRequest::new(filter_id),
        );
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let primary = Self::map_filter_runtime_error(error);
                let diagnostic_id = self.allocate_demux_transaction_diagnostic_id();
                self.record_demux_transaction_diagnostic(
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

    pub(crate) fn transact_export_filter_av_shared_handle(
        &mut self,
        filter_id: i32,
    ) -> Result<maleicacid_tuner_hal2_demux::AvSharedHandleExport, HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let demux = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "owner demux runtime is missing",
                )
            })?;
        demux
            .export_filter_av_shared_handle_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterRuntimeOperationRequest::new(filter_id),
            )
            .map_err(Self::map_filter_runtime_error)
    }

    pub(crate) fn transact_release_filter_av_handle(
        &mut self,
        filter_id: i32,
        descriptor: maleicacid_tuner_hal2_demux::AvHandleReleaseDescriptor,
        av_data_id: i64,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let demux = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "owner demux runtime is missing",
                )
            })?;
        Self::map_av_handle_release_outcome(demux
            .release_filter_av_handle_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterAvHandleReleaseRequest::new(
                    filter_id,
                    descriptor,
                    av_data_id,
                ),
            )
            .map_err(Self::map_filter_runtime_error)?)
    }

    fn map_av_handle_release_outcome(
        outcome: maleicacid_tuner_hal2_demux::AvHandleReleaseOutcome,
    ) -> Result<(), HalError> {
        use maleicacid_tuner_hal2_demux::AvHandleReleaseOutcome;
        match outcome {
            AvHandleReleaseOutcome::EmptyHandleAccepted
            | AvHandleReleaseOutcome::ClientHandleReleased
            | AvHandleReleaseOutcome::ClientHandleReleaseAfterClose
            | AvHandleReleaseOutcome::ClientHandleAlreadyReleased
            | AvHandleReleaseOutcome::EventLocalHandleReleased { .. }
            | AvHandleReleaseOutcome::EventLocalHandleAlreadyReleased { .. }
            | AvHandleReleaseOutcome::SlotReleased { .. } => Ok(()),
            AvHandleReleaseOutcome::InvalidDataId
            | AvHandleReleaseOutcome::InvalidHandleForSlotRelease
            | AvHandleReleaseOutcome::UnknownDataId => Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "AV handle release input is invalid",
            )),
            AvHandleReleaseOutcome::RegistryFailure => Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "AV allocation registry could not classify a release safely",
            )),
        }
    }

    fn filter_id_for_av_handle_release_lifecycle(
        &self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
    ) -> Result<i32, HalError> {
        let entry = self.object_table.entry(object_id).ok_or_else(|| {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter AIDL object is missing",
            )
        })?;
        if entry.generation != generation
            || entry.object_kind
                != maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter
        {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter AIDL object identity does not match release request",
            ));
        }
        if entry.lifecycle == crate::RuntimeObjectLifecycle::Quarantined {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "quarantined filter accepts AV release only from internal cleanup",
            ));
        }
        i32::try_from(entry.ledger_id.0).map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter runtime id is outside i32 range",
            )
        })
    }

    pub fn preflight_filter_av_handle_release_for_any_lifecycle(
        &self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
    ) -> Result<(), HalError> {
        self.filter_id_for_av_handle_release_lifecycle(object_id, generation)
            .map(|_| ())
    }

    pub fn release_filter_av_handle_for_any_lifecycle(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        descriptor: maleicacid_tuner_hal2_demux::AvHandleReleaseDescriptor,
        av_data_id: i64,
    ) -> Result<(), HalError> {
        if av_data_id < 0 {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "AV data id must not be negative",
            ));
        }
        let filter_id = self.filter_id_for_av_handle_release_lifecycle(object_id, generation)?;
        if self.registry.filter(FilterRuntimeId(filter_id)).is_some() {
            return self.transact_release_filter_av_handle(filter_id, descriptor, av_data_id);
        }
        let data_id = maleicacid_tuner_hal2_demux::AvDataId(av_data_id);
        let Some(backing) = self.release_only_filter_av_backings.get_mut(&filter_id) else {
            return if av_data_id == 0
                && descriptor == maleicacid_tuner_hal2_demux::AvHandleReleaseDescriptor::Empty
            {
                Ok(())
            } else if av_data_id == 0
                && matches!(
                    descriptor,
                    maleicacid_tuner_hal2_demux::AvHandleReleaseDescriptor::File(identity)
                        if self.released_filter_av_shared_handle_leases.get(&filter_id)
                            == Some(&identity)
                )
            {
                Ok(())
            } else {
                Err(HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "AV handle release does not match a retained allocation",
                ))
            };
        };
        let outcome = backing.apply_release_after_close(descriptor, data_id);
        let complete = backing.release_is_complete();
        Self::map_av_handle_release_outcome(outcome)?;
        if complete {
            let released_shared_identity = backing.released_shared_handle_identity();
            self.release_only_filter_av_backings.remove(&filter_id);
            self.release_only_filter_types.remove(&filter_id);
            if let Some(identity) = released_shared_identity {
                self.released_filter_av_shared_handle_leases
                    .insert(filter_id, identity);
            }
        }
        Ok(())
    }

    pub fn finalize_filter_av_release_state_after_last_reference(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
    ) -> Result<(), HalError> {
        let entry = self.object_table.entry(object_id).ok_or_else(|| {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter AIDL object is missing at final AV release-state cleanup",
            )
        })?;
        if entry.generation != generation
            || entry.object_kind
                != maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter
        {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter AIDL object identity changed before final AV release-state cleanup",
            ));
        }
        if !entry.lifecycle.is_terminal() {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter must be terminal before final AV release-state cleanup",
            ));
        }
        let filter_id = i32::try_from(entry.ledger_id.0).map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter runtime id is outside i32 range during final AV cleanup",
            )
        })?;
        if self.registry.filter(FilterRuntimeId(filter_id)).is_some() {
            return Err(HalError::cleanup_failed(
                "final filter AV release-state cleanup",
                "filter runtime is still registered after terminalization",
            ));
        }

        let backing = self.release_only_filter_av_backings.remove(&filter_id);
        let open_type = self.release_only_filter_types.remove(&filter_id);
        self.released_filter_av_shared_handle_leases
            .remove(&filter_id);
        if backing.is_some() != open_type.is_some() {
            drop(backing);
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "release-only AV backing/type lifetime registry is inconsistent",
            ));
        }
        drop(backing);
        Ok(())
    }

    pub(crate) fn transact_mark_filter_callback_unhealthy(
        &mut self,
        filter_id: i32,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let demux = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "owner demux runtime is missing",
                )
            })?;
        demux
            .mark_filter_callback_unhealthy_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterRuntimeOperationRequest::new(filter_id),
            )
            .map_err(Self::map_filter_runtime_error)
    }

    pub(crate) fn transact_configure_filter_av_stream_type_request(
        &mut self,
        filter_id: i32,
        request: FilterAvStreamTypeRequest,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let snapshot = demux_runtime
            .filter_snapshot(filter_id)
            .map_err(Self::map_filter_runtime_error)?;
        match snapshot.state {
            FilterRuntimeState::Open
            | FilterRuntimeState::Configured
            | FilterRuntimeState::Stopped => {}
            FilterRuntimeState::Started => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "AV stream type cannot be changed while filter is started",
                ));
            }
            FilterRuntimeState::Closing
            | FilterRuntimeState::CleanupFailed
            | FilterRuntimeState::Closed
            | FilterRuntimeState::Failed => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter is not live",
                ));
            }
        }
        let expected_kind = match snapshot.open_type {
            FilterOpenType::TsAudio => AvStreamKind::Audio,
            FilterOpenType::TsVideo => AvStreamKind::Video,
            _ => {
                return Err(HalError::Unsupported(
                    "configureAvStreamType is available only for AV filters",
                ));
            }
        };
        let requested_kind = match request.kind {
            FilterAvStreamKind::Audio => AvStreamKind::Audio,
            FilterAvStreamKind::Video => AvStreamKind::Video,
        };
        if requested_kind != expected_kind {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedStreamSelector,
                "AV stream type kind must match filter open subtype",
            ));
        }
        demux_runtime
            .configure_filter_av_stream_type_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterAvStreamTypeRuntimeRequest::new(
                    filter_id,
                    AvStreamTypeConfig {
                        kind: requested_kind,
                        stream_type: request.stream_type,
                    },
                ),
            )
            .map_err(Self::map_filter_runtime_error)
    }

    pub(crate) fn transact_set_filter_delay_hint_request(
        &mut self,
        filter_id: i32,
        request: FilterDelayHintRequest,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let snapshot = demux_runtime
            .filter_snapshot(filter_id)
            .map_err(Self::map_filter_runtime_error)?;
        if snapshot.state.is_closed_or_failed() {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter is not live",
            ));
        }
        if matches!(
            snapshot.open_type,
            FilterOpenType::TsAudio | FilterOpenType::TsVideo
        ) {
            return Err(HalError::Unsupported(
                "FilterDelayHint is not available for media filters",
            ));
        }
        let hint = match request.kind {
            FilterDelayHintKind::TimeDelayMs => {
                if request.value > MAX_FILTER_DELAY_MS {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "filter delay time hint exceeds product limit",
                    ));
                }
                FilterDelayHint::TimeDelayMs(u64::try_from(request.value).map_err(|_| {
                    HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "filter delay hint value must be non-negative",
                    )
                })?)
            }
            FilterDelayHintKind::DataSizeDelayBytes => FilterDelayHint::DataSizeDelayBytes(
                usize::try_from(request.value).map_err(|_| {
                    HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "filter delay hint value is too large",
                    )
                })?,
            ),
        };
        demux_runtime
            .set_filter_delay_hint_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterDelayHintRuntimeRequest::new(filter_id, hint),
            )
            .map_err(Self::map_filter_runtime_error)
    }

    pub(crate) fn transact_set_filter_data_source_non_null(
        &mut self,
        demux_id: i32,
        sink_filter_id: i32,
        source_filter_id: i32,
    ) -> Result<PipelineResetReport, HalError> {
        let sink_entry = self
            .registry
            .filter(FilterRuntimeId(sink_filter_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "sink filter registry entry is missing",
                )
            })?;
        let source_entry = self
            .registry
            .filter(FilterRuntimeId(source_filter_id))
            .ok_or_else(|| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "source filter registry entry is missing",
                )
            })?;
        if sink_entry.owner_demux_id != demux_id || source_entry.owner_demux_id != demux_id {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "source filter owner demux mismatch",
            ));
        }
        let Some(demux_runtime) = self.registry.demux_runtime_mut(DemuxRuntimeId(demux_id)) else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let (report, result) = demux_runtime.set_filter_source_non_null_from_typed_request(
            maleicacid_tuner_hal2_demux::FilterSourceConnectRequest::new(
                sink_filter_id,
                source_filter_id,
            ),
        );
        match result {
            Ok(reset_report) => Ok(reset_report),
            Err(err) => {
                let diagnostic_id = self.allocate_demux_transaction_diagnostic_id();
                let hal_error = match err.kind {
                    maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::FilterMissing => {
                        HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("source or sink filter runtime is missing; {}", format_source_boundary_report(diagnostic_id, &report)))
                    }
                    maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SourceLifecycle
                    | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SinkLifecycle
                    | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidState => {
                        HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, format!("source or sink filter lifecycle is invalid; {}", format_source_boundary_report(diagnostic_id, &report)))
                    }
                    maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidSourceSubtype
                    | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidSinkSubtype => {
                        HalError::unsupported_detail(
                            "source or sink filter subtype is unsupported",
                            format_source_boundary_report(diagnostic_id, &report),
                        )
                    }
                    maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::PidMismatch => {
                        HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, format!("source and sink filter PID mismatch; {}", format_source_boundary_report(diagnostic_id, &report)))
                    }
                    maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SelfReference => {
                        HalError::invalid_argument(
                            HalInvalidArgumentKind::NumericRange,
                            format!(
                                "a filter cannot use itself as its data source; {}",
                                format_source_boundary_report(diagnostic_id, &report)
                            ),
                        )
                    }
                    maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed => {
                        HalError::cleanup_failed("filter source boundary rollback", format_source_boundary_report(diagnostic_id, &report))
                    }
                    _ => HalError::internal(maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation, format_source_boundary_report(diagnostic_id, &report)),
                };
                self.record_demux_transaction_diagnostic(
                    DemuxTransactionDiagnosticRecord::source_boundary(
                        diagnostic_id,
                        demux_id,
                        report,
                        hal_error.clone(),
                    ),
                );
                Err(hal_error)
            }
        }
    }

    pub(crate) fn transact_disconnect_filter_data_source(
        &mut self,
        demux_id: i32,
        sink_filter_id: i32,
    ) -> Result<(), HalError> {
        let sink_entry = self
            .registry
            .filter(FilterRuntimeId(sink_filter_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "sink filter registry entry is missing",
                )
            })?;
        if sink_entry.owner_demux_id != demux_id {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "sink filter owner demux mismatch",
            ));
        }
        let Some(demux_runtime) = self.registry.demux_runtime_mut(DemuxRuntimeId(demux_id)) else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let (report, result) = demux_runtime.disconnect_filter_source_from_typed_request(
            maleicacid_tuner_hal2_demux::FilterSourceDisconnectRequest::new(sink_filter_id),
        );
        match result {
            Ok(()) => Ok(()),
            Err(err) => {
                let diagnostic_id = self.allocate_demux_transaction_diagnostic_id();
                let hal_error = match err.kind {
                    maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::FilterMissing => {
                        HalError::invalid_argument(
                            HalInvalidArgumentKind::NumericRange,
                            format!("sink filter runtime is missing; {}", format_source_boundary_report(diagnostic_id, &report)),
                        )
                    }
                    maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SinkLifecycle
                    | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidState => {
                        HalError::invalid_state(
                            HalInvalidStateKind::InvalidLifecycle,
                            format!(
                                "sink filter lifecycle is invalid; {}",
                                format_source_boundary_report(diagnostic_id, &report)
                            ),
                        )
                    }
                    maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed => {
                        HalError::cleanup_failed("filter source boundary rollback", format_source_boundary_report(diagnostic_id, &report))
                    }
                    _ => HalError::internal(
                        maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                        format_source_boundary_report(diagnostic_id, &report),
                    ),
                };
                self.record_demux_transaction_diagnostic(
                    DemuxTransactionDiagnosticRecord::source_boundary(
                        diagnostic_id,
                        demux_id,
                        report,
                        hal_error.clone(),
                    ),
                );
                Err(hal_error)
            }
        }
    }

    pub(crate) fn transact_allocate_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<crate::registry::DvrRegistryEntry, RegistryCommitError> {
        self.registry.allocate_dvr(owner_demux_id)
    }

    pub(crate) fn transact_unregister_dvr_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::DvrRegistryEntry>, HalError> {
        let entry = self.registry.dvr(DvrRuntimeId(id)).cloned();
        let Some(entry_ref) = entry.as_ref() else {
            return Ok(None);
        };
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(entry_ref.owner_demux_id))
        else {
            return Err(HalError::cleanup_failed(
                "DVR runtime unregister owner cleanup",
                format!("owner demux runtime is missing while unregistering DVR: dvr_id={id} owner_demux_id={}", entry_ref.owner_demux_id),
            ));
        };
        let dropped_bytes = self
            .playback_consume_txns
            .get_mut(&id)
            .map(|txn| txn.discard_for_boundary())
            .unwrap_or(0);
        if dropped_bytes > 0 {
            let _ = demux_runtime.note_playback_consume_boundary_discard(id, dropped_bytes);
            eprintln!(
                "maleicacid-tuner-hal2-dvr-playback-diagnostic: dvr_id={} boundary=close dropped_bytes={}",
                id, dropped_bytes,
            );
        }
        if demux_runtime
            .remove_dvr_from_typed_request(
                maleicacid_tuner_hal2_demux::DvrRuntimeOperationRequest::new(id),
            )
            .is_err()
        {
            demux_runtime.quarantine_runtime_from_typed_request(
                maleicacid_tuner_hal2_demux::DemuxRuntimeQuarantineRequest::new(),
            );
            return Err(HalError::cleanup_failed(
                "DVR runtime unregister owner cleanup",
                format!("demux runtime rejected DVR removal during unregister: dvr_id={id} owner_demux_id={}", entry_ref.owner_demux_id),
            ));
        }
        let removed = self.registry.unregister_dvr(DvrRuntimeId(id));
        if removed.is_some() {
            self.capacity_ledger.release_dvr(id)?;
            self.playback_consume_txns.remove(&id);
        }
        Ok(removed)
    }

    pub(crate) fn transact_register_demux_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
        dvr_id: i32,
        request: &OpenDvrRequest,
        callback_present: bool,
    ) -> Result<(), HalError> {
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "owner demux runtime is missing",
            ));
        };
        let kind = match request.kind {
            DvrOpenKind::Record => DvrKind::Record,
            DvrOpenKind::Playback => DvrKind::Playback,
        };
        demux_runtime
            .register_dvr_from_typed_request(
                maleicacid_tuner_hal2_demux::DvrRuntimeRegistrationRequest::new(
                    dvr_id,
                    kind,
                    request.buffer_size,
                    callback_present,
                ),
            )
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "DVR runtime registration failed",
                )
            })
    }

    fn owner_demux_id_for_dvr(&self, dvr_id: i32) -> Result<i32, HalError> {
        self.registry
            .dvr(DvrRuntimeId(dvr_id))
            .map(|entry| entry.owner_demux_id)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "DVR registry entry is missing",
                )
            })
    }

    fn owner_demux_id_for_dvr_filter_relation(
        &self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<i32, HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr(dvr_id)?;
        let filter_entry = self
            .registry
            .filter(FilterRuntimeId(filter_id))
            .ok_or_else(|| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "filter registry entry is missing",
                )
            })?;
        if filter_entry.owner_demux_id != owner_demux_id {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "filter owner demux does not match DVR owner demux",
            ));
        }
        Ok(owner_demux_id)
    }

    fn map_dvr_runtime_error(error: DemuxRuntimeError) -> HalError {
        match error.kind {
            DemuxRuntimeErrorKind::DvrMissing => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "DVR runtime is missing",
            ),
            DemuxRuntimeErrorKind::FilterMissing | DemuxRuntimeErrorKind::InvalidDvrFilter => {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "filter is invalid for requested DVR operation",
                )
            }
            DemuxRuntimeErrorKind::InvalidState => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "DVR lifecycle is invalid for requested operation",
            ),
            DemuxRuntimeErrorKind::UnsupportedDvrOperation => {
                HalError::Unsupported("DVR operation is unavailable for this DVR kind")
            }
            DemuxRuntimeErrorKind::GenerationExhausted => HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR generation exhausted",
            ),
            DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed => HalError::cleanup_failed(
                "DVR source boundary rollback",
                "demux runtime was quarantined after source boundary rollback failure",
            ),
            DemuxRuntimeErrorKind::PipelineFailed
            | DemuxRuntimeErrorKind::RelationCommitUnknown
            | DemuxRuntimeErrorKind::QueueMissing
            | DemuxRuntimeErrorKind::QueueRuntimeFailure
            | DemuxRuntimeErrorKind::AvBackingFailure
            | DemuxRuntimeErrorKind::SourceLifecycle
            | DemuxRuntimeErrorKind::SinkLifecycle
            | DemuxRuntimeErrorKind::InvalidSourceSubtype
            | DemuxRuntimeErrorKind::InvalidSinkSubtype
            | DemuxRuntimeErrorKind::SelfReference
            | DemuxRuntimeErrorKind::PidMismatch => HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR runtime operation failed",
            ),
        }
    }

    pub(crate) fn transact_configure_dvr_runtime_request(
        &mut self,
        dvr_id: i32,
        request: DvrConfigureRequest,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr(dvr_id)?;
        let dvr = self
            .registry
            .demux_runtime(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "owner demux runtime is missing",
                )
            })?
            .dvr_snapshot(dvr_id)
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "DVR runtime is missing",
                )
            })?;
        let newly_reserved = self.capacity_ledger.reserve_playback_processing(
            self.capability_snapshot,
            dvr_id,
            dvr.kind,
            dvr.buffer_size,
        )?;
        let prepared_playback_consume_txn_result = if dvr.kind == DvrKind::Playback {
            match self.playback_consume_txns.get(&dvr_id) {
                Some(existing) if existing.capacity_matches(dvr.buffer_size) => Ok(None),
                Some(_) => {
                    Err(HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "playback processing capacity changed within one DVR lifetime",
                    ))
                }
                None => crate::playback_consume_txn::PlaybackConsumeTxn::prepare(
                    dvr_id,
                    dvr.buffer_size,
                )
                .map(Some)
                .map_err(|error| match error {
                    crate::playback_consume_txn::PlaybackConsumeTxnPrepareError::InvalidCapacity => {
                        HalError::invalid_argument(
                            HalInvalidArgumentKind::NumericRange,
                            "playback processing capacity must be positive",
                        )
                    }
                    crate::playback_consume_txn::PlaybackConsumeTxnPrepareError::OutOfMemory => {
                        HalError::out_of_memory(
                            "playback processing buffer",
                            "playback processing buffer allocation failed",
                        )
                    }
                }),
            }
        } else {
            Ok(None)
        };
        let prepared_playback_consume_txn = match prepared_playback_consume_txn_result {
            Ok(txn) => txn,
            Err(primary) => {
                if newly_reserved {
                    if let Err(cleanup_error) =
                        self.capacity_ledger.rollback_playback_processing(dvr_id)
                    {
                        return Err(compose_primary_cleanup_failure(
                            "playback processing allocation rollback failed",
                            primary,
                            cleanup_error,
                        ));
                    }
                }
                return Err(primary);
            }
        };
        let result = self.transact_configure_dvr_runtime_request_inner(dvr_id, request);
        match result {
            Ok(()) => {
                if let Some(txn) = prepared_playback_consume_txn {
                    self.playback_consume_txns.insert(dvr_id, txn);
                }
                Ok(())
            }
            Err(primary) => {
                if newly_reserved {
                    if let Err(cleanup_error) =
                        self.capacity_ledger.rollback_playback_processing(dvr_id)
                    {
                        return Err(compose_primary_cleanup_failure(
                            "DVR configure capacity rollback failed",
                            primary,
                            cleanup_error,
                        ));
                    }
                }
                Err(primary)
            }
        }
    }

    fn transact_configure_dvr_runtime_request_inner(
        &mut self,
        dvr_id: i32,
        request: DvrConfigureRequest,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr(dvr_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let dvr = demux_runtime.dvr_snapshot(dvr_id).map_err(|_| {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "DVR runtime is missing",
            )
        })?;
        let expected_kind = match dvr.kind {
            DvrKind::Record => DvrConfigureKind::Record,
            DvrKind::Playback => DvrConfigureKind::Playback,
        };
        if request.kind != expected_kind {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR settings kind does not match opened DVR kind",
            ));
        }
        let state = dvr.state;
        if state.is_closed_or_failed() {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "DVR is not live",
            ));
        }
        if state == super::DvrRuntimeState::Started {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "DVR cannot be reconfigured while started",
            ));
        }
        let (low_threshold, high_threshold) =
            Self::validate_dvr_configure_request(dvr.buffer_size, request)?;
        let rollback_token = demux_runtime
            .rollback_token_from_typed_request(
                maleicacid_tuner_hal2_demux::DemuxRuntimeRollbackTokenPrepareRequest::new(
                    owner_demux_id,
                ),
            )
            .map_err(Self::map_dvr_runtime_error)?;
        let (report, result) = demux_runtime.configure_dvr_runtime_with_typed_request(
            maleicacid_tuner_hal2_demux::DvrRuntimeConfigureRequest::new(dvr_id),
        );
        match result {
            Ok(_) => {
                if let Err(error) = demux_runtime.configure_dvr_status_reporting_from_typed_request(
                    maleicacid_tuner_hal2_demux::DvrStatusReportingRequest::new(
                        dvr_id,
                        request.status_mask,
                        low_threshold,
                        high_threshold,
                        RuntimeDvrDataFormat::Ts,
                        request.packet_size,
                    ),
                ) {
                    let primary = Self::map_dvr_runtime_error(error);
                    let rollback_result = demux_runtime.restore_from_rollback_request(
                        maleicacid_tuner_hal2_demux::DemuxRuntimeRollbackRestoreRequest::new(
                            rollback_token,
                        ),
                    );
                    if rollback_result.is_err() {
                        demux_runtime.quarantine_runtime_from_typed_request(
                            maleicacid_tuner_hal2_demux::DemuxRuntimeQuarantineRequest::new(),
                        );
                    }
                    let diagnostic_id = self.allocate_demux_transaction_diagnostic_id();
                    let primary = attach_diagnostic_detail_to_public_error(
                        primary,
                        format_dvr_configure_report(diagnostic_id, &report),
                    );
                    if let Err(rollback_error) = rollback_result {
                        let composed = compose_primary_cleanup_failure(
                            "DVR configure status reporting rollback failed",
                            primary,
                            Self::map_dvr_runtime_error(rollback_error),
                        );
                        self.record_demux_transaction_diagnostic(
                            DemuxTransactionDiagnosticRecord::dvr_configure(
                                diagnostic_id,
                                owner_demux_id,
                                dvr_id,
                                report.clone(),
                                composed.clone(),
                            ),
                        );
                        return Err(composed);
                    }
                    self.record_demux_transaction_diagnostic(
                        DemuxTransactionDiagnosticRecord::dvr_configure(
                            diagnostic_id,
                            owner_demux_id,
                            dvr_id,
                            report.clone(),
                            primary.clone(),
                        ),
                    );
                    return Err(primary);
                }
                demux_runtime
                    .commit_rollback_request(
                        maleicacid_tuner_hal2_demux::DemuxRuntimeRollbackCommitRequest::new(
                            rollback_token,
                        ),
                    )
                    .map_err(Self::map_dvr_runtime_error)
            }
            Err(error) => {
                if let Err(commit_error) = demux_runtime.commit_rollback_request(
                    maleicacid_tuner_hal2_demux::DemuxRuntimeRollbackCommitRequest::new(
                        rollback_token,
                    ),
                ) {
                    demux_runtime.quarantine_runtime_from_typed_request(
                        maleicacid_tuner_hal2_demux::DemuxRuntimeQuarantineRequest::new(),
                    );
                    return Err(compose_primary_cleanup_failure(
                        "DVR configure rollback token cleanup failed",
                        Self::map_dvr_runtime_error(error),
                        Self::map_dvr_runtime_error(commit_error),
                    ));
                }
                let primary = Self::map_dvr_runtime_error(error);
                let diagnostic_id = self.allocate_demux_transaction_diagnostic_id();
                self.record_demux_transaction_diagnostic(
                    DemuxTransactionDiagnosticRecord::dvr_configure(
                        diagnostic_id,
                        owner_demux_id,
                        dvr_id,
                        report.clone(),
                        primary.clone(),
                    ),
                );
                if matches!(
                    report.outcome(),
                    Some(maleicacid_tuner_hal2_demux::DvrConfigureOutcome::Quarantined { .. })
                ) {
                    Err(compose_primary_cleanup_failure(
                        "DVR configure failed and rollback failed",
                        primary,
                        HalError::cleanup_failed(
                            "DVR configure rollback",
                            format_dvr_configure_report(diagnostic_id, &report),
                        ),
                    ))
                } else {
                    Err(attach_diagnostic_detail_to_public_error(
                        primary,
                        format_dvr_configure_report(diagnostic_id, &report),
                    ))
                }
            }
        }
    }

    pub(crate) fn transact_start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr(dvr_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .start_dvr_runtime_from_typed_request(
                maleicacid_tuner_hal2_demux::DvrRuntimeOperationRequest::new(dvr_id),
            )
            .map_err(Self::map_dvr_runtime_error)
    }

    pub(crate) fn transact_attach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr_filter_relation(dvr_id, filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        super::demux_filter_dvr_ops::RecordDvrFilterRelationTxn::attach(dvr_id, filter_id)
            .execute(demux_runtime)
            .map_err(Self::map_dvr_runtime_error)
    }

    pub(crate) fn transact_detach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr_filter_relation(dvr_id, filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        super::demux_filter_dvr_ops::RecordDvrFilterRelationTxn::detach(dvr_id, filter_id)
            .execute(demux_runtime)
            .map_err(Self::map_dvr_runtime_error)
    }

    pub(crate) fn transact_stop_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr(dvr_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .stop_dvr_runtime_from_typed_request(
                maleicacid_tuner_hal2_demux::DvrRuntimeOperationRequest::new(dvr_id),
            )
            .map_err(Self::map_dvr_runtime_error)
    }

    pub(crate) fn transact_flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr(dvr_id)?;
        {
            let Some(demux_runtime) = self
                .registry
                .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
            else {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "owner demux runtime is missing",
                ));
            };
            demux_runtime
                .flush_dvr_runtime_from_typed_request(
                maleicacid_tuner_hal2_demux::DvrRuntimeOperationRequest::new(dvr_id),
            )
                .map_err(Self::map_dvr_runtime_error)?;
        }
        let dropped_bytes = self
            .playback_consume_txns
            .get_mut(&dvr_id)
            .map(|txn| txn.discard_for_boundary())
            .unwrap_or(0);
        if dropped_bytes > 0 {
            self.registry
                .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "owner demux runtime disappeared after playback flush",
                    )
                })?
                .note_playback_consume_boundary_discard(dvr_id, dropped_bytes)
                .map_err(Self::map_dvr_runtime_error)?;
        }
        Ok(())
    }

    pub(crate) fn transact_set_dvr_status_check_interval(
        &mut self,
        dvr_id: i32,
        interval_ms: u64,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr(dvr_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .set_dvr_status_check_interval_from_typed_request(
                maleicacid_tuner_hal2_demux::DvrStatusIntervalRuntimeRequest::new(
                    dvr_id,
                    interval_ms,
                ),
            )
            .map_err(Self::map_dvr_runtime_error)
    }

    pub(crate) fn transact_mark_dvr_callback_unhealthy(
        &mut self,
        dvr_id: i32,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_dvr(dvr_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .mark_dvr_callback_unhealthy_from_typed_request(
                maleicacid_tuner_hal2_demux::DvrRuntimeOperationRequest::new(dvr_id),
            )
            .map_err(Self::map_dvr_runtime_error)
    }
}

/// Private, call-local context used only by the canonical `ChildOpenTxn`.
pub(crate) struct ChildOpenContext<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl<'a> ChildOpenContext<'a> {
    pub(crate) fn new(runtime: &'a mut TunerServiceRuntime) -> Self {
        Self { runtime }
    }
    fn unregister_filter_runtime_for_open_rollback(
        &mut self,
        filter_id: i32,
        context: &'static str,
    ) -> Result<(), HalError> {
        match self.runtime.transact_unregister_filter_runtime(filter_id) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(HalError::cleanup_failed(
                context,
                format!("filter runtime is missing during rollback: id={filter_id}"),
            )),
            Err(error) => Err(error),
        }
    }

    fn unregister_dvr_runtime_for_open_rollback(
        &mut self,
        dvr_id: i32,
        context: &'static str,
    ) -> Result<(), HalError> {
        match self.runtime.transact_unregister_dvr_runtime(dvr_id) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(HalError::cleanup_failed(
                context,
                format!("DVR runtime is missing during rollback: id={dvr_id}"),
            )),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn open_filter_child_runtime_for_demux_object(
        &mut self,
        owner_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        owner_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: &OpenFilterRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<FilterChildRuntimeOpen, HalError> {
        dispatch.consume_for_object(
            self.runtime,
            owner_object_id,
            owner_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        let owner_demux_id = self.runtime.public_runtime_id_for_object_method(
            owner_object_id,
            owner_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        let capacity = self
            .runtime
            .capability_snapshot
            .filter_capacity(request.open_type);
        let capacity = usize::try_from(capacity).map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "published filter capacity cannot be represented as usize",
            )
        })?;
        let live_capacity_use = match request.open_type {
            FilterOpenType::TsRaw | FilterOpenType::TsRecord => self
                .runtime
                .registry
                .filter_open_type_count(FilterOpenType::TsRaw)?
                .checked_add(
                    self.runtime
                        .registry
                        .filter_open_type_count(FilterOpenType::TsRecord)?,
                )
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "shared TS filter capacity counter overflow",
                    )
                })?,
            _ => self
                .runtime
                .registry
                .filter_open_type_count(request.open_type)?,
        };
        let release_only_capacity_use = self
            .runtime
            .release_only_filter_types
            .values()
            .filter(|open_type| **open_type == request.open_type)
            .count();
        let capacity_use = live_capacity_use
            .checked_add(release_only_capacity_use)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter capacity counter overflow",
                )
            })?;
        if capacity_use >= capacity
            || request.open_type == FilterOpenType::TsPes
                && self
                    .runtime
                    .registry
                    .demux_has_filter_open_type(owner_demux_id, request.open_type)?
        {
            return Err(HalError::Unsupported(
                "filter capability lease is exhausted for the requested subtype",
            ));
        }
        let filter_entry = self
            .runtime
            .transact_allocate_filter_runtime(owner_demux_id)
            .map_err(|error| {
                registry_commit_error_to_hal(error, "filter runtime allocation failed")
            })?;
        if let Err(error) = self.runtime.capacity_ledger.reserve_filter(
            self.runtime.capability_snapshot,
            filter_entry.id.0,
            request.open_type,
            request.buffer_size,
        ) {
            let removed = self
                .runtime
                .registry
                .unregister_filter(FilterRuntimeId(filter_entry.id.0));
            return if removed.is_some() {
                Err(error)
            } else {
                Err(compose_primary_cleanup_failure(
                    "filter capacity reservation rollback failed",
                    error,
                    HalError::cleanup_failed(
                        "filter registry rollback",
                        "filter registry entry disappeared after capacity reservation failure",
                    ),
                ))
            };
        }
        if let Err(error) = self.runtime.transact_register_demux_filter_runtime(
            owner_demux_id,
            filter_entry.id.0,
            request,
        ) {
            return match self.unregister_filter_runtime_for_open_rollback(
                filter_entry.id.0,
                "filter child runtime rollback after demux registration failure",
            ) {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                    "filter child runtime open failure",
                    error,
                    cleanup_error,
                )),
            };
        }
        let owner = crate::RuntimeOwnerRelation::Demux {
            demux: owner_object_id,
            generation: owner_generation,
        };
        match self
            .runtime
            .register_aidl_object_for_runtime_auto_generation(
                maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
                i64::from(filter_entry.id.0),
                owner,
            ) {
            Ok(entry) => Ok(FilterChildRuntimeOpen {
                runtime_entry: entry,
                filter_id: filter_entry.id.0,
            }),
            Err(error) => {
                let primary = object_table_error_to_hal(error);
                match self.unregister_filter_runtime_for_open_rollback(
                    filter_entry.id.0,
                    "filter child runtime rollback after AIDL object registration failure",
                ) {
                    Ok(()) => Err(primary),
                    Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                        "filter child AIDL object registration failure",
                        primary,
                        cleanup_error,
                    )),
                }
            }
        }
    }

    pub(crate) fn open_dvr_child_runtime_for_demux_object(
        &mut self,
        owner_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        owner_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: OpenDvrRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<DvrChildRuntimeOpen, HalError> {
        dispatch.consume_for_object(
            self.runtime,
            owner_object_id,
            owner_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        let owner_demux_id = self.runtime.public_runtime_id_for_object_method(
            owner_object_id,
            owner_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        let dvr_kind = match request.kind {
            DvrOpenKind::Record => DvrKind::Record,
            DvrOpenKind::Playback => DvrKind::Playback,
        };
        let dvr_capacity = match dvr_kind {
            DvrKind::Record => self.runtime.capability_snapshot.num_record,
            DvrKind::Playback => self.runtime.capability_snapshot.num_playback,
        };
        let dvr_capacity = usize::try_from(dvr_capacity).map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "published DVR capacity cannot be represented as usize",
            )
        })?;
        if self.runtime.registry.dvr_kind_count(dvr_kind)? >= dvr_capacity
            || self
                .runtime
                .registry
                .demux_has_dvr_kind(owner_demux_id, dvr_kind)?
        {
            return Err(HalError::Unsupported(
                "DVR capability lease is exhausted for the requested kind",
            ));
        }
        let dvr_entry = self
            .runtime
            .transact_allocate_dvr_runtime(owner_demux_id)
            .map_err(|error| {
                registry_commit_error_to_hal(error, "DVR runtime allocation failed")
            })?;
        if let Err(error) = self.runtime.capacity_ledger.reserve_dvr(
            self.runtime.capability_snapshot,
            dvr_entry.id.0,
            request.buffer_size,
        ) {
            let removed = self
                .runtime
                .registry
                .unregister_dvr(DvrRuntimeId(dvr_entry.id.0));
            return if removed.is_some() {
                Err(error)
            } else {
                Err(compose_primary_cleanup_failure(
                    "DVR capacity reservation rollback failed",
                    error,
                    HalError::cleanup_failed(
                        "DVR registry rollback",
                        "DVR registry entry disappeared after capacity reservation failure",
                    ),
                ))
            };
        }
        if let Err(error) = self.runtime.transact_register_demux_dvr_runtime(
            owner_demux_id,
            dvr_entry.id.0,
            &request,
            true,
        ) {
            return match self.unregister_dvr_runtime_for_open_rollback(
                dvr_entry.id.0,
                "DVR child runtime rollback after demux registration failure",
            ) {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                    "DVR child runtime open failure",
                    error,
                    cleanup_error,
                )),
            };
        }
        let owner = crate::RuntimeOwnerRelation::Demux {
            demux: owner_object_id,
            generation: owner_generation,
        };
        match self
            .runtime
            .register_aidl_object_for_runtime_auto_generation(
                maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
                i64::from(dvr_entry.id.0),
                owner,
            ) {
            Ok(entry) => Ok(DvrChildRuntimeOpen {
                runtime_entry: entry,
                dvr_id: dvr_entry.id.0,
            }),
            Err(error) => {
                let primary = object_table_error_to_hal(error);
                match self.unregister_dvr_runtime_for_open_rollback(
                    dvr_entry.id.0,
                    "DVR child runtime rollback after AIDL object registration failure",
                ) {
                    Ok(()) => Err(primary),
                    Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                        "DVR child AIDL object registration failure",
                        primary,
                        cleanup_error,
                    )),
                }
            }
        }
    }

    pub(crate) fn rollback_filter_child_open_after_aidl_failure(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        filter_id: i32,
    ) -> Result<(), HalError> {
        let object_registration_rollback = self
            .runtime
            .unregister_aidl_object_after_registration_failure(object_id, generation)
            .map(|_| ())
            .map_err(object_table_error_to_hal);
        let runtime_cleanup = match self.runtime.transact_unregister_filter_runtime(filter_id) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter runtime rollback target is missing",
            )),
            Err(error) => Err(error),
        };
        let object_error = object_registration_rollback.as_ref().err().cloned();
        let runtime_error = runtime_cleanup.as_ref().err().cloned();
        if let Some(record) = child_open_rollback_diagnostic_record(
            ChildOpenRollbackPhase::FilterOpen,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
            object_id,
            generation,
            filter_id,
            object_error,
            runtime_error,
        ) {
            self.runtime.record_child_open_rollback_diagnostic(record);
        }
        finish_open_rollback(
            object_registration_rollback,
            || runtime_cleanup,
            "filter child object open rollback",
        )
    }

    pub(crate) fn rollback_dvr_child_open_after_aidl_failure(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dvr_id: i32,
    ) -> Result<(), HalError> {
        let object_registration_rollback = self
            .runtime
            .unregister_aidl_object_after_registration_failure(object_id, generation)
            .map(|_| ())
            .map_err(object_table_error_to_hal);
        let runtime_cleanup = match self.runtime.transact_unregister_dvr_runtime(dvr_id) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "DVR runtime rollback target is missing",
            )),
            Err(error) => Err(error),
        };
        let object_error = object_registration_rollback.as_ref().err().cloned();
        let runtime_error = runtime_cleanup.as_ref().err().cloned();
        if let Some(record) = child_open_rollback_diagnostic_record(
            ChildOpenRollbackPhase::DvrOpen,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
            object_id,
            generation,
            dvr_id,
            object_error,
            runtime_error,
        ) {
            self.runtime.record_child_open_rollback_diagnostic(record);
        }
        finish_open_rollback(
            object_registration_rollback,
            || runtime_cleanup,
            "DVR child object open rollback",
        )
    }
}

fn child_open_rollback_diagnostic_record(
    phase: ChildOpenRollbackPhase,
    object_kind: maleicacid_tuner_hal2_domain_request::AidlObjectKind,
    object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
    generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
    runtime_id: i32,
    object_rollback_error: Option<HalError>,
    runtime_cleanup_failure: Option<HalError>,
) -> Option<ChildOpenRollbackDiagnosticRecord> {
    match (object_rollback_error, runtime_cleanup_failure) {
        (Some(object_error), Some(runtime_error)) => Some(ChildOpenRollbackDiagnosticRecord::new(
            phase,
            object_kind,
            object_id,
            generation,
            runtime_id,
            ChildOpenRollbackOutcome::BothFailed {
                object_error,
                runtime_cleanup_error: runtime_error,
            },
        )),
        (Some(error), None) => Some(ChildOpenRollbackDiagnosticRecord::new(
            phase,
            object_kind,
            object_id,
            generation,
            runtime_id,
            ChildOpenRollbackOutcome::ObjectRegistrationRollbackFailed {
                object_error: error,
            },
        )),
        (None, Some(error)) => Some(ChildOpenRollbackDiagnosticRecord::new(
            phase,
            object_kind,
            object_id,
            generation,
            runtime_id,
            ChildOpenRollbackOutcome::RuntimeCleanupMissing {
                runtime_cleanup_error: error,
            },
        )),
        (None, None) => None,
    }
}

#[cfg(test)]
mod child_open_rollback_diagnostic_tests {
    use super::*;

    #[test]
    fn child_open_rollback_diagnostic_records_both_errors() {
        let object_error = HalError::internal(
            HalInternalKind::InvariantViolation,
            "object rollback failed",
        );
        let runtime_error = HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "runtime cleanup target is missing",
        );

        let record = child_open_rollback_diagnostic_record(
            ChildOpenRollbackPhase::FilterOpen,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
            maleicacid_tuner_hal2_domain_request::AidlObjectId(9001),
            maleicacid_tuner_hal2_domain_request::AidlObjectGeneration(7),
            42,
            Some(object_error.clone()),
            Some(runtime_error.clone()),
        )
        .expect("diagnostic is recorded");

        assert_eq!(record.kind(), ChildOpenRollbackKind::BothFailed);
        assert_eq!(
            record.outcome,
            ChildOpenRollbackOutcome::BothFailed {
                object_error,
                runtime_cleanup_error: runtime_error,
            }
        );
    }

    #[test]
    fn child_open_rollback_diagnostic_omits_successful_rollback() {
        assert!(child_open_rollback_diagnostic_record(
            ChildOpenRollbackPhase::DvrOpen,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
            maleicacid_tuner_hal2_domain_request::AidlObjectId(9002),
            maleicacid_tuner_hal2_domain_request::AidlObjectGeneration(3),
            43,
            None,
            None,
        )
        .is_none());
    }
}
