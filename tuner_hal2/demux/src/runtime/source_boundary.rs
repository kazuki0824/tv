use super::demux::{DemuxRuntime, DemuxRuntimeError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBoundaryStep { ValidateEndpoint, DisconnectDownstream, ClearQueue, BumpGeneration, Commit, Quarantine }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBoundaryOutcome { Committed, Failed { step: SourceBoundaryStep }, Quarantined { step: SourceBoundaryStep } }

#[derive(Debug)]
pub struct SourceBoundaryTxn { sink_filter_id: i32, steps: Vec<SourceBoundaryStep>, outcome: Option<SourceBoundaryOutcome> }

impl SourceBoundaryTxn {
    pub fn new(sink_filter_id: i32) -> Self { Self { sink_filter_id, steps: Vec::new(), outcome: None } }
    pub fn record_step(&mut self, step: SourceBoundaryStep) { self.steps.push(step); }
    pub fn steps(&self) -> &[SourceBoundaryStep] { &self.steps }
    pub fn outcome(&self) -> Option<SourceBoundaryOutcome> { self.outcome }

    pub fn apply(mut self, demux: &mut DemuxRuntime) -> (Self, Result<SourceBoundaryOutcome, DemuxRuntimeError>) {
        self.record_step(SourceBoundaryStep::ValidateEndpoint);
        if demux.filter(self.sink_filter_id).is_none() {
            let outcome = SourceBoundaryOutcome::Failed { step: SourceBoundaryStep::ValidateEndpoint };
            self.outcome = Some(outcome);
            return (self, Err(DemuxRuntimeError::filter_missing(self.sink_filter_id)));
        }

        self.record_step(SourceBoundaryStep::DisconnectDownstream);
        if let Err(err) = demux.disconnect_filter_source(self.sink_filter_id) {
            let outcome = SourceBoundaryOutcome::Failed { step: SourceBoundaryStep::DisconnectDownstream };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }

        self.record_step(SourceBoundaryStep::ClearQueue);
        if let Err(err) = demux.clear_existing_filter_queue(self.sink_filter_id) {
            // 存在しない queue を insert(VecDeque::new()) で作って成功にしてはならない。
            let outcome = SourceBoundaryOutcome::Failed { step: SourceBoundaryStep::ClearQueue };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }

        self.record_step(SourceBoundaryStep::BumpGeneration);
        demux.reset_generation_boundary();

        self.record_step(SourceBoundaryStep::Commit);
        let outcome = SourceBoundaryOutcome::Committed;
        self.outcome = Some(outcome);
        (self, Ok(outcome))
    }
}
