use super::demux::{DemuxRuntime, DemuxRuntimeError, DemuxRuntimeErrorKind};
use super::dvr::DvrRuntimeSnapshot;
use super::filter::{FilterRuntimeSnapshot, FilterRuntimeState};
use crate::config::FilterConfig;
use crate::packet_pipeline::{FilterPipelineConfig, PipelineOpenKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterConfigureStep {
    ValidateState,
    ValidateSettings,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterConfigureReport {
    steps: Vec<FilterConfigureStep>,
    outcome: Option<FilterConfigureOutcome>,
}

impl FilterConfigureReport {
    pub fn steps(&self) -> &[FilterConfigureStep] {
        &self.steps
    }

    pub const fn outcome(&self) -> Option<FilterConfigureOutcome> {
        self.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrConfigureReport {
    steps: Vec<DvrConfigureStep>,
    outcome: Option<DvrConfigureOutcome>,
}

impl DvrConfigureReport {
    pub fn steps(&self) -> &[DvrConfigureStep] {
        &self.steps
    }

    pub const fn outcome(&self) -> Option<DvrConfigureOutcome> {
        self.outcome
    }
}

#[derive(Debug, Default)]
pub(crate) struct FilterConfigureTxn {
    filter_id: i32,
    steps: Vec<FilterConfigureStep>,
    outcome: Option<FilterConfigureOutcome>,
}
#[derive(Debug, Default)]
pub(crate) struct DvrConfigureTxn {
    dvr_id: i32,
    steps: Vec<DvrConfigureStep>,
    outcome: Option<DvrConfigureOutcome>,
}

impl FilterConfigureTxn {
    pub(crate) fn new(filter_id: i32) -> Self {
        Self {
            filter_id,
            steps: Vec::new(),
            outcome: None,
        }
    }
    fn record_step(&mut self, step: FilterConfigureStep) {
        self.steps.push(step);
    }
    #[cfg(test)]
    pub(crate) fn outcome(&self) -> Option<FilterConfigureOutcome> {
        self.outcome
    }
    fn report(&self) -> FilterConfigureReport {
        FilterConfigureReport {
            steps: self.steps.clone(),
            outcome: self.outcome,
        }
    }

    pub(crate) fn configure(
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
        if snapshot.state.is_closed_or_failed() || snapshot.state == FilterRuntimeState::Started {
            let filter_id = self.filter_id;
            self.outcome = Some(FilterConfigureOutcome::RolledBack {
                failed_step: FilterConfigureStep::ValidateState,
            });
            return (self, Err(DemuxRuntimeError::invalid_state(filter_id)));
        }
        self.record_step(FilterConfigureStep::ValidateSettings);
        if config.tpid.is_none() {
            let filter_id = self.filter_id;
            self.outcome = Some(FilterConfigureOutcome::RolledBack {
                failed_step: FilterConfigureStep::ValidateSettings,
            });
            return (self, Err(DemuxRuntimeError::invalid_state(filter_id)));
        }
        if snapshot.open_kind != open_kind {
            let filter_id = self.filter_id;
            self.outcome = Some(FilterConfigureOutcome::RolledBack {
                failed_step: FilterConfigureStep::ValidateSettings,
            });
            return (self, Err(DemuxRuntimeError::invalid_state(filter_id)));
        }
        self.record_step(FilterConfigureStep::ClearOldFmq);
        if snapshot.queue_present {
            if let Err(err) = demux.clear_existing_filter_queue(self.filter_id) {
                if demux
                    .restore_filter_snapshot(self.filter_id, snapshot)
                    .is_err()
                {
                    demux.quarantine();
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
        if let Err(err) = demux.disconnect_filter_source_after_boundary(self.filter_id) {
            if demux
                .restore_filter_snapshot(self.filter_id, snapshot)
                .is_err()
            {
                demux.quarantine();
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
                demux.quarantine();
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
    pub(crate) fn new(dvr_id: i32) -> Self {
        Self {
            dvr_id,
            steps: Vec::new(),
            outcome: None,
        }
    }
    fn record_step(&mut self, step: DvrConfigureStep) {
        self.steps.push(step);
    }
    #[cfg(test)]
    pub(crate) fn outcome(&self) -> Option<DvrConfigureOutcome> {
        self.outcome
    }
    fn report(&self) -> DvrConfigureReport {
        DvrConfigureReport {
            steps: self.steps.clone(),
            outcome: self.outcome,
        }
    }

    pub(crate) fn configure(
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
        self.record_step(DvrConfigureStep::ClearQueue);
        if snapshot.queue_present {
            if let Err(err) = demux.clear_dvr_queue_runtime(self.dvr_id) {
                if demux.restore_dvr_snapshot(self.dvr_id, snapshot).is_err() {
                    demux.quarantine();
                    self.record_step(DvrConfigureStep::QuarantineOnRollbackFailure);
                    self.outcome = Some(DvrConfigureOutcome::Quarantined {
                        failed_step: DvrConfigureStep::ClearQueue,
                    });
                } else {
                    self.outcome = Some(DvrConfigureOutcome::RolledBack {
                        failed_step: DvrConfigureStep::ClearQueue,
                    });
                }
                return (self, Err(err));
            }
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
            if demux.restore_dvr_snapshot(self.dvr_id, snapshot).is_err() {
                demux.quarantine();
                self.record_step(DvrConfigureStep::QuarantineOnRollbackFailure);
                self.outcome = Some(DvrConfigureOutcome::Quarantined {
                    failed_step: DvrConfigureStep::ApplySoftDemuxConfig,
                });
            } else {
                self.outcome = Some(DvrConfigureOutcome::RolledBack {
                    failed_step: DvrConfigureStep::ApplySoftDemuxConfig,
                });
            }
            return (self, Err(err));
        }
        self.record_step(DvrConfigureStep::Commit);
        let outcome = DvrConfigureOutcome::Committed;
        self.outcome = Some(outcome);
        (self, Ok(outcome))
    }
}

pub(crate) fn configure_filter_runtime(
    demux: &mut DemuxRuntime,
    filter_id: i32,
    config: FilterConfig,
) -> (
    FilterConfigureReport,
    Result<FilterConfigureOutcome, DemuxRuntimeError>,
) {
    let (txn, result) = FilterConfigureTxn::new(filter_id).configure(
        demux,
        config.open_type.pipeline_open_kind(),
        config.pipeline_config(),
    );
    (txn.report(), result)
}

pub(crate) fn configure_dvr_runtime(
    demux: &mut DemuxRuntime,
    dvr_id: i32,
) -> (
    DvrConfigureReport,
    Result<DvrConfigureOutcome, DemuxRuntimeError>,
) {
    let (txn, result) = DvrConfigureTxn::new(dvr_id).configure(demux);
    (txn.report(), result)
}
