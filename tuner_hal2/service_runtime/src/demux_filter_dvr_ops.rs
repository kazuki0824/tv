use crate::boot::TunerServiceRuntime;
use crate::registry::{
    DemuxRegistryEntry, DvrRegistryEntry, FilterRegistryEntry, RegistryCommitError,
};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_demux::packet_pipeline::PipelineResetReport;
use maleicacid_tuner_hal2_demux::{FilterConfig, OpenFilterRequest};
use maleicacid_tuner_hal2_domain_request::{
    FilterAvStreamTypeRequest, FilterDelayHintRequest, OpenDvrRequest,
};

impl TunerServiceRuntime {
    pub fn allocate_demux_runtime(&mut self) -> Result<DemuxRegistryEntry, RegistryCommitError> {
        self.demux_filter_dvr_txn().allocate_demux_runtime()
    }

    pub fn unregister_demux_runtime(&mut self, id: i32) -> Option<DemuxRegistryEntry> {
        self.demux_filter_dvr_txn().unregister_demux_runtime(id)
    }

    pub fn allocate_filter_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<FilterRegistryEntry, RegistryCommitError> {
        self.demux_filter_dvr_txn().allocate_filter_runtime(owner_demux_id)
    }

    pub fn unregister_filter_runtime(&mut self, id: i32) -> Option<FilterRegistryEntry> {
        self.demux_filter_dvr_txn().unregister_filter_runtime(id)
    }

    pub fn register_demux_filter_runtime(
        &mut self,
        owner_demux_id: i32,
        filter_id: i32,
        request: &OpenFilterRequest,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().register_demux_filter_runtime(owner_demux_id, filter_id, request)
    }

    pub fn configure_filter_runtime_request(
        &mut self,
        filter_id: i32,
        config: FilterConfig,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().configure_filter_runtime_request(filter_id, config)
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
        self.demux_filter_dvr_txn().configure_filter_av_stream_type_request(filter_id, request)
    }

    pub fn set_filter_delay_hint_request(
        &mut self,
        filter_id: i32,
        request: FilterDelayHintRequest,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().set_filter_delay_hint_request(filter_id, request)
    }

    pub fn set_filter_data_source_non_null(
        &mut self,
        demux_id: i32,
        sink_filter_id: i32,
        source_filter_id: i32,
    ) -> Result<PipelineResetReport, HalError> {
        self.demux_filter_dvr_txn().set_filter_data_source_non_null(demux_id, sink_filter_id, source_filter_id)
    }

    pub fn allocate_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<DvrRegistryEntry, RegistryCommitError> {
        self.demux_filter_dvr_txn().allocate_dvr_runtime(owner_demux_id)
    }

    pub fn unregister_dvr_runtime(&mut self, id: i32) -> Option<DvrRegistryEntry> {
        self.demux_filter_dvr_txn().unregister_dvr_runtime(id)
    }

    pub fn register_demux_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
        dvr_id: i32,
        request: &OpenDvrRequest,
        callback_present: bool,
    ) -> Result<(), HalError> {
        self.demux_filter_dvr_txn().register_demux_dvr_runtime(owner_demux_id, dvr_id, request, callback_present)
    }
}
