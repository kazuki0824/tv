use super::demux::{DemuxRuntime, DemuxRuntimeError};
use super::filter_producer_drain_gate::FilterDrainTxn;
use super::pcr_clock_anchor::PreparedPcrInvalidation;
use crate::packet_pipeline::{PacketPipeline, PipelineBoundaryReason, PipelineResetReport};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DemuxStreamGeneration(pub u64);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GenerationBoundaryReport {
    pub reason: PipelineBoundaryReason,
    pub reset: PipelineResetReport,
    pub next_generation: DemuxStreamGeneration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GenerationBoundaryScope {
    Demux(PipelineBoundaryReason),
    FilterSource { filter_id: i32 },
}

#[derive(Debug)]
pub(crate) struct GenerationBoundaryTxn {
    scope: GenerationBoundaryScope,
}

#[derive(Debug)]
pub struct PreparedStreamBoundary {
    pub(super) reason: PipelineBoundaryReason,
    pub(super) expected_generation: u64,
    pub(super) next_generation: DemuxStreamGeneration,
    pub(super) reset: PipelineResetReport,
    pub(super) prepared_pipeline: PacketPipeline,
    pub(super) filter_queue_ids: Vec<i32>,
    pub(super) filter_drains: Vec<(i32, FilterDrainTxn)>,
    pub(super) pcr_invalidation: PreparedPcrInvalidation,
}

impl GenerationBoundaryTxn {
    pub(crate) fn for_reason(reason: PipelineBoundaryReason) -> Self {
        Self {
            scope: GenerationBoundaryScope::Demux(reason),
        }
    }

    pub(crate) fn for_filter_source(filter_id: i32) -> Self {
        Self {
            scope: GenerationBoundaryScope::FilterSource { filter_id },
        }
    }

    pub(crate) fn apply(
        self,
        demux: &mut DemuxRuntime,
    ) -> (Self, Result<GenerationBoundaryReport, DemuxRuntimeError>) {
        let GenerationBoundaryScope::Demux(reason) = self.scope else {
            return (
                self,
                Err(DemuxRuntimeError::invalid_state(demux.demux_id())),
            );
        };
        let prepared = match demux.prepare_generation_boundary(reason) {
            Ok(prepared) => prepared,
            Err(err) => return (self, Err(err)),
        };
        let report = demux.commit_generation_boundary(prepared);
        (self, report)
    }

    pub(crate) fn apply_filter_source(
        self,
        demux: &mut DemuxRuntime,
    ) -> (Self, Result<PipelineResetReport, DemuxRuntimeError>) {
        let GenerationBoundaryScope::FilterSource { filter_id } = self.scope else {
            return (
                self,
                Err(DemuxRuntimeError::invalid_state(demux.demux_id())),
            );
        };
        if let Err(error) = demux.clear_existing_filter_queue(filter_id) {
            return (self, Err(error));
        }
        let reset = demux.reset_filter_source_boundary(filter_id);
        (self, reset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_boundary_abort_preserves_generation() {
        let mut demux = DemuxRuntime::new(7, 11);
        let prepared = demux
            .prepare_generation_boundary(PipelineBoundaryReason::TuneStart)
            .unwrap();
        drop(prepared);
        assert_eq!(demux.generation(), 11);
    }
}
