use crate::boot::TunerServiceRuntime;
use crate::object_method_use_case::ObjectMethodExecutionToken;
use crate::registry::{
    DemuxRegistryEntry, DemuxRuntimeId, DvrRegistryEntry, FilterRegistryEntry, FrontendRuntimeId,
    RegistryCommitError,
};
use maleicacid_tuner_hal2_device::FrontendRuntimeState;
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalInternalKind, HalInvalidStateKind,
};

use crate::queue_cleanup_use_case::QueueCleanupUseCase;
use maleicacid_tuner_hal2_demux::{
    DemuxStreamBoundaryRequest, DemuxRuntime, DemuxRuntimeError, DemuxStreamGeneration,
    DvrFilterLinkRequest, StreamBoundaryReport, PlaybackConsumeReport, PipelineBoundaryReason,
    PipelineResetReport,
};
use maleicacid_tuner_hal2_demux::{FilterConfig, FilterOpenType, OpenFilterRequest};
use maleicacid_tuner_hal2_domain_request::{
    DvrConfigureRequest, FilterAvStreamTypeRequest, FilterDelayHintRequest, OpenDvrRequest,
};

pub(crate) struct DemuxFrontendSourceTxn {
    demux_id: DemuxRuntimeId,
    mutation: DemuxFrontendSourceMutation,
}

enum DemuxFrontendSourceMutation {
    Bind(FrontendRuntimeId),
    Unbind {
        expected_frontend_id: FrontendRuntimeId,
        reason: PipelineBoundaryReason,
    },
}

impl DemuxFrontendSourceTxn {
    pub(crate) const fn new(demux_id: i32, frontend_id: i32) -> Self {
        Self {
            demux_id: DemuxRuntimeId(demux_id),
            mutation: DemuxFrontendSourceMutation::Bind(FrontendRuntimeId(frontend_id)),
        }
    }

    pub(crate) const fn unbind(
        demux_id: i32,
        expected_frontend_id: i32,
        reason: PipelineBoundaryReason,
    ) -> Self {
        Self {
            demux_id: DemuxRuntimeId(demux_id),
            mutation: DemuxFrontendSourceMutation::Unbind {
                expected_frontend_id: FrontendRuntimeId(expected_frontend_id),
                reason,
            },
        }
    }

    pub(crate) fn execute(
        self,
        runtime: &mut TunerServiceRuntime,
    ) -> Result<StreamBoundaryReport, HalError> {
        let (next_frontend_id, reason) = match self.mutation {
            DemuxFrontendSourceMutation::Bind(next_frontend_id) => {
                let Some(frontend_runtime) = runtime.registry.frontend_runtime(next_frontend_id)
                else {
                    return Err(HalError::Unsupported(
                        "frontend id is not available for demux source binding",
                    ));
                };
                match frontend_runtime.snapshot().state {
                    FrontendRuntimeState::Closing | FrontendRuntimeState::Failed => {
                        return Err(HalError::invalid_state(
                            HalInvalidStateKind::InvalidLifecycle,
                            "frontend runtime is closing or failed",
                        ));
                    }
                    FrontendRuntimeState::Idle
                    | FrontendRuntimeState::Tuning { .. }
                    | FrontendRuntimeState::Scanning { .. } => {}
                }
                if runtime.registry.frontend_bound_to_demux(self.demux_id)
                    == Some(next_frontend_id)
                {
                    let generation = runtime
                        .registry
                        .demux_runtime(self.demux_id)
                        .map(|demux| demux.generation())
                        .ok_or_else(|| {
                            HalError::invalid_state(
                                HalInvalidStateKind::InvalidLifecycle,
                                "demux runtime is missing",
                            )
                        })?;
                    return Ok(StreamBoundaryReport {
                        reason: PipelineBoundaryReason::TuneStart,
                        reset: PipelineResetReport::default(),
                        next_generation: DemuxStreamGeneration(generation),
                    });
                }
                (Some(next_frontend_id), PipelineBoundaryReason::TuneStart)
            }
            DemuxFrontendSourceMutation::Unbind {
                expected_frontend_id,
                reason,
            } => {
                if runtime.registry.frontend_bound_to_demux(self.demux_id)
                    != Some(expected_frontend_id)
                {
                    return Err(HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "demux/frontend relation changed before unbind",
                    ));
                }
                (None, reason)
            }
        };

