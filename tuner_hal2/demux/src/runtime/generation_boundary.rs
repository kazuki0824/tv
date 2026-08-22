use super::filter_producer_drain_gate::FilterDrainTxn;
use super::pcr_clock_anchor::PreparedPcrInvalidation;
use crate::packet_pipeline::{PacketPipeline, PipelineBoundaryReason, PipelineResetReport};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DemuxStreamGeneration(pub u64);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StreamBoundaryReport {
    pub reason: PipelineBoundaryReason,
    pub reset: PipelineResetReport,
    pub next_generation: DemuxStreamGeneration,
}

#[derive(Debug)]
pub(crate) struct StreamBoundaryTxn {
    generation: u64,
}

#[must_use = "a prepared filter-source boundary must be consumed or explicitly abandoned"]
#[derive(Debug)]
pub(super) struct PreparedFilterSourceBoundary {
    filter_id: i32,
    expected_generation: u64,
}

#[must_use = "a prepared stream boundary must be committed or explicitly abandoned"]
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

impl StreamBoundaryTxn {
    pub(crate) const fn new(generation: u64) -> Self {
        Self { generation }
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn prepare_next_generation(&self) -> Option<DemuxStreamGeneration> {
        self.generation
            .checked_add(1)
            .map(DemuxStreamGeneration)
    }

    pub(super) fn prepare_filter_source_boundary(
        &self,
        filter_id: i32,
    ) -> PreparedFilterSourceBoundary {
        PreparedFilterSourceBoundary {
            filter_id,
            expected_generation: self.generation,
        }
    }

    pub(super) fn consume_filter_source_boundary(
        &self,
        prepared: PreparedFilterSourceBoundary,
    ) -> Option<i32> {
        (self.generation == prepared.expected_generation).then_some(prepared.filter_id)
    }

    pub(crate) fn commit_prepared_generation(
        &mut self,
        expected_generation: u64,
        next_generation: DemuxStreamGeneration,
    ) -> bool {
        if self.generation != expected_generation
            || expected_generation.checked_add(1) != Some(next_generation.0)
        {
            return false;
        }
        self.generation = next_generation.0;
        true
    }

    pub(crate) fn restore(&mut self, generation: u64) {
        self.generation = generation;
    }
}

#[cfg(test)]
mod tests {
    use super::super::demux::DemuxRuntime;
    use super::*;

    #[test]
    fn prepared_boundary_abort_preserves_generation() {
        let mut demux = DemuxRuntime::new(7, 11);
        let prepared = demux
            .prepare_stream_boundary(PipelineBoundaryReason::TuneStart)
            .unwrap();
        drop(prepared);
        assert_eq!(demux.generation(), 11);
    }

    #[test]
    fn stale_or_non_successor_commit_preserves_generation() {
        let mut owner = StreamBoundaryTxn::new(11);

        assert!(!owner.commit_prepared_generation(10, DemuxStreamGeneration(11)));
        assert!(!owner.commit_prepared_generation(11, DemuxStreamGeneration(13)));
        assert_eq!(owner.generation(), 11);
    }

    #[test]
    fn generation_exhaustion_cannot_prepare_a_boundary() {
        let owner = StreamBoundaryTxn::new(u64::MAX);

        assert_eq!(owner.prepare_next_generation(), None);
    }
}
