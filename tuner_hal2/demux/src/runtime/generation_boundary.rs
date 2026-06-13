use crate::packet_pipeline::{PipelineBoundaryReason, PipelineResetReport};
use super::demux::DemuxRuntime;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DemuxStreamGeneration(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationBoundaryStep { InvalidateAssembler, ClearContinuity, BumpGeneration, Commit }

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GenerationBoundaryReport { pub reason: PipelineBoundaryReason, pub reset: PipelineResetReport, pub next_generation: DemuxStreamGeneration }

#[derive(Debug)]
pub struct GenerationBoundaryTxn { generation: DemuxStreamGeneration, reason: PipelineBoundaryReason, steps: Vec<GenerationBoundaryStep> }

impl GenerationBoundaryTxn {
    pub fn new(generation: DemuxStreamGeneration) -> Self { Self { generation, reason: PipelineBoundaryReason::TuneStart, steps: Vec::new() } }
    pub fn for_reason(generation: DemuxStreamGeneration, reason: PipelineBoundaryReason) -> Self { Self { generation, reason, steps: Vec::new() } }
    pub fn generation(&self) -> DemuxStreamGeneration { self.generation }
    pub fn record_step(&mut self, step: GenerationBoundaryStep) { self.steps.push(step); }
    pub fn steps(&self) -> &[GenerationBoundaryStep] { &self.steps }

    pub fn apply(mut self, demux: &mut DemuxRuntime) -> (Self, GenerationBoundaryReport) {
        self.record_step(GenerationBoundaryStep::InvalidateAssembler);
        self.record_step(GenerationBoundaryStep::ClearContinuity);
        let reset = demux.reset_generation_boundary();
        self.record_step(GenerationBoundaryStep::BumpGeneration);
        let next = DemuxStreamGeneration(demux.generation());
        self.record_step(GenerationBoundaryStep::Commit);
        (self, GenerationBoundaryReport { reason: self.reason, reset, next_generation: next })
    }
}
