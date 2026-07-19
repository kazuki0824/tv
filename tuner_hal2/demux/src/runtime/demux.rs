#[cfg(test)]
use std::collections::VecDeque;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_control_core::{
    FmqDeliveryAction, FmqDeliveryTxn, FmqFailureKind, FmqObjectKind,
};

use crate::av::{
    AvDataId, AvDataIdState, AvFilterReleaseState, AvHandleReleaseInput, AvHandleReleaseOutcome,
    AvHandleReleaseTxn, AvPayloadDeliveryOutcome, AvSharedBacking, AvSharedHandleExport,
    ClientHandleState,
};
#[cfg(test)]
use crate::config::FilterDelayReadiness;
#[cfg(test)]
use crate::config::FilterOpenType;
use crate::config::{AvStreamTypeConfig, FilterDelayHint, OpenFilterRequest};
use crate::packet_pipeline::{
    FilterPipelineConfig, PacketPipeline, PipelineBoundaryReason, PipelineDeliveryAction,
    PipelineFilterView, PipelineGeneratedEvent, PipelineInputKind, PipelineOpenKind,
    PipelineReport, PipelineResetReport,
};
use crate::TsInputOrigin;

use super::dvr::{
    DvrKind, DvrRuntime, DvrRuntimeRollbackIdentity, DvrRuntimeSnapshot, DvrStatusEvent,
};
use super::filter::{
    FilterRuntime, FilterRuntimeRollbackIdentity, FilterRuntimeSnapshot, FilterRuntimeState,
};
use super::generation_boundary::{
    DemuxGenerationBoundaryAuthorization, DemuxStreamGeneration, GenerationBoundaryReport,
};
use super::queue_runtime::{
    QueueDescriptorExportPlan, QueueDescriptorExportTarget, QueueRuntime, QueueRuntimeError,
    QueueWaitHandle,
};
use super::source_boundary::{
    apply_filter_source_boundary_change, connect_filter_source_boundary_change,
    SourceBoundaryReport,
};

