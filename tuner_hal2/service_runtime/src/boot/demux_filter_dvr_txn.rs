use super::{
    AvStreamKind, AvStreamTypeConfig, DemuxRuntimeError, DemuxRuntimeErrorKind, DemuxRuntimeId,
    DvrChildRuntimeOpen, DvrConfigureKind, DvrConfigureRequest, DvrKind, DvrOpenKind, DvrRuntimeId,
    FilterAvStreamKind, FilterAvStreamTypeRequest, FilterChildRuntimeOpen, FilterConfig,
    FilterDelayHint, FilterDelayHintKind, FilterDelayHintRequest, FilterOpenType, FilterRuntimeId,
    HalError, HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind, OpenDvrRequest,
    OpenFilterRequest, PipelineResetReport, RegistryCommitError, TunerServiceRuntime,
};
use crate::diagnostics::{
    ChildOpenRollbackDiagnosticRecord, ChildOpenRollbackKind, ChildOpenRollbackOutcome,
    ChildOpenRollbackPhase, DemuxTransactionDiagnosticId, DemuxTransactionDiagnosticRecord,
};
use crate::error_mapping::{object_table_error_to_hal, registry_commit_error_to_hal};
use crate::object_method_txn::ObjectMethodExecutionToken;
use crate::open_rollback::finish_open_rollback;
use maleicacid_tuner_hal2_common::compose_primary_cleanup_failure;
use maleicacid_tuner_hal2_demux::{FilterRuntimeState, SourceBoundaryReport};

const MAX_FILTER_DELAY_MS: i64 = 10_000;

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
        if demux_runtime
            .remove_filter_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterRuntimeOperationRequest::new(id),
            )
            .is_err()
        {
            demux_runtime.quarantine_runtime_from_typed_request(
                maleicacid_tuner_hal2_demux::DemuxRuntimeQuarantineRequest::new(),
            );
            return Err(HalError::cleanup_failed(
                "filter runtime unregister owner cleanup",
                format!("demux runtime rejected filter removal during unregister: filter_id={id} owner_demux_id={}", entry_ref.owner_demux_id),
            ));
        }
        Ok(self.registry.unregister_filter(FilterRuntimeId(id)))
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
                    filter_id, request,
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
            DemuxRuntimeErrorKind::GenerationExhausted => HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter generation exhausted",
            ),
            DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed => HalError::cleanup_failed(
                "filter source boundary rollback",
                "demux runtime was quarantined after source boundary rollback failure",
            ),
            DemuxRuntimeErrorKind::PipelineFailed
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
        has_fd: bool,
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
        use maleicacid_tuner_hal2_demux::AvHandleReleaseOutcome;
        match demux
            .release_filter_av_handle_from_typed_request(
                maleicacid_tuner_hal2_demux::FilterAvHandleReleaseRequest::new(
                    filter_id, has_fd, av_data_id,
                ),
            )
            .map_err(Self::map_filter_runtime_error)?
        {
            AvHandleReleaseOutcome::ClientHandleReleased
            | AvHandleReleaseOutcome::ClientHandleReleaseAfterClose
            | AvHandleReleaseOutcome::SlotReleased { .. }
            | AvHandleReleaseOutcome::StaleReleaseAccepted { .. }
            | AvHandleReleaseOutcome::StaleReleaseAfterClose { .. } => Ok(()),
            AvHandleReleaseOutcome::ClientHandleAlreadyReleased
            | AvHandleReleaseOutcome::InvalidDataId
            | AvHandleReleaseOutcome::InvalidHandleForSlotRelease
            | AvHandleReleaseOutcome::UnknownDataId => Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "AV handle release input is invalid",
            )),
            AvHandleReleaseOutcome::UnavailableForNonAvFilter => Err(HalError::Unsupported(
                "AV shared handle is unavailable for non-AV filter",
            )),
            AvHandleReleaseOutcome::InvalidStateWithoutSharedHandle => {
                Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "AV shared handle has not been exported for this filter state",
                ))
            }
        }
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
            FilterRuntimeState::Configured
            | FilterRuntimeState::Started
            | FilterRuntimeState::Stopped => {}
            FilterRuntimeState::Open => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "AV stream type can be configured only after filter configure",
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
        if snapshot.state == FilterRuntimeState::Started {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AV stream type cannot be changed while filter is started",
            ));
        }
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
        Ok(self.registry.unregister_dvr(DvrRuntimeId(id)))
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
            | DemuxRuntimeErrorKind::QueueMissing
            | DemuxRuntimeErrorKind::QueueRuntimeFailure
            | DemuxRuntimeErrorKind::AvBackingFailure
            | DemuxRuntimeErrorKind::SourceLifecycle
            | DemuxRuntimeErrorKind::SinkLifecycle
            | DemuxRuntimeErrorKind::InvalidSourceSubtype
            | DemuxRuntimeErrorKind::InvalidSinkSubtype
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
        if dvr.callback_unhealthy {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "DVR callback is unhealthy",
            ));
        }
        let (low_threshold, high_threshold) =
            Self::validate_dvr_configure_request(dvr.buffer_size, request)?;
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
                    ),
                ) {
                    let primary = Self::map_dvr_runtime_error(error);
                    let diagnostic_id = self.allocate_demux_transaction_diagnostic_id();
                    let primary = attach_diagnostic_detail_to_public_error(
                        primary,
                        format_dvr_configure_report(diagnostic_id, &report),
                    );
                    if let Err(rollback_error) =
                        demux_runtime.restore_dvr_snapshot(dvr_id, dvr.clone())
                    {
                        demux_runtime.quarantine();
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
                Ok(())
            }
            Err(error) => {
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
        demux_runtime
            .attach_dvr_filter_from_typed_request(
                maleicacid_tuner_hal2_demux::DvrFilterLinkRequest::new(dvr_id, filter_id),
            )
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
        demux_runtime
            .detach_dvr_filter_from_typed_request(
                maleicacid_tuner_hal2_demux::DvrFilterLinkRequest::new(dvr_id, filter_id),
            )
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
            .map_err(Self::map_dvr_runtime_error)
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

pub(crate) struct DemuxFilterDvrTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn demux_filter_dvr_txn(&mut self) -> DemuxFilterDvrTxn<'_> {
        DemuxFilterDvrTxn { runtime: self }
    }
}

impl<'a> DemuxFilterDvrTxn<'a> {
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
        let filter_entry = self
            .runtime
            .transact_allocate_filter_runtime(owner_demux_id)
            .map_err(|error| {
                registry_commit_error_to_hal(error, "filter runtime allocation failed")
            })?;
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
        let dvr_entry = self
            .runtime
            .transact_allocate_dvr_runtime(owner_demux_id)
            .map_err(|error| {
                registry_commit_error_to_hal(error, "DVR runtime allocation failed")
            })?;
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
