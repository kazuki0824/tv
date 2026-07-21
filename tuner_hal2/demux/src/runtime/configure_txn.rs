use super::demux::{DemuxRuntime, DemuxRuntimeError};
use super::filter::{FilterRuntimeSnapshot, FilterRuntimeState};
use super::source_boundary::{SourceBoundaryOutcome, SourceBoundaryReport, SourceBoundaryStep};
use crate::config::FilterConfig;
use crate::packet_pipeline::{FilterPipelineConfig, PipelineOpenKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterConfigureStep {
    ValidateState,
    ValidateSettings,
    ApplySoftDemuxConfig,
    DisconnectOldSource,
    ResetQueue,
    Commit,
    RestoreSnapshot,
    QuarantineOnRollbackFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrConfigureStep {
    ValidateState,
    ValidateSettings,
    ApplySoftDemuxConfig,
    ApplyStatusReporting,
    ResetQueue,
    Commit,
    RestoreSnapshot,
    QuarantineOnRollbackFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterConfigureOutcome {
    Committed,
    Failed {
        failed_step: FilterConfigureStep,
        primary_error: DemuxRuntimeError,
    },
    RolledBack {
        failed_step: FilterConfigureStep,
        primary_error: DemuxRuntimeError,
        rollback_step: FilterConfigureStep,
    },
    Quarantined {
        failed_step: FilterConfigureStep,
        primary_error: DemuxRuntimeError,
        rollback_step: FilterConfigureStep,
        rollback_error: DemuxRuntimeError,
    },
    PartialEffectQuarantined {
        failed_step: FilterConfigureStep,
        primary_error: DemuxRuntimeError,
        partial_effect_step: FilterConfigureStep,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrConfigureOutcome {
    Committed,
    Failed {
        failed_step: DvrConfigureStep,
        primary_error: DemuxRuntimeError,
    },
    RolledBack {
        failed_step: DvrConfigureStep,
        primary_error: DemuxRuntimeError,
        rollback_step: DvrConfigureStep,
    },
    Quarantined {
        failed_step: DvrConfigureStep,
        primary_error: DemuxRuntimeError,
        rollback_step: DvrConfigureStep,
        rollback_error: DemuxRuntimeError,
    },
    PartialEffectQuarantined {
        failed_step: DvrConfigureStep,
        primary_error: DemuxRuntimeError,
        partial_effect_step: DvrConfigureStep,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterConfigureReport {
    steps: Vec<FilterConfigureStep>,
    outcome: Option<FilterConfigureOutcome>,
    source_boundary_report: Option<SourceBoundaryReport>,
}

impl FilterConfigureReport {
    pub fn steps(&self) -> &[FilterConfigureStep] {
        &self.steps
    }

    pub const fn outcome(&self) -> Option<FilterConfigureOutcome> {
        self.outcome
    }

    pub fn source_boundary_report(&self) -> Option<&SourceBoundaryReport> {
        self.source_boundary_report.as_ref()
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
    source_boundary_report: Option<SourceBoundaryReport>,
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
            source_boundary_report: None,
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
            source_boundary_report: self.source_boundary_report.clone(),
        }
    }

    fn restore_after_failure(
        &mut self,
        demux: &mut DemuxRuntime,
        snapshot: super::demux::DemuxRuntimeSnapshot,
        failed_step: FilterConfigureStep,
        primary_error: DemuxRuntimeError,
    ) {
        self.record_step(FilterConfigureStep::RestoreSnapshot);
        match demux.restore(snapshot) {
            Ok(()) => {
                self.outcome = Some(FilterConfigureOutcome::RolledBack {
                    failed_step,
                    primary_error,
                    rollback_step: FilterConfigureStep::RestoreSnapshot,
                });
            }
            Err(rollback_error) => {
                demux.quarantine();
                self.record_step(FilterConfigureStep::QuarantineOnRollbackFailure);
                self.outcome = Some(FilterConfigureOutcome::Quarantined {
                    failed_step,
                    primary_error,
                    rollback_step: FilterConfigureStep::RestoreSnapshot,
                    rollback_error,
                });
            }
        }
    }

    pub(crate) fn configure(
        mut self,
        demux: &mut DemuxRuntime,
        open_kind: PipelineOpenKind,
        config: FilterPipelineConfig,
    ) -> (Self, Result<FilterConfigureOutcome, DemuxRuntimeError>) {
        self.record_step(FilterConfigureStep::ValidateState);
        let filter_snapshot: FilterRuntimeSnapshot = match demux.filter_snapshot(self.filter_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.outcome = Some(FilterConfigureOutcome::Failed {
                    failed_step: FilterConfigureStep::ValidateState,
                    primary_error: error,
                });
                return (self, Err(error));
            }
        };
        if filter_snapshot.state.is_closed_or_failed()
            || filter_snapshot.state == FilterRuntimeState::Started
        {
            let error = DemuxRuntimeError::invalid_state(self.filter_id);
            self.outcome = Some(FilterConfigureOutcome::Failed {
                failed_step: FilterConfigureStep::ValidateState,
                primary_error: error,
            });
            return (self, Err(error));
        }

        self.record_step(FilterConfigureStep::ValidateSettings);
        if filter_snapshot.open_kind != open_kind {
            let error = DemuxRuntimeError::invalid_state(self.filter_id);
            self.outcome = Some(FilterConfigureOutcome::Failed {
                failed_step: FilterConfigureStep::ValidateSettings,
                primary_error: error,
            });
            return (self, Err(error));
        }

        let rollback_snapshot = match demux.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.outcome = Some(FilterConfigureOutcome::Failed {
                    failed_step: FilterConfigureStep::ValidateState,
                    primary_error: error,
                });
                return (self, Err(error));
            }
        };
        self.record_step(FilterConfigureStep::ApplySoftDemuxConfig);
        if let Err(error) = demux.configure_filter_runtime(self.filter_id, config) {
            self.restore_after_failure(
                demux,
                rollback_snapshot,
                FilterConfigureStep::ApplySoftDemuxConfig,
                error,
            );
            return (self, Err(error));
        }

        // source-boundary commit と generation/pipeline reset を最終操作にする。
        // queue identity は維持し、export 済み queue が非空または不一致なら、
        // shared pointer reset や queue instance 置換を行わず失敗させる。
        self.record_step(FilterConfigureStep::DisconnectOldSource);
        let (source_boundary_report, disconnect_result) =
            demux.disconnect_filter_source(self.filter_id);
        let source_outcome = source_boundary_report.outcome();
        self.source_boundary_report = Some(source_boundary_report);
        if let Err(error) = disconnect_result {
            match source_outcome {
                SourceBoundaryOutcome::PartialEffectQuarantined {
                    failed_step: SourceBoundaryStep::ClearQueue,
                    ..
                } => {
                    self.record_step(FilterConfigureStep::ResetQueue);
                    demux.quarantine();
                    self.outcome = Some(FilterConfigureOutcome::PartialEffectQuarantined {
                        failed_step: FilterConfigureStep::ResetQueue,
                        primary_error: error,
                        partial_effect_step: FilterConfigureStep::ResetQueue,
                    });
                }
                SourceBoundaryOutcome::Quarantined {
                    primary_error,
                    rollback_error,
                    ..
                } => {
                    demux.quarantine();
                    self.outcome = Some(FilterConfigureOutcome::Quarantined {
                        failed_step: FilterConfigureStep::DisconnectOldSource,
                        primary_error,
                        rollback_step: FilterConfigureStep::RestoreSnapshot,
                        rollback_error,
                    });
                }
                _ => self.restore_after_failure(
                    demux,
                    rollback_snapshot,
                    FilterConfigureStep::DisconnectOldSource,
                    error,
                ),
            }
            return (self, Err(error));
        }

        self.record_step(FilterConfigureStep::ResetQueue);
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

    fn restore_after_failure(
        &mut self,
        demux: &mut DemuxRuntime,
        snapshot: super::demux::DemuxRuntimeSnapshot,
        failed_step: DvrConfigureStep,
        primary_error: DemuxRuntimeError,
    ) {
        self.record_step(DvrConfigureStep::RestoreSnapshot);
        match demux.restore(snapshot) {
            Ok(()) => {
                self.outcome = Some(DvrConfigureOutcome::RolledBack {
                    failed_step,
                    primary_error,
                    rollback_step: DvrConfigureStep::RestoreSnapshot,
                });
            }
            Err(rollback_error) => {
                demux.quarantine();
                self.record_step(DvrConfigureStep::QuarantineOnRollbackFailure);
                self.outcome = Some(DvrConfigureOutcome::Quarantined {
                    failed_step,
                    primary_error,
                    rollback_step: DvrConfigureStep::RestoreSnapshot,
                    rollback_error,
                });
            }
        }
    }

    pub(crate) fn configure(
        mut self,
        demux: &mut DemuxRuntime,
        status_mask: i32,
        low_threshold_bytes: usize,
        high_threshold_bytes: usize,
    ) -> (Self, Result<DvrConfigureOutcome, DemuxRuntimeError>) {
        self.record_step(DvrConfigureStep::ValidateState);
        let _snapshot = match demux.dvr_snapshot(self.dvr_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.outcome = Some(DvrConfigureOutcome::Failed {
                    failed_step: DvrConfigureStep::ValidateState,
                    primary_error: error,
                });
                return (self, Err(error));
            }
        };
        self.record_step(DvrConfigureStep::ValidateSettings);
        let rollback_snapshot = match demux.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.outcome = Some(DvrConfigureOutcome::Failed {
                    failed_step: DvrConfigureStep::ValidateState,
                    primary_error: error,
                });
                return (self, Err(error));
            }
        };

        self.record_step(DvrConfigureStep::ApplySoftDemuxConfig);
        if let Err(error) = demux.configure_dvr_runtime(self.dvr_id) {
            self.restore_after_failure(
                demux,
                rollback_snapshot,
                DvrConfigureStep::ApplySoftDemuxConfig,
                error,
            );
            return (self, Err(error));
        }

        self.record_step(DvrConfigureStep::ApplyStatusReporting);
        if let Err(error) = demux.configure_dvr_status_reporting(
            self.dvr_id,
            status_mask,
            low_threshold_bytes,
            high_threshold_bytes,
        ) {
            self.restore_after_failure(
                demux,
                rollback_snapshot,
                DvrConfigureStep::ApplyStatusReporting,
                error,
            );
            return (self, Err(error));
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
    status_mask: i32,
    low_threshold_bytes: usize,
    high_threshold_bytes: usize,
) -> (
    DvrConfigureReport,
    Result<DvrConfigureOutcome, DemuxRuntimeError>,
) {
    let (txn, result) = DvrConfigureTxn::new(dvr_id).configure(
        demux,
        status_mask,
        low_threshold_bytes,
        high_threshold_bytes,
    );
    (txn.report(), result)
}