const TUNER_EVENT_DATA_READY: u32 = 1 << 0;
const TUNER_EVENT_DATA_CONSUMED: u32 = 1 << 1;
const MAX_FILTER_DELAY_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordDvrMirrorWriteOutcome {
    Written,
    Overflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DemuxRuntimeDiagnosticSnapshot {
    rollback_token_drop_failure_count: u64,
}

impl DemuxRuntimeDiagnosticSnapshot {
    pub const fn rollback_token_drop_failure_count(self) -> u64 {
        self.rollback_token_drop_failure_count
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxRuntimeState {
    Open,
    Closing,
    CleanupFailed,
    Closed,
    Failed,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxRuntimeErrorKind {
    FilterMissing,
    DvrMissing,
    QueueMissing,
    InvalidState,
    InvalidDvrFilter,
    UnsupportedDvrOperation,
    SourceLifecycle,
    SinkLifecycle,
    InvalidSourceSubtype,
    InvalidSinkSubtype,
    PidMismatch,
    PipelineFailed,
    GenerationExhausted,
    QueueRuntimeFailure,
    AvBackingFailure,
    SourceBoundaryRollbackFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxGenerationTarget {
    Unscoped,
    Demux(i32),
    Filter(i32),
    Dvr(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxRuntimeError {
    FilterMissing { filter_id: i32 },
    DvrMissing { dvr_id: i32 },
    QueueMissing { object_id: i32 },
    InvalidState { object_id: i32 },
    InvalidDvrFilter { filter_id: i32 },
    UnsupportedDvrOperation { dvr_id: i32 },
    SourceLifecycle { filter_id: i32 },
    SinkLifecycle { filter_id: i32 },
    InvalidSourceSubtype { filter_id: i32 },
    InvalidSinkSubtype { filter_id: i32 },
    PidMismatch { filter_id: i32 },
    PipelineFailed,
    GenerationExhausted { target: DemuxGenerationTarget },
    QueueRuntimeFailure { object_id: i32 },
    AvBackingFailure { filter_id: i32 },
    SourceBoundaryRollbackFailed { filter_id: i32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrFlushStep {
    ValidateState,
    ValidateQueueEmpty,
    ClearPlaybackCompletion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrFlushStepOutcome {
    Succeeded(DvrFlushStep),
    Failed {
        step: DvrFlushStep,
        error: DemuxRuntimeError,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrFlushOutcome {
    Committed,
    Failed { failed_step: DvrFlushStep },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrFlushReport {
    dvr_id: i32,
    steps: Vec<DvrFlushStepOutcome>,
    outcome: Option<DvrFlushOutcome>,
}

impl DvrFlushReport {
    fn new(dvr_id: i32) -> Self {
        Self {
            dvr_id,
            steps: Vec::new(),
            outcome: None,
        }
    }

    fn succeeded(&mut self, step: DvrFlushStep) {
        self.steps.push(DvrFlushStepOutcome::Succeeded(step));
    }

    fn failed(&mut self, step: DvrFlushStep, error: DemuxRuntimeError) {
        self.steps.push(DvrFlushStepOutcome::Failed { step, error });
        self.outcome = Some(DvrFlushOutcome::Failed { failed_step: step });
    }

    fn committed(&mut self) {
        self.outcome = Some(DvrFlushOutcome::Committed);
    }

    pub const fn dvr_id(&self) -> i32 {
        self.dvr_id
    }

    pub fn steps(&self) -> &[DvrFlushStepOutcome] {
        &self.steps
    }

    pub const fn outcome(&self) -> Option<DvrFlushOutcome> {
        self.outcome
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterRuntimeOperationKind {
    Stop,
    Flush,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterRuntimeOperationStep {
    ValidateState,
    PipelineStop,
    PipelineFlush,
    QueueClear,
    PipelineRollback,
    MirrorQueueClear,
    QueuedPayloadClear,
    AvBackingFlush,
    MarkStopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterRuntimeOperationSkipReason {
    QueueNotPresent,
    QueueClearFailed,
    FilterMissingForOptionalFlush,
    AvBackingNotPresent,
    AlreadyStoppedOrConfigured,
    OpenStateNoop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterRuntimeOperationStepOutcome {
    Succeeded(FilterRuntimeOperationStep),
    Failed {
        step: FilterRuntimeOperationStep,
        error: DemuxRuntimeError,
    },
    Skipped {
        step: FilterRuntimeOperationStep,
        reason: FilterRuntimeOperationSkipReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterRuntimeOperationOutcome {
    Committed,
    Noop,
    Failed {
        failed_step: FilterRuntimeOperationStep,
    },
    RolledBack {
        failed_step: FilterRuntimeOperationStep,
        rollback_step: FilterRuntimeOperationStep,
    },
    PartialEffectQuarantined {
        failed_step: FilterRuntimeOperationStep,
        partial_effect_step: FilterRuntimeOperationStep,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterRuntimeOperationReport {
    operation: FilterRuntimeOperationKind,
    filter_id: i32,
    steps: Vec<FilterRuntimeOperationStepOutcome>,
    outcome: Option<FilterRuntimeOperationOutcome>,
}

impl FilterRuntimeOperationReport {
    fn new(operation: FilterRuntimeOperationKind, filter_id: i32) -> Self {
        Self {
            operation,
            filter_id,
            steps: Vec::new(),
            outcome: None,
        }
    }

    fn succeeded(&mut self, step: FilterRuntimeOperationStep) {
        self.steps
            .push(FilterRuntimeOperationStepOutcome::Succeeded(step));
    }

    fn failed(&mut self, step: FilterRuntimeOperationStep, error: DemuxRuntimeError) {
        self.steps
            .push(FilterRuntimeOperationStepOutcome::Failed { step, error });
    }

    fn skipped(
        &mut self,
        step: FilterRuntimeOperationStep,
        reason: FilterRuntimeOperationSkipReason,
    ) {
        self.steps
            .push(FilterRuntimeOperationStepOutcome::Skipped { step, reason });
    }

    fn finish(&mut self, outcome: FilterRuntimeOperationOutcome) {
        self.outcome = Some(outcome);
    }

    pub const fn operation(&self) -> FilterRuntimeOperationKind {
        self.operation
    }

    pub const fn filter_id(&self) -> i32 {
        self.filter_id
    }

    pub fn steps(&self) -> &[FilterRuntimeOperationStepOutcome] {
        &self.steps
    }

    pub const fn outcome(&self) -> Option<FilterRuntimeOperationOutcome> {
        self.outcome
    }
}

impl DemuxRuntimeError {
    pub const fn kind(self) -> DemuxRuntimeErrorKind {
        match self {
            Self::FilterMissing { .. } => DemuxRuntimeErrorKind::FilterMissing,
            Self::DvrMissing { .. } => DemuxRuntimeErrorKind::DvrMissing,
            Self::QueueMissing { .. } => DemuxRuntimeErrorKind::QueueMissing,
            Self::InvalidState { .. } => DemuxRuntimeErrorKind::InvalidState,
            Self::InvalidDvrFilter { .. } => DemuxRuntimeErrorKind::InvalidDvrFilter,
            Self::UnsupportedDvrOperation { .. } => DemuxRuntimeErrorKind::UnsupportedDvrOperation,
            Self::SourceLifecycle { .. } => DemuxRuntimeErrorKind::SourceLifecycle,
            Self::SinkLifecycle { .. } => DemuxRuntimeErrorKind::SinkLifecycle,
            Self::InvalidSourceSubtype { .. } => DemuxRuntimeErrorKind::InvalidSourceSubtype,
            Self::InvalidSinkSubtype { .. } => DemuxRuntimeErrorKind::InvalidSinkSubtype,
            Self::PidMismatch { .. } => DemuxRuntimeErrorKind::PidMismatch,
            Self::PipelineFailed => DemuxRuntimeErrorKind::PipelineFailed,
            Self::GenerationExhausted { .. } => DemuxRuntimeErrorKind::GenerationExhausted,
            Self::QueueRuntimeFailure { .. } => DemuxRuntimeErrorKind::QueueRuntimeFailure,
            Self::AvBackingFailure { .. } => DemuxRuntimeErrorKind::AvBackingFailure,
            Self::SourceBoundaryRollbackFailed { .. } => {
                DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed
            }
        }
    }

    pub const fn filter_missing(filter_id: i32) -> Self {
        Self::FilterMissing { filter_id }
    }
    pub const fn dvr_missing(dvr_id: i32) -> Self {
        Self::DvrMissing { dvr_id }
    }
    pub const fn queue_missing(object_id: i32) -> Self {
        Self::QueueMissing { object_id }
    }
    pub const fn invalid_state(object_id: i32) -> Self {
        Self::InvalidState { object_id }
    }
    pub const fn invalid_dvr_filter(filter_id: i32) -> Self {
        Self::InvalidDvrFilter { filter_id }
    }
    pub const fn unsupported_dvr_operation(dvr_id: i32) -> Self {
        Self::UnsupportedDvrOperation { dvr_id }
    }
    pub const fn source_lifecycle(filter_id: i32) -> Self {
        Self::SourceLifecycle { filter_id }
    }
    pub const fn sink_lifecycle(filter_id: i32) -> Self {
        Self::SinkLifecycle { filter_id }
    }
    pub const fn invalid_source_subtype(filter_id: i32) -> Self {
        Self::InvalidSourceSubtype { filter_id }
    }
    pub const fn invalid_sink_subtype(filter_id: i32) -> Self {
        Self::InvalidSinkSubtype { filter_id }
    }
    pub const fn pid_mismatch(filter_id: i32) -> Self {
        Self::PidMismatch { filter_id }
    }
    pub const fn pipeline_failed() -> Self {
        Self::PipelineFailed
    }
    pub const fn generation_exhausted(target: DemuxGenerationTarget) -> Self {
        Self::GenerationExhausted { target }
    }
    pub const fn queue_runtime_failure(object_id: i32) -> Self {
        Self::QueueRuntimeFailure { object_id }
    }
    pub const fn av_backing_failure(filter_id: i32) -> Self {
        Self::AvBackingFailure { filter_id }
    }
    pub const fn source_boundary_rollback_failed(filter_id: i32) -> Self {
        Self::SourceBoundaryRollbackFailed { filter_id }
    }
}

pub fn next_generation(current: u64) -> Result<u64, DemuxRuntimeError> {
    current
        .checked_add(1)
        .ok_or(DemuxRuntimeError::generation_exhausted(DemuxGenerationTarget::Unscoped))
}

fn av_payload_delivery_outcome_diagnostic(
    outcome: AvPayloadDeliveryOutcome,
    pid: crate::packet_pipeline::PacketPid,
    filter_id: i32,
) -> Option<crate::packet_pipeline::PipelineDiagnostic> {
    match outcome {
        AvPayloadDeliveryOutcome::Delivered(_) => None,
        AvPayloadDeliveryOutcome::SharedHandleNotExported => Some(
            crate::packet_pipeline::PipelineDiagnostic::av_shared_handle_not_exported(
                pid, filter_id,
            ),
        ),
        AvPayloadDeliveryOutcome::ClientHandleReleased => Some(
            crate::packet_pipeline::PipelineDiagnostic::av_client_handle_released(pid, filter_id),
        ),
        AvPayloadDeliveryOutcome::PayloadOversized => {
            Some(crate::packet_pipeline::PipelineDiagnostic::av_payload_oversized(pid, filter_id))
        }
        AvPayloadDeliveryOutcome::NoFreeSlot => Some(
            crate::packet_pipeline::PipelineDiagnostic::av_no_free_slot(pid, filter_id),
        ),
        AvPayloadDeliveryOutcome::DataIdExhausted => {
            Some(crate::packet_pipeline::PipelineDiagnostic::av_data_id_exhausted(pid, filter_id))
        }
    }
}

#[derive(Clone, Debug)]
pub struct DemuxRuntimeSnapshot {
    state: DemuxRuntimeState,
    generation: u64,
    pipeline: PacketPipeline,
    filters: BTreeMap<i32, FilterRuntime>,
    dvrs: BTreeMap<i32, DvrRuntime>,
    filter_queue_identities: BTreeMap<i32, u64>,
    dvr_queue_identities: BTreeMap<i32, u64>,
    filter_av_backing_identities: BTreeMap<i32, u64>,
    #[cfg(test)]
    filter_queue_mirror: BTreeMap<i32, VecDeque<Vec<u8>>>,
}

/// External-join rollback authority only captures stable control-plane identity.
/// Volatile pipeline/FMQ/AV client progress is deliberately preserved across rollback.
#[derive(Clone, Debug)]
struct DemuxRuntimeRollbackSnapshot {
    state: DemuxRuntimeState,
    generation: u64,
    filters: BTreeMap<i32, FilterRuntimeRollbackIdentity>,
    dvrs: BTreeMap<i32, DvrRuntimeRollbackIdentity>,
    filter_queue_identities: BTreeMap<i32, u64>,
    dvr_queue_identities: BTreeMap<i32, u64>,
    filter_av_backing_identities: BTreeMap<i32, u64>,
    authorized_post_generation: Option<u64>,
}

#[derive(Debug)]
struct DemuxRuntimeRollbackLedger {
    snapshots: BTreeMap<u64, DemuxRuntimeRollbackSnapshot>,
    next_token_id: u64,
    active_token_id: Option<u64>,
}

impl Default for DemuxRuntimeRollbackLedger {
    fn default() -> Self {
        Self {
            snapshots: BTreeMap::new(),
            next_token_id: 1,
            active_token_id: None,
        }
    }
}

pub struct DemuxRuntimeRollbackToken {
    demux_id: i32,
    token_id: u64,
    generation: u64,
    ledger: Arc<Mutex<DemuxRuntimeRollbackLedger>>,
    drop_failure_count: Arc<AtomicU64>,
    armed: bool,
}

impl fmt::Debug for DemuxRuntimeRollbackToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DemuxRuntimeRollbackToken")
            .field("demux_id", &self.demux_id)
            .field("token_id", &self.token_id)
            .field("generation", &self.generation)
            .field("armed", &self.armed)
            .finish_non_exhaustive()
    }
}

impl DemuxRuntimeRollbackToken {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn demux_id(&self) -> i32 {
        self.demux_id
    }

    fn new(
        demux_id: i32,
        token_id: u64,
        generation: u64,
        ledger: Arc<Mutex<DemuxRuntimeRollbackLedger>>,
        drop_failure_count: Arc<AtomicU64>,
    ) -> Self {
        Self {
            demux_id,
            token_id,
            generation,
            ledger,
            drop_failure_count,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    pub fn discard_without_runtime(mut self) -> Result<(), DemuxRuntimeError> {
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| DemuxRuntimeError::invalid_state(self.demux_id))?;
        let snapshot = ledger
            .snapshots
            .get(&self.token_id)
            .ok_or(DemuxRuntimeError::invalid_state(self.demux_id))?;
        if snapshot.generation != self.generation
            || ledger.active_token_id != Some(self.token_id)
        {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        ledger.snapshots.remove(&self.token_id);
        ledger.active_token_id = None;
        drop(ledger);
        self.disarm();
        Ok(())
    }
}

impl Drop for DemuxRuntimeRollbackToken {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        match self.ledger.lock() {
            Ok(mut ledger) => {
                ledger.snapshots.remove(&self.token_id);
                if ledger.active_token_id == Some(self.token_id) {
                    ledger.active_token_id = None;
                }
                self.armed = false;
            }
            Err(_) => {
                let _ = self.drop_failure_count.fetch_update(
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                    |value| Some(value.saturating_add(1)),
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DemuxRuntimeRollbackTokenPrepareRequest {
    demux_id: i32,
}

impl DemuxRuntimeRollbackTokenPrepareRequest {
    pub const fn new(demux_id: i32) -> Self {
        Self { demux_id }
    }
}

#[derive(Debug)]
pub struct DemuxRuntimeRollbackRestoreRequest<'a> {
    token: &'a mut DemuxRuntimeRollbackToken,
}

impl<'a> DemuxRuntimeRollbackRestoreRequest<'a> {
    pub fn new(token: &'a mut DemuxRuntimeRollbackToken) -> Self {
        Self { token }
    }
}

#[derive(Debug)]
pub struct DemuxRuntimeRollbackCommitRequest {
    token: DemuxRuntimeRollbackToken,
}

impl DemuxRuntimeRollbackCommitRequest {
    pub const fn new(token: DemuxRuntimeRollbackToken) -> Self {
        Self { token }
    }
}

impl DemuxRuntimeSnapshot {
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Debug)]
pub struct FilterRuntimeRegistrationRequest<'a> {
    filter_id: i32,
    request: &'a OpenFilterRequest,
}

impl<'a> FilterRuntimeRegistrationRequest<'a> {
    pub const fn new(filter_id: i32, request: &'a OpenFilterRequest) -> Self {
        Self { filter_id, request }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrRuntimeRegistrationRequest {
    dvr_id: i32,
    kind: DvrKind,
    buffer_size: i32,
    callback_present: bool,
}

impl DvrRuntimeRegistrationRequest {
    pub const fn new(dvr_id: i32, kind: DvrKind, buffer_size: i32, callback_present: bool) -> Self {
        Self {
            dvr_id,
            kind,
            buffer_size,
            callback_present,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FilterRuntimeConfigureRequest {
    filter_id: i32,
    config: crate::config::FilterConfig,
}

impl FilterRuntimeConfigureRequest {
    pub fn new(filter_id: i32, config: crate::config::FilterConfig) -> Self {
        Self { filter_id, config }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrRuntimeConfigureRequest {
    dvr_id: i32,
    status_mask: i32,
    low_threshold_bytes: usize,
    high_threshold_bytes: usize,
}

impl DvrRuntimeConfigureRequest {
    pub const fn new(
        dvr_id: i32,
        status_mask: i32,
        low_threshold_bytes: usize,
        high_threshold_bytes: usize,
    ) -> Self {
        Self {
            dvr_id,
            status_mask,
            low_threshold_bytes,
            high_threshold_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrFilterLinkRequest {
    dvr_id: i32,
    filter_id: i32,
}

impl DvrFilterLinkRequest {
    pub const fn new(dvr_id: i32, filter_id: i32) -> Self {
        Self { dvr_id, filter_id }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrStatusIntervalRuntimeRequest {
    dvr_id: i32,
    interval_ms: u64,
}

impl DvrStatusIntervalRuntimeRequest {
    pub const fn new(dvr_id: i32, interval_ms: u64) -> Self {
        Self {
            dvr_id,
            interval_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterSourceDisconnectRequest {
    sink_filter_id: i32,
}

impl FilterSourceDisconnectRequest {
    pub const fn new(sink_filter_id: i32) -> Self {
        Self { sink_filter_id }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterSourceConnectRequest {
    sink_filter_id: i32,
    source_filter_id: i32,
}

impl FilterSourceConnectRequest {
    pub const fn new(sink_filter_id: i32, source_filter_id: i32) -> Self {
        Self {
            sink_filter_id,
            source_filter_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DemuxRuntimeQuarantineRequest;

impl DemuxRuntimeQuarantineRequest {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ValidatedPacketIngressRequest<'a> {
    validated: &'a crate::packet_pipeline::ValidatedTsPacket<'a>,
    origin: TsInputOrigin,
}

impl<'a> ValidatedPacketIngressRequest<'a> {
    pub const fn new(
        validated: &'a crate::packet_pipeline::ValidatedTsPacket<'a>,
        origin: TsInputOrigin,
    ) -> Self {
        Self { validated, origin }
    }
}

#[derive(Debug)]
pub struct DemuxRuntime {
    demux_id: i32,
    state: DemuxRuntimeState,
    generation: u64,
    pipeline: PacketPipeline,
    filters: BTreeMap<i32, FilterRuntime>,
    dvrs: BTreeMap<i32, DvrRuntime>,
    #[cfg(test)]
    filter_queue_mirror: BTreeMap<i32, VecDeque<Vec<u8>>>,
    filter_queue_runtimes: BTreeMap<i32, QueueRuntime>,
    dvr_queue_runtimes: BTreeMap<i32, QueueRuntime>,
    filter_av_backings: BTreeMap<i32, AvSharedBacking>,
    filter_av_stale_data_ids: BTreeMap<i32, BTreeSet<AvDataId>>,
    rollback_ledger: Arc<Mutex<DemuxRuntimeRollbackLedger>>,
    rollback_token_drop_failure_count: Arc<AtomicU64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaybackConsumeReport {
    pub bytes_read: usize,
    pub completed_packets: usize,
    pub malformed_bytes: usize,
    pub completed_packet_bytes: Vec<[u8; crate::TS_PACKET_SIZE]>,
    pub packet_reports: Vec<PipelineReport>,
    pub data_consumed_wake_failed: bool,
}

#[derive(Debug)]
pub enum QueueDescriptorQueryError {
    FilterMissing(i32),
    DvrMissing(i32),
    InvalidState(i32),
    Unavailable(i32),
    RuntimeMissing(i32),
    Runtime(QueueRuntimeError),
}

impl DemuxRuntime {
    pub fn new(demux_id: i32, generation: u64) -> Self {
        Self {
            demux_id,
            state: DemuxRuntimeState::Open,
            generation,
            pipeline: PacketPipeline::default(),
            filters: BTreeMap::new(),
            dvrs: BTreeMap::new(),
            #[cfg(test)]
            filter_queue_mirror: BTreeMap::new(),
            filter_queue_runtimes: BTreeMap::new(),
            dvr_queue_runtimes: BTreeMap::new(),
            filter_av_backings: BTreeMap::new(),
            filter_av_stale_data_ids: BTreeMap::new(),
            rollback_ledger: Arc::new(Mutex::new(DemuxRuntimeRollbackLedger::default())),
            rollback_token_drop_failure_count: Arc::new(AtomicU64::new(0)),
        }
    }
    pub fn demux_id(&self) -> i32 {
        self.demux_id
    }
    pub fn diagnostic_snapshot(&self) -> DemuxRuntimeDiagnosticSnapshot {
        DemuxRuntimeDiagnosticSnapshot {
            rollback_token_drop_failure_count: self
                .rollback_token_drop_failure_count
                .load(Ordering::Relaxed),
        }
    }

    pub fn state(&self) -> DemuxRuntimeState {
        self.state
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    #[cfg(test)]
    pub(crate) fn pipeline(&self) -> &PacketPipeline {
        &self.pipeline
    }
    #[cfg(test)]
    pub(crate) fn pipeline_mut(&mut self) -> &mut PacketPipeline {
        &mut self.pipeline
    }

    #[cfg(test)]
    pub(crate) fn rollback_snapshot_count_for_test(&self) -> usize {
        self.rollback_ledger
            .lock()
            .map(|ledger| ledger.snapshots.len())
            .unwrap_or(usize::MAX)
    }
    pub(crate) fn filter(&self, filter_id: i32) -> Option<&FilterRuntime> {
        self.filters.get(&filter_id)
    }
    pub(crate) fn filter_mut(&mut self, filter_id: i32) -> Option<&mut FilterRuntime> {
        self.filters.get_mut(&filter_id)
    }
    pub(crate) fn dvr(&self, dvr_id: i32) -> Option<&DvrRuntime> {
        self.dvrs.get(&dvr_id)
    }
    pub(crate) fn dvr_mut(&mut self, dvr_id: i32) -> Option<&mut DvrRuntime> {
        self.dvrs.get_mut(&dvr_id)
    }
    pub fn mark_dvr_callback_unhealthy_from_typed_request(
        &mut self,
        request: DvrRuntimeOperationRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.mark_dvr_callback_unhealthy(request.dvr_id)
    }

    pub(crate) fn mark_dvr_callback_unhealthy(
        &mut self,
        dvr_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        dvr.mark_callback_unhealthy();
        Ok(())
    }
    pub fn mark_filter_callback_unhealthy_from_typed_request(
        &mut self,
        request: FilterRuntimeOperationRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.mark_filter_callback_unhealthy(request.filter_id)
    }

    pub(crate) fn mark_filter_callback_unhealthy(
        &mut self,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        let filter = self
            .filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        filter.mark_callback_unhealthy();
        Ok(())
    }

    pub fn export_filter_av_shared_handle_from_typed_request(
        &mut self,
        request: FilterRuntimeOperationRequest,
    ) -> Result<AvSharedHandleExport, DemuxRuntimeError> {
        self.export_filter_av_shared_handle(request.filter_id)
    }

    pub(crate) fn export_filter_av_shared_handle(
        &mut self,
        filter_id: i32,
    ) -> Result<AvSharedHandleExport, DemuxRuntimeError> {
        let filter = self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        if filter.state().is_closed_or_failed() || !filter.av_backing_present() {
            return Err(DemuxRuntimeError::invalid_state(filter_id));
        }
        self.filter_av_backings
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::av_backing_failure(filter_id))?
            .export_handle()
            .map_err(|_| DemuxRuntimeError::av_backing_failure(filter_id))
    }

    pub fn release_filter_av_handle_from_typed_request(
        &mut self,
        request: FilterAvHandleReleaseRequest,
    ) -> Result<AvHandleReleaseOutcome, DemuxRuntimeError> {
        self.release_filter_av_handle(request.filter_id, request.has_fd, request.av_data_id)
    }

    pub(crate) fn release_filter_av_handle(
        &mut self,
        filter_id: i32,
        has_fd: bool,
        av_data_id: i64,
    ) -> Result<AvHandleReleaseOutcome, DemuxRuntimeError> {
        let filter = self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        let filter_state = if filter.state().is_closed_or_failed() {
            AvFilterReleaseState::Closed
        } else if filter.av_backing_present() {
            AvFilterReleaseState::OpenAv
        } else {
            AvFilterReleaseState::OpenNonAv
        };
        let data_id = AvDataId(av_data_id);
        let known_stale_data_id = self.known_stale_av_data_id(filter_id, data_id);
        if let Some(backing) = self.filter_av_backings.get_mut(&filter_id) {
            let outcome = backing.apply_release(has_fd, data_id, filter_state);
            if outcome == AvHandleReleaseOutcome::UnknownDataId && known_stale_data_id {
                return Ok(AvHandleReleaseOutcome::StaleReleaseAccepted { data_id });
            }
            return Ok(outcome);
        }
        if filter_state == AvFilterReleaseState::OpenAv {
            return Err(DemuxRuntimeError::av_backing_failure(filter_id));
        }
        let fallback_outcome = AvHandleReleaseTxn::classify(AvHandleReleaseInput {
            has_fd,
            data_id,
            client_state: ClientHandleState::NotExported,
            filter_state,
            shared_handle_exported: false,
            data_id_state: if known_stale_data_id {
                AvDataIdState::Stale
            } else {
                AvDataIdState::Unknown
            },
        });
        match fallback_outcome {
            AvHandleReleaseOutcome::ClientHandleReleaseAfterClose
            | AvHandleReleaseOutcome::StaleReleaseAfterClose { .. }
            | AvHandleReleaseOutcome::UnknownDataId
            | AvHandleReleaseOutcome::InvalidStateWithoutSharedHandle
            | AvHandleReleaseOutcome::InvalidDataId
            | AvHandleReleaseOutcome::InvalidHandleForSlotRelease => Ok(fallback_outcome),
            AvHandleReleaseOutcome::UnavailableForNonAvFilter if known_stale_data_id => {
                Ok(AvHandleReleaseOutcome::StaleReleaseAccepted { data_id })
            }
            AvHandleReleaseOutcome::UnavailableForNonAvFilter => Ok(fallback_outcome),
            _ => Err(DemuxRuntimeError::av_backing_failure(filter_id)),
        }
    }

    fn rollback_snapshot(&self) -> DemuxRuntimeRollbackSnapshot {
        DemuxRuntimeRollbackSnapshot {
            state: self.state,
            generation: self.generation,
            filters: self
                .filters
                .iter()
                .map(|(id, runtime)| (*id, runtime.rollback_identity()))
                .collect(),
            dvrs: self
                .dvrs
                .iter()
                .map(|(id, runtime)| (*id, runtime.rollback_identity()))
                .collect(),
            filter_queue_identities: self
                .filter_queue_runtimes
                .iter()
                .map(|(id, queue)| (*id, queue.rollback_identity()))
                .collect(),
            dvr_queue_identities: self
                .dvr_queue_runtimes
                .iter()
                .map(|(id, queue)| (*id, queue.rollback_identity()))
                .collect(),
            filter_av_backing_identities: self
                .filter_av_backings
                .iter()
                .map(|(id, backing)| (*id, backing.rollback_identity()))
                .collect(),
            authorized_post_generation: None,
        }
    }

    pub fn snapshot(&self) -> Result<DemuxRuntimeSnapshot, DemuxRuntimeError> {
        Ok(DemuxRuntimeSnapshot {
            state: self.state,
            generation: self.generation,
            pipeline: self.pipeline.clone(),
            filters: self.filters.clone(),
            dvrs: self.dvrs.clone(),
            filter_queue_identities: self
                .filter_queue_runtimes
                .iter()
                .map(|(id, queue)| (*id, queue.rollback_identity()))
                .collect(),
            dvr_queue_identities: self
                .dvr_queue_runtimes
                .iter()
                .map(|(id, queue)| (*id, queue.rollback_identity()))
                .collect(),
            filter_av_backing_identities: self
                .filter_av_backings
                .iter()
                .map(|(id, backing)| (*id, backing.rollback_identity()))
                .collect(),
            #[cfg(test)]
            filter_queue_mirror: self.filter_queue_mirror.clone(),
        })
    }

    pub fn rollback_token_from_typed_request(
        &mut self,
        request: DemuxRuntimeRollbackTokenPrepareRequest,
    ) -> Result<DemuxRuntimeRollbackToken, DemuxRuntimeError> {
        if request.demux_id != self.demux_id {
            return Err(DemuxRuntimeError::invalid_state(request.demux_id));
        }
        let snapshot = self.rollback_snapshot();
        let generation = snapshot.generation;
        let token_id = {
            let mut ledger = self.rollback_ledger.lock().map_err(|_| {
                DemuxRuntimeError::invalid_state(self.demux_id)
            })?;
            if ledger.active_token_id.is_some() {
                return Err(DemuxRuntimeError::invalid_state(self.demux_id));
            }
            let token_id = ledger.next_token_id;
            ledger.next_token_id = ledger
                .next_token_id
                .checked_add(1)
                .ok_or(DemuxRuntimeError::generation_exhausted(
                    DemuxGenerationTarget::Demux(self.demux_id),
                ))?;
            ledger.snapshots.insert(token_id, snapshot);
            ledger.active_token_id = Some(token_id);
            token_id
        };
        Ok(DemuxRuntimeRollbackToken::new(
            self.demux_id,
            token_id,
            generation,
            Arc::clone(&self.rollback_ledger),
            Arc::clone(&self.rollback_token_drop_failure_count),
        ))
    }

    pub fn authorize_rollback_post_generation(
        &mut self,
        token: &DemuxRuntimeRollbackToken,
        generation: u64,
    ) -> Result<DemuxGenerationBoundaryAuthorization, DemuxRuntimeError> {
        if token.demux_id != self.demux_id
            || !Arc::ptr_eq(&token.ledger, &self.rollback_ledger)
        {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let mut ledger = self
            .rollback_ledger
            .lock()
            .map_err(|_| DemuxRuntimeError::invalid_state(self.demux_id))?;
        if ledger.active_token_id != Some(token.token_id) {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let snapshot = ledger
            .snapshots
            .get_mut(&token.token_id)
            .ok_or(DemuxRuntimeError::invalid_state(self.demux_id))?;
        let expected = snapshot
            .generation
            .checked_add(1)
            .ok_or(DemuxRuntimeError::generation_exhausted(
                DemuxGenerationTarget::Demux(self.demux_id),
            ))?;
        if generation != expected || self.generation != snapshot.generation {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        // Register the transaction-owned next generation before the destructive boundary is
        // applied. Once every bound demux has accepted this provenance, the boundary phase is
        // infallible except for an invariant violation already excluded by this validation.
        snapshot.authorized_post_generation = Some(generation);
        Ok(DemuxGenerationBoundaryAuthorization::new(
            self.demux_id,
            token.token_id,
            generation,
        ))
    }

    pub fn commit_authorized_generation_boundary(
        &mut self,
        authorization: DemuxGenerationBoundaryAuthorization,
        reason: PipelineBoundaryReason,
    ) -> Result<GenerationBoundaryReport, DemuxRuntimeError> {
        if authorization.demux_id != self.demux_id
            || authorization.expected_generation != self.generation.checked_add(1).ok_or_else(|| {
                DemuxRuntimeError::invalid_state(self.demux_id)
            })?
        {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let authorized = self
            .rollback_ledger
            .lock()
            .map_err(|_| DemuxRuntimeError::invalid_state(self.demux_id))?
            .snapshots
            .get(&authorization.token_id)
            .is_some_and(|snapshot| {
                snapshot.authorized_post_generation == Some(authorization.expected_generation)
            });
        if !authorized {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        self.generation = authorization.expected_generation;
        Ok(GenerationBoundaryReport {
            reason,
            reset: self.pipeline.reset_boundary(),
            next_generation: DemuxStreamGeneration(self.generation),
        })
    }

    pub fn matches_rollback_token(
        &self,
        token: &DemuxRuntimeRollbackToken,
    ) -> Result<bool, DemuxRuntimeError> {
        if token.demux_id != self.demux_id
            || !Arc::ptr_eq(&token.ledger, &self.rollback_ledger)
        {
            return Ok(false);
        }
        let ledger = self
            .rollback_ledger
            .lock()
            .map_err(|_| DemuxRuntimeError::invalid_state(self.demux_id))?;
        if ledger.active_token_id != Some(token.token_id) {
            return Ok(false);
        }
        let Some(snapshot) = ledger.snapshots.get(&token.token_id) else {
            return Ok(false);
        };
        let generation_matches = self.generation == token.generation
            || snapshot.authorized_post_generation == Some(self.generation);
        if snapshot.generation != token.generation
            || !generation_matches
            || self.state != snapshot.state
        {
            return Ok(false);
        }
        Ok(self.validate_rollback_identity(snapshot).is_ok())
    }

    pub fn restore_from_rollback_request(
        &mut self,
        request: DemuxRuntimeRollbackRestoreRequest<'_>,
    ) -> Result<(), DemuxRuntimeError> {
        self.restore_from_rollback_token(request.token)
    }

    pub fn commit_rollback_request(
        &mut self,
        request: DemuxRuntimeRollbackCommitRequest,
    ) -> Result<(), DemuxRuntimeError> {
        if request.token.demux_id != self.demux_id
            || !Arc::ptr_eq(&request.token.ledger, &self.rollback_ledger)
        {
            return Err(DemuxRuntimeError::invalid_state(request.token.demux_id));
        }
        request.token.discard_without_runtime()
    }

    pub(crate) fn restore_from_rollback_token(
        &mut self,
        token: &mut DemuxRuntimeRollbackToken,
    ) -> Result<(), DemuxRuntimeError> {
        if token.demux_id != self.demux_id
            || !Arc::ptr_eq(&token.ledger, &self.rollback_ledger)
        {
            return Err(DemuxRuntimeError::invalid_state(token.demux_id));
        }
        let ledger_handle = Arc::clone(&self.rollback_ledger);
        let mut ledger = ledger_handle
            .lock()
            .map_err(|_| DemuxRuntimeError::invalid_state(self.demux_id))?;
        if ledger.active_token_id != Some(token.token_id) {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let snapshot = ledger
            .snapshots
            .get(&token.token_id)
            .cloned()
            .ok_or(DemuxRuntimeError::invalid_state(self.demux_id))?;
        if snapshot.generation != token.generation {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let generation_matches = self.generation == token.generation
            || snapshot.authorized_post_generation == Some(self.generation);
        if self.state != snapshot.state
            || self.state != DemuxRuntimeState::Open
            || !generation_matches
        {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        self.validate_rollback_identity(&snapshot)?;
        // Rollback only the control-plane generation/lifecycle. Data-plane assembler,
        // FMQ pointer/content, AV client slot state and stale-ID bookkeeping may have
        // progressed legitimately while the runtime lock was released and must not be
        // overwritten by a stale external-join token.
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        if ledger.snapshots.remove(&token.token_id).is_none() {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        ledger.active_token_id = None;
        token.disarm();
        Ok(())
    }

    pub(crate) fn restore(
        &mut self,
        snapshot: DemuxRuntimeSnapshot,
    ) -> Result<(), DemuxRuntimeError> {
        self.validate_runtime_identity_for_snapshot(&snapshot)?;
        self.filter_queue_runtimes
            .retain(|filter_id, _| snapshot.filter_queue_identities.contains_key(filter_id));
        self.dvr_queue_runtimes
            .retain(|dvr_id, _| snapshot.dvr_queue_identities.contains_key(dvr_id));
        self.filter_av_backings.retain(|filter_id, _| {
            snapshot.filter_av_backing_identities.contains_key(filter_id)
        });
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.pipeline
            .restore_control_plane_preserving_data_plane(&snapshot.pipeline);
        for (filter_id, filter_snapshot) in snapshot.filters {
            let filter = self
                .filters
                .get_mut(&filter_id)
                .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
            filter.restore_control_plane_preserving_volatile(filter_snapshot.snapshot());
        }
        for (dvr_id, dvr_snapshot) in snapshot.dvrs {
            let dvr = self
                .dvrs
                .get_mut(&dvr_id)
                .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
            dvr.restore_control_plane_preserving_volatile(dvr_snapshot.snapshot());
        }
        #[cfg(test)]
        {
            self.filter_queue_mirror = snapshot.filter_queue_mirror;
        }
        // AV stale-ID bookkeeping is data-plane state. Preserve current progress instead of
        // replacing it with the pre-transaction snapshot.
        Ok(())
    }

    fn validate_rollback_identity(
        &self,
        snapshot: &DemuxRuntimeRollbackSnapshot,
    ) -> Result<(), DemuxRuntimeError> {
        let filter_identities: BTreeMap<i32, FilterRuntimeRollbackIdentity> = self
            .filters
            .iter()
            .map(|(id, runtime)| (*id, runtime.rollback_identity()))
            .collect();
        if filter_identities != snapshot.filters {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let dvr_identities: BTreeMap<i32, DvrRuntimeRollbackIdentity> = self
            .dvrs
            .iter()
            .map(|(id, runtime)| (*id, runtime.rollback_identity()))
            .collect();
        if dvr_identities != snapshot.dvrs {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let filter_queue_identities: BTreeMap<i32, u64> = self
            .filter_queue_runtimes
            .iter()
            .map(|(id, queue)| (*id, queue.rollback_identity()))
            .collect();
        if filter_queue_identities != snapshot.filter_queue_identities {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let dvr_queue_identities: BTreeMap<i32, u64> = self
            .dvr_queue_runtimes
            .iter()
            .map(|(id, queue)| (*id, queue.rollback_identity()))
            .collect();
        if dvr_queue_identities != snapshot.dvr_queue_identities {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let filter_av_backing_identities: BTreeMap<i32, u64> = self
            .filter_av_backings
            .iter()
            .map(|(id, backing)| (*id, backing.rollback_identity()))
            .collect();
        if filter_av_backing_identities != snapshot.filter_av_backing_identities {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        Ok(())
    }

    fn validate_runtime_identity_for_snapshot(
        &self,
        snapshot: &DemuxRuntimeSnapshot,
    ) -> Result<(), DemuxRuntimeError> {
        if self.filters.keys().ne(snapshot.filters.keys())
            || self.dvrs.keys().ne(snapshot.dvrs.keys())
        {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        for (filter_id, expected_identity) in &snapshot.filter_queue_identities {
            let Some(queue) = self.filter_queue_runtimes.get(filter_id) else {
                return Err(DemuxRuntimeError::queue_runtime_failure(*filter_id));
            };
            if queue.rollback_identity() != *expected_identity {
                return Err(DemuxRuntimeError::queue_runtime_failure(*filter_id));
            }
        }
        for (dvr_id, expected_identity) in &snapshot.dvr_queue_identities {
            let Some(queue) = self.dvr_queue_runtimes.get(dvr_id) else {
                return Err(DemuxRuntimeError::queue_runtime_failure(*dvr_id));
            };
            if queue.rollback_identity() != *expected_identity {
                return Err(DemuxRuntimeError::queue_runtime_failure(*dvr_id));
            }
        }
        for (filter_id, expected_identity) in &snapshot.filter_av_backing_identities {
            let Some(backing) = self.filter_av_backings.get(filter_id) else {
                return Err(DemuxRuntimeError::av_backing_failure(*filter_id));
            };
            if backing.rollback_identity() != *expected_identity {
                return Err(DemuxRuntimeError::av_backing_failure(*filter_id));
            }
        }
        Ok(())
    }

    pub(crate) fn register_filter(
        &mut self,
        filter: FilterRuntime,
    ) -> Result<(), DemuxRuntimeError> {
        if filter.state().is_closed_or_failed() {
            return Err(DemuxRuntimeError::invalid_state(filter.filter_id()));
        }
        let filter_id = filter.filter_id();
        // Initial registration precedes descriptor export, so this is the only production point
        // where a normal filter FMQ may be created. Configure/source-boundary repair paths must
        // preserve the existing instance identity and therefore never create a replacement.
        let queue_runtime = if Self::should_keep_filter_queue_runtime(&filter) {
            Some(
                QueueRuntime::new_writer(filter.buffer_size(), true)
                    .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?,
            )
        } else {
            None
        };
        self.filter_queue_runtimes.remove(&filter_id);
        #[cfg(test)]
        {
            self.filter_queue_mirror.remove(&filter_id);
        }
        self.filter_av_backings.remove(&filter_id);
        self.filter_av_stale_data_ids.remove(&filter_id);
        self.filters.insert(filter_id, filter);
        if let Some(queue_runtime) = queue_runtime {
            self.filter_queue_runtimes.insert(filter_id, queue_runtime);
        }
        #[cfg(test)]
        {
            if self
                .filters
                .get(&filter_id)
                .is_some_and(FilterRuntime::queue_present)
            {
                self.filter_queue_mirror.entry(filter_id).or_default();
            }
        }
        Ok(())
    }

    pub fn register_filter_from_typed_request(
        &mut self,
        request: FilterRuntimeRegistrationRequest<'_>,
    ) -> Result<(), DemuxRuntimeError> {
        self.register_filter_from_open_request(request.filter_id, request.request)
    }

    pub(crate) fn register_filter_from_open_request(
        &mut self,
        filter_id: i32,
        request: &OpenFilterRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.register_filter(FilterRuntime::new_open_request(
            filter_id,
            self.generation(),
            request,
        ))
    }

    pub fn remove_filter_from_typed_request(
        &mut self,
        request: FilterRuntimeOperationRequest,
    ) -> Result<FilterRuntimeSnapshot, DemuxRuntimeError> {
        self.remove_filter(request.filter_id)
    }

    pub(crate) fn remove_filter(
        &mut self,
        filter_id: i32,
    ) -> Result<FilterRuntimeSnapshot, DemuxRuntimeError> {
        if !self.filters.contains_key(&filter_id) {
            return Err(DemuxRuntimeError::filter_missing(filter_id));
        }
        self.pipeline.remove_filter(filter_id);
        self.filter_queue_runtimes.remove(&filter_id);
        #[cfg(test)]
        {
            self.filter_queue_mirror.remove(&filter_id);
        }
        self.filter_av_backings.remove(&filter_id);
        self.filter_av_stale_data_ids.remove(&filter_id);
        self.filters
            .remove(&filter_id)
            .map(|filter| filter.snapshot())
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))
    }

    pub(crate) fn register_dvr(&mut self, dvr: DvrRuntime) -> Result<(), DemuxRuntimeError> {
        if dvr.state().is_closed_or_failed() {
            return Err(DemuxRuntimeError::invalid_state(dvr.dvr_id()));
        }
        let dvr_id = dvr.dvr_id();
        // DVR FMQ creation is restricted to initial registration, before getQueueDesc() can
        // export the descriptor. Later configure/repair paths preserve this exact instance.
        let queue_runtime = Self::new_queue_runtime_for_dvr(&dvr)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        self.dvr_queue_runtimes.remove(&dvr_id);
        self.dvrs.insert(dvr_id, dvr);
        self.dvr_queue_runtimes.insert(dvr_id, queue_runtime);
        Ok(())
    }

    pub fn register_dvr_from_typed_request(
        &mut self,
        request: DvrRuntimeRegistrationRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.register_dvr_from_open_request(
            request.dvr_id,
            request.kind,
            request.buffer_size,
            request.callback_present,
        )
    }

    pub(crate) fn register_dvr_from_open_request(
        &mut self,
        dvr_id: i32,
        kind: DvrKind,
        buffer_size: i32,
        callback_present: bool,
    ) -> Result<(), DemuxRuntimeError> {
        self.register_dvr(DvrRuntime::new_open_request(
            dvr_id,
            kind,
            self.generation(),
            buffer_size,
            callback_present,
        ))
    }

    pub fn remove_dvr_from_typed_request(
        &mut self,
        request: DvrRuntimeOperationRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.remove_dvr(request.dvr_id)
    }

    pub(crate) fn remove_dvr(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        if !self.dvrs.contains_key(&dvr_id) {
            return Err(DemuxRuntimeError::dvr_missing(dvr_id));
        }
        self.dvr_queue_runtimes.remove(&dvr_id);
        self.dvrs.remove(&dvr_id);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn create_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let filter = self
            .filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        let mut snapshot = filter.snapshot();
        snapshot.queue_present = true;
        filter.restore(snapshot);
        #[cfg(test)]
        {
            self.filter_queue_mirror.entry(filter_id).or_default();
        }
        let filter = self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        let queue_runtime = QueueRuntime::new_writer(filter.buffer_size(), true)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        self.filter_queue_runtimes.insert(filter_id, queue_runtime);
        Ok(())
    }

    pub fn queue_exists(&self, filter_id: i32) -> bool {
        self.filters
            .get(&filter_id)
            .is_some_and(FilterRuntime::queue_present)
    }

    pub(crate) fn clear_existing_filter_queue(
        &mut self,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        if !self.queue_exists(filter_id) {
            return Err(DemuxRuntimeError::queue_missing(filter_id));
        }
        self.clear_filter_queue_runtime(filter_id)?;
        #[cfg(test)]
        {
            if let Some(queue) = self.filter_queue_mirror.get_mut(&filter_id) {
                queue.clear();
            }
        }
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.clear_queued_payload_state();
        }
        Ok(())
    }

    pub(crate) fn enqueue_filter_queue_payload(
        &mut self,
        filter_id: i32,
        payload: Vec<u8>,
    ) -> Result<(), DemuxRuntimeError> {
        if !self.filters.contains_key(&filter_id) {
            return Err(DemuxRuntimeError::filter_missing(filter_id));
        }
        let Some(queue) = self.filter_queue_runtimes.get(&filter_id) else {
            return Err(DemuxRuntimeError::queue_missing(filter_id));
        };
        let available = queue
            .available_to_write()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        let result = if available < payload.len() {
            FmqDeliveryTxn::new(FmqObjectKind::Filter).overflow()
        } else {
            FmqDeliveryTxn::new(FmqObjectKind::Filter).commit_payload(
                payload.len(),
                queue
                    .write_checked(&payload)
                    .map_err(|_| FmqFailureKind::WriteFailed),
                queue
                    .wake(TUNER_EVENT_DATA_READY)
                    .map_err(|_| FmqFailureKind::EventFlagWakeFailed),
            )
        };
        match result.action {
            FmqDeliveryAction::Continue => {
                #[cfg(test)]
                {
                    self.filter_queue_mirror
                        .entry(filter_id)
                        .or_default()
                        .push_back(payload);
                }
                if let Some(filter) = self.filters.get_mut(&filter_id) {
                    filter.note_payload_queued(result.bytes);
                }
                Ok(())
            }
            FmqDeliveryAction::Overflow => Err(DemuxRuntimeError::queue_runtime_failure(filter_id)),
            FmqDeliveryAction::RuntimeFailed(_) => {
                if let Some(filter) = self.filters.get_mut(&filter_id) {
                    filter.mark_failed();
                }
                Err(DemuxRuntimeError::queue_runtime_failure(filter_id))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn filter_delivery_readiness_for_test(
        &self,
        filter_id: i32,
    ) -> Result<FilterDelayReadiness, DemuxRuntimeError> {
        self.filters
            .get(&filter_id)
            .map(FilterRuntime::delivery_readiness)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))
    }

    #[cfg(test)]
    pub(crate) fn drain_filter_queue_for_delivery_for_test(
        &mut self,
        filter_id: i32,
    ) -> Result<Vec<Vec<u8>>, DemuxRuntimeError> {
        let readiness = self.filter_delivery_readiness_for_test(filter_id)?;
        let filter = self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        if !filter.state().is_started() || readiness != FilterDelayReadiness::Ready {
            return Ok(Vec::new());
        }
        let queue = self
            .filter_queue_runtimes
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        let available = queue
            .current_fill()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        if available == 0 {
            return Ok(Vec::new());
        }
        let mut drained = vec![0u8; available];
        let read = queue
            .peer_read_for_test(&mut drained)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        drained.truncate(read);
        let filter = self
            .filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        filter.clear_queued_payload_state();
        #[cfg(test)]
        {
            if let Some(queue) = self.filter_queue_mirror.get_mut(&filter_id) {
                let drained = queue.drain(..).collect();
                return Ok(drained);
            }
        }
        Ok(vec![drained])
    }

    #[cfg(test)]
    pub(crate) fn snapshot_filter_queue_bytes_for_test(&self, filter_id: i32) -> Option<Vec<u8>> {
        let queue = self.filter_queue_mirror.get(&filter_id)?;
        let mut out = Vec::new();
        for payload in queue {
            out.extend_from_slice(payload);
        }
        Some(out)
    }

    #[cfg(test)]
    pub(crate) fn mark_filter_av_shared_handle_exported_for_test(
        &mut self,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        self.filter_av_backings
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::av_backing_failure(filter_id))?
            .mark_exported();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn allocate_filter_av_payload_for_test(
        &mut self,
        filter_id: i32,
        data_length: usize,
    ) -> Result<AvPayloadDeliveryOutcome, DemuxRuntimeError> {
        self.filter_av_backings
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::av_backing_failure(filter_id))
            .map(|backing| backing.allocate_payload(data_length))
    }

    #[cfg(test)]
    pub(crate) fn filter_av_active_slot_count_for_test(&self, filter_id: i32) -> Option<usize> {
        self.filter_av_backings
            .get(&filter_id)
            .map(AvSharedBacking::active_slot_count)
    }

    #[cfg(test)]
    pub(crate) fn remove_filter_av_backing_for_test(&mut self, filter_id: i32) -> bool {
        self.filter_av_backings.remove(&filter_id).is_some()
    }

    pub fn filter_snapshot(
        &self,
        filter_id: i32,
    ) -> Result<FilterRuntimeSnapshot, DemuxRuntimeError> {
        self.filters
            .get(&filter_id)
            .map(FilterRuntime::snapshot)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))
    }

    pub fn dvr_snapshot(&self, dvr_id: i32) -> Result<DvrRuntimeSnapshot, DemuxRuntimeError> {
        self.dvrs
            .get(&dvr_id)
            .map(DvrRuntime::snapshot)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))
    }

    pub(crate) fn restore_filter_snapshot(
        &mut self,
        filter_id: i32,
        snapshot: FilterRuntimeSnapshot,
    ) -> Result<(), DemuxRuntimeError> {
        if snapshot.queue_present {
            let queue = self
                .filter_queue_runtimes
                .get(&filter_id)
                .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
            if !queue.is_hal_writer() || !queue.capacity_matches_buffer_size(snapshot.buffer_size) {
                return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
            }
        } else {
            self.filter_queue_runtimes.remove(&filter_id);
        }
        self.filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?
            .restore(snapshot);
        Ok(())
    }

    pub(crate) fn restore_dvr_snapshot(
        &mut self,
        dvr_id: i32,
        snapshot: DvrRuntimeSnapshot,
    ) -> Result<(), DemuxRuntimeError> {
        if snapshot.queue_present {
            let queue = self
                .dvr_queue_runtimes
                .get(&dvr_id)
                .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?;
            let role_matches = match snapshot.kind {
                DvrKind::Record => queue.is_hal_writer(),
                DvrKind::Playback => queue.is_hal_reader(),
            };
            if !role_matches || !queue.capacity_matches_buffer_size(snapshot.buffer_size) {
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        } else {
            self.dvr_queue_runtimes.remove(&dvr_id);
        }
        self.dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?
            .restore(snapshot);
        Ok(())
    }

    pub(crate) fn configure_dvr_status_reporting(
        &mut self,
        dvr_id: i32,
        status_mask: i32,
        low_threshold_bytes: usize,
        high_threshold_bytes: usize,
    ) -> Result<(), DemuxRuntimeError> {
        self.dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?
            .configure_status_reporting(status_mask, low_threshold_bytes, high_threshold_bytes);
        Ok(())
    }

    pub(crate) fn configure_filter_runtime(
        &mut self,
        filter_id: i32,
        config: FilterPipelineConfig,
    ) -> Result<(), DemuxRuntimeError> {
        enum QueueConfigureAction {
            Remove,
            Reuse,
        }

        let (next, queue_action, av_backing_present) = {
            let filter = self
                .filters
                .get(&filter_id)
                .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
            let next = match next_generation(filter.generation()) {
                Ok(next) => next,
                Err(_) => {
                    self.quarantine();
                    return Err(DemuxRuntimeError::generation_exhausted(
                        DemuxGenerationTarget::Filter(filter_id),
                    ));
                }
            };
            let queue_action = if filter.supports_normal_fmq_queue() && filter.buffer_size() > 0 {
                let queue = self
                    .filter_queue_runtimes
                    .get(&filter_id)
                    .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
                if !queue.is_hal_writer()
                    || !queue.capacity_matches_buffer_size(filter.buffer_size())
                    || queue
                        .current_fill()
                        .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?
                        != 0
                {
                    return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
                }
                QueueConfigureAction::Reuse
            } else {
                QueueConfigureAction::Remove
            };
            (
                next,
                queue_action,
                matches!(filter.open_kind(), PipelineOpenKind::Av),
            )
        };
        self.pipeline.configure_filter(filter_id, config.clone());
        let filter = self
            .filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        filter.configure_with_generation(next, config);
        filter.clear_queued_payload_state();
        #[cfg(test)]
        {
            if self
                .filters
                .get(&filter_id)
                .is_some_and(FilterRuntime::queue_present)
            {
                self.filter_queue_mirror.entry(filter_id).or_default();
            } else {
                self.filter_queue_mirror.remove(&filter_id);
            }
        }
        if av_backing_present {
            self.filter_av_backings
                .entry(filter_id)
                .or_insert_with(AvSharedBacking::default);
        } else {
            self.drop_filter_av_backing_to_stale(filter_id);
        }
        match queue_action {
            QueueConfigureAction::Remove => {
                self.filter_queue_runtimes.remove(&filter_id);
            }
            QueueConfigureAction::Reuse => {}
        }
        Ok(())
    }

    pub fn configure_filter_runtime_with_typed_request(
        &mut self,
        request: FilterRuntimeConfigureRequest,
    ) -> (
        super::configure_txn::FilterConfigureReport,
        Result<super::configure_txn::FilterConfigureOutcome, DemuxRuntimeError>,
    ) {
        super::configure_txn::configure_filter_runtime(self, request.filter_id, request.config)
    }

    pub fn start_filter_runtime_from_typed_request(
        &mut self,
        request: FilterRuntimeOperationRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.start_filter_runtime(request.filter_id)
    }

    pub(crate) fn start_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let snapshot = self.filter_snapshot(filter_id)?;
        match snapshot.state {
            FilterRuntimeState::Configured | FilterRuntimeState::Stopped => {
                if snapshot.queue_present && !self.filter_queue_runtimes.contains_key(&filter_id) {
                    return Err(DemuxRuntimeError::queue_missing(filter_id));
                }
                let filter = self
                    .filters
                    .get_mut(&filter_id)
                    .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
                filter.mark_started();
                Ok(())
            }
            FilterRuntimeState::Started => Ok(()),
            FilterRuntimeState::Open => Err(DemuxRuntimeError::invalid_state(filter_id)),
            FilterRuntimeState::Closing
            | FilterRuntimeState::CleanupFailed
            | FilterRuntimeState::Closed
            | FilterRuntimeState::Failed => Err(DemuxRuntimeError::sink_lifecycle(filter_id)),
        }
    }

    pub fn stop_filter_runtime_with_typed_request(
        &mut self,
        request: FilterRuntimeOperationRequest,
    ) -> (FilterRuntimeOperationReport, Result<(), DemuxRuntimeError>) {
        self.stop_filter_runtime_report(request.filter_id)
    }

    #[cfg(test)]
    pub(crate) fn stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        self.stop_filter_runtime_report(filter_id).1
    }

    fn stop_filter_runtime_report(
        &mut self,
        filter_id: i32,
    ) -> (FilterRuntimeOperationReport, Result<(), DemuxRuntimeError>) {
        let mut report =
            FilterRuntimeOperationReport::new(FilterRuntimeOperationKind::Stop, filter_id);
        let snapshot = match self.filter_snapshot(filter_id) {
            Ok(snapshot) => {
                report.succeeded(FilterRuntimeOperationStep::ValidateState);
                snapshot
            }
            Err(error) => {
                report.failed(FilterRuntimeOperationStep::ValidateState, error);
                report.finish(FilterRuntimeOperationOutcome::Failed {
                    failed_step: FilterRuntimeOperationStep::ValidateState,
                });
                return (report, Err(error));
            }
        };
        match snapshot.state {
            FilterRuntimeState::Started => {
                self.pipeline.stop_filter(filter_id);
                report.succeeded(FilterRuntimeOperationStep::PipelineStop);
                if snapshot.queue_present {
                    if let Err(error) = self.clear_filter_queue_runtime(filter_id) {
                        report.failed(FilterRuntimeOperationStep::QueueClear, error);
                        report.skipped(
                            FilterRuntimeOperationStep::MirrorQueueClear,
                            FilterRuntimeOperationSkipReason::QueueClearFailed,
                        );
                        report.skipped(
                            FilterRuntimeOperationStep::QueuedPayloadClear,
                            FilterRuntimeOperationSkipReason::QueueClearFailed,
                        );
                        report.skipped(
                            FilterRuntimeOperationStep::MarkStopped,
                            FilterRuntimeOperationSkipReason::QueueClearFailed,
                        );
                        self.quarantine();
                        report.finish(FilterRuntimeOperationOutcome::PartialEffectQuarantined {
                            failed_step: FilterRuntimeOperationStep::QueueClear,
                            partial_effect_step: FilterRuntimeOperationStep::QueueClear,
                        });
                        return (report, Err(error));
                    }
                    report.succeeded(FilterRuntimeOperationStep::QueueClear);
                } else {
                    report.skipped(
                        FilterRuntimeOperationStep::QueueClear,
                        FilterRuntimeOperationSkipReason::QueueNotPresent,
                    );
                }
                #[cfg(test)]
                {
                    if let Some(queue) = self.filter_queue_mirror.get_mut(&filter_id) {
                        queue.clear();
                        report.succeeded(FilterRuntimeOperationStep::MirrorQueueClear);
                    } else {
                        report.skipped(
                            FilterRuntimeOperationStep::MirrorQueueClear,
                            FilterRuntimeOperationSkipReason::QueueNotPresent,
                        );
                    }
                }
                let filter = match self.filters.get_mut(&filter_id) {
                    Some(filter) => filter,
                    None => {
                        let error = DemuxRuntimeError::filter_missing(filter_id);
                        report.failed(FilterRuntimeOperationStep::QueuedPayloadClear, error);
                        report.finish(FilterRuntimeOperationOutcome::Failed {
                            failed_step: FilterRuntimeOperationStep::QueuedPayloadClear,
                        });
                        return (report, Err(error));
                    }
                };
                filter.clear_queued_payload_state();
                report.succeeded(FilterRuntimeOperationStep::QueuedPayloadClear);
                filter.mark_stopped();
                report.succeeded(FilterRuntimeOperationStep::MarkStopped);
                report.finish(FilterRuntimeOperationOutcome::Committed);
                (report, Ok(()))
            }
            FilterRuntimeState::Configured | FilterRuntimeState::Stopped => {
                report.skipped(
                    FilterRuntimeOperationStep::PipelineStop,
                    FilterRuntimeOperationSkipReason::AlreadyStoppedOrConfigured,
                );
                report.finish(FilterRuntimeOperationOutcome::Noop);
                (report, Ok(()))
            }
            FilterRuntimeState::Open => {
                report.skipped(
                    FilterRuntimeOperationStep::PipelineStop,
                    FilterRuntimeOperationSkipReason::OpenStateNoop,
                );
                report.finish(FilterRuntimeOperationOutcome::Noop);
                (report, Ok(()))
            }
            FilterRuntimeState::Closing
            | FilterRuntimeState::CleanupFailed
            | FilterRuntimeState::Closed
            | FilterRuntimeState::Failed => {
                let error = DemuxRuntimeError::sink_lifecycle(filter_id);
                report.failed(FilterRuntimeOperationStep::ValidateState, error);
                report.finish(FilterRuntimeOperationOutcome::Failed {
                    failed_step: FilterRuntimeOperationStep::ValidateState,
                });
                (report, Err(error))
            }
        }
    }

    pub fn flush_filter_runtime_with_typed_request(
        &mut self,
        request: FilterRuntimeOperationRequest,
    ) -> (FilterRuntimeOperationReport, Result<(), DemuxRuntimeError>) {
        self.flush_filter_runtime_report(request.filter_id)
    }

    #[cfg(test)]
    pub(crate) fn flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        self.flush_filter_runtime_report(filter_id).1
    }

    fn flush_filter_runtime_report(
        &mut self,
        filter_id: i32,
    ) -> (FilterRuntimeOperationReport, Result<(), DemuxRuntimeError>) {
        let mut report =
            FilterRuntimeOperationReport::new(FilterRuntimeOperationKind::Flush, filter_id);
        let snapshot = match self.filter_snapshot(filter_id) {
            Ok(snapshot) => {
                report.succeeded(FilterRuntimeOperationStep::ValidateState);
                snapshot
            }
            Err(error) => {
                report.failed(FilterRuntimeOperationStep::ValidateState, error);
                report.finish(FilterRuntimeOperationOutcome::Failed {
                    failed_step: FilterRuntimeOperationStep::ValidateState,
                });
                return (report, Err(error));
            }
        };
        match snapshot.state {
            FilterRuntimeState::Configured
            | FilterRuntimeState::Started
            | FilterRuntimeState::Stopped => {
                if let Some(tpid) = snapshot.tpid {
                    let origins = [(snapshot.source.origin(), tpid)];
                    self.pipeline.flush_filter(filter_id, &origins);
                } else {
                    self.pipeline.clear_filter_state_after_flush(filter_id);
                }
                report.succeeded(FilterRuntimeOperationStep::PipelineFlush);
                if snapshot.queue_present {
                    if let Err(error) = self.clear_filter_queue_runtime(filter_id) {
                        report.failed(FilterRuntimeOperationStep::QueueClear, error);
                        report.skipped(
                            FilterRuntimeOperationStep::MirrorQueueClear,
                            FilterRuntimeOperationSkipReason::QueueClearFailed,
                        );
                        report.skipped(
                            FilterRuntimeOperationStep::QueuedPayloadClear,
                            FilterRuntimeOperationSkipReason::QueueClearFailed,
                        );
                        report.skipped(
                            FilterRuntimeOperationStep::AvBackingFlush,
                            FilterRuntimeOperationSkipReason::QueueClearFailed,
                        );
                        self.quarantine();
                        report.finish(FilterRuntimeOperationOutcome::PartialEffectQuarantined {
                            failed_step: FilterRuntimeOperationStep::QueueClear,
                            partial_effect_step: FilterRuntimeOperationStep::QueueClear,
                        });
                        return (report, Err(error));
                    }
                    report.succeeded(FilterRuntimeOperationStep::QueueClear);
                } else {
                    report.skipped(
                        FilterRuntimeOperationStep::QueueClear,
                        FilterRuntimeOperationSkipReason::QueueNotPresent,
                    );
                }
                #[cfg(test)]
                {
                    if let Some(queue) = self.filter_queue_mirror.get_mut(&filter_id) {
                        queue.clear();
                        report.succeeded(FilterRuntimeOperationStep::MirrorQueueClear);
                    } else {
                        report.skipped(
                            FilterRuntimeOperationStep::MirrorQueueClear,
                            FilterRuntimeOperationSkipReason::QueueNotPresent,
                        );
                    }
                }
                if let Some(filter) = self.filters.get_mut(&filter_id) {
                    filter.clear_queued_payload_state();
                    report.succeeded(FilterRuntimeOperationStep::QueuedPayloadClear);
                } else {
                    report.skipped(
                        FilterRuntimeOperationStep::QueuedPayloadClear,
                        FilterRuntimeOperationSkipReason::FilterMissingForOptionalFlush,
                    );
                }
                if let Some(backing) = self.filter_av_backings.get_mut(&filter_id) {
                    backing.flush_slots_keep_exported_handle();
                    report.succeeded(FilterRuntimeOperationStep::AvBackingFlush);
                } else {
                    report.skipped(
                        FilterRuntimeOperationStep::AvBackingFlush,
                        FilterRuntimeOperationSkipReason::AvBackingNotPresent,
                    );
                }
                report.finish(FilterRuntimeOperationOutcome::Committed);
                (report, Ok(()))
            }
            FilterRuntimeState::Open => {
                let error = DemuxRuntimeError::invalid_state(filter_id);
                report.failed(FilterRuntimeOperationStep::ValidateState, error);
                report.finish(FilterRuntimeOperationOutcome::Failed {
                    failed_step: FilterRuntimeOperationStep::ValidateState,
                });
                (report, Err(error))
            }
            FilterRuntimeState::Closing
            | FilterRuntimeState::CleanupFailed
            | FilterRuntimeState::Closed
            | FilterRuntimeState::Failed => {
                let error = DemuxRuntimeError::sink_lifecycle(filter_id);
                report.failed(FilterRuntimeOperationStep::ValidateState, error);
                report.finish(FilterRuntimeOperationOutcome::Failed {
                    failed_step: FilterRuntimeOperationStep::ValidateState,
                });
                (report, Err(error))
            }
        }
    }

    pub fn configure_filter_av_stream_type_from_typed_request(
        &mut self,
        request: FilterAvStreamTypeRuntimeRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.configure_filter_av_stream_type(request.filter_id, request.config)
    }

    pub(crate) fn configure_filter_av_stream_type(
        &mut self,
        filter_id: i32,
        config: AvStreamTypeConfig,
    ) -> Result<(), DemuxRuntimeError> {
        self.filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?
            .set_av_stream_type_hint(config);
        if let Some(backing) = self.filter_av_backings.get_mut(&filter_id) {
            backing.flush_slots_keep_exported_handle();
        }
        Ok(())
    }

    pub fn set_filter_delay_hint_from_typed_request(
        &mut self,
        request: FilterDelayHintRuntimeRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.set_filter_delay_hint(request.filter_id, request.hint)
    }

    pub(crate) fn set_filter_delay_hint(
        &mut self,
        filter_id: i32,
        hint: FilterDelayHint,
    ) -> Result<(), DemuxRuntimeError> {
        if matches!(hint, FilterDelayHint::TimeDelayMs(ms) if ms > MAX_FILTER_DELAY_MS) {
            return Err(DemuxRuntimeError::invalid_state(filter_id));
        }
        self.filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?
            .set_delay_hint(hint);
        Ok(())
    }

    pub(crate) fn configure_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let next = {
            let dvr = self
                .dvrs
                .get(&dvr_id)
                .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
            let next = match next_generation(dvr.generation()) {
                Ok(next) => next,
                Err(_) => {
                    self.quarantine();
                    return Err(DemuxRuntimeError::generation_exhausted(
                        DemuxGenerationTarget::Dvr(dvr_id),
                    ));
                }
            };
            let queue = self
                .dvr_queue_runtimes
                .get(&dvr_id)
                .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?;
            let role_matches = match dvr.kind() {
                DvrKind::Record => queue.is_hal_writer(),
                DvrKind::Playback => queue.is_hal_reader(),
            };
            if !role_matches
                || !queue.capacity_matches_buffer_size(dvr.buffer_size())
                || queue
                    .current_fill()
                    .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?
                    != 0
            {
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
            next
        };
        self.dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?
            .configure_with_generation(next);
        Ok(())
    }

    pub fn configure_dvr_runtime_with_typed_request(
        &mut self,
        request: DvrRuntimeConfigureRequest,
    ) -> (
        super::configure_txn::DvrConfigureReport,
        Result<super::configure_txn::DvrConfigureOutcome, DemuxRuntimeError>,
    ) {
        super::configure_txn::configure_dvr_runtime(
            self,
            request.dvr_id,
            request.status_mask,
            request.low_threshold_bytes,
            request.high_threshold_bytes,
        )
    }

    pub fn attach_dvr_filter_from_typed_request(
        &mut self,
        request: DvrFilterLinkRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.attach_dvr_filter(request.dvr_id, request.filter_id)
    }

    pub(crate) fn attach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        if dvr.kind() != DvrKind::Record {
            return Err(DemuxRuntimeError::unsupported_dvr_operation(dvr_id));
        }
        let filter = self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        if filter.state().is_closed_or_failed() || filter.open_kind() != PipelineOpenKind::Record {
            return Err(DemuxRuntimeError::invalid_dvr_filter(filter_id));
        }
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        match dvr.state() {
            super::dvr::DvrRuntimeState::Configured
            | super::dvr::DvrRuntimeState::Started
            | super::dvr::DvrRuntimeState::Stopped => {
                dvr.attach_record_filter(filter_id);
                Ok(())
            }
            super::dvr::DvrRuntimeState::Open
            | super::dvr::DvrRuntimeState::Closing
            | super::dvr::DvrRuntimeState::CleanupFailed
            | super::dvr::DvrRuntimeState::Closed
            | super::dvr::DvrRuntimeState::Failed => Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
    }

    pub fn detach_dvr_filter_from_typed_request(
        &mut self,
        request: DvrFilterLinkRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.detach_dvr_filter(request.dvr_id, request.filter_id)
    }

    pub(crate) fn detach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        if dvr.kind() != DvrKind::Record {
            return Err(DemuxRuntimeError::unsupported_dvr_operation(dvr_id));
        }
        self.filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        match dvr.state() {
            super::dvr::DvrRuntimeState::Configured
            | super::dvr::DvrRuntimeState::Started
            | super::dvr::DvrRuntimeState::Stopped => {
                dvr.detach_record_filter(filter_id);
                Ok(())
            }
            super::dvr::DvrRuntimeState::Open
            | super::dvr::DvrRuntimeState::Closing
            | super::dvr::DvrRuntimeState::CleanupFailed
            | super::dvr::DvrRuntimeState::Closed
            | super::dvr::DvrRuntimeState::Failed => Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
    }

    pub fn start_dvr_runtime_from_typed_request(
        &mut self,
        request: DvrRuntimeOperationRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.start_dvr_runtime(request.dvr_id)
    }

    pub(crate) fn start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let snapshot = self.dvr_snapshot(dvr_id)?;
        if snapshot.callback_unhealthy {
            return Err(DemuxRuntimeError::invalid_state(dvr_id));
        }
        match snapshot.state {
            super::dvr::DvrRuntimeState::Configured | super::dvr::DvrRuntimeState::Stopped => {
                if snapshot.queue_present && !self.dvr_queue_runtimes.contains_key(&dvr_id) {
                    return Err(DemuxRuntimeError::queue_missing(dvr_id));
                }
                let dvr = self
                    .dvrs
                    .get_mut(&dvr_id)
                    .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
                dvr.mark_started();
                Ok(())
            }
            super::dvr::DvrRuntimeState::Started => Ok(()),
            super::dvr::DvrRuntimeState::Open => Err(DemuxRuntimeError::invalid_state(dvr_id)),
            super::dvr::DvrRuntimeState::Closing
            | super::dvr::DvrRuntimeState::CleanupFailed
            | super::dvr::DvrRuntimeState::Closed
            | super::dvr::DvrRuntimeState::Failed => Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
    }

    pub fn dvr_status_event(
        &self,
        dvr_id: i32,
    ) -> Result<Option<DvrStatusEvent>, DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        if matches!(dvr.state(), super::dvr::DvrRuntimeState::Open)
            || dvr.state().is_closed_or_failed()
        {
            return Err(DemuxRuntimeError::invalid_state(dvr_id));
        }
        let queue = self
            .dvr_queue_runtimes
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?;
        let fill_bytes = queue
            .current_fill()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        Ok(dvr.status_event_for_fill(fill_bytes))
    }

    pub fn stop_dvr_runtime_from_typed_request(
        &mut self,
        request: DvrRuntimeOperationRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.stop_dvr_runtime(request.dvr_id)
    }

    pub(crate) fn stop_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let state = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        match state.state() {
            super::dvr::DvrRuntimeState::Started => {
                let dvr = self
                    .dvrs
                    .get_mut(&dvr_id)
                    .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
                dvr.mark_stopped();
                Ok(())
            }
            super::dvr::DvrRuntimeState::Configured | super::dvr::DvrRuntimeState::Stopped => {
                Ok(())
            }
            super::dvr::DvrRuntimeState::Open => Ok(()),
            super::dvr::DvrRuntimeState::Closing
            | super::dvr::DvrRuntimeState::CleanupFailed
            | super::dvr::DvrRuntimeState::Closed
            | super::dvr::DvrRuntimeState::Failed => Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
    }

    pub fn flush_dvr_runtime_from_typed_request(
        &mut self,
        request: DvrRuntimeOperationRequest,
    ) -> (DvrFlushReport, Result<(), DemuxRuntimeError>) {
        self.flush_dvr_runtime(request.dvr_id)
    }

    pub(crate) fn flush_dvr_runtime(
        &mut self,
        dvr_id: i32,
    ) -> (DvrFlushReport, Result<(), DemuxRuntimeError>) {
        let mut report = DvrFlushReport::new(dvr_id);
        let Some(state) = self.dvrs.get(&dvr_id) else {
            let error = DemuxRuntimeError::dvr_missing(dvr_id);
            report.failed(DvrFlushStep::ValidateState, error);
            return (report, Err(error));
        };
        match state.state() {
            super::dvr::DvrRuntimeState::Configured
            | super::dvr::DvrRuntimeState::Started
            | super::dvr::DvrRuntimeState::Stopped => {
                report.succeeded(DvrFlushStep::ValidateState);
            }
            super::dvr::DvrRuntimeState::Open
            | super::dvr::DvrRuntimeState::Closing
            | super::dvr::DvrRuntimeState::CleanupFailed
            | super::dvr::DvrRuntimeState::Closed
            | super::dvr::DvrRuntimeState::Failed => {
                let error = DemuxRuntimeError::invalid_state(dvr_id);
                report.failed(DvrFlushStep::ValidateState, error);
                return (report, Err(error));
            }
        }
        if let Err(error) = self.clear_dvr_queue_runtime(dvr_id) {
            report.failed(DvrFlushStep::ValidateQueueEmpty, error);
            return (report, Err(error));
        }
        report.succeeded(DvrFlushStep::ValidateQueueEmpty);
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            if dvr.kind() == DvrKind::Playback {
                dvr.clear_playback_completion();
            }
        }
        report.succeeded(DvrFlushStep::ClearPlaybackCompletion);
        report.committed();
        (report, Ok(()))
    }

    pub fn set_dvr_status_check_interval_from_typed_request(
        &mut self,
        request: DvrStatusIntervalRuntimeRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.set_dvr_status_check_interval(request.dvr_id, request.interval_ms)
    }

    pub(crate) fn set_dvr_status_check_interval(
        &mut self,
        dvr_id: i32,
        interval_ms: u64,
    ) -> Result<(), DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        match dvr.state() {
            super::dvr::DvrRuntimeState::Open
            | super::dvr::DvrRuntimeState::Configured
            | super::dvr::DvrRuntimeState::Started
            | super::dvr::DvrRuntimeState::Stopped => {
                dvr.set_status_check_interval_ms(interval_ms);
                Ok(())
            }
            super::dvr::DvrRuntimeState::Closing
            | super::dvr::DvrRuntimeState::CleanupFailed
            | super::dvr::DvrRuntimeState::Closed
            | super::dvr::DvrRuntimeState::Failed => Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
    }

    #[cfg(test)]
    pub(crate) fn write_playback_dvr_queue_bytes_for_test(
        &mut self,
        dvr_id: i32,
        data: &[u8],
    ) -> Result<usize, DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        match dvr.state() {
            super::dvr::DvrRuntimeState::Configured
            | super::dvr::DvrRuntimeState::Started
            | super::dvr::DvrRuntimeState::Stopped
                if dvr.kind() == DvrKind::Playback => {}
            _ => return Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
        if data.is_empty() {
            return Ok(0);
        }
        let queue = self
            .dvr_queue_runtimes
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?;
        let written = queue
            .peer_write_for_test(data)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        queue
            .wake(TUNER_EVENT_DATA_READY)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        Ok(written)
    }

    pub fn playback_dvr_wait_handle_from_typed_request(
        &self,
        request: DvrRuntimeOperationRequest,
    ) -> Result<QueueWaitHandle, DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get(&request.dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(request.dvr_id))?;
        if dvr.kind() != DvrKind::Playback {
            return Err(DemuxRuntimeError::unsupported_dvr_operation(request.dvr_id));
        }
        let queue = self
            .dvr_queue_runtimes
            .get(&request.dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(request.dvr_id))?;
        queue
            .wait_handle()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(request.dvr_id))
    }

    pub fn consume_playback_dvr_queue_from_typed_request(
        &mut self,
        request: DvrRuntimeOperationRequest,
    ) -> Result<PlaybackConsumeReport, DemuxRuntimeError> {
        self.consume_playback_dvr_queue_once(request.dvr_id)
    }

    pub(crate) fn consume_playback_dvr_queue_once(
        &mut self,
        dvr_id: i32,
    ) -> Result<PlaybackConsumeReport, DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        if dvr.kind() != DvrKind::Playback || dvr.state() != super::dvr::DvrRuntimeState::Started {
            return Ok(PlaybackConsumeReport::default());
        }
        let queue = self
            .dvr_queue_runtimes
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?;
        let available = match queue.available_to_read() {
            Ok(available) => available,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        if available == 0 {
            return Ok(PlaybackConsumeReport::default());
        }
        let mut payload = vec![0u8; available];
        let read = match queue.read_into(&mut payload) {
            Ok(read) => read,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        if read == 0 {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.mark_failed();
            }
            return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
        }
        payload.truncate(read);
        let drain = {
            let dvr = self
                .dvrs
                .get_mut(&dvr_id)
                .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
            dvr.push_playback_bytes(&payload)
        };
        let completed_packets = drain.packets.len();
        Ok(PlaybackConsumeReport {
            bytes_read: read,
            completed_packets,
            malformed_bytes: drain.malformed_bytes,
            completed_packet_bytes: drain.packets,
            packet_reports: Vec::new(),
            data_consumed_wake_failed: false,
        })
    }

    pub fn wake_playback_dvr_data_consumed_from_typed_request(
        &mut self,
        request: DvrRuntimeOperationRequest,
    ) -> Result<(), DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get(&request.dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(request.dvr_id))?;
        if dvr.kind() != DvrKind::Playback {
            return Err(DemuxRuntimeError::unsupported_dvr_operation(request.dvr_id));
        }
        let queue = self
            .dvr_queue_runtimes
            .get(&request.dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(request.dvr_id))?;
        if queue.wake(TUNER_EVENT_DATA_CONSUMED).is_err() {
            // The service-runtime playback transaction owns bounded retry and terminal
            // fail-close. A single transient wake failure must not commit DVR Failed before
            // those retries are exhausted.
            return Err(DemuxRuntimeError::queue_runtime_failure(request.dvr_id));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn consume_playback_dvr_queue_for_test(
        &mut self,
        dvr_id: i32,
    ) -> Result<PlaybackConsumeReport, DemuxRuntimeError> {
        self.consume_playback_dvr_queue_once(dvr_id)
    }

    pub fn disconnect_filter_source_from_typed_request(
        &mut self,
        request: FilterSourceDisconnectRequest,
    ) -> (SourceBoundaryReport, Result<(), DemuxRuntimeError>) {
        self.disconnect_filter_source(request.sink_filter_id)
    }

    pub(crate) fn disconnect_filter_source(
        &mut self,
        sink_filter_id: i32,
    ) -> (SourceBoundaryReport, Result<(), DemuxRuntimeError>) {
        let (source_boundary, outcome) =
            apply_filter_source_boundary_change(self, sink_filter_id, None);
        let result = outcome.map(|_| ());
        (source_boundary, result)
    }

    pub(super) fn disconnect_filter_source_after_boundary(
        &mut self,
        sink_filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        self.filters
            .get_mut(&sink_filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?
            .disconnect_source();
        self.rebuild_filter_queue_runtime(sink_filter_id)?;
        Ok(())
    }

    pub(super) fn connect_filter_source_after_boundary(
        &mut self,
        sink_filter_id: i32,
        source_filter_id: i32,
        source_filter_generation: u64,
    ) -> Result<(), DemuxRuntimeError> {
        self.filters
            .get_mut(&sink_filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?
            .set_source_filter(source_filter_id, source_filter_generation);
        self.rebuild_filter_queue_runtime(sink_filter_id)?;
        Ok(())
    }

    pub fn set_filter_source_non_null_from_typed_request(
        &mut self,
        request: FilterSourceConnectRequest,
    ) -> (
        SourceBoundaryReport,
        Result<PipelineResetReport, DemuxRuntimeError>,
    ) {
        self.set_filter_source_non_null(request.sink_filter_id, request.source_filter_id)
    }

    pub(crate) fn set_filter_source_non_null(
        &mut self,
        sink_filter_id: i32,
        source_filter_id: i32,
    ) -> (
        SourceBoundaryReport,
        Result<PipelineResetReport, DemuxRuntimeError>,
    ) {
        let (source_boundary, outcome) =
            connect_filter_source_boundary_change(self, sink_filter_id, source_filter_id);
        let result = outcome.map(|_| source_boundary.reset_report().cloned().unwrap_or_default());
        (source_boundary, result)
    }

    pub(crate) fn reset_generation_boundary(
        &mut self,
    ) -> Result<PipelineResetReport, DemuxRuntimeError> {
        let next = match next_generation(self.generation) {
            Ok(next) => next,
            Err(_) => {
                self.state = DemuxRuntimeState::Failed;
                return Err(DemuxRuntimeError::generation_exhausted(
                    DemuxGenerationTarget::Demux(self.demux_id),
                ));
            }
        };
        self.generation = next;
        Ok(self.pipeline.reset_boundary())
    }

    pub fn apply_generation_boundary_from_typed_request(
        &mut self,
        request: DemuxGenerationBoundaryRequest,
    ) -> Result<super::generation_boundary::GenerationBoundaryReport, DemuxRuntimeError> {
        self.apply_generation_boundary(request.reason)
    }

    pub(crate) fn apply_generation_boundary(
        &mut self,
        reason: PipelineBoundaryReason,
    ) -> Result<super::generation_boundary::GenerationBoundaryReport, DemuxRuntimeError> {
        let (_, report) =
            super::generation_boundary::GenerationBoundaryTxn::for_reason(reason).apply(self);
        report
    }

    pub fn quarantine_runtime_from_typed_request(
        &mut self,
        _request: DemuxRuntimeQuarantineRequest,
    ) {
        self.quarantine();
    }

    pub(crate) fn quarantine(&mut self) {
        self.state = DemuxRuntimeState::Quarantined;
        for filter in self.filters.values_mut() {
            filter.mark_failed();
        }
        for dvr in self.dvrs.values_mut() {
            dvr.mark_failed();
        }
    }

    pub fn filter_views(&self) -> Vec<PipelineFilterView> {
        self.filters
            .values()
            .map(FilterRuntime::pipeline_view)
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn push_ts_packet_from_origin(
        &mut self,
        packet: &[u8],
        origin: TsInputOrigin,
    ) -> PipelineReport {
        let validated = match crate::packet_pipeline::ValidatedTsPacket::validate(packet) {
            Ok(validated) => validated,
            Err(_) => {
                return crate::packet_pipeline::PacketPipeline::malformed_ts_packet_report();
            }
        };
        self.push_validated_ts_packet_from_origin(&validated, origin)
    }

    pub fn push_validated_ts_packet_from_typed_request(
        &mut self,
        request: ValidatedPacketIngressRequest<'_>,
    ) -> PipelineReport {
        self.push_validated_ts_packet_from_origin(request.validated, request.origin)
    }

    pub(crate) fn push_validated_ts_packet_from_origin(
        &mut self,
        validated: &crate::packet_pipeline::ValidatedTsPacket<'_>,
        origin: TsInputOrigin,
    ) -> PipelineReport {
        let packet = validated.packet_bytes();
        let kind = match origin {
            TsInputOrigin::Frontend => PipelineInputKind::Live,
            TsInputOrigin::Playback => PipelineInputKind::Playback,
            TsInputOrigin::SourceFilter {
                source_filter_id,
                source_filter_generation,
            } => PipelineInputKind::SourceFilter {
                source_filter_id,
                source_filter_generation,
            },
        };
        let mut report = self.pipeline.push_validated_ts_packet(validated, kind);
        if report.accepted_packets == 0 {
            return report;
        }
        let filters = self.filter_views();
        let downstream = self
            .pipeline
            .plan_and_assemble_ts_packet_report_after_preflight(
                validated,
                origin,
                &filters,
                &report.assembly_suppression_reasons,
            );
        report.dropped_packets += downstream.dropped_packets;
        report.malformed_packets += downstream.malformed_packets;
        report.drop_reasons.extend(downstream.drop_reasons);
        report
            .assembly_suppression_reasons
            .extend(downstream.assembly_suppression_reasons);
        report.delivery_actions.extend(downstream.delivery_actions);
        report.generated_events.extend(downstream.generated_events);
        report.diagnostics.extend(downstream.diagnostics);
        self.mark_filters_failed_for_generation_overflow(&report.diagnostics);
        let av_filter_ids: Vec<i32> = report
            .delivery_actions
            .iter()
            .filter_map(|action| match action {
                PipelineDeliveryAction::AvPayload { filter_id } => Some(*filter_id),
                _ => None,
            })
            .collect();
        for filter_id in av_filter_ids {
            let outcome = self
                .filter_av_backings
                .get_mut(&filter_id)
                .map(|backing| backing.allocate_payload_bytes(packet));
            match outcome {
                Some(Ok(AvPayloadDeliveryOutcome::Delivered(descriptor))) => {
                    report
                        .generated_events
                        .push(PipelineGeneratedEvent::AvMedia {
                            filter_id,
                            descriptor,
                        });
                }
                Some(Ok(outcome)) => {
                    if let Some(diagnostic) =
                        av_payload_delivery_outcome_diagnostic(outcome, validated.pid(), filter_id)
                    {
                        report.diagnostics.push(diagnostic);
                    }
                }
                Some(Err(error)) => {
                    if let Some(filter) = self.filters.get_mut(&filter_id) {
                        filter.mark_failed();
                    }
                    report.diagnostics.push(
                        crate::packet_pipeline::PipelineDiagnostic::av_shared_backing_failure(
                            validated.pid(),
                            filter_id,
                            error,
                        ),
                    );
                }
                None => {
                    report.diagnostics.push(
                        crate::packet_pipeline::PipelineDiagnostic::av_shared_backing_missing(
                            validated.pid(),
                            filter_id,
                        ),
                    );
                }
            }
        }
        let packet_pid = validated.pid();
        let mirror_diagnostics =
            self.mirror_record_dvr_packets(packet, &report.delivery_actions, packet_pid);
        report.diagnostics.extend(mirror_diagnostics);
        let queue_payload_diagnostics = self.enqueue_queue_payloads_from_generated_events(
            packet,
            &report.generated_events,
            packet_pid,
        );
        report.diagnostics.extend(queue_payload_diagnostics);
        report
    }

    fn mark_filters_failed_for_generation_overflow(
        &mut self,
        diagnostics: &[crate::packet_pipeline::PipelineDiagnostic],
    ) {
        for diagnostic in diagnostics {
            let filter_ids = match diagnostic {
                crate::packet_pipeline::PipelineDiagnostic::SectionGenerationOverflow {
                    filter_ids,
                    ..
                }
                | crate::packet_pipeline::PipelineDiagnostic::PesGenerationOverflow {
                    filter_ids,
                    ..
                } => filter_ids,
                _ => continue,
            };
            for filter_id in filter_ids {
                if let Some(filter) = self.filters.get_mut(filter_id) {
                    filter.mark_failed();
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn open_filter_runtime(
        filter_id: i32,
        generation: u64,
        kind: PipelineOpenKind,
        config: Option<FilterPipelineConfig>,
    ) -> FilterRuntime {
        let mut runtime = FilterRuntime::new(filter_id, generation, kind);
        if let Some(config) = config {
            runtime.configure_with_generation(generation, config);
        }
        runtime
    }

    #[cfg(test)]
    pub(crate) fn open_filter_runtime_typed(
        filter_id: i32,
        generation: u64,
        open_type: FilterOpenType,
        config: Option<FilterPipelineConfig>,
    ) -> FilterRuntime {
        let mut runtime = FilterRuntime::new_typed(filter_id, generation, open_type);
        if let Some(config) = config {
            runtime.configure_with_generation(generation, config);
        }
        runtime
    }

    #[cfg(test)]
    pub(crate) fn open_filter_runtime_from_request(
        filter_id: i32,
        generation: u64,
        request: &OpenFilterRequest,
        config: Option<FilterPipelineConfig>,
    ) -> FilterRuntime {
        let mut runtime = FilterRuntime::new_open_request(filter_id, generation, request);
        if let Some(config) = config {
            runtime.configure_with_generation(generation, config);
        }
        runtime
    }

    #[cfg(test)]
    pub(crate) fn open_record_dvr_runtime(dvr_id: i32, generation: u64) -> DvrRuntime {
        DvrRuntime::new(dvr_id, DvrKind::Record, generation)
    }

    #[cfg(test)]
    pub(crate) fn open_dvr_runtime(
        dvr_id: i32,
        generation: u64,
        kind: DvrKind,
        buffer_size: i32,
        callback_present: bool,
    ) -> DvrRuntime {
        DvrRuntime::new_open_request(dvr_id, kind, generation, buffer_size, callback_present)
    }

    pub fn filter_queue_descriptor_export_plan(
        &self,
        filter_id: i32,
    ) -> Result<QueueDescriptorExportPlan, QueueDescriptorQueryError> {
        let snapshot = self
            .filter(filter_id)
            .map(FilterRuntime::snapshot)
            .ok_or(QueueDescriptorQueryError::FilterMissing(filter_id))?;
        if snapshot.state.is_closed_or_failed() {
            return Err(QueueDescriptorQueryError::InvalidState(filter_id));
        }
        if !self
            .filter(filter_id)
            .is_some_and(FilterRuntime::allows_queue_desc)
        {
            return Err(QueueDescriptorQueryError::Unavailable(filter_id));
        }
        self.filter_queue_runtimes
            .get(&filter_id)
            .map(|queue| {
                QueueDescriptorExportPlan::new(
                    QueueDescriptorExportTarget::Filter { filter_id },
                    queue.descriptor_export_handle(),
                )
            })
            .ok_or(QueueDescriptorQueryError::RuntimeMissing(filter_id))
    }

    pub fn dvr_queue_descriptor_export_plan(
        &self,
        dvr_id: i32,
    ) -> Result<QueueDescriptorExportPlan, QueueDescriptorQueryError> {
        let snapshot = self
            .dvr(dvr_id)
            .map(DvrRuntime::snapshot)
            .ok_or(QueueDescriptorQueryError::DvrMissing(dvr_id))?;
        if snapshot.state.is_closed_or_failed() {
            return Err(QueueDescriptorQueryError::InvalidState(dvr_id));
        }
        if !self.dvr(dvr_id).is_some_and(DvrRuntime::allows_queue_desc) {
            return Err(QueueDescriptorQueryError::InvalidState(dvr_id));
        }
        self.dvr_queue_runtimes
            .get(&dvr_id)
            .map(|queue| {
                QueueDescriptorExportPlan::new(
                    QueueDescriptorExportTarget::Dvr { dvr_id },
                    queue.descriptor_export_handle(),
                )
            })
            .ok_or(QueueDescriptorQueryError::RuntimeMissing(dvr_id))
    }

    fn clear_filter_queue_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let queue = self
            .filter_queue_runtimes
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        let fill = queue
            .current_fill()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        if fill == 0 {
            Ok(())
        } else {
            Err(DemuxRuntimeError::queue_runtime_failure(filter_id))
        }
    }

    pub(crate) fn clear_dvr_queue_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let queue = self
            .dvr_queue_runtimes
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?;
        let fill = queue
            .current_fill()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        if fill == 0 {
            Ok(())
        } else {
            Err(DemuxRuntimeError::queue_runtime_failure(dvr_id))
        }
    }

    fn new_queue_runtime_for_dvr(dvr: &DvrRuntime) -> Result<QueueRuntime, QueueRuntimeError> {
        match dvr.kind() {
            DvrKind::Record => QueueRuntime::new_writer(dvr.buffer_size(), true),
            DvrKind::Playback => QueueRuntime::new_reader(dvr.buffer_size(), true),
        }
    }

    fn rebuild_filter_queue_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let Some(filter) = self.filters.get(&filter_id) else {
            return Err(DemuxRuntimeError::filter_missing(filter_id));
        };
        if !Self::should_keep_filter_queue_runtime(filter) {
            self.filter_queue_runtimes.remove(&filter_id);
            return Ok(());
        }
        if self
            .filter_queue_runtimes
            .get(&filter_id)
            .is_some_and(|queue| {
                queue.is_hal_writer()
                    && queue.capacity_matches_buffer_size(filter.buffer_size())
            })
        {
            return Ok(());
        }
        Err(DemuxRuntimeError::queue_runtime_failure(filter_id))
    }

    fn drop_filter_av_backing_to_stale(&mut self, filter_id: i32) {
        let Some(backing) = self.filter_av_backings.remove(&filter_id) else {
            return;
        };
        let stale_ids = backing.known_data_ids();
        if stale_ids.is_empty() {
            return;
        }
        self.filter_av_stale_data_ids
            .entry(filter_id)
            .or_default()
            .extend(stale_ids);
    }

    fn known_stale_av_data_id(&self, filter_id: i32, data_id: AvDataId) -> bool {
        data_id.0 > 0
            && self
                .filter_av_stale_data_ids
                .get(&filter_id)
                .is_some_and(|ids| ids.contains(&data_id))
    }

    fn should_keep_filter_queue_runtime(filter: &FilterRuntime) -> bool {
        !filter.state().is_closed_or_failed()
            && filter.supports_normal_fmq_queue()
            && filter.buffer_size() > 0
    }

    fn mirror_record_dvr_packets(
        &mut self,
        packet: &[u8],
        delivery_actions: &[PipelineDeliveryAction],
        pid: crate::packet_pipeline::PacketPid,
    ) -> Vec<crate::packet_pipeline::PipelineDiagnostic> {
        let mut diagnostics = Vec::new();
        for action in delivery_actions {
            let PipelineDeliveryAction::DvrMirror { dvr_id: filter_id } = *action else {
                continue;
            };
            let target_ids = self.record_dvr_target_ids_for_filter(filter_id);
            for dvr_id in target_ids {
                match self.try_write_record_dvr_packet(dvr_id, packet) {
                    Ok(RecordDvrMirrorWriteOutcome::Written) => {}
                    Ok(RecordDvrMirrorWriteOutcome::Overflow) => {
                        diagnostics.push(
                            crate::packet_pipeline::PipelineDiagnostic::record_dvr_mirror_overflow(
                                pid, filter_id, dvr_id,
                            ),
                        );
                    }
                    Err(error) => {
                        diagnostics.push(
                            crate::packet_pipeline::PipelineDiagnostic::record_dvr_mirror_failure(
                                pid, filter_id, dvr_id, error,
                            ),
                        );
                    }
                }
            }
        }
        diagnostics
    }

    fn record_dvr_target_ids_for_filter(&self, filter_id: i32) -> Vec<i32> {
        self.dvrs
            .iter()
            .filter_map(|(dvr_id, dvr)| {
                (dvr.kind() == DvrKind::Record
                    && dvr.state() == super::dvr::DvrRuntimeState::Started
                    && dvr.attached_record_filters().contains(&filter_id))
                .then_some(*dvr_id)
            })
            .collect()
    }

    fn try_write_record_dvr_packet(
        &mut self,
        dvr_id: i32,
        packet: &[u8],
    ) -> Result<RecordDvrMirrorWriteOutcome, DemuxRuntimeError> {
        let Some(queue) = self.dvr_queue_runtimes.get(&dvr_id) else {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.mark_failed();
            }
            return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
        };
        let available = match queue.available_to_write() {
            Ok(available) => available,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        if available < packet.len() {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.mark_pending_overflow();
            }
            return Ok(RecordDvrMirrorWriteOutcome::Overflow);
        }
        let result = FmqDeliveryTxn::new(FmqObjectKind::DvrRecord).commit_payload(
            packet.len(),
            queue
                .write_checked(packet)
                .map_err(|_| FmqFailureKind::WriteFailed),
            queue
                .wake(TUNER_EVENT_DATA_READY)
                .map_err(|_| FmqFailureKind::EventFlagWakeFailed),
        );
        match result.action {
            FmqDeliveryAction::Continue => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.clear_pending_overflow();
                }
                Ok(RecordDvrMirrorWriteOutcome::Written)
            }
            FmqDeliveryAction::Overflow => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_pending_overflow();
                }
                Ok(RecordDvrMirrorWriteOutcome::Overflow)
            }
            FmqDeliveryAction::RuntimeFailed(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                Err(DemuxRuntimeError::queue_runtime_failure(dvr_id))
            }
        }
    }

    fn enqueue_queue_payloads_from_generated_events(
        &mut self,
        packet: &[u8],
        generated_events: &[PipelineGeneratedEvent],
        packet_pid: crate::packet_pipeline::PacketPid,
    ) -> Vec<crate::packet_pipeline::PipelineDiagnostic> {
        let mut diagnostics = Vec::new();
        for event in generated_events {
            let (filter_id, pid, result) = match event {
                PipelineGeneratedEvent::DataReady { filter_id }
                | PipelineGeneratedEvent::Record { filter_id } => (
                    *filter_id,
                    packet_pid,
                    self.enqueue_filter_queue_payload(*filter_id, packet.to_vec()),
                ),
                PipelineGeneratedEvent::SectionPayloadReady {
                    filter_id,
                    pid,
                    bytes,
                    ..
                } => (
                    *filter_id,
                    *pid,
                    self.enqueue_filter_queue_payload(*filter_id, bytes.clone()),
                ),
                PipelineGeneratedEvent::PesPacketReady {
                    filter_id,
                    pid,
                    packet,
                    ..
                } => (
                    *filter_id,
                    *pid,
                    self.enqueue_filter_queue_payload(*filter_id, packet.raw_bytes.clone()),
                ),
                PipelineGeneratedEvent::Section { .. }
                | PipelineGeneratedEvent::Pes { .. }
                | PipelineGeneratedEvent::RecordIndex { .. }
                | PipelineGeneratedEvent::AvMedia { .. } => continue,
            };
            if let Err(error) = result {
                diagnostics.push(crate::packet_pipeline::PipelineDiagnostic::filter_queue_payload_delivery_failure(
                    pid,
                    filter_id,
                    error,
                ));
            }
        }
        diagnostics
    }

    #[cfg(test)]
    pub(crate) fn read_record_dvr_queue_bytes_for_test(
        &self,
        dvr_id: i32,
    ) -> Result<Vec<u8>, DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        match dvr.state() {
            super::dvr::DvrRuntimeState::Configured
            | super::dvr::DvrRuntimeState::Started
            | super::dvr::DvrRuntimeState::Stopped => {
                if dvr.kind() != DvrKind::Record {
                    return Err(DemuxRuntimeError::invalid_state(dvr_id));
                }
            }
            _ => return Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
        let queue = self
            .dvr_queue_runtimes
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?;
        let bytes = queue
            .current_fill()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        let mut out = vec![0u8; bytes];
        let read = queue
            .peer_read_for_test(&mut out)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        out.truncate(read);
        Ok(out)
    }
}
