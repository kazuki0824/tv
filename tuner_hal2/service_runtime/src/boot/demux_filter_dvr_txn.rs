use super::{
    AvStreamKind, AvStreamTypeConfig, DemuxRuntimeError, DemuxRuntimeErrorKind, DemuxRuntimeId,
    DvrConfigureKind, DvrConfigureRequest, DvrKind, DvrOpenKind, DvrRuntime, DvrRuntimeId,
    FilterAvStreamKind, FilterAvStreamTypeRequest, FilterConfig, FilterConfigureTxn,
    FilterDelayHint, FilterDelayHintKind, FilterDelayHintRequest, FilterOpenType, FilterRuntime,
    FilterRuntimeId, FilterRuntimeState, HalError, HalInternalKind, HalInvalidArgumentKind,
    HalInvalidStateKind, OpenDvrRequest, OpenFilterRequest, PipelineResetReport,
    RegistryCommitError, TunerServiceRuntime,
};
use crate::diagnostics::{
    ChildOpenRollbackDiagnosticRecord, ChildOpenRollbackKind, ChildOpenRollbackPhase,
};
use crate::error_mapping::{object_table_error_to_hal, registry_commit_error_to_hal};
use crate::object_method_txn::ObjectMethodDispatchPreflight;
use crate::open_rollback::finish_open_rollback;
use maleicacid_tuner_hal2_common::compose_primary_cleanup_failure;

impl TunerServiceRuntime {
    fn transact_allocate_demux_runtime(
        &mut self,
    ) -> Result<crate::registry::DemuxRegistryEntry, RegistryCommitError> {
        self.registry.allocate_demux()
    }

    fn transact_unregister_demux_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::DemuxRegistryEntry>, HalError> {
        let cleanup = self.cleanup_descramblers_for_demux_owner_loss(id);
        let entry = self.registry.unregister_demux(DemuxRuntimeId(id));
        cleanup.map(|()| entry)
    }

    fn transact_allocate_filter_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<crate::registry::FilterRegistryEntry, RegistryCommitError> {
        self.registry.allocate_filter(owner_demux_id)
    }

    fn transact_unregister_filter_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::FilterRegistryEntry>, HalError> {
        let entry = self.registry.unregister_filter(FilterRuntimeId(id));
        let Some(entry_ref) = entry.as_ref() else {
            return Ok(entry);
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
        if demux_runtime.remove_filter(id).is_err() {
            demux_runtime.quarantine();
            return Err(HalError::cleanup_failed(
                "filter runtime unregister owner cleanup",
                format!("demux runtime rejected filter removal during unregister: filter_id={id} owner_demux_id={}", entry_ref.owner_demux_id),
            ));
        }
        Ok(entry)
    }

    fn transact_register_demux_filter_runtime(
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
            .register_filter(FilterRuntime::new_open_request(
                filter_id,
                demux_runtime.generation(),
                request,
            ))
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
            DemuxRuntimeErrorKind::PidMismatch => HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "filter PID does not match requested operation",
            ),
            DemuxRuntimeErrorKind::GenerationExhausted => HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter generation exhausted",
            ),
            DemuxRuntimeErrorKind::PipelineFailed
            | DemuxRuntimeErrorKind::DvrMissing
            | DemuxRuntimeErrorKind::InvalidDvrFilter
            | DemuxRuntimeErrorKind::QueueMissing
            | DemuxRuntimeErrorKind::QueueRuntimeFailure => HalError::internal(
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

    fn transact_configure_filter_runtime_request(
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
        let (_txn, result) = FilterConfigureTxn::new(filter_id).configure(
            demux_runtime,
            config.open_type.pipeline_open_kind(),
            config.pipeline_config(),
        );
        result.map(|_| ()).map_err(Self::map_filter_runtime_error)
    }

    fn transact_start_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
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
            .start_filter_runtime(filter_id)
            .map_err(Self::map_filter_runtime_error)
    }

