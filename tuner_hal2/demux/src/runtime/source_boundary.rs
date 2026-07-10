use crate::packet_pipeline::{PipelineOpenKind, PipelineResetReport};

use super::demux::{next_generation, DemuxRuntime, DemuxRuntimeError, DemuxRuntimeErrorKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBoundaryStep {
    ValidateEndpoint,
    ValidateSink,
    ValidateSource,
    ValidateSinkLifecycle,
    ValidateSourceLifecycle,
    ValidateSourceSubtype,
    ValidateSinkSubtype,
    ValidatePid,
    ValidateQueue,
    ValidateGeneration,
    ClearQueue,
    BumpGeneration,
    DisconnectDownstream,
    Commit,
    RestoreSnapshot,
    Quarantine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBoundaryOutcome {
    Committed,
    Failed {
        step: SourceBoundaryStep,
        primary_error: DemuxRuntimeErrorKind,
    },
    RolledBack {
        primary_step: SourceBoundaryStep,
        primary_error: DemuxRuntimeErrorKind,
        rollback_step: SourceBoundaryStep,
    },
    Quarantined {
        primary_step: SourceBoundaryStep,
        primary_error: DemuxRuntimeErrorKind,
        rollback_step: SourceBoundaryStep,
        rollback_error: DemuxRuntimeErrorKind,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBoundaryReport {
    sink_filter_id: i32,
    source_filter_id: Option<i32>,
    steps: Vec<SourceBoundaryStep>,
    outcome: SourceBoundaryOutcome,
    reset_report: Option<PipelineResetReport>,
}

impl SourceBoundaryReport {
    pub const fn sink_filter_id(&self) -> i32 {
        self.sink_filter_id
    }

    pub const fn source_filter_id(&self) -> Option<i32> {
        self.source_filter_id
    }

    pub fn steps(&self) -> &[SourceBoundaryStep] {
        &self.steps
    }

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
    next_source: Option<i32>,
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
    fn with_new_source(mut self, source_filter_id: i32) -> Self {
        self.next_source = Some(source_filter_id);
        self
    }

    fn record_step(&mut self, step: SourceBoundaryStep) {
        self.steps.push(step);
    }
    fn report(&self) -> SourceBoundaryReport {
        SourceBoundaryReport {
            sink_filter_id: self.sink_filter_id,
            source_filter_id: self.next_source,
            steps: self.steps.clone(),
            outcome: self.outcome.unwrap_or(SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateEndpoint,
                primary_error: DemuxRuntimeErrorKind::InvalidState,
            }),
            reset_report: self.reset_report.clone(),
        }
    }

    fn apply(
        mut self,
        demux: &mut DemuxRuntime,
    ) -> (Self, Result<SourceBoundaryOutcome, DemuxRuntimeError>) {
        self.record_step(SourceBoundaryStep::ValidateEndpoint);
        self.record_step(SourceBoundaryStep::ValidateSink);
        let sink_snapshot = match demux.filter_snapshot(self.sink_filter_id) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                let outcome = SourceBoundaryOutcome::Failed {
                    step: SourceBoundaryStep::ValidateSink,
                    primary_error: err.kind,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
        };
        self.record_step(SourceBoundaryStep::ValidateSinkLifecycle);
        if sink_snapshot.state.is_closed_or_failed() {
            let err = DemuxRuntimeError::sink_lifecycle(self.sink_filter_id);
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateSinkLifecycle,
                primary_error: err.kind,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }
        let mut validated_next_source = None;
        if let Some(source_filter_id) = self.next_source {
            self.record_step(SourceBoundaryStep::ValidateSource);
            let source_snapshot = match demux.filter_snapshot(source_filter_id) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    let outcome = SourceBoundaryOutcome::Failed {
                        step: SourceBoundaryStep::ValidateSource,
                        primary_error: err.kind,
                    };
                    self.outcome = Some(outcome);
                    return (self, Err(err));
                }
            };
            self.record_step(SourceBoundaryStep::ValidateSourceLifecycle);
            if source_snapshot.state.is_closed_or_failed() {
                let err = DemuxRuntimeError::source_lifecycle(source_filter_id);
                let outcome = SourceBoundaryOutcome::Failed {
                    step: SourceBoundaryStep::ValidateSourceLifecycle,
                    primary_error: err.kind,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
            self.record_step(SourceBoundaryStep::ValidateSourceSubtype);
            if source_snapshot.open_kind != PipelineOpenKind::Raw {
                let err = DemuxRuntimeError::invalid_source_subtype(source_filter_id);
                let outcome = SourceBoundaryOutcome::Failed {
                    step: SourceBoundaryStep::ValidateSourceSubtype,
                    primary_error: err.kind,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
            self.record_step(SourceBoundaryStep::ValidateSinkSubtype);
            if matches!(sink_snapshot.open_kind, PipelineOpenKind::Other) {
                let err = DemuxRuntimeError::invalid_sink_subtype(self.sink_filter_id);
                let outcome = SourceBoundaryOutcome::Failed {
                    step: SourceBoundaryStep::ValidateSinkSubtype,
                    primary_error: err.kind,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
            self.record_step(SourceBoundaryStep::ValidatePid);
            if sink_snapshot.tpid.is_some()
                && source_snapshot.tpid.is_some()
                && sink_snapshot.tpid != source_snapshot.tpid
            {
                let err = DemuxRuntimeError::pid_mismatch(source_filter_id);
                let outcome = SourceBoundaryOutcome::Failed {
                    step: SourceBoundaryStep::ValidatePid,
                    primary_error: err.kind,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
            validated_next_source = Some((source_filter_id, source_snapshot.generation));
        }

        self.record_step(SourceBoundaryStep::ValidateQueue);
        if !demux.queue_exists(self.sink_filter_id) {
            let sink_filter_id = self.sink_filter_id;
            let err = DemuxRuntimeError::queue_missing(sink_filter_id);
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateQueue,
                primary_error: err.kind,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }

        self.record_step(SourceBoundaryStep::ValidateGeneration);
        if next_generation(demux.generation()).is_err() {
            let err = DemuxRuntimeError::generation_exhausted(Some(demux.demux_id()));
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateGeneration,
                primary_error: err.kind,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }

        let rollback_snapshot = demux.snapshot();

        self.record_step(SourceBoundaryStep::ClearQueue);
        if let Err(err) = demux.clear_existing_filter_queue(self.sink_filter_id) {
            // 存在しない queue を insert(VecDeque::new()) で作って成功にしてはならない。
            self.record_step(SourceBoundaryStep::RestoreSnapshot);
            if let Err(rollback_err) = demux.restore(rollback_snapshot) {
                self.record_step(SourceBoundaryStep::Quarantine);
                demux.quarantine();
                let outcome = SourceBoundaryOutcome::Quarantined {
                    primary_step: SourceBoundaryStep::ClearQueue,
                    primary_error: err.kind,
                    rollback_step: SourceBoundaryStep::RestoreSnapshot,
                    rollback_error: rollback_err.kind,
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
            let outcome = SourceBoundaryOutcome::RolledBack {
                primary_step: SourceBoundaryStep::ClearQueue,
                primary_error: err.kind,
                rollback_step: SourceBoundaryStep::RestoreSnapshot,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }

        self.record_step(SourceBoundaryStep::BumpGeneration);
        match demux.reset_generation_boundary() {
            Ok(reset_report) => self.reset_report = Some(reset_report),
            Err(err) => {
                self.record_step(SourceBoundaryStep::RestoreSnapshot);
                if let Err(rollback_err) = demux.restore(rollback_snapshot) {
                    self.record_step(SourceBoundaryStep::Quarantine);
                    demux.quarantine();
                    let outcome = SourceBoundaryOutcome::Quarantined {
                        primary_step: SourceBoundaryStep::BumpGeneration,
                        primary_error: err.kind,
                        rollback_step: SourceBoundaryStep::RestoreSnapshot,
                        rollback_error: rollback_err.kind,
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
                let outcome = SourceBoundaryOutcome::RolledBack {
                    primary_step: SourceBoundaryStep::BumpGeneration,
                    primary_error: err.kind,
                    rollback_step: SourceBoundaryStep::RestoreSnapshot,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
        }

        self.record_step(SourceBoundaryStep::DisconnectDownstream);
        if let Err(err) = demux.disconnect_filter_source_after_boundary(self.sink_filter_id) {
            self.record_step(SourceBoundaryStep::RestoreSnapshot);
            if let Err(rollback_err) = demux.restore(rollback_snapshot) {
                self.record_step(SourceBoundaryStep::Quarantine);
                demux.quarantine();
                let outcome = SourceBoundaryOutcome::Quarantined {
                    primary_step: SourceBoundaryStep::DisconnectDownstream,
                    primary_error: err.kind,
                    rollback_step: SourceBoundaryStep::RestoreSnapshot,
                    rollback_error: rollback_err.kind,
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
            let outcome = SourceBoundaryOutcome::RolledBack {
                primary_step: SourceBoundaryStep::DisconnectDownstream,
                primary_error: err.kind,
                rollback_step: SourceBoundaryStep::RestoreSnapshot,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }

        self.record_step(SourceBoundaryStep::Commit);
        let commit_result = match validated_next_source {
            Some((source_filter_id, source_filter_generation)) => demux
                .connect_filter_source_after_boundary(
                    self.sink_filter_id,
                    source_filter_id,
                    source_filter_generation,
                ),
            None => Ok(()),
        };
        if let Err(err) = commit_result {
            self.record_step(SourceBoundaryStep::RestoreSnapshot);
            if let Err(rollback_err) = demux.restore(rollback_snapshot) {
                self.record_step(SourceBoundaryStep::Quarantine);
                demux.quarantine();
                let outcome = SourceBoundaryOutcome::Quarantined {
                    primary_step: SourceBoundaryStep::Commit,
                    primary_error: err.kind,
                    rollback_step: SourceBoundaryStep::RestoreSnapshot,
                    rollback_error: rollback_err.kind,
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
            let outcome = SourceBoundaryOutcome::RolledBack {
                primary_step: SourceBoundaryStep::Commit,
                primary_error: err.kind,
                rollback_step: SourceBoundaryStep::RestoreSnapshot,
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
    next_source: Option<i32>,
) -> (
    SourceBoundaryReport,
    Result<SourceBoundaryOutcome, DemuxRuntimeError>,
) {
    let txn = SourceBoundaryTxn::new(sink_filter_id);
    let txn = match next_source {
        Some(source_filter_id) => txn.with_new_source(source_filter_id),
        None => txn,
    };
    let (txn, result) = txn.apply(demux);
    (txn.report(), result)
}

pub(crate) fn connect_filter_source_boundary_change(
    demux: &mut DemuxRuntime,
    sink_filter_id: i32,
    source_filter_id: i32,
) -> (
    SourceBoundaryReport,
    Result<SourceBoundaryOutcome, DemuxRuntimeError>,
) {
    apply_filter_source_boundary_change(demux, sink_filter_id, Some(source_filter_id))
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

        demux.set_filter_source_non_null(20, 10).1.unwrap();

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
        demux.set_filter_source_non_null(20, 10).1.unwrap();
        demux.create_filter_queue(20).unwrap();

        demux.disconnect_filter_source(20).1.unwrap();

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
        demux.set_filter_source_non_null(20, 10).1.unwrap();

        let result = demux.set_filter_source_non_null(20, 99).1;

        assert!(result.is_err());
        assert_eq!(
            demux.filter(20).unwrap().pipeline_view().source_filter,
            Some((10, 1))
        );
    }
}
