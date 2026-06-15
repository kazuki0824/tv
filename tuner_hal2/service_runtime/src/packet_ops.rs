use crate::boot::TunerServiceRuntime;
use crate::registry::DemuxRuntimeId;
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_demux::packet_pipeline::{PipelineBoundaryReason, PipelineReport};
use maleicacid_tuner_hal2_demux::runtime::GenerationBoundaryReport;

impl TunerServiceRuntime {
    pub fn set_demux_frontend_data_source(
        &mut self,
        demux_id: i32,
        frontend_id: i32,
    ) -> Result<GenerationBoundaryReport, HalError> {
        self.packet_txn().set_demux_frontend_data_source(demux_id, frontend_id)
    }

    pub fn reset_bound_demuxes_for_frontend_tune_start(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.packet_txn().reset_bound_demuxes_for_frontend_tune_start(frontend_id)
    }

    pub fn reset_and_unbind_bound_demuxes_for_frontend(
        &mut self,
        frontend_id: i32,
        reason: PipelineBoundaryReason,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.packet_txn().reset_and_unbind_bound_demuxes_for_frontend(frontend_id, reason)
    }

    pub fn quarantine_frontend_and_bound_demuxes(
        &mut self,
        frontend_id: i32,
        error: HalError,
    ) -> Result<Vec<DemuxRuntimeId>, HalError> {
        self.packet_txn().quarantine_frontend_and_bound_demuxes(frontend_id, error)
    }

    pub fn push_frontend_ts_packet_to_bound_demuxes(
        &mut self,
        frontend_id: i32,
        packet: &[u8],
    ) -> Result<Vec<PipelineReport>, HalError> {
        self.packet_txn().push_frontend_ts_packet_to_bound_demuxes(frontend_id, packet)
    }
}