        let prepared = runtime
            .registry
            .demux_runtime_mut(self.demux_id)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "demux runtime is missing",
                )
            })?
            .prepare_stream_boundary_from_typed_request(
                DemuxStreamBoundaryRequest::new(reason),
            )
            .map_err(super::demux_runtime_error_to_hal)?;
        let report = runtime
            .registry
            .demux_runtime_mut(self.demux_id)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "demux runtime disappeared after stream-boundary prepare",
                )
            })?
            .commit_stream_boundary_from_typed_request(prepared)
            .map_err(super::demux_runtime_error_to_hal)?;
        match next_frontend_id {
            Some(frontend_id) => runtime
                .registry
                .bind_demux_frontend(self.demux_id, frontend_id),
            None => runtime.registry.unbind_demux_frontend(self.demux_id),
        }
        Ok(report)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordDvrFilterRelationMutation {
    Attach,
    Detach,
}

pub(crate) struct RecordDvrFilterRelationTxn {
    dvr_id: i32,
    filter_id: i32,
    mutation: RecordDvrFilterRelationMutation,
}

impl RecordDvrFilterRelationTxn {
    pub(crate) const fn attach(dvr_id: i32, filter_id: i32) -> Self {
        Self {
            dvr_id,
            filter_id,
            mutation: RecordDvrFilterRelationMutation::Attach,
        }
    }

    pub(crate) const fn detach(dvr_id: i32, filter_id: i32) -> Self {
        Self {
            dvr_id,
            filter_id,
            mutation: RecordDvrFilterRelationMutation::Detach,
        }
    }

    pub(crate) fn execute(
        self,
        demux: &mut DemuxRuntime,
    ) -> Result<(), DemuxRuntimeError> {
        let request = DvrFilterLinkRequest::new(self.dvr_id, self.filter_id);
        let prepared = match self.mutation {
            RecordDvrFilterRelationMutation::Attach => {
                demux.prepare_attach_dvr_filter_from_typed_request(request)?
            }
            RecordDvrFilterRelationMutation::Detach => {
                demux.prepare_detach_dvr_filter_from_typed_request(request)?
            }
        };
        demux.commit_prepared_dvr_filter_relation(prepared)
    }
}

impl TunerServiceRuntime {
    pub(crate) fn allocate_demux_runtime(
        &mut self,
    ) -> Result<DemuxRegistryEntry, RegistryCommitError> {
        self.transact_allocate_demux_runtime()
    }

    pub(crate) fn allocate_demux_runtime_for_public_id(
        &mut self,
        id: i32,
    ) -> Result<DemuxRegistryEntry, RegistryCommitError> {
        self.transact_allocate_demux_runtime_for_public_id(id)
    }

    pub(crate) fn unregister_demux_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<DemuxRegistryEntry>, HalError> {
        self.transact_unregister_demux_runtime(id)
    }

    #[cfg(test)]
    pub(crate) fn allocate_filter_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<FilterRegistryEntry, RegistryCommitError> {
        self.transact_allocate_filter_runtime(owner_demux_id)
    }

    pub(crate) fn unregister_filter_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<FilterRegistryEntry>, HalError> {
        self.transact_unregister_filter_runtime(id)
    }

