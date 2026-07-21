use super::demux::{DemuxRuntime, DemuxRuntimeError};
use crate::packet_pipeline::{PipelineBoundaryReason, PipelineResetReport};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DemuxStreamGeneration(pub u64);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GenerationBoundaryReport {
    pub reason: PipelineBoundaryReason,
    pub reset: PipelineResetReport,
    pub next_generation: DemuxStreamGeneration,
}

#[derive(Debug)]
pub(crate) struct GenerationBoundaryTxn {
    reason: PipelineBoundaryReason,
}

impl GenerationBoundaryTxn {
    pub(crate) fn for_reason(reason: PipelineBoundaryReason) -> Self {
        Self { reason }
    }

    pub(crate) fn apply(
        self,
        demux: &mut DemuxRuntime,
    ) -> (Self, Result<GenerationBoundaryReport, DemuxRuntimeError>) {
        let reason = self.reason;
        let reset = match demux.reset_generation_boundary() {
            Ok(reset) => reset,
            Err(err) => return (self, Err(err)),
        };
        let next = DemuxStreamGeneration(demux.generation());
        (
            self,
            Ok(GenerationBoundaryReport {
                reason,
                reset,
                next_generation: next,
            }),
        )
    }
}
