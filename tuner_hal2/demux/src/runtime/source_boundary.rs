use crate::packet_pipeline::PipelineResetReport;

use super::demux::{next_generation, DemuxRuntime, DemuxRuntimeError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBoundaryStep {
    ValidateEndpoint,
    ValidateQueue,
    ValidateGeneration,
    ClearQueue,
    BumpGeneration,
    DisconnectDownstream,
    Commit,
    Quarantine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBoundaryOutcome {
    Committed,
    Failed { step: SourceBoundaryStep },
    Quarantined { step: SourceBoundaryStep },
}

#[derive(Debug)]
pub struct SourceBoundaryTxn {
    sink_filter_id: i32,
    next_source: Option<(i32, u64)>,
    steps: Vec<SourceBoundaryStep>,
    outcome: Option<SourceBoundaryOutcome>,
    reset_report: Option<PipelineResetReport>,
}

impl SourceBoundaryTxn {
    pub(crate) fn new(sink_filter_id: i32) -> Self {
        Self {
            sink_filter_id,
            next_source: None,
            steps: Vec::new(),
            outcome: None,
            reset_report: None,
        }
    }
    pub(crate) fn with_new_source(
        mut self,
        source_filter_id: i32,
        source_filter_generation: u64,
    ) -> Self {
        self.next_source = Some((source_filter_id, source_filter_generation));
        self
    }

    pub fn record_step(&mut self, step: SourceBoundaryStep) {
        self.steps.push(step);
    }
    pub fn steps(&self) -> &[SourceBoundaryStep] {
        &self.steps
    }
    pub fn outcome(&self) -> Option<SourceBoundaryOutcome> {
        self.outcome
    }
    pub fn reset_report(&self) -> Option<&PipelineResetReport> {
        self.reset_report.as_ref()
    }

    pub(crate) fn apply(
        mut self,
        demux: &mut DemuxRuntime,
    ) -> (Self, Result<SourceBoundaryOutcome, DemuxRuntimeError>) {
        self.record_step(SourceBoundaryStep::ValidateEndpoint);
        if demux.filter(self.sink_filter_id).is_none() {
            let sink_filter_id = self.sink_filter_id;
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateEndpoint,
            };
            self.outcome = Some(outcome);
            return (self, Err(DemuxRuntimeError::filter_missing(sink_filter_id)));
        }

        self.record_step(SourceBoundaryStep::ValidateQueue);
        if !demux.queue_exists(self.sink_filter_id) {
            let sink_filter_id = self.sink_filter_id;
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateQueue,
            };
            self.outcome = Some(outcome);
            return (self, Err(DemuxRuntimeError::queue_missing(sink_filter_id)));
        }

        self.record_step(SourceBoundaryStep::ValidateGeneration);
        if next_generation(demux.generation()).is_err() {
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateGeneration,
            };
            self.outcome = Some(outcome);
            return (
                self,
                Err(DemuxRuntimeError::generation_exhausted(Some(
                    demux.demux_id(),
                ))),
            );
        }

        let rollback_snapshot = demux.snapshot();

        self.record_step(SourceBoundaryStep::ClearQueue);
        if let Err(err) = demux.clear_existing_filter_queue(self.sink_filter_id) {
            // 存在しない queue を insert(VecDeque::new()) で作って成功にしてはならない。
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ClearQueue,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }

        self.record_step(SourceBoundaryStep::BumpGeneration);
        match demux.reset_generation_boundary() {
            Ok(reset_report) => self.reset_report = Some(reset_report),
            Err(err) => {
                if demux.restore(rollback_snapshot).is_err() {
                    demux.quarantine();
                    let outcome = SourceBoundaryOutcome::Quarantined {
                        step: SourceBoundaryStep::BumpGeneration,
                    };
                    self.outcome = Some(outcome);
                    return (
                        self,
                        Err(DemuxRuntimeError::source_boundary_rollback_failed(
                            self.sink_filter_id,
                        )),
                    );
                }
                let outcome = SourceBoundaryOutcome::Failed {
                    step: SourceBoundaryStep::BumpGeneration,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
        }

        self.record_step(SourceBoundaryStep::DisconnectDownstream);
        if let Err(err) = demux.disconnect_filter_source_after_boundary(self.sink_filter_id) {
            if demux.restore(rollback_snapshot).is_err() {
                demux.quarantine();
                let outcome = SourceBoundaryOutcome::Quarantined {
                    step: SourceBoundaryStep::DisconnectDownstream,
                };
                self.outcome = Some(outcome);
                return (
                    self,
                    Err(DemuxRuntimeError::source_boundary_rollback_failed(
                        self.sink_filter_id,
                    )),
                );
            }
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::DisconnectDownstream,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }

        self.record_step(SourceBoundaryStep::Commit);
        let commit_result = match self.next_source {
            Some((source_filter_id, source_filter_generation)) => demux
                .connect_filter_source_after_boundary(
                    self.sink_filter_id,
                    source_filter_id,
                    source_filter_generation,
                ),
            None => Ok(()),
        };
        if let Err(err) = commit_result {
            if demux.restore(rollback_snapshot).is_err() {
                demux.quarantine();
                let outcome = SourceBoundaryOutcome::Quarantined {
                    step: SourceBoundaryStep::Commit,
                };
                self.outcome = Some(outcome);
                return (
                    self,
                    Err(DemuxRuntimeError::source_boundary_rollback_failed(
                        self.sink_filter_id,
                    )),
                );
            }
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::Commit,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }
        let outcome = SourceBoundaryOutcome::Committed;
        self.outcome = Some(outcome);
        (self, Ok(outcome))
    }
}