    #[cfg(test)]
    pub(crate) fn register_demux_filter_runtime(
        &mut self,
        owner_demux_id: i32,
        filter_id: i32,
        request: &OpenFilterRequest,
    ) -> Result<(), HalError> {
        self.reserve_filter_capacity_for_test(
            filter_id,
            request.open_type,
            request.buffer_size,
        )?;
        if let Err(error) =
            self.transact_register_demux_filter_runtime(owner_demux_id, filter_id, request)
        {
            return match self.release_filter_capacity_for_test(filter_id) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(compose_primary_cleanup_failure(
                    "test filter registration capacity rollback failed",
                    error,
                    cleanup,
                )),
            };
        }
        Ok(())
    }

    pub(crate) fn configure_filter_runtime_request(
        &mut self,
        filter_id: i32,
        config: FilterConfig,
    ) -> Result<(), HalError> {
        self.transact_configure_filter_runtime_request(filter_id, config)
    }

    #[cfg(test)]
    pub(crate) fn start_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.transact_start_filter_runtime(filter_id)
    }

    pub(crate) fn configure_filter_av_stream_type_request(
        &mut self,
        filter_id: i32,
        request: FilterAvStreamTypeRequest,
    ) -> Result<(), HalError> {
        self.transact_configure_filter_av_stream_type_request(filter_id, request)
    }

    pub(crate) fn set_filter_delay_hint_request(
        &mut self,
        filter_id: i32,
        request: FilterDelayHintRequest,
    ) -> Result<(), HalError> {
        self.transact_set_filter_delay_hint_request(filter_id, request)
    }

    pub(crate) fn set_filter_data_source_non_null(
        &mut self,
        demux_id: i32,
        sink_filter_id: i32,
        source_filter_id: i32,
    ) -> Result<PipelineResetReport, HalError> {
        self.transact_set_filter_data_source_non_null(demux_id, sink_filter_id, source_filter_id)
    }

    pub(crate) fn disconnect_filter_data_source(
        &mut self,
        demux_id: i32,
        sink_filter_id: i32,
    ) -> Result<(), HalError> {
        self.transact_disconnect_filter_data_source(demux_id, sink_filter_id)
    }

    #[cfg(test)]
    pub(crate) fn allocate_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<DvrRegistryEntry, RegistryCommitError> {
        self.transact_allocate_dvr_runtime(owner_demux_id)
    }

    pub(crate) fn unregister_dvr_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<DvrRegistryEntry>, HalError> {
        self.transact_unregister_dvr_runtime(id)
    }

    #[cfg(test)]
    pub(crate) fn register_demux_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
        dvr_id: i32,
        request: &OpenDvrRequest,
        callback_present: bool,
    ) -> Result<(), HalError> {
        self.reserve_dvr_capacity_for_test(dvr_id, request.buffer_size)?;
        if let Err(error) = self.transact_register_demux_dvr_runtime(
            owner_demux_id,
            dvr_id,
            request,
            callback_present,
        ) {
            return match self.release_dvr_capacity_for_test(dvr_id) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(compose_primary_cleanup_failure(
                    "test DVR registration capacity rollback failed",
                    error,
                    cleanup,
                )),
            };
        }
        Ok(())
    }

    pub(crate) fn configure_dvr_runtime_request(
        &mut self,
        dvr_id: i32,
        request: DvrConfigureRequest,
    ) -> Result<(), HalError> {
        self.transact_configure_dvr_runtime_request(dvr_id, request)
    }

    pub(crate) fn attach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), HalError> {
        self.transact_attach_dvr_filter(dvr_id, filter_id)
    }

    pub(crate) fn detach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), HalError> {
        self.transact_detach_dvr_filter(dvr_id, filter_id)
    }
}

/// Call-local owner for child object allocation, registration, and rollback.
pub struct ChildOpenTxn<'a> {
    pub(crate) runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub fn child_open_txn(&mut self) -> ChildOpenTxn<'_> {
        ChildOpenTxn { runtime: self }
    }
}

impl TunerServiceRuntime {
    pub fn set_demux_frontend_data_source_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        frontend_id: i32,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;