    fn transact_stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
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
            .stop_filter_runtime(filter_id)
            .map_err(Self::map_filter_runtime_error)
    }

    fn transact_flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
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
            .flush_filter_runtime(filter_id)
            .map_err(Self::map_filter_runtime_error)
    }

    fn transact_configure_filter_av_stream_type_request(
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
            .configure_filter_av_stream_type(
                filter_id,
                AvStreamTypeConfig {
                    kind: requested_kind,
                    stream_type: request.stream_type,
                },
            )
            .map_err(Self::map_filter_runtime_error)
    }

    fn transact_set_filter_delay_hint_request(
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
                FilterDelayHint::TimeDelayMs(u64::try_from(request.value).map_err(|_| {
                    HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "filter delay hint value must be non-negative",
                    )
                })?)
            }
            FilterDelayHintKind::DataSizeDelayBytes => {
                if snapshot.open_type == FilterOpenType::TsRecord {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "record filters do not accept data-size delay hints",
                    ));
                }
                FilterDelayHint::DataSizeDelayBytes(usize::try_from(request.value).map_err(
                    |_| {
                        HalError::invalid_argument(
                            HalInvalidArgumentKind::NumericRange,
                            "filter delay hint value is too large",
                        )
                    },
                )?)
            }
        };
        demux_runtime
            .set_filter_delay_hint(filter_id, hint)
            .map_err(Self::map_filter_runtime_error)
    }

    fn transact_set_filter_data_source_non_null(
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
        demux_runtime
            .set_filter_source_non_null(sink_filter_id, source_filter_id)
            .map_err(|err| match err.kind {
                maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::FilterMissing => {
                    HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "source or sink filter runtime is missing")
                }
                maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::SourceLifecycle
                | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::SinkLifecycle
                | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidState => {
                    HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, "source or sink filter lifecycle is invalid")
                }
                maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidSourceSubtype
                | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidSinkSubtype => {
                    HalError::Unsupported("source or sink filter subtype is unsupported")
                }
                maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::PidMismatch => {
                    HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "source and sink filter PID mismatch")
                }
                _ => HalError::internal(maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation, "filter source boundary failed"),
            })
    }

    fn transact_allocate_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<crate::registry::DvrRegistryEntry, RegistryCommitError> {
        self.registry.allocate_dvr(owner_demux_id)
    }

    fn transact_unregister_dvr_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::DvrRegistryEntry>, HalError> {
        let entry = self.registry.unregister_dvr(DvrRuntimeId(id));
        let Some(entry_ref) = entry.as_ref() else {
            return Ok(entry);
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
        if demux_runtime.remove_dvr(id).is_err() {
            demux_runtime.quarantine();
            return Err(HalError::cleanup_failed(
                "DVR runtime unregister owner cleanup",
                format!("demux runtime rejected DVR removal during unregister: dvr_id={id} owner_demux_id={}", entry_ref.owner_demux_id),
            ));
        }
        Ok(entry)
    }

    fn transact_register_demux_dvr_runtime(
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
            .register_dvr(DvrRuntime::new_open_request(
                dvr_id,
                kind,
                demux_runtime.generation(),
                request.buffer_size,
                callback_present,
            ))
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
            DemuxRuntimeErrorKind::GenerationExhausted => HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR generation exhausted",
            ),
            DemuxRuntimeErrorKind::PipelineFailed
            | DemuxRuntimeErrorKind::QueueMissing
            | DemuxRuntimeErrorKind::QueueRuntimeFailure
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

    fn transact_configure_dvr_runtime_request(
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
        let Some(dvr) = demux_runtime.dvr(dvr_id) else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "DVR runtime is missing",
            ));
        };
        let expected_kind = match dvr.kind() {
            DvrKind::Record => DvrConfigureKind::Record,
            DvrKind::Playback => DvrConfigureKind::Playback,
        };
        if request.kind != expected_kind {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DVR settings kind does not match opened DVR kind",
            ));
        }
        let state = dvr.state();
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
        let (_txn, result) = super::DvrConfigureTxn::new(dvr_id).configure(demux_runtime);
        result.map(|_| ()).map_err(Self::map_dvr_runtime_error)
    }

    fn transact_start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
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
            .start_dvr_runtime(dvr_id)
            .map_err(Self::map_dvr_runtime_error)
    }

    fn transact_attach_dvr_filter(&mut self, dvr_id: i32, filter_id: i32) -> Result<(), HalError> {
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
            .attach_dvr_filter(dvr_id, filter_id)
            .map_err(Self::map_dvr_runtime_error)
    }

    fn transact_detach_dvr_filter(&mut self, dvr_id: i32, filter_id: i32) -> Result<(), HalError> {
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
            .detach_dvr_filter(dvr_id, filter_id)
            .map_err(Self::map_dvr_runtime_error)
    }

    fn transact_stop_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
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
            .stop_dvr_runtime(dvr_id)
            .map_err(Self::map_dvr_runtime_error)
    }

    fn transact_flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
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
            .flush_dvr_runtime(dvr_id)
            .map_err(Self::map_dvr_runtime_error)
    }

    fn transact_set_dvr_status_check_interval(
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
            .set_dvr_status_check_interval(dvr_id, interval_ms)
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
    pub(crate) fn allocate_demux_runtime(
        &mut self,
    ) -> Result<crate::registry::DemuxRegistryEntry, RegistryCommitError> {
        self.runtime.transact_allocate_demux_runtime()
    }

    pub(crate) fn unregister_demux_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::DemuxRegistryEntry>, HalError> {
        self.runtime.transact_unregister_demux_runtime(id)
    }

    pub(crate) fn allocate_filter_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<crate::registry::FilterRegistryEntry, RegistryCommitError> {
        self.runtime
            .transact_allocate_filter_runtime(owner_demux_id)
    }

    pub(crate) fn unregister_filter_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::FilterRegistryEntry>, HalError> {
        self.runtime.transact_unregister_filter_runtime(id)
    }

    pub(crate) fn register_demux_filter_runtime(
        &mut self,
        owner_demux_id: i32,
        filter_id: i32,
        request: &OpenFilterRequest,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_register_demux_filter_runtime(owner_demux_id, filter_id, request)
    }

    pub(crate) fn configure_filter_runtime_request(
        &mut self,
        filter_id: i32,
        config: FilterConfig,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_configure_filter_runtime_request(filter_id, config)
    }

    pub(crate) fn start_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.runtime.transact_start_filter_runtime(filter_id)
    }

    pub(crate) fn stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.runtime.transact_stop_filter_runtime(filter_id)
    }

    pub(crate) fn flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.runtime.transact_flush_filter_runtime(filter_id)
    }

    pub(crate) fn configure_filter_av_stream_type_request(
        &mut self,
        filter_id: i32,
        request: FilterAvStreamTypeRequest,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_configure_filter_av_stream_type_request(filter_id, request)
    }

    pub(crate) fn set_filter_delay_hint_request(
        &mut self,
        filter_id: i32,
        request: FilterDelayHintRequest,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_set_filter_delay_hint_request(filter_id, request)
    }

    pub(crate) fn set_filter_data_source_non_null(
        &mut self,
        demux_id: i32,
        sink_filter_id: i32,
        source_filter_id: i32,
    ) -> Result<PipelineResetReport, HalError> {
        self.runtime.transact_set_filter_data_source_non_null(
            demux_id,
            sink_filter_id,
            source_filter_id,
        )
    }

    pub(crate) fn allocate_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<crate::registry::DvrRegistryEntry, RegistryCommitError> {
        self.runtime.transact_allocate_dvr_runtime(owner_demux_id)
    }

    pub(crate) fn unregister_dvr_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<crate::registry::DvrRegistryEntry>, HalError> {
        self.runtime.transact_unregister_dvr_runtime(id)
    }

    fn unregister_filter_runtime_for_open_rollback(
        &mut self,
        filter_id: i32,
        context: &'static str,
    ) -> Result<(), HalError> {
        match self.unregister_filter_runtime(filter_id) {
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
        match self.unregister_dvr_runtime(dvr_id) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(HalError::cleanup_failed(
                context,
                format!("DVR runtime is missing during rollback: id={dvr_id}"),
            )),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn register_demux_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
        dvr_id: i32,
        request: &OpenDvrRequest,
        callback_present: bool,
    ) -> Result<(), HalError> {
        self.runtime.transact_register_demux_dvr_runtime(
            owner_demux_id,
            dvr_id,
            request,
            callback_present,
        )
    }

    pub(crate) fn configure_dvr_runtime_request(
        &mut self,
        dvr_id: i32,
        request: DvrConfigureRequest,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_configure_dvr_runtime_request(dvr_id, request)
    }

    pub(crate) fn start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.runtime.transact_start_dvr_runtime(dvr_id)
    }

    pub(crate) fn attach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), HalError> {
        self.runtime.transact_attach_dvr_filter(dvr_id, filter_id)
    }

    pub(crate) fn detach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), HalError> {
        self.runtime.transact_detach_dvr_filter(dvr_id, filter_id)
    }

    pub(crate) fn stop_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.runtime.transact_stop_dvr_runtime(dvr_id)
    }

    pub(crate) fn flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.runtime.transact_flush_dvr_runtime(dvr_id)
    }

    pub(crate) fn set_dvr_status_check_interval(
        &mut self,
        dvr_id: i32,
        interval_ms: u64,
    ) -> Result<(), HalError> {
        self.runtime
            .transact_set_dvr_status_check_interval(dvr_id, interval_ms)
    }

    pub(crate) fn open_filter_child_runtime_for_demux_object(
        &mut self,
        owner_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        owner_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: &OpenFilterRequest,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<crate::RuntimeObjectEntry, HalError> {
        let owner_demux_id = self.runtime.public_runtime_id_for_object_method(
            owner_object_id,
            owner_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        dispatch.plan(self.runtime)?;
        let filter_entry = self
            .allocate_filter_runtime(owner_demux_id)
            .map_err(|error| {
                registry_commit_error_to_hal(error, "filter runtime allocation failed")
            })?;
        if let Err(error) =
            self.register_demux_filter_runtime(owner_demux_id, filter_entry.id.0, request)
        {
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
            Ok(entry) => Ok(entry),
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
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<crate::RuntimeObjectEntry, HalError> {
        let owner_demux_id = self.runtime.public_runtime_id_for_object_method(
            owner_object_id,
            owner_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        dispatch.plan(self.runtime)?;
        let dvr_entry = self.allocate_dvr_runtime(owner_demux_id).map_err(|error| {
            registry_commit_error_to_hal(error, "DVR runtime allocation failed")
        })?;
        if let Err(error) =
            self.register_demux_dvr_runtime(owner_demux_id, dvr_entry.id.0, &request, true)
        {
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
            Ok(entry) => Ok(entry),
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
        let runtime_cleanup = match self.unregister_filter_runtime(filter_id) {
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
        let runtime_cleanup = match self.unregister_dvr_runtime(dvr_id) {
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
    object_error: Option<HalError>,
    runtime_error: Option<HalError>,
) -> Option<ChildOpenRollbackDiagnosticRecord> {
    match (object_error, runtime_error) {
        (Some(object_error), Some(runtime_error)) => Some(ChildOpenRollbackDiagnosticRecord::new(
            phase,
            ChildOpenRollbackKind::BothFailed,
            object_kind,
            object_id,
            generation,
            runtime_id,
            Some(object_error),
            Some(runtime_error),
        )),
        (Some(error), None) => Some(ChildOpenRollbackDiagnosticRecord::new(
            phase,
            ChildOpenRollbackKind::ObjectRegistrationRollbackFailed,
            object_kind,
            object_id,
            generation,
            runtime_id,
            Some(error),
            None,
        )),
        (None, Some(error)) => Some(ChildOpenRollbackDiagnosticRecord::new(
            phase,
            ChildOpenRollbackKind::RuntimeCleanupMissing,
            object_kind,
            object_id,
            generation,
            runtime_id,
            None,
            Some(error),
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

        assert_eq!(record.kind, ChildOpenRollbackKind::BothFailed);
        assert_eq!(record.object_error, Some(object_error));
        assert_eq!(record.runtime_cleanup_error, Some(runtime_error));
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
