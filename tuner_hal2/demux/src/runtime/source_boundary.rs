use crate::packet_pipeline::{PipelineOpenKind, PipelineResetReport};

use super::demux::{DemuxRuntime, DemuxRuntimeError, DemuxRuntimeErrorKind};

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
    PrepareRelation,
    StreamBoundary,
    DisconnectDownstream,
    Commit,
    RestoreSnapshot,
    Quarantine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceBoundaryOutcome {
    Committed,
    Isolated {
        step: SourceBoundaryStep,
        primary_error: DemuxRuntimeErrorKind,
    },
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
        if sink_snapshot.state.is_closed_or_failed()
            || sink_snapshot.state == super::filter::FilterRuntimeState::Started
        {
            let err = DemuxRuntimeError::sink_lifecycle(self.sink_filter_id);
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateSinkLifecycle,
                primary_error: err.kind,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }
        self.record_step(SourceBoundaryStep::PrepareRelation);
        let expected_relation_generation = sink_snapshot.source_relation_generation;
        let Some(next_relation_generation) = expected_relation_generation
            .checked_add(1)
            .filter(|generation| *generation != 0)
        else {
            let err = DemuxRuntimeError::generation_exhausted(Some(self.sink_filter_id));
            self.record_step(SourceBoundaryStep::Quarantine);
            demux.quarantine_filter_runtime(self.sink_filter_id);
            let outcome = SourceBoundaryOutcome::Isolated {
                step: SourceBoundaryStep::PrepareRelation,
                primary_error: err.kind,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        };
        let mut validated_next_source = None;
        if let Some(source_filter_id) = self.next_source {
            if source_filter_id == self.sink_filter_id {
                let err = DemuxRuntimeError::self_reference(self.sink_filter_id);
                let outcome = SourceBoundaryOutcome::Failed {
                    step: SourceBoundaryStep::ValidateSource,
                    primary_error: err.kind,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
            if demux.source_connection_would_cycle(self.sink_filter_id, source_filter_id) {
                let err = DemuxRuntimeError::self_reference(self.sink_filter_id);
                let outcome = SourceBoundaryOutcome::Failed {
                    step: SourceBoundaryStep::ValidateSource,
                    primary_error: err.kind,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
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
            if !matches!(
                sink_snapshot.open_kind,
                PipelineOpenKind::Raw
                    | PipelineOpenKind::Section
                    | PipelineOpenKind::Pes
                    | PipelineOpenKind::Av
                    | PipelineOpenKind::Pcr
                    | PipelineOpenKind::Record
            ) {
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
        if let Err(err) = demux.validate_filter_delivery_boundary(self.sink_filter_id) {
            let outcome = SourceBoundaryOutcome::Failed {
                step: SourceBoundaryStep::ValidateQueue,
                primary_error: err.kind,
            };
            self.outcome = Some(outcome);
            return (self, Err(err));
        }

        self.record_step(SourceBoundaryStep::StreamBoundary);
        let stream_boundary_result =
            demux.apply_filter_source_stream_boundary(self.sink_filter_id);
        match stream_boundary_result {
            Ok(reset_report) => self.reset_report = Some(reset_report),
            Err(err) => {
                self.record_step(SourceBoundaryStep::Quarantine);
                demux.quarantine_filter_runtime(self.sink_filter_id);
                let outcome = SourceBoundaryOutcome::Isolated {
                    step: SourceBoundaryStep::StreamBoundary,
                    primary_error: err.kind,
                };
                self.outcome = Some(outcome);
                return (self, Err(err));
            }
        }

        self.record_step(SourceBoundaryStep::DisconnectDownstream);
        self.record_step(SourceBoundaryStep::Commit);
        let commit_result = match validated_next_source {
            Some((source_filter_id, source_filter_generation)) => demux
                .connect_filter_source_after_boundary(
                    self.sink_filter_id,
                    expected_relation_generation,
                    next_relation_generation,
                    source_filter_id,
                    source_filter_generation,
                ),
            None => demux.disconnect_filter_source_after_boundary(
                self.sink_filter_id,
                expected_relation_generation,
                next_relation_generation,
            ),
        };
        if let Err(err) = commit_result {
            self.record_step(SourceBoundaryStep::Quarantine);
            demux.quarantine_filter_runtime(self.sink_filter_id);
            let outcome = SourceBoundaryOutcome::Isolated {
                step: SourceBoundaryStep::Commit,
                primary_error: err.kind,
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
            PipelineOpenKind::Pcr => FilterOpenType::TsPcr,
            PipelineOpenKind::Av => FilterOpenType::TsVideo,
            PipelineOpenKind::Section => FilterOpenType::TsSection,
            PipelineOpenKind::Pes => FilterOpenType::TsPes,
            PipelineOpenKind::Record => FilterOpenType::TsRecord,
            PipelineOpenKind::Other => FilterOpenType::TsUndefined,
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
            .register_filter(configured_filter(20, 1, PipelineOpenKind::Raw))
            .unwrap();

        demux.set_filter_source_non_null(20, 10).1.unwrap();

        assert_eq!(
            demux.filter(20).unwrap().pipeline_view().source_filter,
            Some((10, 1))
        );
        assert_eq!(demux.filter(20).unwrap().source_relation_generation(), 2);
    }

    #[test]
    fn source_boundary_accepts_every_published_ts_packet_sink_kind() {
        for (index, sink_kind) in [
            PipelineOpenKind::Raw,
            PipelineOpenKind::Section,
            PipelineOpenKind::Pes,
            PipelineOpenKind::Av,
            PipelineOpenKind::Pcr,
            PipelineOpenKind::Record,
        ]
        .into_iter()
        .enumerate()
        {
            let mut demux = DemuxRuntime::new(100 + index as i32, 1);
            demux
                .register_filter(configured_filter(10, 1, PipelineOpenKind::Raw))
                .unwrap();
            demux
                .register_filter(configured_filter(20, 1, sink_kind))
                .unwrap();

            demux.set_filter_source_non_null(20, 10).1.unwrap();

            assert_eq!(
                demux.filter(20).unwrap().pipeline_view().source_filter,
                Some((10, 1))
            );
        }
    }

    #[test]
    fn source_boundary_disconnects_to_demux_input() {
        let mut demux = DemuxRuntime::new(2, 1);
        demux
            .register_filter(configured_filter(10, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux
            .register_filter(configured_filter(20, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux.set_filter_source_non_null(20, 10).1.unwrap();
        demux.create_filter_queue(20).unwrap();

        demux.disconnect_filter_source(20).1.unwrap();

        assert_eq!(
            demux.filter(20).unwrap().pipeline_view().source_filter,
            None
        );
        assert_eq!(demux.filter(20).unwrap().source_relation_generation(), 3);
    }

    #[test]
    fn source_boundary_failure_keeps_existing_source() {
        let mut demux = DemuxRuntime::new(3, 1);
        demux
            .register_filter(configured_filter(10, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux
            .register_filter(configured_filter(20, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux.set_filter_source_non_null(20, 10).1.unwrap();

        let result = demux.set_filter_source_non_null(20, 99).1;

        assert!(result.is_err());
        assert_eq!(
            demux.filter(20).unwrap().pipeline_view().source_filter,
            Some((10, 1))
        );
        assert_eq!(demux.filter(20).unwrap().source_relation_generation(), 2);
    }

    #[test]
    fn source_boundary_uses_one_stream_boundary_step() {
        let mut demux = DemuxRuntime::new(4, 1);
        demux
            .register_filter(configured_filter(10, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux
            .register_filter(configured_filter(20, 1, PipelineOpenKind::Raw))
            .unwrap();

        let (report, result) = demux.set_filter_source_non_null(20, 10);
        result.unwrap();

        assert_eq!(
            report
                .steps()
                .iter()
                .filter(|step| **step == SourceBoundaryStep::StreamBoundary)
                .count(),
            1
        );
    }

    #[test]
    fn source_relation_generation_exhaustion_never_reuses_or_wraps() {
        let mut demux = DemuxRuntime::new(5, 1);
        demux
            .register_filter(configured_filter(10, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux
            .register_filter(configured_filter(20, 1, PipelineOpenKind::Raw))
            .unwrap();
        demux
            .filter_mut(20)
            .unwrap()
            .set_source_relation_generation_for_test(u64::MAX);

        let error = demux.set_filter_source_non_null(20, 10).1.unwrap_err();

        assert_eq!(error.kind, DemuxRuntimeErrorKind::GenerationExhausted);
        assert_eq!(
            demux.filter(20).unwrap().source_relation_generation(),
            u64::MAX
        );
        assert_eq!(
            demux.filter(20).unwrap().pipeline_view().source_filter,
            None
        );
    }
}
