use super::demux::{DemuxRuntime, DemuxRuntimeError, DemuxRuntimeErrorKind};
use super::dvr::DvrRuntimeSnapshot;
use super::filter::FilterRuntimeSnapshot;
use crate::packet_pipeline::{FilterPipelineConfig, PipelineOpenKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterConfigureStep {
    ValidateState,
    ValidateSettings,
    StopWorker,
    ClearOldFmq,
    ClearOldAvBacking,
    DisconnectOldSource,
    ApplySoftDemuxConfig,
    Commit,
    RollbackSoftDemuxConfig,
    QuarantineOnRollbackFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrConfigureStep {
    ValidateState,
    ValidateSettings,
    StopWorker,
    ClearQueue,
    ResetPlaybackAssembler,
    ApplySoftDemuxConfig,
    Commit,
    RollbackSoftDemuxConfig,
    QuarantineOnRollbackFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterConfigureOutcome {
    Committed,
    Failed { failed_step: FilterConfigureStep },
    RolledBack { failed_step: FilterConfigureStep },
    Quarantined { failed_step: FilterConfigureStep },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrConfigureOutcome {
    Committed,
    Failed { failed_step: DvrConfigureStep },
    RolledBack { failed_step: DvrConfigureStep },
    Quarantined { failed_step: DvrConfigureStep },
}

#[derive(Debug, Default)]
pub struct FilterConfigureTxn {
    filter_id: i32,
    steps: Vec<FilterConfigureStep>,
    outcome: Option<FilterConfigureOutcome>,
}
#[derive(Debug, Default)]
pub struct DvrConfigureTxn {
    dvr_id: i32,
    steps: Vec<DvrConfigureStep>,
    outcome: Option<DvrConfigureOutcome>,
}

impl FilterConfigureTxn {
    pub fn new(filter_id: i32) -> Self {
        Self {
            filter_id,
            steps: Vec::new(),
            outcome: None,
        }
    }
    pub fn record_step(&mut self, step: FilterConfigureStep) {
        self.steps.push(step);
    }
    pub fn steps(&self) -> &[FilterConfigureStep] {
        &self.steps
    }
    pub fn outcome(&self) -> Option<FilterConfigureOutcome> {
        self.outcome
    }

    pub fn configure(
        mut self,
        demux: &mut DemuxRuntime,
        open_kind: PipelineOpenKind,
        config: FilterPipelineConfig,
    ) -> (Self, Result<FilterConfigureOutcome, DemuxRuntimeError>) {
        self.record_step(FilterConfigureStep::ValidateState);
        let snapshot: FilterRuntimeSnapshot = match demux.filter_snapshot(self.filter_id) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                self.outcome = Some(FilterConfigureOutcome::RolledBack {
                    failed_step: FilterConfigureStep::ValidateState,
                });
                return (self, Err(err));
            }
        };
        self.record_step(FilterConfigureStep::ValidateSettings);
        if snapshot.open_kind != open_kind {
            let filter_id = self.filter_id;
            self.outcome = Some(FilterConfigureOutcome::RolledBack {
                failed_step: FilterConfigureStep::ValidateSettings,
            });
            return (self, Err(DemuxRuntimeError::invalid_state(filter_id)));
        }
        self.record_step(FilterConfigureStep::StopWorker);
        self.record_step(FilterConfigureStep::ClearOldFmq);
        if snapshot.queue_present {
            if let Err(err) = demux.clear_existing_filter_queue(self.filter_id) {
                if demux
                    .restore_filter_snapshot(self.filter_id, snapshot)
                    .is_err()
                {
                    self.record_step(FilterConfigureStep::QuarantineOnRollbackFailure);
                    self.outcome = Some(FilterConfigureOutcome::Quarantined {
                        failed_step: FilterConfigureStep::ClearOldFmq,
                    });
                } else {
                    self.outcome = Some(FilterConfigureOutcome::RolledBack {
                        failed_step: FilterConfigureStep::ClearOldFmq,
                    });
                }
                return (self, Err(err));
            }
        }
        self.record_step(FilterConfigureStep::ClearOldAvBacking);
        if let Some(filter) = demux.filter_mut(self.filter_id) {
            filter.clear_av_backing_marker();
        }
        self.record_step(FilterConfigureStep::DisconnectOldSource);
        if let Err(err) = demux.disconnect_filter_source(self.filter_id) {
            if demux
                .restore_filter_snapshot(self.filter_id, snapshot)
                .is_err()
            {
                self.record_step(FilterConfigureStep::QuarantineOnRollbackFailure);
                self.outcome = Some(FilterConfigureOutcome::Quarantined {
                    failed_step: FilterConfigureStep::DisconnectOldSource,
                });
            } else {
                self.outcome = Some(FilterConfigureOutcome::RolledBack {
                    failed_step: FilterConfigureStep::DisconnectOldSource,
                });
            }
            return (self, Err(err));
        }
        self.record_step(FilterConfigureStep::ApplySoftDemuxConfig);
        if let Err(err) = demux.configure_filter_runtime(self.filter_id, config) {
            if err.kind == DemuxRuntimeErrorKind::GenerationExhausted {
                self.outcome = Some(FilterConfigureOutcome::Failed {
                    failed_step: FilterConfigureStep::ApplySoftDemuxConfig,
                });
                return (self, Err(err));
            }
            self.record_step(FilterConfigureStep::RollbackSoftDemuxConfig);
            if demux
                .restore_filter_snapshot(self.filter_id, snapshot)
                .is_err()
            {
                self.record_step(FilterConfigureStep::QuarantineOnRollbackFailure);
                self.outcome = Some(FilterConfigureOutcome::Quarantined {
                    failed_step: FilterConfigureStep::ApplySoftDemuxConfig,
                });
            } else {
                self.outcome = Some(FilterConfigureOutcome::RolledBack {
                    failed_step: FilterConfigureStep::ApplySoftDemuxConfig,
                });
            }
            return (self, Err(err));
        }
        self.record_step(FilterConfigureStep::Commit);
        let outcome = FilterConfigureOutcome::Committed;
        self.outcome = Some(outcome);
        (self, Ok(outcome))
    }
}

impl DvrConfigureTxn {
    pub fn new(dvr_id: i32) -> Self {
        Self {
            dvr_id,
            steps: Vec::new(),
            outcome: None,
        }
    }
    pub fn record_step(&mut self, step: DvrConfigureStep) {
        self.steps.push(step);
    }
    pub fn steps(&self) -> &[DvrConfigureStep] {
        &self.steps
    }
    pub fn outcome(&self) -> Option<DvrConfigureOutcome> {
        self.outcome
    }

    pub fn configure(
        mut self,
        demux: &mut DemuxRuntime,
    ) -> (Self, Result<DvrConfigureOutcome, DemuxRuntimeError>) {
        self.record_step(DvrConfigureStep::ValidateState);
        let snapshot: DvrRuntimeSnapshot = match demux.dvr(self.dvr_id).map(|dvr| dvr.snapshot()) {
            Some(snapshot) => snapshot,
            None => {
                let dvr_id = self.dvr_id;
                self.outcome = Some(DvrConfigureOutcome::RolledBack {
                    failed_step: DvrConfigureStep::ValidateState,
                });
                return (self, Err(DemuxRuntimeError::dvr_missing(dvr_id)));
            }
        };
        self.record_step(DvrConfigureStep::ValidateSettings);
        self.record_step(DvrConfigureStep::StopWorker);
        self.record_step(DvrConfigureStep::ClearQueue);
        if snapshot.queue_present {
            if let Some(dvr) = demux.dvr_mut(self.dvr_id) {
                dvr.clear_queue_marker();
            }
        }
        self.record_step(DvrConfigureStep::ResetPlaybackAssembler);
        if snapshot.playback_assembler_present {
            if let Some(dvr) = demux.dvr_mut(self.dvr_id) {
                dvr.reset_playback_assembler_marker();
            }
        }
        self.record_step(DvrConfigureStep::ApplySoftDemuxConfig);
        if let Err(err) = demux.configure_dvr_runtime(self.dvr_id) {
            if err.kind == DemuxRuntimeErrorKind::GenerationExhausted {
                self.outcome = Some(DvrConfigureOutcome::Failed {
                    failed_step: DvrConfigureStep::ApplySoftDemuxConfig,
                });
                return (self, Err(err));
            }
            self.record_step(DvrConfigureStep::RollbackSoftDemuxConfig);
            if let Some(dvr) = demux.dvr_mut(self.dvr_id) {
                dvr.restore(snapshot);
            }
            self.outcome = Some(DvrConfigureOutcome::RolledBack {
                failed_step: DvrConfigureStep::ApplySoftDemuxConfig,
            });
            return (self, Err(err));
        }
        self.record_step(DvrConfigureStep::Commit);
        let outcome = DvrConfigureOutcome::Committed;
        self.outcome = Some(outcome);
        (self, Ok(outcome))
    }
}
