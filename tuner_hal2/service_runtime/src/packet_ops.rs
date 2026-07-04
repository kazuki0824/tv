use crate::boot::TunerServiceRuntime;
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_demux::GenerationBoundaryReport;
use maleicacid_tuner_hal2_demux::PipelineReport;

impl TunerServiceRuntime {
    pub(crate) fn set_demux_frontend_data_source(
        &mut self,
        demux_id: i32,
        frontend_id: i32,
    ) -> Result<GenerationBoundaryReport, HalError> {
        self.packet_txn()
            .set_demux_frontend_data_source(demux_id, frontend_id)
    }

    pub(crate) fn reset_bound_demuxes_for_frontend_tune_start(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.packet_txn()
            .reset_bound_demuxes_for_frontend_tune_start(frontend_id)
    }

    pub(crate) fn push_frontend_ts_packet_to_bound_demuxes(
        &mut self,
        frontend_id: i32,
        packet: &[u8; maleicacid_tuner_hal2_common::TS_PACKET_SIZE],
    ) -> Result<Vec<PipelineReport>, HalError> {
        self.packet_txn()
            .push_frontend_ts_packet_to_bound_demuxes(frontend_id, packet)
    }
}
