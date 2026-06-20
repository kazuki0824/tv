use crate::boot::TunerServiceRuntime;
use crate::method_dispatch::plan_object_method_dispatch;
use crate::object_method_txn::ObjectMethodDispatchPreflight;
use crate::registry::{
    DemuxRegistryEntry, DvrRegistryEntry, FilterRegistryEntry, RegistryCommitError,
};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_demux::packet_pipeline::PipelineResetReport;
use maleicacid_tuner_hal2_demux::{FilterConfig, FilterOpenType, OpenFilterRequest};
use maleicacid_tuner_hal2_domain_request::{
    DvrConfigureRequest, FilterAvStreamTypeRequest, FilterDelayHintRequest, OpenDvrRequest,
};

impl TunerServiceRuntime {
    pub fn allocate_demux_runtime(&mut self) -> Result<DemuxRegistryEntry, RegistryCommitError> {
        self.demux_filter_dvr_txn().allocate_demux_runtime()
    }

    pub fn unregister_demux_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<DemuxRegistryEntry>, HalError> {
        self.demux_filter_dvr_txn().unregister_demux_runtime(id)
    }

    pub fn allocate_filter_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<FilterRegistryEntry, RegistryCommitError> {
        self.demux_filter_dvr_txn()
            .allocate_filter_runtime(owner_demux_id)
    }

    pub fn unregister_filter_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<FilterRegistryEntry>, HalError> {
        self.demux_filter_dvr_txn().unregister_filter_runtime(id)
    }

    pub fn register_demux_filter_runtime(
        &mut self,
        owner_demux_id: i32,
        filter_id: i32,
        request: &OpenFilterRequest,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().register_demux_filter_runtime(
            owner_demux_id,
            filter_id,
            request,
        )
    }

    pub fn configure_filter_runtime_request(
        &mut self,
        filter_id: i32,
        config: FilterConfig,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn()
            .configure_filter_runtime_request(filter_id, config)
    }

    pub fn start_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().start_filter_runtime(filter_id)
    }

    pub fn stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().stop_filter_runtime(filter_id)
    }

    pub fn flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().flush_filter_runtime(filter_id)
    }

    pub fn configure_filter_av_stream_type_request(
        &mut self,
        filter_id: i32,
        request: FilterAvStreamTypeRequest,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn()
            .configure_filter_av_stream_type_request(filter_id, request)
    }

    pub fn set_filter_delay_hint_request(
        &mut self,
        filter_id: i32,
        request: FilterDelayHintRequest,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn()
            .set_filter_delay_hint_request(filter_id, request)
    }

    pub fn set_filter_data_source_non_null(
        &mut self,
        demux_id: i32,
        sink_filter_id: i32,
        source_filter_id: i32,
    ) -> Result<PipelineResetReport, HalError> {
        self.demux_filter_dvr_txn().set_filter_data_source_non_null(
            demux_id,
            sink_filter_id,
            source_filter_id,
        )
    }

    pub fn allocate_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<DvrRegistryEntry, RegistryCommitError> {
        self.demux_filter_dvr_txn()
            .allocate_dvr_runtime(owner_demux_id)
    }

    pub fn unregister_dvr_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<DvrRegistryEntry>, HalError> {
        self.demux_filter_dvr_txn().unregister_dvr_runtime(id)
    }

    pub fn register_demux_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
        dvr_id: i32,
        request: &OpenDvrRequest,
        callback_present: bool,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().register_demux_dvr_runtime(
            owner_demux_id,
            dvr_id,
            request,
            callback_present,
        )
    }

    pub fn configure_dvr_runtime_request(
        &mut self,
        dvr_id: i32,
        request: DvrConfigureRequest,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn()
            .configure_dvr_runtime_request(dvr_id, request)
    }

    pub fn start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().start_dvr_runtime(dvr_id)
    }

    pub fn attach_dvr_filter(&mut self, dvr_id: i32, filter_id: i32) -> Result<(), HalError> {
        self.demux_filter_dvr_txn()
            .attach_dvr_filter(dvr_id, filter_id)
    }

    pub fn detach_dvr_filter(&mut self, dvr_id: i32, filter_id: i32) -> Result<(), HalError> {
        self.demux_filter_dvr_txn()
            .detach_dvr_filter(dvr_id, filter_id)
    }

    pub fn stop_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().stop_dvr_runtime(dvr_id)
    }

    pub fn flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().flush_dvr_runtime(dvr_id)
    }
}

impl TunerServiceRuntime {
    pub fn open_filter_child_runtime_for_demux_object(
        &mut self,
        owner_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        owner_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: &OpenFilterRequest,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<crate::RuntimeObjectEntry, HalError> {
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
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<crate::RuntimeObjectEntry, HalError> {
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
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let demux_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Demux,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.set_demux_frontend_data_source(demux_id, frontend_id)
            .map(|_| ())
    }

    pub fn configure_filter_runtime_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        config: FilterConfig,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.configure_filter_runtime_request(filter_id, config)
    }

    pub fn configure_filter_runtime_for_object_with_current_open_type<F>(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
        build_config: F,
    ) -> Result<(), HalError>
    where
        F: FnOnce(FilterOpenType) -> Result<FilterConfig, HalError>,
    {
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
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.configure_filter_runtime_request(filter_id, config)
    }

    pub fn configure_filter_av_stream_type_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: FilterAvStreamTypeRequest,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        dispatch.plan(self)?;
        self.configure_filter_av_stream_type_request(filter_id, request)
    }

    pub fn plan_filter_runtime_noop_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)
    }

    pub fn start_filter_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.start_filter_runtime(filter_id)
    }

    pub fn stop_filter_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.stop_filter_runtime(filter_id)
    }

    pub fn flush_filter_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.flush_filter_runtime(filter_id)
    }

    pub fn configure_dvr_runtime_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: DvrConfigureRequest,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.configure_dvr_runtime_request(dvr_id, request)
    }

    pub fn start_dvr_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.start_dvr_runtime(dvr_id)
    }

    pub fn attach_dvr_filter_for_object(
        &mut self,
        dvr_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        dvr_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        filter_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        filter_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
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
        dispatch.plan(self)?;
        self.attach_dvr_filter(dvr_entry.public_id(), filter_entry.public_id())
    }

    pub fn detach_dvr_filter_for_object(
        &mut self,
        dvr_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        dvr_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        filter_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        filter_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
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
        dispatch.plan(self)?;
        self.detach_dvr_filter(dvr_entry.public_id(), filter_entry.public_id())
    }

    pub fn stop_dvr_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.stop_dvr_runtime(dvr_id)
    }

    pub fn flush_dvr_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let dvr_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.flush_dvr_runtime(dvr_id)
    }

    pub fn set_filter_delay_hint_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        request: FilterDelayHintRequest,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
        let filter_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        dispatch.plan(self)?;
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
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<PipelineResetReport, HalError> {
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
        let _source_demux_id = self.public_runtime_id_for_object_method(
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
        dispatch.plan(self)?;
        self.set_filter_data_source_non_null(
            demux_id,
            sink_entry.public_id(),
            source_entry.public_id(),
        )
    }
}
