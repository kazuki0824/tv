use crate::boot::{DvrChildRuntimeOpen, FilterChildRuntimeOpen, TunerServiceRuntime};
use crate::object_method_txn::ObjectMethodExecutionToken;
use crate::registry::{
    DemuxRegistryEntry, DvrRegistryEntry, FilterRegistryEntry, RegistryCommitError,
};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_demux::PipelineResetReport;
use maleicacid_tuner_hal2_demux::{FilterConfig, FilterOpenType, OpenFilterRequest};
use maleicacid_tuner_hal2_domain_request::{
    DvrConfigureRequest, FilterAvStreamTypeRequest, FilterDelayHintRequest, OpenDvrRequest,
};

impl TunerServiceRuntime {
    pub(crate) fn allocate_demux_runtime(
        &mut self,
    ) -> Result<DemuxRegistryEntry, RegistryCommitError> {
        self.transact_allocate_demux_runtime()
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
        self.transact_register_demux_filter_runtime(owner_demux_id, filter_id, request)
    }

    pub(crate) fn configure_filter_runtime_request(
        &mut self,
        filter_id: i32,
        config: FilterConfig,
    ) -> Result<(), HalError> {
        self.transact_configure_filter_runtime_request(filter_id, config)
    }

    pub(crate) fn start_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.transact_start_filter_runtime(filter_id)
    }

    pub(crate) fn stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.transact_stop_filter_runtime(filter_id)
    }

    pub(crate) fn flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.transact_flush_filter_runtime(filter_id)
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
        self.transact_register_demux_dvr_runtime(owner_demux_id, dvr_id, request, callback_present)
    }

    pub(crate) fn configure_dvr_runtime_request(
        &mut self,
        dvr_id: i32,
        request: DvrConfigureRequest,
    ) -> Result<(), HalError> {
        self.transact_configure_dvr_runtime_request(dvr_id, request)
    }

    pub(crate) fn start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.transact_start_dvr_runtime(dvr_id)
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

    pub(crate) fn stop_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.transact_stop_dvr_runtime(dvr_id)
    }

    pub(crate) fn flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.transact_flush_dvr_runtime(dvr_id)
    }

    pub(crate) fn set_dvr_status_check_interval(
        &mut self,
        dvr_id: i32,
        interval_ms: u64,
    ) -> Result<(), HalError> {
        self.transact_set_dvr_status_check_interval(dvr_id, interval_ms)
    }
}

impl TunerServiceRuntime {
    pub fn open_filter_child_runtime_for_demux_object(
        &mut self,
        owner_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        owner_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: &OpenFilterRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<FilterChildRuntimeOpen, HalError> {
        self.demux_filter_dvr_txn()
            .open_filter_child_runtime_for_demux_object(
                owner_object_id,
                owner_generation,
                request,
                dispatch,
            )
    }

    pub fn open_dvr_child_runtime_for_demux_object(
        &mut self,
        owner_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        owner_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: OpenDvrRequest,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<DvrChildRuntimeOpen, HalError> {
        self.demux_filter_dvr_txn()
            .open_dvr_child_runtime_for_demux_object(
                owner_object_id,
                owner_generation,
                request,
                dispatch,
            )
    }

    pub fn rollback_filter_child_open_after_aidl_failure(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        filter_id: i32,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn()
            .rollback_filter_child_open_after_aidl_failure(object_id, generation, filter_id)
    }

    pub fn rollback_dvr_child_open_after_aidl_failure(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dvr_id: i32,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn()
            .rollback_dvr_child_open_after_aidl_failure(object_id, generation, dvr_id)
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
        self.start_filter_runtime(filter_id)
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
        self.stop_filter_runtime(filter_id)
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
        self.flush_filter_runtime(filter_id)
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
        has_fd: bool,
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
        self.transact_release_filter_av_handle(filter_id, has_fd, av_data_id)
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
        self.start_dvr_runtime(dvr_id)
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
        self.stop_dvr_runtime(dvr_id)
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
        self.flush_dvr_runtime(dvr_id)
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
        self.set_dvr_status_check_interval(dvr_id, interval_ms)
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