        let demux_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        self.set_demux_frontend_data_source(demux_id, frontend_id)
            .map(|_| ())
    }

    pub fn configure_filter_runtime_for_object_with_current_open_type<F>(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
        build_config: F,
    ) -> Result<(), HalError>
    where
        F: FnOnce(FilterOpenType) -> Result<FilterConfig, HalError>,
    {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        let open_type = self.filter_open_type(filter_id).ok_or_else(|| {
            HalError::invalid_state(
                maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                "filter runtime is missing",
            )
        })?;
        let config = build_config(open_type)?;
        self.configure_filter_runtime_request(filter_id, config)
    }

    pub fn configure_filter_av_stream_type_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: FilterAvStreamTypeRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        self.configure_filter_av_stream_type_request(filter_id, request)
    }

    pub fn plan_filter_runtime_noop_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        Ok(())
    }

    pub fn start_filter_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        self.transact_start_filter_runtime(filter_id)
    }

    pub fn stop_filter_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        self.transact_stop_filter_runtime(filter_id)
    }

    pub fn flush_filter_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        QueueCleanupUseCase::filter(self, filter_id).execute()
    }

    pub fn export_filter_av_shared_handle_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<maleicacid_tuner_hal2_demux::AvSharedHandleExport, HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        self.transact_export_filter_av_shared_handle(filter_id)
    }

    pub fn release_filter_av_handle_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        descriptor: maleicacid_tuner_hal2_demux::AvHandleReleaseDescriptor,
        av_data_id: i64,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        self.transact_release_filter_av_handle(filter_id, descriptor, av_data_id)
    }

    pub fn disconnect_filter_data_source_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let sink_entry = self.public_entry_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        let (demux_object_id, demux_generation) = match sink_entry.owner() {
            crate::RuntimeOwnerRelation::Demux { demux, generation } => (demux, generation),
            _ => {
                return Err(HalError::invalid_state(
                    maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                    "filter owner demux is not live",
                ))
            }
        };
        let demux_id = self.public_runtime_id_for_object_method(
            demux_object_id,
            demux_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        let sink_filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        self.disconnect_filter_data_source(demux_id, sink_filter_id)
    }

    pub fn configure_dvr_runtime_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: DvrConfigureRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;

        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        self.configure_dvr_runtime_request(dvr_id, request)
    }

    pub fn start_dvr_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;

        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        self.transact_start_dvr_runtime(dvr_id)
    }

    pub fn consume_playback_dvr_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
    ) -> Result<PlaybackConsumeReport, HalError> {
        let entry = self.public_entry_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        let (demux_object_id, demux_generation) = match entry.owner() {
            crate::RuntimeOwnerRelation::Demux { demux, generation } => (demux, generation),
            _ => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "DVR owner demux is not live",
                ))
            }
        };
        let demux_id = self.public_runtime_id_for_object_method(
            demux_object_id,
            demux_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        let mut consume_txn = self
            .playback_consume_txns
            .remove(&dvr_id)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "playback consume transaction is not configured",
                )
            })?;
        let result = match self.registry.demux_runtime_mut(DemuxRuntimeId(demux_id)) {
            Some(demux) => match consume_txn.consume(demux) {
                Ok(report) => Ok(report),
                Err(_) => {
                    let dropped_bytes = consume_txn.discard_for_boundary();
                    if dropped_bytes > 0 {
                        let _ = demux
                            .note_playback_consume_boundary_discard(dvr_id, dropped_bytes);
                        eprintln!(
                            "maleicacid-tuner-hal2-dvr-playback-diagnostic: dvr_id={} boundary=fatal dropped_bytes={}",
                            dvr_id, dropped_bytes,
                        );
                    }
                    Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "playback DVR consume failed",
                    ))
                }
            },
            None => Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "owner demux runtime is missing for playback DVR consume",
            )),
        };
        self.playback_consume_txns.insert(dvr_id, consume_txn);
        result
    }

    pub fn attach_dvr_filter_for_object(
        &mut self,
        dvr_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        dvr_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        filter_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        filter_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            dvr_object_id,
            dvr_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;

        let dvr_entry = self.public_entry_for_object_method(
            dvr_object_id,
            dvr_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        let filter_entry = self.public_entry_for_object_method(
            filter_object_id,
            filter_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        let (dvr_demux_object_id, dvr_demux_generation) = match dvr_entry.owner() {
            crate::RuntimeOwnerRelation::Demux { demux, generation } => (demux, generation),
            _ => {
                return Err(HalError::invalid_state(
                    maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                    "DVR owner demux is not live",
                ))
            }
        };
        self.public_runtime_id_for_object_method(
            dvr_demux_object_id,
            dvr_demux_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        let (filter_demux_object_id, filter_demux_generation) = match filter_entry.owner() {
            crate::RuntimeOwnerRelation::Demux { demux, generation } => (demux, generation),
            _ => {
                return Err(HalError::invalid_state(
                    maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                    "filter owner demux is not live",
                ))
            }
        };
        self.public_runtime_id_for_object_method(
            filter_demux_object_id,
            filter_demux_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        if dvr_demux_object_id != filter_demux_object_id
            || dvr_demux_generation != filter_demux_generation
        {
            return Err(HalError::invalid_argument(
                maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
                "filter owner demux does not match DVR owner demux",
            ));
        }
        self.attach_dvr_filter(dvr_entry.public_id(), filter_entry.public_id())
    }

    pub fn detach_dvr_filter_for_object(
        &mut self,
        dvr_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        dvr_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        filter_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        filter_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            dvr_object_id,
            dvr_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;

        let dvr_entry = self.public_entry_for_object_method(
            dvr_object_id,
            dvr_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        let filter_entry = self.public_entry_for_object_method(
            filter_object_id,
            filter_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        let (dvr_demux_object_id, dvr_demux_generation) = match dvr_entry.owner() {
            crate::RuntimeOwnerRelation::Demux { demux, generation } => (demux, generation),
            _ => {
                return Err(HalError::invalid_state(
                    maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                    "DVR owner demux is not live",
                ))
            }
        };
        self.public_runtime_id_for_object_method(
            dvr_demux_object_id,
            dvr_demux_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        let (filter_demux_object_id, filter_demux_generation) = match filter_entry.owner() {
            crate::RuntimeOwnerRelation::Demux { demux, generation } => (demux, generation),
            _ => {
                return Err(HalError::invalid_state(
                    maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                    "filter owner demux is not live",
                ))
            }
        };
        self.public_runtime_id_for_object_method(
            filter_demux_object_id,
            filter_demux_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        if dvr_demux_object_id != filter_demux_object_id
            || dvr_demux_generation != filter_demux_generation
        {
            return Err(HalError::invalid_argument(
                maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
                "filter owner demux does not match DVR owner demux",
            ));
        }
        self.detach_dvr_filter(dvr_entry.public_id(), filter_entry.public_id())
    }

    pub fn stop_dvr_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;

        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        self.transact_stop_dvr_runtime(dvr_id)
    }

    pub fn flush_dvr_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;

        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        QueueCleanupUseCase::dvr(self, dvr_id).execute()
    }

    pub fn set_dvr_status_check_interval_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        interval_ms: u64,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;

        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        self.transact_set_dvr_status_check_interval(dvr_id, interval_ms)
    }

    pub(crate) fn mark_dvr_callback_unhealthy_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
    ) -> Result<(), HalError> {
        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        self.transact_mark_dvr_callback_unhealthy(dvr_id)
    }

    pub(crate) fn mark_filter_callback_unhealthy_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
    ) -> Result<(), HalError> {
        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        self.transact_mark_filter_callback_unhealthy(filter_id)
    }

    pub fn set_filter_delay_hint_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: FilterDelayHintRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        self.set_filter_delay_hint_request(filter_id, request)
    }
}

impl TunerServiceRuntime {
    pub fn set_filter_data_source_for_object(
        &mut self,
        sink_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        sink_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        source_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        source_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<PipelineResetReport, HalError> {
        dispatch.consume_for_object(
            self,
            sink_object_id,
            sink_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;

        let sink_entry = self.public_entry_for_object_method(
            sink_object_id,
            sink_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        let source_entry = self.public_entry_for_object_method(
            source_object_id,
            source_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        let (sink_demux_object_id, sink_demux_generation) = match sink_entry.owner() {
            crate::RuntimeOwnerRelation::Demux { demux, generation } => (demux, generation),
            _ => {
                return Err(HalError::invalid_state(
                    maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                    "sink filter owner demux is not live",
                ))
            }
        };
        let demux_id = self.public_runtime_id_for_object_method(
            sink_demux_object_id,
            sink_demux_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        let (source_demux_object_id, source_demux_generation) = match source_entry.owner() {
            crate::RuntimeOwnerRelation::Demux { demux, generation } => (demux, generation),
            _ => {
                return Err(HalError::invalid_state(
                    maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                    "source filter owner demux is not live",
                ))
            }
        };
        self.public_runtime_id_for_object_method(
            source_demux_object_id,
            source_demux_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        if sink_demux_object_id != source_demux_object_id
            || sink_demux_generation != source_demux_generation
        {
            return Err(HalError::invalid_argument(
                maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
                "source filter owner demux does not match sink filter owner demux",
            ));
        }
        if sink_object_id == source_object_id && sink_generation == source_generation {
            return Err(HalError::invalid_argument(
                maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
                "filter cannot use itself as source",
            ));
        }
        self.set_filter_data_source_non_null(
            demux_id,
            sink_entry.public_id(),
            source_entry.public_id(),
        )
    }
}
