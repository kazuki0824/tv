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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBoundaryOutcome {
    Committed,
    Failed { step: SourceBoundaryStep },
    Quarantined { step: SourceBoundaryStep },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBoundaryReport {
    steps: Vec<SourceBoundaryStep>,
    outcome: SourceBoundaryOutcome,
    reset_report: Option<PipelineResetReport>,
}

impl SourceBoundaryReport {
    #[cfg(test)]
    pub fn steps(&self) -> &[SourceBoundaryStep] {
        &self.steps
    }
    #[cfg(test)]
    pub const fn outcome(&self) -> SourceBoundaryOutcome {
        self.outcome
    }
    pub fn reset_report(&self) -> Option<&PipelineResetReport> {
        self.reset_report.as_ref()
    }
}

#[derive(Debug)]
struct SourceBoundaryTxn {
    sink_filter_id: i32,
    next_source: Option<(i32, u64)>,
    steps: Vec<SourceBoundaryStep>,
    outcome: Option<SourceBoundaryOutcome>,
    reset_report: Option<PipelineResetReport>,
}

impl SourceBoundaryTxn {
    fn new(sink_filter_id: i32) -> Self {
        Self {
            sink_filter_id,
            next_source: None,
            steps: Vec::new(),
            outcome: None,
            reset_report: None,
        }
    }
    fn with_new_source(mut self, source_filter_id: i32, source_filter_generation: u64) -> Self {
        self.next_source = Some((source_filter_id, source_filter_generation));
        self
    }

    fn record_step(&mut self, step: SourceBoundaryStep) {
        self.steps.push(step);
    }
    fn report(&self) -> SourceBoundaryReport {
        SourceBoundaryReport {
            steps: self.steps.clone(),
            outcome: self.outcome.unwrap_or(SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateEndpoint,
            }),
            reset_report: self.reset_report.clone(),
        }
    }

    fn apply(
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
                    let sink_filter_id = self.sink_filter_id;
                    return (
                        self,
                        Err(DemuxRuntimeError::source_boundary_rollback_failed(
                            sink_filter_id,
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
                let sink_filter_id = self.sink_filter_id;
                return (
                    self,
                    Err(DemuxRuntimeError::source_boundary_rollback_failed(
                        sink_filter_id,
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
                let sink_filter_id = self.sink_filter_id;
                return (
                    self,
                    Err(DemuxRuntimeError::source_boundary_rollback_failed(
                        sink_filter_id,
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

pub(crate) fn apply_filter_source_boundary_change(
    demux: &mut DemuxRuntime,
    sink_filter_id: i32,
    next_source: Option<(i32, u64)>,
) -> (
    SourceBoundaryReport,
    Result<SourceBoundaryOutcome, DemuxRuntimeError>,
) {
    let txn = SourceBoundaryTxn::new(sink_filter_id);
    let txn = match next_source {
        Some((source_filter_id, source_filter_generation)) => {
            txn.with_new_source(source_filter_id, source_filter_generation)
        }
        None => txn,
    };
    let (txn, result) = txn.apply(demux);
    (txn.report(), result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_pipeline::{FilterPipelineConfig, PipelineOpenKind};
    use crate::{FilterOpenType, OpenFilterRequest};

    fn configured_filter(
        filter_id: i32,
        generation: u64,
        kind: PipelineOpenKind,
    ) -> super::super::filter::FilterRuntime {
        let open_type = match kind {
            PipelineOpenKind::Raw => FilterOpenType::TsRaw,
            PipelineOpenKind::Av => FilterOpenType::TsVideo,
            PipelineOpenKind::Section => FilterOpenType::TsSection,
            PipelineOpenKind::Pes => FilterOpenType::TsPes,
            PipelineOpenKind::Record => FilterOpenType::TsRecord,
            PipelineOpenKind::Other => FilterOpenType::TsRaw,
        };
        super::super::demux::DemuxRuntime::open_filter_runtime_from_request(
            filter_id,
            generation,
            &OpenFilterRequest {
                open_type,
                buffer_size: 4096,
                callback_present: false,
            },
            Some(FilterPipelineConfig {
                tpid: Some(0x0100),
                raw: false,
                record_index: None,
            }),
        )
    }

    #[test]
    fn source_boundary_connects_source_filter_and_records_reset() {
        let mut demux = DemuxRuntime::new(1, 1);
        demux
            .register_filter(configured_filter(10, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux
            .register_filter(configured_filter(20, 1, PipelineOpenKind::Section))
            .unwrap();

        demux.set_filter_source_non_null(20, 10).unwrap();

        assert_eq!(
            demux.filter(20).unwrap().pipeline_view().source_filter,
            Some((10, 1))
        );
    }

    #[test]
    fn source_boundary_disconnects_to_demux_input() {
        let mut demux = DemuxRuntime::new(2, 1);
        demux
            .register_filter(configured_filter(10, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux
            .register_filter(configured_filter(20, 1, PipelineOpenKind::Section))
            .unwrap();
        demux.set_filter_source_non_null(20, 10).unwrap();
        demux.create_filter_queue(20).unwrap();

        demux.disconnect_filter_source(20).unwrap();

        assert_eq!(
            demux.filter(20).unwrap().pipeline_view().source_filter,
            None
        );
    }

    #[test]
    fn source_boundary_failure_keeps_existing_source() {
        let mut demux = DemuxRuntime::new(3, 1);
        demux
            .register_filter(configured_filter(10, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux
            .register_filter(configured_filter(20, 1, PipelineOpenKind::Section))
            .unwrap();
        demux.set_filter_source_non_null(20, 10).unwrap();

        let result = demux.set_filter_source_non_null(20, 99);

        assert!(result.is_err());
        assert_eq!(
            demux.filter(20).unwrap().pipeline_view().source_filter,
            Some((10, 1))
        );
    }
}
