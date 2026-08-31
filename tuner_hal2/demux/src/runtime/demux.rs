#[cfg(test)]
use std::collections::VecDeque;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use maleicacid_tuner_hal2_control_core::{
    FmqDeliveryAction, FmqDeliveryTxn, FmqFailureKind, FmqObjectKind,
};
use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

use crate::av::{
    AvDataId, AvDataIdAllocator, AvDataIdState, AvFilterReleaseState, AvHandleReleaseDescriptor,
    AvHandleReleaseInput, AvHandleReleaseKind, AvHandleReleaseOutcome, AvHandleReleaseTxn,
    AvPayloadDeliveryOutcome, AvRuntimeBudget, AvSharedBacking, AvSharedHandleExport,
    ClientHandleState, DEFAULT_AV_MAX_EVENT_BYTES,
    DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER, DEFAULT_AV_PER_FILTER_LIVE_BYTES,
};
use crate::config::{
    AvStreamTypeConfig, ConfigInputPid, FilterDelayHint, FilterDelayReadiness, FilterOpenType,
    OpenFilterRequest,
};
use crate::packet_pipeline::{
    FilterPipelineConfig, PacketPipeline, PipelineBoundaryReason, PipelineDeliveryAction,
    PipelineDiagnostic, PipelineDiagnosticCounters, PipelineFilterView, PipelineGeneratedEvent,
    PipelineInputKind, PipelineOpenKind, PipelineReport, PipelineResetReport,
};
use crate::TsInputOrigin;

use super::av_sync_registry::AvSyncRegistry;
use super::dvr::{
    DvrDataFormat, DvrKind, DvrRuntime, DvrRuntimeSnapshot, DvrStatusEvent,
    RecordDvrFilterRelationState,
};
use super::filter::{
    FilterRuntime, FilterRuntimeSnapshot, FilterRuntimeState, FilterSource, FilterStatusEvent,
};
use super::filter_producer_drain_gate::{
    FilterDrainBoundary, FilterProducerDrainGate, FilterProducerPermit,
};
use super::queue_runtime::{
    DvrQueueDrainCommitError, FilterDrainTxn, QueueDescriptorExportPlan,
    QueueDescriptorExportTarget, QueueEpochDrainTxn, QueueRuntime, QueueRuntimeError,
};
use super::pcr_clock_anchor::{PcrClockAnchorStore, PcrObservationOutcome};
use super::source_boundary::{
    apply_filter_source_boundary_change, connect_filter_source_boundary_change,
    SourceBoundaryReport,
};
const TUNER_EVENT_DATA_READY: u32 = 1 << 0;
#[cfg(test)]
const TEST_PENDING_FILTER_EVENT_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordDvrMirrorWriteOutcome {
    Written,
    WakePending,
    Overflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FilterQueuePayloadError {
    Overflow(DemuxRuntimeError),
    Runtime(DemuxRuntimeError),
}

impl FilterQueuePayloadError {
    const fn runtime_error(self) -> DemuxRuntimeError {
        match self {
            Self::Overflow(error) | Self::Runtime(error) => error,
        }
    }

    const fn is_overflow(self) -> bool {
        matches!(self, Self::Overflow(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordIndexCommitMode {
    Parse,
    ResetThenParse,
    AdvanceOnly,
    ResetAndAdvance,
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
    SelfReference,
    PidMismatch,
    PipelineFailed,
    GenerationExhausted,
    QueueRuntimeFailure,
    AvBackingFailure,
    SourceBoundaryRollbackFailed,
    RelationCommitUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DemuxRuntimeError {
    pub kind: DemuxRuntimeErrorKind,
    pub id: Option<i32>,
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
    PendingEventDiscard,
    PipelineRollback,
    MirrorQueueClear,
    QueuedPayloadClear,
    AvBackingFlush,
    PcrAnchorInvalidate,
    ProducerDrainCommit,
    SourceGenerationRefresh,
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
    StopPreservesQueue,
    NoSourceDownstreams,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterRuntimeOperationStepOutcome {
    Succeeded(FilterRuntimeOperationStep),
    Failed {
        step: FilterRuntimeOperationStep,
        error: DemuxRuntimeErrorKind,
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
    Isolated {
        failed_step: FilterRuntimeOperationStep,
    },
    Failed {
        failed_step: FilterRuntimeOperationStep,
    },
    RolledBack {
        failed_step: FilterRuntimeOperationStep,
        rollback_step: FilterRuntimeOperationStep,
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
    pub fn new(operation: FilterRuntimeOperationKind, filter_id: i32) -> Self {
        Self {
            operation,
            filter_id,
            steps: Vec::new(),
            outcome: None,
        }
    }

    pub fn succeeded(&mut self, step: FilterRuntimeOperationStep) {
        self.steps
            .push(FilterRuntimeOperationStepOutcome::Succeeded(step));
    }

    pub fn failed(&mut self, step: FilterRuntimeOperationStep, error: DemuxRuntimeErrorKind) {
        self.steps
            .push(FilterRuntimeOperationStepOutcome::Failed { step, error });
    }

    pub fn skipped(
        &mut self,
        step: FilterRuntimeOperationStep,
        reason: FilterRuntimeOperationSkipReason,
    ) {
        self.steps
            .push(FilterRuntimeOperationStepOutcome::Skipped { step, reason });
    }

    pub fn finish(&mut self, outcome: FilterRuntimeOperationOutcome) {
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

/// Queue cleanup の lower protocol が発行する opaque な call-local plan。
///
/// producer drain と snapshot は Demux 内部に閉じたまま、service 側の
/// QueueCleanupUseCase が各 phase の呼び出し順序と結果集約を所有する。
pub struct FilterQueueCleanupPlan {
    filter_id: i32,
    snapshot: FilterRuntimeSnapshot,
    next_source_generation: Option<u64>,
    drain: FilterDrainTxn,
}

/// producer drain commit 済みであることを示す one-shot token。
pub struct CommittedFilterQueueCleanup {
    filter_id: i32,
    source_generation: Option<(u64, u64)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrQueueCleanupStep {
    Prepare,
    QueueClear,
    QueueEpochCommit,
    RuntimeStateCommit,
    PlaybackPipelineReset,
    PcrAnchorInvalidate,
    RecordIndexReset,
    PlaybackResidualDiscard,
    PlaybackDiscardDiagnosticCommit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrQueueCleanupCommitError {
    failed_step: DvrQueueCleanupStep,
    error: DemuxRuntimeError,
}

impl DvrQueueCleanupCommitError {
    const fn new(failed_step: DvrQueueCleanupStep, error: DemuxRuntimeError) -> Self {
        Self { failed_step, error }
    }

    pub const fn failed_step(self) -> DvrQueueCleanupStep {
        self.failed_step
    }

    pub const fn error(self) -> DemuxRuntimeError {
        self.error
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrQueueCleanupSkipReason {
    PlaybackOnly,
    RecordOnly,
    NoRetainedPlaybackBytes,
    PrerequisiteFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrQueueCleanupStepOutcome {
    Succeeded(DvrQueueCleanupStep),
    Failed {
        step: DvrQueueCleanupStep,
        error: DemuxRuntimeErrorKind,
    },
    Skipped {
        step: DvrQueueCleanupStep,
        reason: DvrQueueCleanupSkipReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrQueueCleanupOutcome {
    Committed,
    Isolated { failed_step: DvrQueueCleanupStep },
    Failed { failed_step: DvrQueueCleanupStep },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrQueueCleanupReport {
    dvr_id: i32,
    steps: Vec<DvrQueueCleanupStepOutcome>,
    outcome: Option<DvrQueueCleanupOutcome>,
}

impl DvrQueueCleanupReport {
    pub fn new(dvr_id: i32) -> Self {
        Self {
            dvr_id,
            steps: Vec::new(),
            outcome: None,
        }
    }

    pub fn succeeded(&mut self, step: DvrQueueCleanupStep) {
        self.steps
            .push(DvrQueueCleanupStepOutcome::Succeeded(step));
    }

    pub fn failed(&mut self, step: DvrQueueCleanupStep, error: DemuxRuntimeErrorKind) {
        self.steps
            .push(DvrQueueCleanupStepOutcome::Failed { step, error });
    }

    pub fn skipped(&mut self, step: DvrQueueCleanupStep, reason: DvrQueueCleanupSkipReason) {
        self.steps
            .push(DvrQueueCleanupStepOutcome::Skipped { step, reason });
    }

    pub fn finish(&mut self, outcome: DvrQueueCleanupOutcome) {
        self.outcome = Some(outcome);
    }

    pub const fn dvr_id(&self) -> i32 {
        self.dvr_id
    }

    pub fn steps(&self) -> &[DvrQueueCleanupStepOutcome] {
        &self.steps
    }

    pub const fn outcome(&self) -> Option<DvrQueueCleanupOutcome> {
        self.outcome
    }
}

/// QueueEpochProtocol が発行する opaque な call-local DVR cleanup plan。
///
/// queue epoch と drain transaction は Demux 内部に閉じたまま、service 側の
/// QueueCleanupUseCase が各 phase の呼び出し順序と結果集約を所有する。
pub struct DvrQueueCleanupPlan {
    dvr_id: i32,
    kind: DvrKind,
    next_playback_generation: Option<u64>,
    attached_record_filters: Vec<i32>,
    playback_coordinates: Option<(u64, u64)>,
    drain: QueueEpochDrainTxn,
}

impl DvrQueueCleanupPlan {
    pub const fn kind(&self) -> DvrKind {
        self.kind
    }
}

/// queue epoch commit 済みであることを示す one-shot cleanup token。
pub struct CommittedDvrQueueCleanup {
    dvr_id: i32,
    kind: DvrKind,
    next_playback_generation: Option<u64>,
    attached_record_filters: Vec<i32>,
    playback_coordinates: Option<(u64, u64)>,
    queue_dropped_bytes: usize,
}

impl CommittedDvrQueueCleanup {
    pub const fn is_playback(&self) -> bool {
        matches!(self.kind, DvrKind::Playback)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterQueuePayloadCleanupOutcome {
    filter_state_cleared: bool,
}

impl FilterQueuePayloadCleanupOutcome {
    pub const fn filter_state_cleared(self) -> bool {
        self.filter_state_cleared
    }
}

impl DemuxRuntimeError {
    pub const fn filter_missing(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::FilterMissing,
            id: Some(filter_id),
        }
    }
    pub const fn dvr_missing(dvr_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::DvrMissing,
            id: Some(dvr_id),
        }
    }
    pub const fn queue_missing(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::QueueMissing,
            id: Some(filter_id),
        }
    }
    pub const fn invalid_state(id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::InvalidState,
            id: Some(id),
        }
    }
    pub const fn invalid_dvr_filter(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::InvalidDvrFilter,
            id: Some(filter_id),
        }
    }
    pub const fn unsupported_dvr_operation(dvr_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::UnsupportedDvrOperation,
            id: Some(dvr_id),
        }
    }
    pub const fn source_lifecycle(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::SourceLifecycle,
            id: Some(filter_id),
        }
    }
    pub const fn sink_lifecycle(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::SinkLifecycle,
            id: Some(filter_id),
        }
    }
    pub const fn invalid_source_subtype(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::InvalidSourceSubtype,
            id: Some(filter_id),
        }
    }
    pub const fn invalid_sink_subtype(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::InvalidSinkSubtype,
            id: Some(filter_id),
        }
    }
    pub const fn self_reference(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::SelfReference,
            id: Some(filter_id),
        }
    }
    pub const fn pid_mismatch(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::PidMismatch,
            id: Some(filter_id),
        }
    }
    pub const fn pipeline_failed() -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::PipelineFailed,
            id: None,
        }
    }
    pub const fn generation_exhausted(id: Option<i32>) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::GenerationExhausted,
            id,
        }
    }
    pub const fn queue_runtime_failure(id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::QueueRuntimeFailure,
            id: Some(id),
        }
    }
    pub const fn av_backing_failure(id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::AvBackingFailure,
            id: Some(id),
        }
    }
    pub const fn source_boundary_rollback_failed(filter_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed,
            id: Some(filter_id),
        }
    }
    pub const fn relation_commit_unknown(dvr_id: i32) -> Self {
        Self {
            kind: DemuxRuntimeErrorKind::RelationCommitUnknown,
            id: Some(dvr_id),
        }
    }
}

pub fn next_generation(current: u64) -> Result<u64, DemuxRuntimeError> {
    current
        .checked_add(1)
        .ok_or(DemuxRuntimeError::generation_exhausted(None))
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
        AvPayloadDeliveryOutcome::PayloadEmpty => {
            Some(crate::packet_pipeline::PipelineDiagnostic::av_payload_empty(pid, filter_id))
        }
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
    stream_boundary_generation: u64,
    pipeline: PacketPipeline,
    filters: BTreeMap<i32, FilterRuntime>,
    dvrs: BTreeMap<i32, DvrRuntime>,
    filter_producer_gates: BTreeMap<i32, FilterProducerDrainGate>,
    filter_queue_runtimes: BTreeMap<i32, QueueRuntime>,
    dvr_queue_runtimes: BTreeMap<i32, QueueRuntime>,
    pcr_clock_anchor_store: PcrClockAnchorStore,
    av_sync_registry: AvSyncRegistry,
    #[cfg(test)]
    filter_queue_mirror: BTreeMap<i32, VecDeque<Vec<u8>>>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct DemuxRuntimeRollbackToken {
    demux_id: i32,
    token_id: u64,
    generation: u64,
}

impl DemuxRuntimeRollbackToken {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    const fn new(demux_id: i32, token_id: u64, generation: u64) -> Self {
        Self {
            demux_id,
            token_id,
            generation,
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
pub struct DemuxRuntimeRollbackRestoreRequest {
    token: DemuxRuntimeRollbackToken,
}

impl DemuxRuntimeRollbackRestoreRequest {
    pub const fn new(token: DemuxRuntimeRollbackToken) -> Self {
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
        self.stream_boundary_generation
    }
}

#[derive(Debug)]
pub struct FilterRuntimeRegistrationRequest<'a> {
    filter_id: i32,
    request: &'a OpenFilterRequest,
    pending_event_capacity: usize,
}

impl<'a> FilterRuntimeRegistrationRequest<'a> {
    pub const fn new(
        filter_id: i32,
        request: &'a OpenFilterRequest,
        pending_event_capacity: usize,
    ) -> Self {
        Self {
            filter_id,
            request,
            pending_event_capacity,
        }
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
}

impl DvrRuntimeConfigureRequest {
    pub const fn new(dvr_id: i32) -> Self {
        Self { dvr_id }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrStatusReportingRequest {
    dvr_id: i32,
    status_mask: i32,
    low_threshold_bytes: usize,
    high_threshold_bytes: usize,
    data_format: DvrDataFormat,
    packet_size: i64,
}

impl DvrStatusReportingRequest {
    pub const fn new(
        dvr_id: i32,
        status_mask: i32,
        low_threshold_bytes: usize,
        high_threshold_bytes: usize,
        data_format: DvrDataFormat,
        packet_size: i64,
    ) -> Self {
        Self {
            dvr_id,
            status_mask,
            low_threshold_bytes,
            high_threshold_bytes,
            data_format,
            packet_size,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DemuxStreamBoundaryRequest {
    reason: PipelineBoundaryReason,
}

impl DemuxStreamBoundaryRequest {
    pub const fn new(reason: PipelineBoundaryReason) -> Self {
        Self { reason }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterRuntimeOperationRequest {
    filter_id: i32,
}

impl FilterRuntimeOperationRequest {
    pub const fn new(filter_id: i32) -> Self {
        Self { filter_id }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrRuntimeOperationRequest {
    dvr_id: i32,
}

impl DvrRuntimeOperationRequest {
    pub const fn new(dvr_id: i32) -> Self {
        Self { dvr_id }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterAvHandleReleaseRequest {
    filter_id: i32,
    descriptor: AvHandleReleaseDescriptor,
    av_data_id: i64,
}

impl FilterAvHandleReleaseRequest {
    pub const fn new(
        filter_id: i32,
        descriptor: AvHandleReleaseDescriptor,
        av_data_id: i64,
    ) -> Self {
        Self {
            filter_id,
            descriptor,
            av_data_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterAvStreamTypeRuntimeRequest {
    filter_id: i32,
    config: AvStreamTypeConfig,
}

impl FilterAvStreamTypeRuntimeRequest {
    pub const fn new(filter_id: i32, config: AvStreamTypeConfig) -> Self {
        Self { filter_id, config }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterDelayHintRuntimeRequest {
    filter_id: i32,
    hint: FilterDelayHint,
}

impl FilterDelayHintRuntimeRequest {
    pub const fn new(filter_id: i32, hint: FilterDelayHint) -> Self {
        Self { filter_id, hint }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrFilterLinkRequest {
    dvr_id: i32,
    filter_id: i32,
}

#[derive(Debug)]
pub struct PreparedDvrFilterRelation {
    dvr_id: i32,
    filter_id: i32,
    expected_generation: u64,
    next_generation: u64,
    expected_filters: BTreeSet<i32>,
    next_filters: BTreeSet<i32>,
    changed: bool,
    reset_record_index: bool,
    #[cfg(test)]
    commit_fault: Option<RecordDvrFilterRelationCommitFault>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecordDvrFilterRelationCommitFault {
    RejectBeforeCommit,
    UnknownAfterApply,
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
    stream_boundary: super::generation_boundary::StreamBoundaryTxn,
    pipeline: PacketPipeline,
    filters: BTreeMap<i32, FilterRuntime>,
    dvrs: BTreeMap<i32, DvrRuntime>,
    filter_producer_gates: BTreeMap<i32, FilterProducerDrainGate>,
    #[cfg(test)]
    filter_queue_mirror: BTreeMap<i32, VecDeque<Vec<u8>>>,
    filter_queue_runtimes: BTreeMap<i32, QueueRuntime>,
    dvr_queue_runtimes: BTreeMap<i32, QueueRuntime>,
    filter_av_backings: BTreeMap<i32, AvSharedBacking>,
    av_data_id_allocator: Arc<AvDataIdAllocator>,
    av_runtime_budget: Arc<AvRuntimeBudget>,
    av_max_event_bytes: usize,
    av_max_outstanding_events_per_filter: usize,
    av_per_filter_live_bytes: usize,
    pcr_clock_anchor_store: PcrClockAnchorStore,
    av_sync_registry: AvSyncRegistry,
    rollback_snapshots: BTreeMap<u64, DemuxRuntimeSnapshot>,
    next_rollback_token_id: u64,
    #[cfg(test)]
    next_record_relation_commit_fault: Option<RecordDvrFilterRelationCommitFault>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaybackConsumeReport {
    pub bytes_read: usize,
    pub completed_packets: usize,
    pub malformed_packets: usize,
    pub malformed_bytes: usize,
    pub dropped_bytes: usize,
    pub packet_reports: Vec<PipelineReport>,
}

#[derive(Debug)]
pub struct PlaybackQueueReadTxn {
    dvr_id: i32,
    token: Option<super::queue_runtime::QueueEpochToken>,
    origin: TsInputOrigin,
    read_limit: usize,
}

impl PlaybackQueueReadTxn {
    pub const fn origin(&self) -> TsInputOrigin {
        self.origin
    }

    pub const fn read_limit(&self) -> usize {
        self.read_limit
    }
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
    fn register_av_sync_pcr_filter(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let prepared = self
            .av_sync_registry
            .prepare_register_pcr_filter(filter_id)
            .map_err(|_| DemuxRuntimeError::invalid_state(filter_id))?;
        self.av_sync_registry.commit(prepared);
        Ok(())
    }

    fn register_av_sync_media_filter(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let prepared = self
            .av_sync_registry
            .prepare_register_media_filter(filter_id)
            .map_err(|_| DemuxRuntimeError::invalid_state(filter_id))?;
        self.av_sync_registry.commit(prepared);
        Ok(())
    }

    fn unregister_av_sync_filter(&mut self, filter_id: i32) {
        let prepared = self
            .av_sync_registry
            .prepare_unregister_filter(filter_id);
        self.av_sync_registry.commit(prepared);
    }

    pub fn av_sync_hw_id_for_media_filter(&self, filter_id: i32) -> Option<i32> {
        self.av_sync_registry
            .hw_sync_id_for_media_filter(filter_id)
    }

    pub fn pcr_filter_id_for_av_sync_hw_id(&self, hw_id: i32) -> Option<i32> {
        self.av_sync_registry
            .pcr_filter_id_for_hw_sync_id(hw_id)
    }

    pub fn pcr_clock_time_90khz(&self, filter_id: i32) -> Option<u64> {
        let filter = self.filters.get(&filter_id)?;
        self.pcr_clock_anchor_store
            .current_time_90khz(filter_id, filter.generation())
    }

    #[cfg(test)]
    pub(crate) fn pcr_anchor_observation_for_test(
        &self,
        filter_id: i32,
    ) -> Option<(u64, u64)> {
        self.pcr_clock_anchor_store.observation_for_test(filter_id)
    }

    fn invalidate_pcr_clock_anchor(&mut self, filter_id: i32) {
        let prepared = self
            .pcr_clock_anchor_store
            .prepare_invalidate_filter(filter_id);
        self.pcr_clock_anchor_store.commit_invalidation(prepared);
    }

    fn invalidate_all_pcr_clock_anchors(&mut self) {
        let prepared = self.pcr_clock_anchor_store.prepare_invalidate_all();
        self.pcr_clock_anchor_store.commit_invalidation(prepared);
    }

    fn observe_pcr_clock(
        &mut self,
        validated: &crate::packet_pipeline::ValidatedTsPacket<'_>,
        origin: TsInputOrigin,
        suppression_reasons: &[crate::packet_pipeline::PipelineAssemblySuppressionReason],
    ) {
        use crate::packet_pipeline::PipelineAssemblySuppressionReason as Suppression;

        let view = validated.view();
        let packet_pid = validated.pid();
        let targets: Vec<(i32, u64)> = self
            .filters
            .values()
            .filter_map(|filter| {
                let snapshot = filter.snapshot();
                (snapshot.open_type == FilterOpenType::TsPcr
                    && snapshot.state.is_started()
                    && filter
                        .pipeline_view()
                        .accepts_packet_pid_from_origin(packet_pid, origin))
                .then_some((filter.filter_id(), snapshot.generation))
            })
            .collect();
        if view.discontinuity_indicator() {
            for (filter_id, _) in targets {
                self.invalidate_pcr_clock_anchor(filter_id);
            }
            return;
        }
        if suppression_reasons.iter().any(|reason| {
            matches!(
                reason,
                Suppression::TransportErrorIndicator
                    | Suppression::DuplicatePacket
                    | Suppression::ContinuityCounterCollision
            )
        }) {
            return;
        }
        let Some(raw_pcr_base_33) = view.pcr_base_90khz() else {
            return;
        };
        for (filter_id, generation) in targets {
            match self.pcr_clock_anchor_store.observe(
                filter_id,
                generation,
                raw_pcr_base_33,
                false,
            ) {
                PcrObservationOutcome::Observed
                | PcrObservationOutcome::Invalidated
                | PcrObservationOutcome::ClockUnavailable => {}
                PcrObservationOutcome::StaleGeneration => {
                    self.quarantine_filter_runtime(filter_id);
                }
            }
        }
    }

    pub fn new(demux_id: i32, generation: u64) -> Self {
        Self::new_with_av_data_id_allocator(
            demux_id,
            generation,
            Arc::new(AvDataIdAllocator::default()),
        )
    }

    pub fn new_with_av_data_id_allocator(
        demux_id: i32,
        generation: u64,
        av_data_id_allocator: Arc<AvDataIdAllocator>,
    ) -> Self {
        Self::new_with_av_runtime_limits(
            demux_id,
            generation,
            av_data_id_allocator,
            Arc::new(AvRuntimeBudget::unlimited()),
            DEFAULT_AV_MAX_EVENT_BYTES,
            DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER,
            DEFAULT_AV_PER_FILTER_LIVE_BYTES,
        )
    }

    pub fn new_with_av_runtime_limits(
        demux_id: i32,
        generation: u64,
        av_data_id_allocator: Arc<AvDataIdAllocator>,
        av_runtime_budget: Arc<AvRuntimeBudget>,
        av_max_event_bytes: usize,
        av_max_outstanding_events_per_filter: usize,
        av_per_filter_live_bytes: usize,
    ) -> Self {
        Self {
            demux_id,
            state: DemuxRuntimeState::Open,
            stream_boundary: super::generation_boundary::StreamBoundaryTxn::new(generation),
            pipeline: PacketPipeline::default(),
            filters: BTreeMap::new(),
            dvrs: BTreeMap::new(),
            filter_producer_gates: BTreeMap::new(),
            #[cfg(test)]
            filter_queue_mirror: BTreeMap::new(),
            filter_queue_runtimes: BTreeMap::new(),
            dvr_queue_runtimes: BTreeMap::new(),
            filter_av_backings: BTreeMap::new(),
            av_data_id_allocator,
            av_runtime_budget,
            av_max_event_bytes,
            av_max_outstanding_events_per_filter,
            av_per_filter_live_bytes,
            pcr_clock_anchor_store: PcrClockAnchorStore::default(),
            av_sync_registry: AvSyncRegistry::default(),
            rollback_snapshots: BTreeMap::new(),
            next_rollback_token_id: 1,
            #[cfg(test)]
            next_record_relation_commit_fault: None,
        }
    }
    pub fn demux_id(&self) -> i32 {
        self.demux_id
    }
    pub fn state(&self) -> DemuxRuntimeState {
        self.state
    }
    pub fn generation(&self) -> u64 {
        self.stream_boundary.generation()
    }
    #[cfg(test)]
    pub(crate) fn pipeline(&self) -> &PacketPipeline {
        &self.pipeline
    }
    #[cfg(test)]
    pub(crate) fn pipeline_mut(&mut self) -> &mut PacketPipeline {
        &mut self.pipeline
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
        self.release_filter_av_handle(request.filter_id, request.descriptor, request.av_data_id)
    }

    pub(crate) fn release_filter_av_handle(
        &mut self,
        filter_id: i32,
        descriptor: AvHandleReleaseDescriptor,
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
        if let Some(backing) = self.filter_av_backings.get_mut(&filter_id) {
            return Ok(backing.apply_release(descriptor, data_id, filter_state));
        }
        if filter_state == AvFilterReleaseState::OpenAv {
            return Err(DemuxRuntimeError::av_backing_failure(filter_id));
        }
        let fallback_outcome = AvHandleReleaseTxn::classify(AvHandleReleaseInput {
            handle_kind: match descriptor {
                AvHandleReleaseDescriptor::Empty => AvHandleReleaseKind::Empty,
                AvHandleReleaseDescriptor::File(_) => AvHandleReleaseKind::UnknownFile,
            },
            data_id,
            client_state: ClientHandleState::NotExported,
            filter_state,
            data_id_state: AvDataIdState::Unknown,
        });
        Ok(fallback_outcome)
    }

    pub fn take_filter_av_backing_for_release_only(
        &mut self,
        filter_id: i32,
    ) -> Option<AvSharedBacking> {
        self.filter_av_backings.remove(&filter_id)
    }

    pub fn restore_filter_av_backing_after_failed_remove(
        &mut self,
        filter_id: i32,
        backing: AvSharedBacking,
    ) {
        self.filter_av_backings.insert(filter_id, backing);
    }

    pub fn snapshot(&self) -> DemuxRuntimeSnapshot {
        DemuxRuntimeSnapshot {
            state: self.state,
            stream_boundary_generation: self.stream_boundary.generation(),
            pipeline: self.pipeline.clone(),
            filters: self.filters.clone(),
            dvrs: self.dvrs.clone(),
            filter_producer_gates: self.filter_producer_gates.clone(),
            filter_queue_runtimes: self.filter_queue_runtimes.clone(),
            dvr_queue_runtimes: self.dvr_queue_runtimes.clone(),
            pcr_clock_anchor_store: self.pcr_clock_anchor_store.clone(),
            av_sync_registry: self.av_sync_registry.clone(),
            #[cfg(test)]
            filter_queue_mirror: self.filter_queue_mirror.clone(),
        }
    }

    pub fn rollback_token_from_typed_request(
        &mut self,
        request: DemuxRuntimeRollbackTokenPrepareRequest,
    ) -> Result<DemuxRuntimeRollbackToken, DemuxRuntimeError> {
        if request.demux_id != self.demux_id {
            return Err(DemuxRuntimeError::invalid_state(request.demux_id));
        }
        let token_id = self.next_rollback_token_id;
        self.next_rollback_token_id =
            self.next_rollback_token_id
                .checked_add(1)
                .ok_or(DemuxRuntimeError {
                    kind: DemuxRuntimeErrorKind::GenerationExhausted,
                    id: Some(self.demux_id),
                })?;
        let snapshot = self.snapshot();
        let generation = snapshot.generation();
        self.rollback_snapshots.insert(token_id, snapshot);
        Ok(DemuxRuntimeRollbackToken::new(
            self.demux_id,
            token_id,
            generation,
        ))
    }

    pub fn restore_from_rollback_request(
        &mut self,
        request: DemuxRuntimeRollbackRestoreRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.restore_from_rollback_token(request.token)
    }

    pub fn commit_rollback_request(
        &mut self,
        request: DemuxRuntimeRollbackCommitRequest,
    ) -> Result<(), DemuxRuntimeError> {
        let token = request.token;
        if token.demux_id != self.demux_id {
            return Err(DemuxRuntimeError::invalid_state(token.demux_id));
        }
        let snapshot = self
            .rollback_snapshots
            .remove(&token.token_id)
            .ok_or(DemuxRuntimeError::invalid_state(self.demux_id))?;
        if snapshot.generation() != token.generation {
            self.rollback_snapshots.clear();
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        Ok(())
    }

    pub(crate) fn restore_from_rollback_token(
        &mut self,
        token: DemuxRuntimeRollbackToken,
    ) -> Result<(), DemuxRuntimeError> {
        if token.demux_id != self.demux_id {
            return Err(DemuxRuntimeError::invalid_state(token.demux_id));
        }
        let snapshot = self
            .rollback_snapshots
            .remove(&token.token_id)
            .ok_or(DemuxRuntimeError::invalid_state(self.demux_id))?;
        if snapshot.generation() != token.generation {
            self.rollback_snapshots.clear();
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        self.rollback_snapshots.clear();
        self.restore(snapshot)
    }

    pub(crate) fn restore(
        &mut self,
        snapshot: DemuxRuntimeSnapshot,
    ) -> Result<(), DemuxRuntimeError> {
        let mut filter_av_backings = std::mem::take(&mut self.filter_av_backings);
        filter_av_backings.retain(|filter_id, _| {
            snapshot
                .filters
                .get(filter_id)
                .map(|filter| filter.av_backing_present())
                .unwrap_or(false)
        });

        self.state = snapshot.state;
        self.stream_boundary
            .restore(snapshot.stream_boundary_generation);
        self.pipeline = snapshot.pipeline;
        self.filters = snapshot.filters;
        self.dvrs = snapshot.dvrs;
        self.filter_producer_gates = snapshot.filter_producer_gates;
        #[cfg(test)]
        {
            self.filter_queue_mirror = snapshot.filter_queue_mirror;
        }
        self.filter_queue_runtimes = snapshot.filter_queue_runtimes;
        self.dvr_queue_runtimes = snapshot.dvr_queue_runtimes;
        self.filter_av_backings = filter_av_backings;
        self.pcr_clock_anchor_store = snapshot.pcr_clock_anchor_store;
        self.av_sync_registry = snapshot.av_sync_registry;
        Ok(())
    }

    fn register_filter_with_pending_event_capacity(
        &mut self,
        filter: FilterRuntime,
        pending_event_capacity: usize,
    ) -> Result<(), DemuxRuntimeError> {
        if filter.state().is_closed_or_failed() {
            return Err(DemuxRuntimeError::invalid_state(filter.filter_id()));
        }
        let filter_id = filter.filter_id();
        let gate = FilterProducerDrainGate::new(pending_event_capacity)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        self.filter_queue_runtimes.remove(&filter_id);
        #[cfg(test)]
        {
            self.filter_queue_mirror.remove(&filter_id);
        }
        self.filter_av_backings.remove(&filter_id);
        self.invalidate_pcr_clock_anchor(filter_id);
        self.unregister_av_sync_filter(filter_id);
        let has_av_backing = matches!(filter.open_kind(), PipelineOpenKind::Av);
        let has_pcr_sync_id = matches!(filter.open_kind(), PipelineOpenKind::Pcr)
            && matches!(
                filter.state(),
                FilterRuntimeState::Configured
                    | FilterRuntimeState::Started
                    | FilterRuntimeState::Stopped
            );
        let has_media_sync_relation = matches!(filter.open_kind(), PipelineOpenKind::Av)
            && matches!(
                filter.state(),
                FilterRuntimeState::Configured
                    | FilterRuntimeState::Started
                    | FilterRuntimeState::Stopped
            );
        self.filters.insert(filter_id, filter);
        self.filter_producer_gates.insert(filter_id, gate);
        if has_pcr_sync_id {
            self.register_av_sync_pcr_filter(filter_id)?;
        } else if has_media_sync_relation {
            self.register_av_sync_media_filter(filter_id)?;
        }
        if has_av_backing {
            self.filter_av_backings.insert(
                filter_id,
                AvSharedBacking::with_runtime_limits(
                    self.av_max_event_bytes,
                    self.av_max_outstanding_events_per_filter,
                    self.av_per_filter_live_bytes,
                    Arc::clone(&self.av_data_id_allocator),
                    Arc::clone(&self.av_runtime_budget),
                ),
            );
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
        self.rebuild_filter_queue_runtime(filter_id)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn register_filter(
        &mut self,
        filter: FilterRuntime,
    ) -> Result<(), DemuxRuntimeError> {
        self.register_filter_with_pending_event_capacity(
            filter,
            TEST_PENDING_FILTER_EVENT_CAPACITY,
        )
    }

    pub fn register_filter_from_typed_request(
        &mut self,
        request: FilterRuntimeRegistrationRequest<'_>,
    ) -> Result<(), DemuxRuntimeError> {
        self.register_filter_with_pending_event_capacity(
            FilterRuntime::new_open_request(
                request.filter_id,
                self.generation(),
                request.request,
            ),
            request.pending_event_capacity,
        )
    }

    #[cfg(test)]
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
        let source_generation = self
            .filters
            .get(&filter_id)
            .map(FilterRuntime::generation)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        let had_downstreams = !self.source_filter_downstream_ids(filter_id).is_empty();
        self.disconnect_source_filter_downstreams(filter_id)?;
        if had_downstreams {
            self.pipeline.reset_origin(TsInputOrigin::SourceFilter {
                source_filter_id: filter_id,
                source_filter_generation: source_generation,
            });
        }
        self.pipeline
            .remove_filter(filter_id)
            .map_err(|_| DemuxRuntimeError::pipeline_failed())?;
        if let Some(gate) = self.filter_producer_gates.get(&filter_id) {
            gate
                .close()
                .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        }
        self.filter_producer_gates.remove(&filter_id);
        self.filter_queue_runtimes.remove(&filter_id);
        #[cfg(test)]
        {
            self.filter_queue_mirror.remove(&filter_id);
        }
        self.filter_av_backings.remove(&filter_id);
        self.invalidate_pcr_clock_anchor(filter_id);
        self.unregister_av_sync_filter(filter_id);
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
        self.dvr_queue_runtimes.remove(&dvr_id);
        self.dvrs.insert(dvr_id, dvr);
        self.rebuild_dvr_queue_runtime(dvr_id)?;
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
        let kind = self
            .dvrs
            .get(&dvr_id)
            .map(DvrRuntime::kind)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        let playback_coordinates = if kind == DvrKind::Playback {
            Some(
                self.dvr_queue_runtimes
                    .get(&dvr_id)
                    .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?
                    .playback_coordinates()
                    .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?,
            )
        } else {
            None
        };
        if let Some(queue) = self.dvr_queue_runtimes.get(&dvr_id) {
            queue
                .close_dvr_protocol()
                .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        }
        self.dvr_queue_runtimes.remove(&dvr_id);
        self.dvrs.remove(&dvr_id);
        if let Some((queue_identity, queue_epoch)) = playback_coordinates {
            self.pipeline.reset_origin(TsInputOrigin::PlaybackDvr {
                dvr_id,
                queue_identity,
                queue_epoch,
            });
            self.invalidate_all_pcr_clock_anchors();
        }
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
        self.rebuild_filter_queue_runtime(filter_id)?;
        Ok(())
    }

    pub fn queue_exists(&self, filter_id: i32) -> bool {
        self.filters
            .get(&filter_id)
            .is_some_and(FilterRuntime::queue_present)
    }

    pub(super) fn validate_filter_delivery_boundary(
        &self,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        let filter = self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        if !self.filter_producer_gates.contains_key(&filter_id) {
            return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
        }
        if filter.queue_present()
            && !self
                .filter_queue_runtimes
                .get(&filter_id)
                .is_some_and(|queue| queue.capacity_matches_buffer_size(filter.buffer_size()))
        {
            return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
        }
        if filter.open_kind() == PipelineOpenKind::Av
            && !self.filter_av_backings.contains_key(&filter_id)
        {
            return Err(DemuxRuntimeError::av_backing_failure(filter_id));
        }
        if filter.open_kind() == PipelineOpenKind::Record
            && self
                .attached_record_dvr_ids_for_filter(filter_id)
                .iter()
                .any(|dvr_id| !self.dvr_queue_runtimes.contains_key(dvr_id))
        {
            return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
        }
        Ok(())
    }

    pub(crate) fn clear_existing_filter_queue(
        &mut self,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        self.validate_filter_delivery_boundary(filter_id)?;
        let queue_present = self
            .filters
            .get(&filter_id)
            .is_some_and(FilterRuntime::queue_present);
        let gate = self
            .filter_producer_gates
            .get(&filter_id)
            .cloned()
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        let mut drain = gate
            .begin_drain(FilterDrainBoundary::Reconfigure)
            .map_err(|_| {
                self.quarantine_filter_runtime(filter_id);
                DemuxRuntimeError::queue_runtime_failure(filter_id)
            })?;
        if queue_present && self.clear_filter_queue_runtime(filter_id).is_err() {
            self.quarantine_filter_runtime(filter_id);
            return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
        }
        let pending_events = match drain.take_pending_events() {
            Ok(events) => events,
            Err(_) => {
                self.quarantine_filter_runtime(filter_id);
                return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
            }
        };
        if self
            .discard_undelivered_filter_events(filter_id, pending_events)
            .is_err()
        {
            self.quarantine_filter_runtime(filter_id);
            return Err(DemuxRuntimeError::av_backing_failure(filter_id));
        }
        #[cfg(test)]
        {
            if let Some(queue) = self.filter_queue_mirror.get_mut(&filter_id) {
                queue.clear();
            }
        }
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.clear_queued_payload_state();
            filter.reset_section_delivery_state();
            filter.reset_audio_timestamp_association();
            filter.clear_pending_start_id();
        }
        if drain.commit().is_err() {
            self.quarantine_filter_runtime(filter_id);
            return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
        }
        Ok(())
    }

    pub(crate) fn apply_filter_source_stream_boundary(
        &mut self,
        filter_id: i32,
    ) -> Result<PipelineResetReport, DemuxRuntimeError> {
        let prepared = self
            .stream_boundary
            .prepare_filter_source_boundary(filter_id);
        let filter_id = self
            .stream_boundary
            .consume_filter_source_boundary(prepared)
            .ok_or(DemuxRuntimeError::invalid_state(self.demux_id))?;
        self.clear_existing_filter_queue(filter_id)?;
        self.reset_filter_source_boundary(filter_id)
    }

    pub(crate) fn enqueue_filter_queue_payload(
        &mut self,
        filter_id: i32,
        payload: Vec<u8>,
    ) -> Result<(), DemuxRuntimeError> {
        self.preflight_filter_queue_payload(filter_id, payload.len())
            .map_err(FilterQueuePayloadError::runtime_error)?;
        let gate = self
            .filter_producer_gates
            .get(&filter_id)
            .cloned()
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        let mut permit = gate
            .begin_producer()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        self.enqueue_filter_queue_payload_with_permit(filter_id, payload, &mut permit)
            .map_err(FilterQueuePayloadError::runtime_error)?;
        permit
            .commit()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))
    }

    fn preflight_filter_queue_payload(
        &self,
        filter_id: i32,
        payload_len: usize,
    ) -> Result<(), FilterQueuePayloadError> {
        if !self.filters.contains_key(&filter_id) {
            return Err(FilterQueuePayloadError::Runtime(
                DemuxRuntimeError::filter_missing(filter_id),
            ));
        }
        let queue = self
            .filter_queue_runtimes
            .get(&filter_id)
            .ok_or_else(|| {
                FilterQueuePayloadError::Runtime(DemuxRuntimeError::queue_missing(filter_id))
            })?;
        if queue.retry_pending_wake(TUNER_EVENT_DATA_READY).is_err() {
            return Err(FilterQueuePayloadError::Runtime(
                DemuxRuntimeError::queue_runtime_failure(filter_id),
            ));
        }
        let available = queue
            .available_to_write()
            .map_err(|_| {
                FilterQueuePayloadError::Runtime(DemuxRuntimeError::queue_runtime_failure(
                    filter_id,
                ))
            })?;
        if available < payload_len {
            return Err(FilterQueuePayloadError::Overflow(
                DemuxRuntimeError::queue_runtime_failure(filter_id),
            ));
        }
        Ok(())
    }

    fn enqueue_filter_queue_payload_with_permit(
        &mut self,
        filter_id: i32,
        payload: Vec<u8>,
        _permit: &mut FilterProducerPermit,
    ) -> Result<(), FilterQueuePayloadError> {
        let Some(queue) = self.filter_queue_runtimes.get(&filter_id) else {
            return Err(FilterQueuePayloadError::Runtime(
                DemuxRuntimeError::queue_missing(filter_id),
            ));
        };
        let result = FmqDeliveryTxn::new(FmqObjectKind::Filter).commit_payload(
            payload.len(),
            queue
                .write_checked(&payload)
                .map_err(|_| FmqFailureKind::WriteFailed),
            queue
                .wake(TUNER_EVENT_DATA_READY)
                .map_err(|_| FmqFailureKind::EventFlagWakeFailed),
        );
        match result.action {
            FmqDeliveryAction::Continue | FmqDeliveryAction::WakePending => {
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
            FmqDeliveryAction::Overflow => Err(FilterQueuePayloadError::Overflow(
                DemuxRuntimeError::queue_runtime_failure(filter_id),
            )),
            FmqDeliveryAction::RuntimeFailed(_) => {
                if let Some(filter) = self.filters.get_mut(&filter_id) {
                    filter.mark_failed();
                }
                Err(FilterQueuePayloadError::Runtime(
                    DemuxRuntimeError::queue_runtime_failure(filter_id),
                ))
            }
        }
    }

    fn committed_filter_status_events(
        &mut self,
        filter_id: i32,
    ) -> Result<Vec<PipelineGeneratedEvent>, DemuxRuntimeError> {
        let queue = self
            .filter_queue_runtimes
            .get(&filter_id)
            .ok_or_else(|| DemuxRuntimeError::queue_missing(filter_id))?;
        let capacity_bytes = queue.capacity_bytes();
        let readable_bytes = queue
            .availability_snapshot()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?
            .readable_bytes;
        let filter = self
            .filters
            .get_mut(&filter_id)
            .ok_or_else(|| DemuxRuntimeError::filter_missing(filter_id))?;
        let mut events = vec![PipelineGeneratedEvent::FilterStatus {
            filter_id,
            status: FilterStatusEvent::DataReady,
        }];
        if let Some(status) =
            filter.classify_watermark_transition(capacity_bytes, readable_bytes)
        {
            events.push(PipelineGeneratedEvent::FilterStatus { filter_id, status });
        }
        Ok(events)
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
            .available_to_read()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        if available == 0 {
            return Ok(Vec::new());
        }
        let mut drained = vec![0u8; available];
        let read = queue
            .read_into(&mut drained)
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
        self.filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?
            .restore(snapshot);
        self.rebuild_filter_queue_runtime(filter_id)?;
        Ok(())
    }

    pub(crate) fn restore_dvr_snapshot(
        &mut self,
        dvr_id: i32,
        snapshot: DvrRuntimeSnapshot,
    ) -> Result<(), DemuxRuntimeError> {
        self.dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?
            .restore(snapshot);
        self.rebuild_dvr_queue_runtime(dvr_id)?;
        Ok(())
    }

    pub fn configure_dvr_status_reporting_from_typed_request(
        &mut self,
        request: DvrStatusReportingRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.configure_dvr_status_reporting(
            request.dvr_id,
            request.status_mask,
            request.low_threshold_bytes,
            request.high_threshold_bytes,
            request.data_format,
            request.packet_size,
        )
    }

    pub(crate) fn configure_dvr_status_reporting(
        &mut self,
        dvr_id: i32,
        status_mask: i32,
        low_threshold_bytes: usize,
        high_threshold_bytes: usize,
        data_format: DvrDataFormat,
        packet_size: i64,
    ) -> Result<(), DemuxRuntimeError> {
        self.dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?
            .configure_settings(
                status_mask,
                low_threshold_bytes,
                high_threshold_bytes,
                data_format,
                packet_size,
            );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn configure_filter_runtime(
        &mut self,
        filter_id: i32,
        config: FilterPipelineConfig,
    ) -> Result<(), DemuxRuntimeError> {
        self.configure_filter_runtime_with_pes_stream_id(filter_id, config, None)
    }

    #[cfg(test)]
    pub(crate) fn configure_filter_runtime_with_pes_stream_id(
        &mut self,
        filter_id: i32,
        config: FilterPipelineConfig,
        pes_stream_id: Option<i32>,
    ) -> Result<(), DemuxRuntimeError> {
        let section_config = self
            .filters
            .get(&filter_id)
            .is_some_and(|filter| filter.open_kind() == PipelineOpenKind::Section)
            .then(crate::config::SectionRuntimeConfig::match_all_repeat);
        self.configure_filter_runtime_with_full_config(
            filter_id,
            config,
            pes_stream_id,
            section_config,
        )
    }

    pub(crate) fn configure_filter_runtime_with_full_config(
        &mut self,
        filter_id: i32,
        config: FilterPipelineConfig,
        pes_stream_id: Option<i32>,
        section_config: Option<crate::config::SectionRuntimeConfig>,
    ) -> Result<(), DemuxRuntimeError> {
        let (
            current_generation,
            queue_present,
            av_backing_present,
            is_pcr_filter,
            is_media_filter,
            has_downstreams,
        ) = {
            let filter = self
                .filters
                .get(&filter_id)
                .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
            let queue_present = filter.supports_normal_fmq_queue() && filter.buffer_size() > 0;
            if queue_present
                && !self
                    .filter_queue_runtimes
                    .get(&filter_id)
                    .is_some_and(|queue| queue.capacity_matches_buffer_size(filter.buffer_size()))
            {
                return Err(DemuxRuntimeError::queue_missing(filter_id));
            }
            (
                filter.generation(),
                queue_present,
                matches!(filter.open_kind(), PipelineOpenKind::Av),
                matches!(filter.open_kind(), PipelineOpenKind::Pcr),
                matches!(filter.open_kind(), PipelineOpenKind::Av),
                !self.source_filter_downstream_ids(filter_id).is_empty(),
            )
        };
        let next = match next_generation(current_generation) {
            Ok(next) => next,
            Err(_) => {
                self.quarantine_filter_runtime(filter_id);
                return Err(DemuxRuntimeError::generation_exhausted(Some(filter_id)));
            }
        };
        if av_backing_present && !self.filter_av_backings.contains_key(&filter_id) {
            self.quarantine_filter_runtime(filter_id);
            return Err(DemuxRuntimeError::av_backing_failure(filter_id));
        }
        let gate = self
            .filter_producer_gates
            .get(&filter_id)
            .cloned()
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        let mut drain = gate
            .begin_drain(FilterDrainBoundary::Reconfigure)
            .map_err(|_| {
                self.quarantine_filter_runtime(filter_id);
                DemuxRuntimeError::queue_runtime_failure(filter_id)
            })?;
        if queue_present {
            if self
                .filter_queue_runtimes
                .get(&filter_id)
                .ok_or(DemuxRuntimeError::queue_missing(filter_id))?
                .clear_contents()
                .is_err()
            {
                self.quarantine_filter_runtime(filter_id);
                return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
            }
        }
        let pending_events = match drain.take_pending_events() {
            Ok(events) => events,
            Err(_) => {
                self.quarantine_filter_runtime(filter_id);
                return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
            }
        };
        if self
            .discard_undelivered_filter_events(filter_id, pending_events)
            .is_err()
        {
            self.quarantine_filter_runtime(filter_id);
            return Err(DemuxRuntimeError::av_backing_failure(filter_id));
        }
        if self
            .pipeline
            .configure_filter(filter_id, config.clone())
            .is_err()
        {
            if gate.close().is_err() {
                self.state = DemuxRuntimeState::Quarantined;
            }
            if let Some(filter) = self.filters.get_mut(&filter_id) {
                filter.mark_failed();
            }
            return Err(DemuxRuntimeError::pipeline_failed());
        }
        let Some(filter) = self.filters.get_mut(&filter_id) else {
            self.quarantine_filter_runtime(filter_id);
            return Err(DemuxRuntimeError::filter_missing(filter_id));
        };
        filter.configure_with_generation(next, config, pes_stream_id);
        filter.set_section_runtime_config(section_config);
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
        if !av_backing_present {
            self.filter_av_backings.remove(&filter_id);
        }
        if drain.commit().is_err() {
            if gate.close().is_err() {
                self.state = DemuxRuntimeState::Quarantined;
            }
            if let Some(filter) = self.filters.get_mut(&filter_id) {
                filter.mark_failed();
            }
            return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
        }
        if has_downstreams {
            self.pipeline.reset_origin(TsInputOrigin::SourceFilter {
                source_filter_id: filter_id,
                source_filter_generation: current_generation,
            });
        }
        self.refresh_source_filter_downstreams(filter_id, next)?;
        self.unregister_av_sync_filter(filter_id);
        if is_pcr_filter {
            self.register_av_sync_pcr_filter(filter_id)?;
        } else if is_media_filter {
            self.register_av_sync_media_filter(filter_id)?;
        }
        self.invalidate_pcr_clock_anchor(filter_id);
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

    pub fn schedule_filter_start_id_after_reconfigure(
        &mut self,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        self.filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?
            .schedule_start_id_after_reconfigure()
            .map_err(|_| DemuxRuntimeError::generation_exhausted(Some(filter_id)))
    }

    pub fn pending_filter_start_id(
        &self,
        filter_id: i32,
    ) -> Result<Option<i32>, DemuxRuntimeError> {
        Ok(self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?
            .pending_start_id())
    }

    pub fn commit_pending_filter_start_id(
        &mut self,
        filter_id: i32,
        expected_start_id: i32,
    ) -> Result<bool, DemuxRuntimeError> {
        Ok(self
            .filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?
            .commit_pending_start_id(expected_start_id))
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
                self.pipeline
                    .start_filter(filter_id)
                    .map_err(|_| DemuxRuntimeError::pipeline_failed())?;
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
                report.failed(FilterRuntimeOperationStep::ValidateState, error.kind);
                report.finish(FilterRuntimeOperationOutcome::Failed {
                    failed_step: FilterRuntimeOperationStep::ValidateState,
                });
                return (report, Err(error));
            }
        };
        match snapshot.state {
            FilterRuntimeState::Started => {
                if let Err(_error) = self.pipeline.stop_filter(filter_id) {
                    let error = DemuxRuntimeError::pipeline_failed();
                    report.failed(FilterRuntimeOperationStep::PipelineStop, error.kind);
                    report.finish(FilterRuntimeOperationOutcome::Failed {
                        failed_step: FilterRuntimeOperationStep::PipelineStop,
                    });
                    return (report, Err(error));
                }
                report.succeeded(FilterRuntimeOperationStep::PipelineStop);
                report.skipped(
                    FilterRuntimeOperationStep::QueueClear,
                    FilterRuntimeOperationSkipReason::StopPreservesQueue,
                );
                report.skipped(
                    FilterRuntimeOperationStep::MirrorQueueClear,
                    FilterRuntimeOperationSkipReason::StopPreservesQueue,
                );
                let filter = match self.filters.get_mut(&filter_id) {
                    Some(filter) => filter,
                    None => {
                        let error = DemuxRuntimeError::filter_missing(filter_id);
                        report.failed(FilterRuntimeOperationStep::QueuedPayloadClear, error.kind);
                        report.finish(FilterRuntimeOperationOutcome::Failed {
                            failed_step: FilterRuntimeOperationStep::QueuedPayloadClear,
                        });
                        return (report, Err(error));
                    }
                };
                report.skipped(
                    FilterRuntimeOperationStep::QueuedPayloadClear,
                    FilterRuntimeOperationSkipReason::StopPreservesQueue,
                );
                filter.mark_stopped();
                self.invalidate_pcr_clock_anchor(filter_id);
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
                report.failed(FilterRuntimeOperationStep::ValidateState, error.kind);
                report.finish(FilterRuntimeOperationOutcome::Failed {
                    failed_step: FilterRuntimeOperationStep::ValidateState,
                });
                (report, Err(error))
            }
        }
    }

    pub fn prepare_filter_queue_cleanup(
        &mut self,
        request: FilterRuntimeOperationRequest,
    ) -> Result<FilterQueueCleanupPlan, DemuxRuntimeError> {
        let filter_id = request.filter_id;
        let snapshot = self.filter_snapshot(filter_id)?;
        match snapshot.state {
            FilterRuntimeState::Configured
            | FilterRuntimeState::Started
            | FilterRuntimeState::Stopped => {}
            FilterRuntimeState::Open => {
                return Err(DemuxRuntimeError::invalid_state(filter_id));
            }
            FilterRuntimeState::Closing
            | FilterRuntimeState::CleanupFailed
            | FilterRuntimeState::Closed
            | FilterRuntimeState::Failed => {
                return Err(DemuxRuntimeError::sink_lifecycle(filter_id));
            }
        }
        let next_source_generation = if self.source_filter_downstream_ids(filter_id).is_empty() {
            None
        } else {
            match next_generation(snapshot.generation) {
                Ok(next) => Some(next),
                Err(_) => {
                    self.quarantine_filter_runtime(filter_id);
                    return Err(DemuxRuntimeError::generation_exhausted(Some(filter_id)));
                }
            }
        };
        let gate = self
            .filter_producer_gates
            .get(&filter_id)
            .cloned()
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        let drain = match gate.begin_drain(FilterDrainBoundary::Flush) {
            Ok(drain) => drain,
            Err(_) => {
                self.quarantine_filter_runtime(filter_id);
                return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
            }
        };
        Ok(FilterQueueCleanupPlan {
            filter_id,
            snapshot,
            next_source_generation,
            drain,
        })
    }

    pub fn flush_filter_pipeline_for_queue_cleanup(
        &mut self,
        plan: &FilterQueueCleanupPlan,
    ) {
        if let (Some(origin), Some(tpid)) = (
            plan.snapshot.source.source_filter_origin(),
            plan.snapshot.tpid.and_then(ConfigInputPid::validate_tpid),
        ) {
            let origins = [(origin, tpid)];
            self.pipeline.flush_filter(plan.filter_id, &origins);
        } else {
            self.pipeline
                .clear_filter_state_after_flush(plan.filter_id);
        }
    }

    pub fn clear_filter_fmq_for_queue_cleanup(
        &mut self,
        plan: &FilterQueueCleanupPlan,
    ) -> Result<bool, DemuxRuntimeError> {
        if !plan.snapshot.queue_present {
            return Ok(false);
        }
        if let Err(error) = self.clear_filter_queue_runtime(plan.filter_id) {
            self.quarantine_filter_runtime(plan.filter_id);
            return Err(error);
        }
        Ok(true)
    }

    pub fn discard_filter_pending_events_for_queue_cleanup(
        &mut self,
        plan: &mut FilterQueueCleanupPlan,
    ) -> Result<(), DemuxRuntimeError> {
        let pending_events = match plan.drain.take_pending_events() {
            Ok(events) => events,
            Err(_) => {
                self.quarantine_filter_runtime(plan.filter_id);
                return Err(DemuxRuntimeError::queue_runtime_failure(plan.filter_id));
            }
        };
        if let Err(error) = self.discard_undelivered_filter_events(plan.filter_id, pending_events) {
            self.quarantine_filter_runtime(plan.filter_id);
            return Err(error);
        }
        Ok(())
    }

    pub fn clear_filter_payload_state_for_queue_cleanup(
        &mut self,
        plan: &FilterQueueCleanupPlan,
    ) -> FilterQueuePayloadCleanupOutcome {
        #[cfg(test)]
        if let Some(queue) = self.filter_queue_mirror.get_mut(&plan.filter_id) {
            queue.clear();
        }
        let filter_state_cleared = if let Some(filter) = self.filters.get_mut(&plan.filter_id) {
            filter.clear_queued_payload_state();
            filter.reset_section_delivery_state();
            filter.reset_audio_timestamp_association();
            filter.clear_pending_start_id();
            true
        } else {
            false
        };
        FilterQueuePayloadCleanupOutcome {
            filter_state_cleared,
        }
    }

    pub fn flush_filter_av_backing_for_queue_cleanup(
        &mut self,
        plan: &FilterQueueCleanupPlan,
    ) -> bool {
        if let Some(backing) = self.filter_av_backings.get_mut(&plan.filter_id) {
            backing.flush_slots_keep_exported_handle();
            true
        } else {
            false
        }
    }

    pub fn invalidate_filter_pcr_for_queue_cleanup(&mut self, plan: &FilterQueueCleanupPlan) {
        self.invalidate_pcr_clock_anchor(plan.filter_id);
    }

    pub fn commit_filter_producer_drain_for_queue_cleanup(
        &mut self,
        plan: FilterQueueCleanupPlan,
    ) -> Result<CommittedFilterQueueCleanup, DemuxRuntimeError> {
        let FilterQueueCleanupPlan {
            filter_id,
            snapshot,
            next_source_generation,
            drain,
        } = plan;
        if drain.commit().is_err() {
            self.quarantine_filter_runtime(filter_id);
            return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
        }
        Ok(CommittedFilterQueueCleanup {
            filter_id,
            source_generation: next_source_generation
                .map(|next_generation| (snapshot.generation, next_generation)),
        })
    }

    pub fn refresh_filter_source_generation_for_queue_cleanup(
        &mut self,
        committed: CommittedFilterQueueCleanup,
    ) -> Result<bool, DemuxRuntimeError> {
        let Some((previous_generation, next_generation)) = committed.source_generation else {
            return Ok(false);
        };
        self.pipeline.reset_origin(TsInputOrigin::SourceFilter {
            source_filter_id: committed.filter_id,
            source_filter_generation: previous_generation,
        });
        let Some(source_filter) = self.filters.get_mut(&committed.filter_id) else {
            return Err(DemuxRuntimeError::filter_missing(committed.filter_id));
        };
        source_filter.set_generation(next_generation);
        if let Err(error) =
            self.refresh_source_filter_downstreams(committed.filter_id, next_generation)
        {
            self.quarantine_filter_runtime(committed.filter_id);
            return Err(error);
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let mut plan =
            self.prepare_filter_queue_cleanup(FilterRuntimeOperationRequest::new(filter_id))?;
        self.flush_filter_pipeline_for_queue_cleanup(&plan);
        self.clear_filter_fmq_for_queue_cleanup(&plan)?;
        self.discard_filter_pending_events_for_queue_cleanup(&mut plan)?;
        let _ = self.clear_filter_payload_state_for_queue_cleanup(&plan);
        let _ = self.flush_filter_av_backing_for_queue_cleanup(&plan);
        self.invalidate_filter_pcr_for_queue_cleanup(&plan);
        let committed = self.commit_filter_producer_drain_for_queue_cleanup(plan)?;
        self.refresh_filter_source_generation_for_queue_cleanup(committed)
            .map(|_| ())
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
                    return Err(DemuxRuntimeError::generation_exhausted(Some(dvr_id)));
                }
            };
            if !self
                .dvr_queue_runtimes
                .get(&dvr_id)
                .is_some_and(|queue| queue.capacity_matches_buffer_size(dvr.buffer_size()))
            {
                return Err(DemuxRuntimeError::queue_missing(dvr_id));
            }
            next
        };
        #[cfg(test)]
        let playback_processing_buffer = {
            let dvr = self
                .dvrs
                .get(&dvr_id)
                .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
            let mut buffer = Vec::new();
            if dvr.kind() == DvrKind::Playback {
                let buffer_size = usize::try_from(dvr.buffer_size())
                    .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
                buffer
                    .try_reserve_exact(buffer_size)
                    .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
                buffer.resize(buffer_size, 0);
            }
            buffer
        };
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        dvr.configure_with_generation(next);
        #[cfg(test)]
        dvr.install_test_playback_processing_buffer(playback_processing_buffer);
        Ok(())
    }

    pub fn configure_dvr_runtime_with_typed_request(
        &mut self,
        request: DvrRuntimeConfigureRequest,
    ) -> (
        super::configure_txn::DvrConfigureReport,
        Result<super::configure_txn::DvrConfigureOutcome, DemuxRuntimeError>,
    ) {
        super::configure_txn::configure_dvr_runtime(self, request.dvr_id)
    }

    pub(crate) fn attach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        let prepared = self.prepare_dvr_filter_relation(
            DvrFilterLinkRequest::new(dvr_id, filter_id),
            true,
        )?;
        self.commit_prepared_dvr_filter_relation(prepared)
    }

    pub fn prepare_attach_dvr_filter_from_typed_request(
        &mut self,
        request: DvrFilterLinkRequest,
    ) -> Result<PreparedDvrFilterRelation, DemuxRuntimeError> {
        self.prepare_dvr_filter_relation(request, true)
    }

    fn prepare_dvr_filter_relation(
        &mut self,
        request: DvrFilterLinkRequest,
        attach: bool,
    ) -> Result<PreparedDvrFilterRelation, DemuxRuntimeError> {
        let dvr_id = request.dvr_id;
        let filter_id = request.filter_id;
        let dvr = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        if dvr.kind() != DvrKind::Record {
            return Err(DemuxRuntimeError::unsupported_dvr_operation(dvr_id));
        }
        if dvr.record_filter_relation_state() != RecordDvrFilterRelationState::Healthy {
            return Err(DemuxRuntimeError::invalid_state(dvr_id));
        }
        let filter = self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        if filter.state().is_closed_or_failed() {
            return Err(DemuxRuntimeError::invalid_state(filter_id));
        }
        if filter.open_kind() != PipelineOpenKind::Record {
            return Err(DemuxRuntimeError::invalid_dvr_filter(filter_id));
        }
        if attach && self.dvrs.iter().any(|(other_dvr_id, other_dvr)| {
            *other_dvr_id != dvr_id
                && other_dvr.kind() == DvrKind::Record
                && other_dvr.attached_record_filters().contains(&filter_id)
        }) {
            return Err(DemuxRuntimeError::unsupported_dvr_operation(dvr_id));
        }
        match dvr.state() {
            super::dvr::DvrRuntimeState::Open
            | super::dvr::DvrRuntimeState::Configured
            | super::dvr::DvrRuntimeState::Started
            | super::dvr::DvrRuntimeState::Stopped => {}
            super::dvr::DvrRuntimeState::Closing
            | super::dvr::DvrRuntimeState::CleanupFailed
            | super::dvr::DvrRuntimeState::Closed
            | super::dvr::DvrRuntimeState::Failed => {
                return Err(DemuxRuntimeError::invalid_state(dvr_id));
            }
        }
        let expected_generation = dvr.record_filter_relation_generation();
        let expected_filters = dvr.attached_record_filters().clone();
        let mut next_filters = expected_filters.clone();
        let changed = if attach {
            next_filters.insert(filter_id)
        } else {
            next_filters.remove(&filter_id)
        };
        let next_generation = if changed {
            match expected_generation.checked_add(1) {
                Some(next) => next,
                None => {
                    if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                        dvr.quarantine_record_filter_relation();
                    }
                    return Err(DemuxRuntimeError::generation_exhausted(Some(dvr_id)));
                }
            }
        } else {
            expected_generation
        };
        Ok(PreparedDvrFilterRelation {
            dvr_id,
            filter_id,
            expected_generation,
            next_generation,
            expected_filters,
            next_filters,
            changed,
            reset_record_index: attach && changed,
            #[cfg(test)]
            commit_fault: if changed {
                self.next_record_relation_commit_fault.take()
            } else {
                None
            },
        })
    }

    pub(crate) fn detach_dvr_filter(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        let prepared = self.prepare_dvr_filter_relation(
            DvrFilterLinkRequest::new(dvr_id, filter_id),
            false,
        )?;
        self.commit_prepared_dvr_filter_relation(prepared)
    }

    pub fn prepare_detach_dvr_filter_from_typed_request(
        &mut self,
        request: DvrFilterLinkRequest,
    ) -> Result<PreparedDvrFilterRelation, DemuxRuntimeError> {
        self.prepare_dvr_filter_relation(request, false)
    }

    pub fn commit_prepared_dvr_filter_relation(
        &mut self,
        prepared: PreparedDvrFilterRelation,
    ) -> Result<(), DemuxRuntimeError> {
        let dvr_id = prepared.dvr_id;
        let filter_id = prepared.filter_id;
        let expected_generation = prepared.expected_generation;
        let next_generation = prepared.next_generation;
        let expected_filters = prepared.expected_filters;
        let next_filters = prepared.next_filters;
        let changed = prepared.changed;
        let reset_record_index = prepared.reset_record_index;
        #[cfg(test)]
        let commit_fault = prepared.commit_fault;
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        if dvr.record_filter_relation_state() != RecordDvrFilterRelationState::Healthy
            || dvr.record_filter_relation_generation() != expected_generation
            || dvr.attached_record_filters() != &expected_filters
        {
            return Err(DemuxRuntimeError::invalid_state(dvr_id));
        }
        if !changed {
            return Ok(());
        }
        #[cfg(test)]
        if commit_fault == Some(RecordDvrFilterRelationCommitFault::RejectBeforeCommit) {
            return Err(DemuxRuntimeError::pipeline_failed());
        }
        dvr.commit_record_filter_relation(next_generation, next_filters);
        #[cfg(test)]
        if commit_fault == Some(RecordDvrFilterRelationCommitFault::UnknownAfterApply) {
            dvr.quarantine_record_filter_relation();
            return Err(DemuxRuntimeError::relation_commit_unknown(dvr_id));
        }
        if reset_record_index {
            self.pipeline.reset_record_index_state(filter_id);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_next_record_filter_relation_commit_fault(
        &mut self,
        fault: RecordDvrFilterRelationCommitFault,
    ) {
        self.next_record_relation_commit_fault = Some(fault);
    }

    pub fn start_dvr_runtime_from_typed_request(
        &mut self,
        request: DvrRuntimeOperationRequest,
    ) -> Result<(), DemuxRuntimeError> {
        self.start_dvr_runtime(request.dvr_id)
    }

    pub(crate) fn start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let snapshot = self.dvr_snapshot(dvr_id)?;
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
        let availability = queue
            .availability_snapshot()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        Ok(dvr.status_event_for_snapshot(
            availability.readable_bytes,
            availability.writable_bytes,
        ))
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

    pub fn prepare_dvr_queue_cleanup(
        &mut self,
        request: DvrRuntimeOperationRequest,
    ) -> Result<DvrQueueCleanupPlan, DemuxRuntimeError> {
        let dvr_id = request.dvr_id;
        let (state, kind, generation, attached_record_filters) = {
            let dvr = self
                .dvrs
                .get(&dvr_id)
                .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
            (
                dvr.state(),
                dvr.kind(),
                dvr.generation(),
                dvr.attached_record_filters()
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
        };
        match state {
            super::dvr::DvrRuntimeState::Configured
            | super::dvr::DvrRuntimeState::Started
            | super::dvr::DvrRuntimeState::Stopped => {
                if kind == DvrKind::Record
                    && state == super::dvr::DvrRuntimeState::Started
                {
                    return Err(DemuxRuntimeError::invalid_state(dvr_id));
                }
                let next_playback_generation = if kind == DvrKind::Playback {
                    match next_generation(generation) {
                        Ok(next) => Some(next),
                        Err(_) => {
                            self.quarantine_dvr_runtime(dvr_id);
                            return Err(DemuxRuntimeError::generation_exhausted(Some(dvr_id)));
                        }
                    }
                } else {
                    None
                };
                let playback_coordinates = if kind == DvrKind::Playback {
                    match self
                        .dvr_queue_runtimes
                        .get(&dvr_id)
                        .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?
                        .playback_coordinates()
                    {
                        Ok(coordinates) => Some(coordinates),
                        Err(_) => {
                            self.quarantine_dvr_runtime(dvr_id);
                            return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
                        }
                    }
                } else {
                    None
                };
                let drain = match self
                    .dvr_queue_runtimes
                    .get(&dvr_id)
                    .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?
                    .begin_dvr_drain()
                {
                    Ok(drain) => drain,
                    Err(_) => {
                        self.quarantine_dvr_runtime(dvr_id);
                        return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
                    }
                };
                Ok(DvrQueueCleanupPlan {
                    dvr_id,
                    kind,
                    next_playback_generation,
                    attached_record_filters,
                    playback_coordinates,
                    drain,
                })
            }
            super::dvr::DvrRuntimeState::Open => Err(DemuxRuntimeError::invalid_state(dvr_id)),
            super::dvr::DvrRuntimeState::Closing
            | super::dvr::DvrRuntimeState::CleanupFailed
            | super::dvr::DvrRuntimeState::Closed
            | super::dvr::DvrRuntimeState::Failed => Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
    }

    pub fn commit_dvr_queue_boundary_for_queue_cleanup(
        &mut self,
        plan: DvrQueueCleanupPlan,
    ) -> Result<CommittedDvrQueueCleanup, DvrQueueCleanupCommitError> {
        let DvrQueueCleanupPlan {
            dvr_id,
            kind,
            next_playback_generation,
            attached_record_filters,
            playback_coordinates,
            drain,
        } = plan;
        let Some(queue) = self.dvr_queue_runtimes.get(&dvr_id) else {
            self.quarantine_dvr_runtime(dvr_id);
            return Err(DvrQueueCleanupCommitError::new(
                DvrQueueCleanupStep::QueueClear,
                DemuxRuntimeError::queue_missing(dvr_id),
            ));
        };
        let queue_commit_result = queue.commit_dvr_drain_with_queue_clear(drain);
        let queue_dropped_bytes = match queue_commit_result {
            Ok(dropped_bytes) => dropped_bytes,
            Err(DvrQueueDrainCommitError::QueueClear) => {
                return Err(DvrQueueCleanupCommitError::new(
                    DvrQueueCleanupStep::QueueClear,
                    DemuxRuntimeError::queue_runtime_failure(dvr_id),
                ));
            }
            Err(DvrQueueDrainCommitError::EpochCommit) => {
                self.quarantine_dvr_runtime(dvr_id);
                return Err(DvrQueueCleanupCommitError::new(
                    DvrQueueCleanupStep::QueueEpochCommit,
                    DemuxRuntimeError::queue_runtime_failure(dvr_id),
                ));
            }
        };
        Ok(CommittedDvrQueueCleanup {
            dvr_id,
            kind,
            next_playback_generation,
            attached_record_filters,
            playback_coordinates,
            queue_dropped_bytes,
        })
    }

    pub fn commit_dvr_runtime_state_for_queue_cleanup(
        &mut self,
        committed: &CommittedDvrQueueCleanup,
    ) -> Result<(), DemuxRuntimeError> {
        let Some(dvr) = self.dvrs.get_mut(&committed.dvr_id) else {
            self.quarantine_dvr_runtime(committed.dvr_id);
            return Err(DemuxRuntimeError::dvr_missing(committed.dvr_id));
        };
        if committed.kind == DvrKind::Playback {
            #[cfg(test)]
            let assembler_dropped_bytes = dvr.drain_playback_completion_for_boundary();
            #[cfg(not(test))]
            let assembler_dropped_bytes = 0;
            dvr.reset_playback_stats_after_flush(
                committed
                    .queue_dropped_bytes
                    .saturating_add(assembler_dropped_bytes),
            );
            if let Some(next) = committed.next_playback_generation {
                dvr.set_generation(next);
            }
        } else {
            dvr.clear_pending_overflow();
        }
        Ok(())
    }

    pub fn reset_dvr_playback_pipeline_for_queue_cleanup(
        &mut self,
        committed: &CommittedDvrQueueCleanup,
    ) -> bool {
        let Some((queue_identity, queue_epoch)) = committed.playback_coordinates else {
            return false;
        };
        let origin = TsInputOrigin::PlaybackDvr {
            dvr_id: committed.dvr_id,
            queue_identity,
            queue_epoch,
        };
        self.pipeline.reset_origin(origin);
        for filter in self.filters.values_mut() {
            filter.reset_audio_timestamp_association_for_origin(origin);
        }
        true
    }

    pub fn invalidate_dvr_playback_pcr_for_queue_cleanup(
        &mut self,
        committed: &CommittedDvrQueueCleanup,
    ) -> bool {
        if !committed.is_playback() {
            return false;
        }
        self.invalidate_all_pcr_clock_anchors();
        true
    }

    pub fn reset_dvr_record_index_for_queue_cleanup(
        &mut self,
        committed: &CommittedDvrQueueCleanup,
    ) -> bool {
        if committed.kind != DvrKind::Record {
            return false;
        }
        for filter_id in &committed.attached_record_filters {
            self.pipeline.reset_record_index_state(*filter_id);
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let plan = self.prepare_dvr_queue_cleanup(DvrRuntimeOperationRequest::new(dvr_id))?;
        let committed = self
            .commit_dvr_queue_boundary_for_queue_cleanup(plan)
            .map_err(DvrQueueCleanupCommitError::error)?;
        self.commit_dvr_runtime_state_for_queue_cleanup(&committed)?;
        self.reset_dvr_playback_pipeline_for_queue_cleanup(&committed);
        self.invalidate_dvr_playback_pcr_for_queue_cleanup(&committed);
        self.reset_dvr_record_index_for_queue_cleanup(&committed);
        Ok(())
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
            | super::dvr::DvrRuntimeState::Stopped => {
                if dvr.kind() != DvrKind::Playback {
                    return Err(DemuxRuntimeError::invalid_state(dvr_id));
                }
            }
            _ => return Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
        let Some(queue) = self.dvr_queue_runtimes.get(&dvr_id) else {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.mark_failed();
            }
            return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
        };
        if queue.retry_pending_wake(TUNER_EVENT_DATA_READY).is_err() {
            return Ok(0);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let available = match queue.available_to_write() {
            Ok(available) => available,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        if available < data.len() {
            return Ok(0);
        }
        let transaction = queue
            .begin_dvr_write(data.len())
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        let result = FmqDeliveryTxn::new(FmqObjectKind::DvrPlayback).commit_payload(
            data.len(),
            queue
                .write_checked(data)
                .map_err(|_| FmqFailureKind::WriteFailed),
            queue
                .wake(TUNER_EVENT_DATA_READY)
                .map_err(|_| FmqFailureKind::EventFlagWakeFailed),
        );
        if matches!(
            result.action,
            FmqDeliveryAction::Continue | FmqDeliveryAction::WakePending
        ) {
            transaction
                .commit()
                .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        }
        match result.action {
            FmqDeliveryAction::Continue | FmqDeliveryAction::WakePending => Ok(result.bytes),
            FmqDeliveryAction::Overflow => Ok(0),
            FmqDeliveryAction::RuntimeFailed(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                Err(DemuxRuntimeError::queue_runtime_failure(dvr_id))
            }
        }
    }

    pub fn begin_playback_queue_read(
        &mut self,
        dvr_id: i32,
        max_bytes: usize,
    ) -> Result<Option<PlaybackQueueReadTxn>, DemuxRuntimeError> {
        if max_bytes == 0 {
            return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
        }
        let dvr = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        if dvr.kind() != DvrKind::Playback || dvr.state() != super::dvr::DvrRuntimeState::Started {
            return Ok(None);
        }
        let has_started_output = self.filters.values().any(|filter| {
            let view = filter.pipeline_view();
            view.started && view.source_filter.is_none()
        });
        if !has_started_output {
            return Ok(None);
        }
        let available = match self
            .dvr_queue_runtimes
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?
            .available_to_read()
        {
            Ok(available) => available,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        let read_limit = available.min(max_bytes);
        if read_limit == 0 {
            return Ok(None);
        }
        let token = match self
            .dvr_queue_runtimes
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?
            .begin_dvr_read(read_limit)
        {
            Ok(token) => token,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        let (queue_identity, queue_epoch) = match token.playback_coordinates() {
            Ok(coordinates) => coordinates,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        Ok(Some(PlaybackQueueReadTxn {
            dvr_id,
            token: Some(token),
            origin: TsInputOrigin::PlaybackDvr {
                dvr_id,
                queue_identity,
                queue_epoch,
            },
            read_limit,
        }))
    }

    pub fn read_playback_queue(
        &mut self,
        txn: &PlaybackQueueReadTxn,
        destination: &mut [u8],
    ) -> Result<usize, DemuxRuntimeError> {
        if destination.is_empty()
            || destination.len() > txn.read_limit
            || txn.token.is_none()
        {
            return Err(DemuxRuntimeError::queue_runtime_failure(txn.dvr_id));
        }
        match self
            .dvr_queue_runtimes
            .get(&txn.dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(txn.dvr_id))?
            .read_into(destination)
        {
            Ok(read) => Ok(read),
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&txn.dvr_id) {
                    dvr.mark_failed();
                }
                Err(DemuxRuntimeError::queue_runtime_failure(txn.dvr_id))
            }
        }
    }

    pub fn commit_playback_queue_read(
        &mut self,
        mut txn: PlaybackQueueReadTxn,
    ) -> Result<TsInputOrigin, DemuxRuntimeError> {
        let token = txn
            .token
            .take()
            .ok_or(DemuxRuntimeError::queue_runtime_failure(txn.dvr_id))?;
        token.commit().map_err(|_| {
            if let Some(dvr) = self.dvrs.get_mut(&txn.dvr_id) {
                dvr.mark_failed();
            }
            DemuxRuntimeError::queue_runtime_failure(txn.dvr_id)
        })?;
        Ok(txn.origin)
    }

    pub fn inject_playback_packet(
        &mut self,
        packet: &crate::packet_pipeline::ValidatedTsPacket<'_>,
        origin: TsInputOrigin,
    ) -> PipelineReport {
        self.push_validated_ts_packet_from_origin(packet, origin)
    }

    pub fn note_malformed_playback_packet(
        &mut self,
        reason: crate::packet_pipeline::TsPacketValidationError,
    ) -> PipelineReport {
        self.pipeline.note_malformed_ts_packet();
        PacketPipeline::malformed_ts_packet_report(reason)
    }

    pub fn note_playback_consume_result(
        &mut self,
        dvr_id: i32,
        injected_packets: usize,
        malformed_packets: usize,
        malformed_bytes: usize,
    ) -> Result<super::dvr::PlaybackStats, DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        dvr.note_playback_consume(injected_packets, malformed_packets, malformed_bytes);
        Ok(dvr.playback_stats())
    }

    pub fn note_playback_consume_boundary_discard(
        &mut self,
        dvr_id: i32,
        dropped_bytes: usize,
    ) -> Result<(), DemuxRuntimeError> {
        self.dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?
            .augment_playback_flush_diagnostic(dropped_bytes);
        Ok(())
    }

    #[cfg(test)]
    pub fn consume_playback_dvr_queue(
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
        let has_started_output = self.filters.values().any(|filter| {
            let view = filter.pipeline_view();
            view.started && view.source_filter.is_none()
        });
        if !has_started_output {
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
        let mut payload = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?
            .take_playback_processing_buffer();
        let read_limit = available.min(payload.len());
        if read_limit == 0 {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.restore_playback_processing_buffer(payload);
                dvr.mark_failed();
            }
            return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
        }
        let transaction = match queue.begin_dvr_read(read_limit) {
            Ok(transaction) => transaction,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.restore_playback_processing_buffer(payload);
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        let (queue_identity, queue_epoch) = match transaction.playback_coordinates() {
            Ok(coordinates) => coordinates,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.restore_playback_processing_buffer(payload);
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        let read = match queue.read_into(&mut payload[..read_limit]) {
            Ok(read) => read,
            Err(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.restore_playback_processing_buffer(payload);
                    dvr.mark_failed();
                }
                return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
            }
        };
        if read == 0 {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.restore_playback_processing_buffer(payload);
                dvr.mark_failed();
            }
            return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
        }
        if transaction.commit().is_err() {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.restore_playback_processing_buffer(payload);
                dvr.mark_failed();
            }
            return Err(DemuxRuntimeError::queue_runtime_failure(dvr_id));
        }
        let drain = {
            let dvr = self
                .dvrs
                .get_mut(&dvr_id)
                .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
            let drain = dvr.push_playback_bytes(&payload[..read]);
            dvr.restore_playback_processing_buffer(payload);
            drain
        };
        let mut packet_reports = Vec::with_capacity(drain.packets.len());
        let mut injected_packets = 0usize;
        let mut malformed_packets = 0usize;
        for packet in &drain.packets {
            let report = match crate::packet_pipeline::ValidatedTsPacket::validate(packet) {
                Ok(validated) => {
                    injected_packets = injected_packets.saturating_add(1);
                    self.push_validated_ts_packet_from_origin(
                        &validated,
                        TsInputOrigin::PlaybackDvr {
                            dvr_id,
                            queue_identity,
                            queue_epoch,
                        },
                    )
                }
                Err(reason) => {
                    malformed_packets = malformed_packets.saturating_add(1);
                    self.pipeline.note_malformed_ts_packet();
                    crate::packet_pipeline::PacketPipeline::malformed_ts_packet_report(reason)
                }
            };
            packet_reports.push(report);
        }
        let dropped_bytes = drain
            .malformed_bytes
            .saturating_add(malformed_packets.saturating_mul(TS_PACKET_SIZE));
        let playback_stats = {
            let dvr = self
                .dvrs
                .get_mut(&dvr_id)
                .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
            dvr.note_playback_consume(
                injected_packets,
                malformed_packets,
                drain.malformed_bytes,
            );
            dvr.playback_stats()
        };
        if dropped_bytes > 0 {
            eprintln!(
                "maleicacid-tuner-hal2-dvr-playback-diagnostic: dvr_id={} malformed_packets={} malformed_bytes={} dropped_bytes={} total_dropped_bytes={}",
                dvr_id,
                malformed_packets,
                drain.malformed_bytes,
                dropped_bytes,
                playback_stats.dropped_bytes,
            );
        }
        Ok(PlaybackConsumeReport {
            bytes_read: read,
            completed_packets: drain.packets.len(),
            malformed_packets,
            malformed_bytes: drain.malformed_bytes,
            dropped_bytes,
            packet_reports,
        })
    }

    #[cfg(test)]
    pub(crate) fn consume_playback_dvr_queue_for_test(
        &mut self,
        dvr_id: i32,
    ) -> Result<PlaybackConsumeReport, DemuxRuntimeError> {
        self.consume_playback_dvr_queue(dvr_id)
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
        expected_relation_generation: u64,
        next_relation_generation: u64,
    ) -> Result<(), DemuxRuntimeError> {
        let committed = self
            .filters
            .get_mut(&sink_filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?
            .disconnect_source(expected_relation_generation, next_relation_generation);
        if committed {
            Ok(())
        } else {
            Err(DemuxRuntimeError::invalid_state(sink_filter_id))
        }
    }

    pub(super) fn connect_filter_source_after_boundary(
        &mut self,
        sink_filter_id: i32,
        expected_relation_generation: u64,
        next_relation_generation: u64,
        source_filter_id: i32,
        source_filter_generation: u64,
    ) -> Result<(), DemuxRuntimeError> {
        let committed = self
            .filters
            .get_mut(&sink_filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?
            .set_source_filter(
                expected_relation_generation,
                next_relation_generation,
                source_filter_id,
                source_filter_generation,
            );
        if committed {
            Ok(())
        } else {
            Err(DemuxRuntimeError::invalid_state(sink_filter_id))
        }
    }

    pub(super) fn reset_filter_source_boundary(
        &mut self,
        sink_filter_id: i32,
    ) -> Result<PipelineResetReport, DemuxRuntimeError> {
        if !self.filters.contains_key(&sink_filter_id) {
            return Err(DemuxRuntimeError::filter_missing(sink_filter_id));
        }
        self.pipeline
            .clear_filter_state_after_flush(sink_filter_id);
        self.invalidate_pcr_clock_anchor(sink_filter_id);
        Ok(PipelineResetReport {
            cleared: true,
            residual_packets: 0,
            residual_malformed_bytes: 0,
        })
    }

    fn source_filter_downstream_ids(&self, source_filter_id: i32) -> Vec<i32> {
        self.filters
            .values()
            .filter_map(|filter| match filter.snapshot().source {
                FilterSource::SourceFilter {
                    source_filter_id: stored_source_filter_id,
                    ..
                } if stored_source_filter_id == source_filter_id => Some(filter.filter_id()),
                _ => None,
            })
            .collect()
    }

    pub(super) fn validate_source_filter_reconfigure(
        &self,
        source_filter_id: i32,
        next_tpid: Option<i32>,
    ) -> Result<(), DemuxRuntimeError> {
        for sink_filter_id in self.source_filter_downstream_ids(source_filter_id) {
            let sink = self.filter_snapshot(sink_filter_id)?;
            if sink.tpid.is_some() && next_tpid.is_some() && sink.tpid != next_tpid {
                return Err(DemuxRuntimeError::pid_mismatch(source_filter_id));
            }
        }
        Ok(())
    }

    pub(super) fn source_connection_would_cycle(
        &self,
        sink_filter_id: i32,
        source_filter_id: i32,
    ) -> bool {
        let mut current = source_filter_id;
        let mut visited = BTreeSet::new();
        loop {
            if current == sink_filter_id || !visited.insert(current) {
                return true;
            }
            let Ok(snapshot) = self.filter_snapshot(current) else {
                return false;
            };
            match snapshot.source {
                FilterSource::DemuxInput => return false,
                FilterSource::SourceFilter {
                    source_filter_id, ..
                } => current = source_filter_id,
            }
        }
    }

    fn reset_connected_downstream_source_boundary(
        &mut self,
        sink_filter_id: i32,
        source_filter_id: i32,
        next_source_generation: Option<u64>,
    ) -> Result<(), DemuxRuntimeError> {
        let snapshot = self.filter_snapshot(sink_filter_id)?;
        if !matches!(
            snapshot.source,
            FilterSource::SourceFilter {
                source_filter_id: stored_source_filter_id,
                ..
            } if stored_source_filter_id == source_filter_id
        ) {
            return Ok(());
        }
        let next_relation_generation = snapshot
            .source_relation_generation
            .checked_add(1)
            .filter(|generation| *generation != 0)
            .ok_or_else(|| DemuxRuntimeError::generation_exhausted(Some(sink_filter_id)))?;
        let gate = self
            .filter_producer_gates
            .get(&sink_filter_id)
            .cloned()
            .ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?;
        let mut drain = gate
            .begin_drain(FilterDrainBoundary::Reconfigure)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(sink_filter_id))?;
        if snapshot.queue_present {
            self.clear_filter_queue_runtime(sink_filter_id)?;
        }
        let pending_events = drain
            .take_pending_events()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(sink_filter_id))?;
        self.discard_undelivered_filter_events(sink_filter_id, pending_events)?;
        self.pipeline
            .clear_filter_state_after_flush(sink_filter_id);
        #[cfg(test)]
        if let Some(queue) = self.filter_queue_mirror.get_mut(&sink_filter_id) {
            queue.clear();
        }
        let filter = self
            .filters
            .get_mut(&sink_filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?;
        filter.clear_queued_payload_state();
        filter.reset_section_delivery_state();
        filter.reset_audio_timestamp_association();
        filter.clear_pending_start_id();
        match next_source_generation {
            Some(source_filter_generation) => {
                if !filter.set_source_filter(
                    snapshot.source_relation_generation,
                    next_relation_generation,
                    source_filter_id,
                    source_filter_generation,
                ) {
                    return Err(DemuxRuntimeError::invalid_state(sink_filter_id));
                }
            }
            None => {
                if !filter.disconnect_source(
                    snapshot.source_relation_generation,
                    next_relation_generation,
                ) {
                    return Err(DemuxRuntimeError::invalid_state(sink_filter_id));
                }
            }
        }
        self.invalidate_pcr_clock_anchor(sink_filter_id);
        drain
            .commit()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(sink_filter_id))
    }

    fn refresh_source_filter_downstreams(
        &mut self,
        source_filter_id: i32,
        source_filter_generation: u64,
    ) -> Result<(), DemuxRuntimeError> {
        for sink_filter_id in self.source_filter_downstream_ids(source_filter_id) {
            if let Err(error) = self.reset_connected_downstream_source_boundary(
                sink_filter_id,
                source_filter_id,
                Some(source_filter_generation),
            ) {
                self.quarantine_filter_runtime(sink_filter_id);
                return Err(error);
            }
        }
        Ok(())
    }

    fn disconnect_source_filter_downstreams(
        &mut self,
        source_filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        for sink_filter_id in self.source_filter_downstream_ids(source_filter_id) {
            if let Err(error) = self.reset_connected_downstream_source_boundary(
                sink_filter_id,
                source_filter_id,
                None,
            ) {
                self.quarantine_filter_runtime(sink_filter_id);
                return Err(error);
            }
        }
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

    pub(crate) fn prepare_stream_boundary(
        &mut self,
        reason: PipelineBoundaryReason,
    ) -> Result<super::generation_boundary::PreparedStreamBoundary, DemuxRuntimeError> {
        if self.state != DemuxRuntimeState::Open {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        let next = match self.stream_boundary.prepare_next_generation() {
            Some(next) => next,
            None => {
                self.quarantine();
                return Err(DemuxRuntimeError::generation_exhausted(Some(self.demux_id)));
            }
        };
        let filter_queue_ids: Vec<i32> = self
            .filters
            .values()
            .filter(|filter| filter.queue_present())
            .map(FilterRuntime::filter_id)
            .collect();
        if filter_queue_ids
            .iter()
            .any(|filter_id| !self.filter_queue_runtimes.contains_key(filter_id))
        {
            return Err(DemuxRuntimeError::queue_runtime_failure(self.demux_id));
        }
        let mut prepared_pipeline = self.pipeline.clone();
        let reset = prepared_pipeline.reset_boundary();
        let pcr_invalidation = self.pcr_clock_anchor_store.prepare_invalidate_all();
        let mut filter_drains = Vec::with_capacity(self.filter_producer_gates.len());
        for (filter_id, gate) in &self.filter_producer_gates {
            match gate.begin_drain(FilterDrainBoundary::Reconfigure) {
                Ok(drain) => filter_drains.push((*filter_id, drain)),
                Err(_) => return Err(DemuxRuntimeError::queue_runtime_failure(*filter_id)),
            }
        }
        Ok(super::generation_boundary::PreparedStreamBoundary {
            reason,
            expected_generation: self.stream_boundary.generation(),
            next_generation: next,
            reset,
            prepared_pipeline,
            filter_queue_ids,
            filter_drains,
            pcr_invalidation,
        })
    }

    pub fn prepare_stream_boundary_from_typed_request(
        &mut self,
        request: DemuxStreamBoundaryRequest,
    ) -> Result<super::generation_boundary::PreparedStreamBoundary, DemuxRuntimeError> {
        self.prepare_stream_boundary(request.reason)
    }

    pub(crate) fn commit_stream_boundary(
        &mut self,
        prepared: super::generation_boundary::PreparedStreamBoundary,
    ) -> Result<super::generation_boundary::StreamBoundaryReport, DemuxRuntimeError> {
        if self.state != DemuxRuntimeState::Open {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        if !self.stream_boundary.commit_prepared_generation(
            prepared.expected_generation,
            prepared.next_generation,
        ) {
            return Err(DemuxRuntimeError::invalid_state(self.demux_id));
        }
        self.pipeline = prepared.prepared_pipeline;
        for filter in self.filters.values_mut() {
            filter.clear_queued_payload_state();
            filter.reset_section_delivery_state();
            filter.reset_audio_timestamp_association();
            filter.clear_pending_start_id();
        }
        self.pcr_clock_anchor_store
            .commit_invalidation(prepared.pcr_invalidation);

        let mut pending_events = Vec::new();
        for (filter_id, drain) in prepared.filter_drains {
            match drain.commit_and_take_pending_events() {
                Ok(events) => pending_events.push((filter_id, events)),
                Err(_) => {
                    self.quarantine();
                    return Err(DemuxRuntimeError::queue_runtime_failure(filter_id));
                }
            }
        }
        for filter_id in &prepared.filter_queue_ids {
            if self
                .filter_queue_runtimes
                .get(filter_id)
                .and_then(|queue| queue.clear_contents().err())
                .is_some()
            {
                self.quarantine();
                return Err(DemuxRuntimeError::queue_runtime_failure(*filter_id));
            }
        }
        for (filter_id, events) in pending_events {
            if self
                .discard_undelivered_filter_events(filter_id, events)
                .is_err()
            {
                self.quarantine();
                return Err(DemuxRuntimeError::av_backing_failure(filter_id));
            }
        }
        #[cfg(test)]
        for queue in self.filter_queue_mirror.values_mut() {
            queue.clear();
        }
        Ok(super::generation_boundary::StreamBoundaryReport {
            reason: prepared.reason,
            reset: prepared.reset,
            next_generation: prepared.next_generation,
        })
    }

    pub fn commit_stream_boundary_from_typed_request(
        &mut self,
        prepared: super::generation_boundary::PreparedStreamBoundary,
    ) -> Result<super::generation_boundary::StreamBoundaryReport, DemuxRuntimeError> {
        self.commit_stream_boundary(prepared)
    }

    pub fn apply_stream_boundary_from_typed_request(
        &mut self,
        request: DemuxStreamBoundaryRequest,
    ) -> Result<super::generation_boundary::StreamBoundaryReport, DemuxRuntimeError> {
        self.apply_stream_boundary(request.reason)
    }

    pub(crate) fn apply_stream_boundary(
        &mut self,
        reason: PipelineBoundaryReason,
    ) -> Result<super::generation_boundary::StreamBoundaryReport, DemuxRuntimeError> {
        let prepared = self.prepare_stream_boundary(reason)?;
        self.commit_stream_boundary(prepared)
    }

    pub fn quarantine_runtime_from_typed_request(
        &mut self,
        _request: DemuxRuntimeQuarantineRequest,
    ) {
        self.quarantine();
    }

    pub(crate) fn quarantine(&mut self) {
        self.state = DemuxRuntimeState::Quarantined;
        self.invalidate_all_pcr_clock_anchors();
        for gate in self.filter_producer_gates.values() {
            if gate.close().is_err() {
                self.state = DemuxRuntimeState::Quarantined;
            }
        }
        for queue in self.dvr_queue_runtimes.values() {
            if queue.close_dvr_protocol().is_err() {
                self.state = DemuxRuntimeState::Quarantined;
            }
        }
        for filter in self.filters.values_mut() {
            filter.mark_failed();
        }
        for dvr in self.dvrs.values_mut() {
            dvr.mark_failed();
        }
    }

    pub(crate) fn quarantine_filter_runtime(&mut self, filter_id: i32) {
        let downstream_ids = self.source_filter_downstream_ids(filter_id);
        let source_generation = self
            .filters
            .get(&filter_id)
            .map(FilterRuntime::generation);
        if let Some(gate) = self.filter_producer_gates.get(&filter_id) {
            if gate.close().is_err() {
                self.state = DemuxRuntimeState::Quarantined;
            }
        }
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.mark_failed();
        }
        if !downstream_ids.is_empty() {
            if let Some(source_generation) = source_generation {
                self.pipeline.reset_origin(TsInputOrigin::SourceFilter {
                    source_filter_id: filter_id,
                    source_filter_generation: source_generation,
                });
            }
        }
        self.invalidate_pcr_clock_anchor(filter_id);
        if let Some(source_generation) = source_generation {
            for sink_filter_id in downstream_ids {
                if self
                    .reset_connected_downstream_source_boundary(
                        sink_filter_id,
                        filter_id,
                        Some(source_generation),
                    )
                    .is_err()
                {
                    self.quarantine_filter_runtime(sink_filter_id);
                }
            }
        }
    }

    pub(crate) fn quarantine_dvr_runtime(&mut self, dvr_id: i32) {
        let playback_coordinates = self.dvrs.get(&dvr_id).and_then(|dvr| {
            (dvr.kind() == DvrKind::Playback)
                .then(|| {
                    self.dvr_queue_runtimes
                        .get(&dvr_id)
                        .and_then(|queue| queue.playback_coordinates().ok())
                })
                .flatten()
        });
        if let Some(queue) = self.dvr_queue_runtimes.get(&dvr_id) {
            if queue.close_dvr_protocol().is_err() {
                self.state = DemuxRuntimeState::Quarantined;
            }
        }
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.mark_failed();
        }
        if let Some((queue_identity, queue_epoch)) = playback_coordinates {
            let origin = TsInputOrigin::PlaybackDvr {
                dvr_id,
                queue_identity,
                queue_epoch,
            };
            self.pipeline.reset_origin(origin);
            self.invalidate_all_pcr_clock_anchors();
            for filter in self.filters.values_mut() {
                filter.reset_audio_timestamp_association_for_origin(origin);
            }
        }
    }

    fn discard_undelivered_filter_events(
        &mut self,
        filter_id: i32,
        events: Vec<PipelineGeneratedEvent>,
    ) -> Result<(), DemuxRuntimeError> {
        for event in events {
            if let PipelineGeneratedEvent::AvMedia { descriptor, .. } = event {
                let backing = self
                    .filter_av_backings
                    .get_mut(&filter_id)
                    .ok_or(DemuxRuntimeError::av_backing_failure(filter_id))?;
                if !backing.discard_undelivered_data_id(descriptor.data_id) {
                    return Err(DemuxRuntimeError::av_backing_failure(filter_id));
                }
            }
        }
        Ok(())
    }

    pub fn filter_views(&self) -> Vec<PipelineFilterView> {
        self.filters
            .values()
            .map(FilterRuntime::pipeline_view)
            .collect()
    }

    pub const fn pipeline_diagnostic_counters(&self) -> PipelineDiagnosticCounters {
        self.pipeline.diagnostic_counters()
    }

    #[cfg(test)]
    pub(crate) fn push_ts_packet_from_origin(
        &mut self,
        packet: &[u8],
        origin: TsInputOrigin,
    ) -> PipelineReport {
        let validated = match crate::packet_pipeline::ValidatedTsPacket::validate(packet) {
            Ok(validated) => validated,
            Err(reason) => {
                self.pipeline.note_malformed_ts_packet();
                return crate::packet_pipeline::PacketPipeline::malformed_ts_packet_report(reason);
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
        let mut propagated_source_filters = BTreeSet::new();
        self.push_validated_ts_packet_from_origin_inner(
            validated,
            origin,
            &mut propagated_source_filters,
        )
    }

    fn push_validated_ts_packet_from_origin_inner(
        &mut self,
        validated: &crate::packet_pipeline::ValidatedTsPacket<'_>,
        origin: TsInputOrigin,
        propagated_source_filters: &mut BTreeSet<i32>,
    ) -> PipelineReport {
        let packet = validated.packet_bytes();
        let kind = match origin {
            TsInputOrigin::Frontend {
                frontend_generation,
            } => PipelineInputKind::Frontend {
                frontend_generation,
            },
            TsInputOrigin::PlaybackDvr {
                dvr_id,
                queue_identity,
                queue_epoch,
            } => PipelineInputKind::PlaybackDvr {
                dvr_id,
                queue_identity,
                queue_epoch,
            },
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
        self.observe_pcr_clock(validated, origin, &report.assembly_suppression_reasons);
        let filters = self.filter_views();
        let downstream = self
            .pipeline
            .plan_and_assemble_ts_packet_report_after_preflight(
                validated,
                origin,
                &filters,
                &report.assembly_suppression_reasons,
            );
        report.dropped_packets = report
            .dropped_packets
            .saturating_add(downstream.dropped_packets);
        report.malformed_packets = report
            .malformed_packets
            .saturating_add(downstream.malformed_packets);
        report.drop_reasons.extend(downstream.drop_reasons);
        report
            .assembly_suppression_reasons
            .extend(downstream.assembly_suppression_reasons);
        report.delivery_actions.extend(downstream.delivery_actions);
        report.generated_events.extend(downstream.generated_events);
        report.generated_events.retain(|event| match event {
            PipelineGeneratedEvent::PesPacketReady {
                filter_id, packet, ..
            } => self
                .filters
                .get(filter_id)
                .is_some_and(|filter| filter.accepts_pes_stream_id(packet.stream_id)),
            _ => true,
        });
        report.diagnostics.extend(downstream.diagnostics);
        self.mark_filters_failed_for_generation_overflow(&report.diagnostics);
        self.reset_audio_timestamp_associations_after_packet_gap(
            validated.pid(),
            origin,
            &report.diagnostics,
        );
        let mut av_payloads = Vec::new();
        let generated_events = std::mem::take(&mut report.generated_events);
        for event in generated_events {
            match event {
                PipelineGeneratedEvent::PesPacketReady {
                    filter_id,
                    pid,
                    packet,
                    ..
                } if self
                    .filters
                    .get(&filter_id)
                    .is_some_and(|filter| filter.open_kind() == PipelineOpenKind::Av) =>
                {
                    av_payloads.push((filter_id, pid, packet));
                }
                event => report.generated_events.push(event),
            }
        }
        for (filter_id, pid, packet) in av_payloads {
            let Some(filter) = self.filters.get_mut(&filter_id) else {
                continue;
            };
            let payloads = match filter.prepare_av_media_payloads(packet, origin) {
                Ok(payloads) => payloads,
                Err(_) => {
                    report.diagnostics.push(
                        crate::packet_pipeline::PipelineDiagnostic::av_authoritative_timestamp_unavailable(
                            pid, filter_id,
                        ),
                    );
                    continue;
                }
            };
            for payload in payloads {
                let outcome = self.filter_av_backings.get_mut(&filter_id).map(|backing| {
                    backing.allocate_payload_bytes(&payload.payload, payload.metadata)
                });
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
                            av_payload_delivery_outcome_diagnostic(outcome, pid, filter_id)
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
                                pid, filter_id, error,
                            ),
                        );
                    }
                    None => {
                        report.diagnostics.push(
                            crate::packet_pipeline::PipelineDiagnostic::av_shared_backing_missing(
                                pid, filter_id,
                            ),
                        );
                    }
                }
            }
        }
        let packet_pid = validated.pid();
        let record_index_commit_mode = Self::record_index_commit_mode(&report);
        let (mirror_diagnostics, record_index_events) = self.mirror_record_dvr_packets(
            validated,
            &report.delivery_actions,
            packet_pid,
            record_index_commit_mode,
        );
        report.diagnostics.extend(mirror_diagnostics);
        report.generated_events.extend(record_index_events);
        let queue_payload_diagnostics = self.commit_generated_filter_events(
            packet,
            &mut report.generated_events,
            packet_pid,
            origin,
        );
        report.diagnostics.extend(queue_payload_diagnostics);
        let source_filter_ids: Vec<i32> = report
            .delivery_actions
            .iter()
            .filter_map(|action| match action {
                PipelineDeliveryAction::RawPacket { filter_id } => Some(*filter_id),
                _ => None,
            })
            .collect();
        for source_filter_id in source_filter_ids {
            let Some((source_filter_generation, source_started)) = self
                .filters
                .get(&source_filter_id)
                .map(|filter| (filter.generation(), filter.state().is_started()))
            else {
                continue;
            };
            if !source_started {
                continue;
            }
            let has_started_downstream = self.filters.values().any(|filter| {
                let view = filter.pipeline_view();
                view.started
                    && view.source_filter
                        == Some((source_filter_id, source_filter_generation))
            });
            if !has_started_downstream
                || !propagated_source_filters.insert(source_filter_id)
            {
                continue;
            }
            let downstream = self.push_validated_ts_packet_from_origin_inner(
                validated,
                TsInputOrigin::SourceFilter {
                    source_filter_id,
                    source_filter_generation,
                },
                propagated_source_filters,
            );
            report.dropped_packets = report
                .dropped_packets
                .saturating_add(downstream.dropped_packets);
            report.malformed_packets = report
                .malformed_packets
                .saturating_add(downstream.malformed_packets);
            report.drop_reasons.extend(downstream.drop_reasons);
            report
                .assembly_suppression_reasons
                .extend(downstream.assembly_suppression_reasons);
            report.delivery_actions.extend(downstream.delivery_actions);
            report.generated_events.extend(downstream.generated_events);
            report.diagnostics.extend(downstream.diagnostics);
        }
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

    fn reset_audio_timestamp_associations_after_packet_gap(
        &mut self,
        pid: crate::packet_pipeline::PacketPid,
        origin: TsInputOrigin,
        diagnostics: &[PipelineDiagnostic],
    ) {
        let timeline_gap = diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic,
                PipelineDiagnostic::TeiAssemblySuppressed { pid: diagnostic_pid }
                    | PipelineDiagnostic::ContinuityCounterCollisionAssemblySuppressed {
                        pid: diagnostic_pid,
                        ..
                    }
                    | PipelineDiagnostic::ContinuityDiscontinuityAssemblyReset {
                        pid: diagnostic_pid,
                        ..
                    }
                    | PipelineDiagnostic::KeylessScrambledAssemblySuppressed {
                        pid: diagnostic_pid,
                    }
                    | PipelineDiagnostic::PesGenerationOverflow {
                        pid: diagnostic_pid,
                        ..
                    }
                    | PipelineDiagnostic::PesAssemblerDrop {
                        pid: diagnostic_pid,
                        ..
                    }
                    | PipelineDiagnostic::SourceFilterValidationFailure {
                        pid: diagnostic_pid,
                        ..
                    }
                    | PipelineDiagnostic::SourceFilterDescramblePolicyFailure {
                        pid: diagnostic_pid,
                        ..
                    } if *diagnostic_pid == pid
            )
        });
        if !timeline_gap {
            return;
        }
        for filter in self.filters.values_mut() {
            if filter
                .pipeline_view()
                .accepts_packet_pid_from_origin(pid, origin)
            {
                filter.reset_audio_timestamp_association_for_origin(origin);
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
            runtime.configure_with_generation(generation, config, None);
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
            runtime.configure_with_generation(generation, config, None);
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
            runtime.configure_with_generation(generation, config, None);
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
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        queue
            .clear_contents()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        Ok(())
    }

    fn build_filter_queue_runtimes_for_snapshot(
        filters: &BTreeMap<i32, FilterRuntime>,
    ) -> Result<BTreeMap<i32, QueueRuntime>, DemuxRuntimeError> {
        let mut runtimes = BTreeMap::new();
        for (filter_id, filter) in filters {
            if Self::should_keep_filter_queue_runtime(filter) {
                let queue = QueueRuntime::new_filter(filter.buffer_size(), true)
                    .map_err(|_| DemuxRuntimeError::queue_runtime_failure(*filter_id))?;
                runtimes.insert(*filter_id, queue);
            }
        }
        Ok(runtimes)
    }

    fn build_dvr_queue_runtimes_for_snapshot(
        dvrs: &BTreeMap<i32, DvrRuntime>,
    ) -> Result<BTreeMap<i32, QueueRuntime>, DemuxRuntimeError> {
        let mut runtimes = BTreeMap::new();
        for (dvr_id, dvr) in dvrs {
            if Self::should_keep_dvr_queue_runtime(dvr) {
                let queue = QueueRuntime::new_dvr(
                    dvr.buffer_size(),
                    true,
                    dvr.kind() == DvrKind::Playback,
                )
                    .map_err(|_| DemuxRuntimeError::queue_runtime_failure(*dvr_id))?;
                runtimes.insert(*dvr_id, queue);
            }
        }
        Ok(runtimes)
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
            .is_some_and(|queue| queue.capacity_matches_buffer_size(filter.buffer_size()))
        {
            return Ok(());
        }
        let queue = QueueRuntime::new_filter(filter.buffer_size(), true)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        self.filter_queue_runtimes.insert(filter_id, queue);
        Ok(())
    }

    fn rebuild_dvr_queue_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let Some(dvr) = self.dvrs.get(&dvr_id) else {
            return Err(DemuxRuntimeError::dvr_missing(dvr_id));
        };
        if !Self::should_keep_dvr_queue_runtime(dvr) {
            self.dvr_queue_runtimes.remove(&dvr_id);
            return Ok(());
        }
        if self
            .dvr_queue_runtimes
            .get(&dvr_id)
            .is_some_and(|queue| queue.capacity_matches_buffer_size(dvr.buffer_size()))
        {
            return Ok(());
        }
        let queue = QueueRuntime::new_dvr(
            dvr.buffer_size(),
            true,
            dvr.kind() == DvrKind::Playback,
        )
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        self.dvr_queue_runtimes.insert(dvr_id, queue);
        Ok(())
    }

    fn should_keep_filter_queue_runtime(filter: &FilterRuntime) -> bool {
        !filter.state().is_closed_or_failed()
            && filter.supports_normal_fmq_queue()
            && filter.buffer_size() > 0
    }

    fn should_keep_dvr_queue_runtime(dvr: &DvrRuntime) -> bool {
        !dvr.state().is_closed_or_failed() && dvr.buffer_size() > 0
    }

    fn mirror_record_dvr_packets(
        &mut self,
        packet: &crate::packet_pipeline::ValidatedTsPacket<'_>,
        delivery_actions: &[PipelineDeliveryAction],
        pid: crate::packet_pipeline::PacketPid,
        record_index_commit_mode: RecordIndexCommitMode,
    ) -> (
        Vec<PipelineDiagnostic>,
        Vec<PipelineGeneratedEvent>,
    ) {
        let mut diagnostics = Vec::new();
        let mut generated_events = Vec::new();
        let mut matched_filter_ids = BTreeSet::new();
        for action in delivery_actions {
            let PipelineDeliveryAction::DvrMirror { dvr_id: filter_id } = *action else {
                continue;
            };
            matched_filter_ids.insert(filter_id);
        }

        let mut dvr_filter_union: BTreeMap<i32, Vec<(i32, FilterProducerDrainGate)>> =
            BTreeMap::new();
        for filter_id in matched_filter_ids {
            let target_ids = self.record_dvr_target_ids_for_filter(filter_id);
            if target_ids.is_empty() {
                continue;
            }
            let gate = match self.filter_producer_gates.get(&filter_id).cloned() {
                Some(gate) => gate,
                None => {
                    diagnostics.push(
                        PipelineDiagnostic::filter_queue_payload_delivery_failure(
                            pid,
                            filter_id,
                            DemuxRuntimeError::filter_missing(filter_id),
                        ),
                    );
                    continue;
                }
            };
            for dvr_id in target_ids {
                dvr_filter_union
                    .entry(dvr_id)
                    .or_default()
                    .push((filter_id, gate.clone()));
            }
        }

        let mut committed_permits: BTreeMap<i32, FilterProducerPermit> = BTreeMap::new();
        for (dvr_id, filter_gates) in dvr_filter_union {
            let mut admitted = Vec::new();
            let mut admission_failures = Vec::new();
            let reserve_count = filter_gates.len();
            if admitted.try_reserve_exact(reserve_count).is_err()
                || admission_failures.try_reserve_exact(reserve_count).is_err()
            {
                for (filter_id, _) in filter_gates {
                    diagnostics.push(
                        PipelineDiagnostic::filter_queue_payload_delivery_failure(
                            pid,
                            filter_id,
                            DemuxRuntimeError::queue_runtime_failure(filter_id),
                        ),
                    );
                }
                continue;
            }
            for (filter_id, gate) in filter_gates {
                match gate.begin_producer() {
                    Ok(permit)
                        if permit
                            .record_output_byte_offset()
                            .ok()
                            .and_then(|offset| {
                                u64::try_from(TS_PACKET_SIZE)
                                    .ok()
                                    .and_then(|bytes| offset.checked_add(bytes))
                            })
                            .is_some() =>
                    {
                        admitted.push((filter_id, permit));
                    }
                    Ok(permit) => {
                        drop(permit);
                        self.quarantine_filter_runtime(filter_id);
                        admission_failures.push(filter_id);
                    }
                    Err(_) => admission_failures.push(filter_id),
                }
            }
            if admitted.is_empty() {
                for filter_id in admission_failures {
                    diagnostics.push(
                        PipelineDiagnostic::filter_queue_payload_delivery_failure(
                            pid,
                            filter_id,
                            DemuxRuntimeError::queue_runtime_failure(filter_id),
                        ),
                    );
                }
                continue;
            }

            let write_result = self.try_write_record_dvr_packet(dvr_id, packet.packet_bytes());
            for filter_id in admission_failures {
                diagnostics.push(
                    PipelineDiagnostic::filter_queue_payload_delivery_failure(
                        pid,
                        filter_id,
                        DemuxRuntimeError::queue_runtime_failure(filter_id),
                    ),
                );
            }

            match write_result {
                Ok(RecordDvrMirrorWriteOutcome::Written)
                | Ok(RecordDvrMirrorWriteOutcome::WakePending) => {
                    for (filter_id, permit) in admitted {
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            committed_permits.entry(filter_id)
                        {
                            entry.insert(permit);
                        } else if permit.commit().is_err() {
                            self.quarantine_filter_runtime(filter_id);
                            diagnostics.push(
                                PipelineDiagnostic::filter_queue_payload_delivery_failure(
                                    pid,
                                    filter_id,
                                    DemuxRuntimeError::queue_runtime_failure(filter_id),
                                ),
                            );
                        }
                    }
                }
                Ok(RecordDvrMirrorWriteOutcome::Overflow) => {
                    for (filter_id, permit) in admitted {
                        if permit.commit().is_err() {
                            self.quarantine_filter_runtime(filter_id);
                        }
                        diagnostics.push(
                            PipelineDiagnostic::record_dvr_mirror_overflow(
                                pid, filter_id, dvr_id,
                            ),
                        );
                    }
                }
                Err(error) => {
                    for (filter_id, permit) in admitted {
                        if permit.commit().is_err() {
                            self.quarantine_filter_runtime(filter_id);
                        }
                        diagnostics.push(
                            PipelineDiagnostic::record_dvr_mirror_failure(
                                pid,
                                filter_id,
                                dvr_id,
                                error.clone(),
                            ),
                        );
                    }
                }
            }
        }

        for (filter_id, permit) in committed_permits {
            let byte_number = match permit.record_output_byte_offset() {
                Ok(byte_number) => byte_number,
                Err(_) => {
                    drop(permit);
                    self.quarantine_filter_runtime(filter_id);
                    diagnostics.push(
                        PipelineDiagnostic::filter_queue_payload_delivery_failure(
                            pid,
                            filter_id,
                            DemuxRuntimeError::queue_runtime_failure(filter_id),
                        ),
                    );
                    continue;
                }
            };
            let event = match record_index_commit_mode {
                RecordIndexCommitMode::Parse => self
                    .pipeline
                    .record_index_event_after_record_commit(filter_id, packet, byte_number),
                RecordIndexCommitMode::ResetThenParse => {
                    self.pipeline.reset_record_index_partial_state(filter_id);
                    self.pipeline
                        .record_index_event_after_record_commit(filter_id, packet, byte_number)
                }
                RecordIndexCommitMode::AdvanceOnly => None,
                RecordIndexCommitMode::ResetAndAdvance => {
                    self.pipeline
                        .reset_record_index_without_parsing(filter_id, true);
                    None
                }
            };
            if permit
                .commit_record_output(TS_PACKET_SIZE, event)
                .is_err()
            {
                self.quarantine_filter_runtime(filter_id);
                diagnostics.push(
                    PipelineDiagnostic::filter_queue_payload_delivery_failure(
                        pid,
                        filter_id,
                        DemuxRuntimeError::queue_runtime_failure(filter_id),
                    ),
                );
                continue;
            }
            if let Some(filter) = self.filters.get_mut(&filter_id) {
                filter.note_payload_queued(TS_PACKET_SIZE);
            }
        }
        (diagnostics, generated_events)
    }

    fn record_index_commit_mode(report: &PipelineReport) -> RecordIndexCommitMode {
        use crate::packet_pipeline::PipelineAssemblySuppressionReason as Suppression;

        let has = |target| {
            report
                .assembly_suppression_reasons
                .iter()
                .any(|reason| *reason == target)
        };
        let continuity_reset = report.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic,
                PipelineDiagnostic::ContinuityDiscontinuityAssemblyReset { .. }
            )
        });
        let reset_and_suppress = has(Suppression::TransportErrorIndicator)
            || has(Suppression::ContinuityCounterCollision);
        if reset_and_suppress {
            return RecordIndexCommitMode::ResetAndAdvance;
        }
        let suppress = has(Suppression::DuplicatePacket);
        if suppress {
            return if continuity_reset {
                RecordIndexCommitMode::ResetAndAdvance
            } else {
                RecordIndexCommitMode::AdvanceOnly
            };
        }
        if continuity_reset {
            RecordIndexCommitMode::ResetThenParse
        } else {
            RecordIndexCommitMode::Parse
        }
    }

    fn record_dvr_target_ids_for_filter(&self, filter_id: i32) -> Vec<i32> {
        self.dvrs
            .iter()
            .filter_map(|(dvr_id, dvr)| {
                (dvr.kind() == DvrKind::Record
                    && dvr.state() == super::dvr::DvrRuntimeState::Started
                    && dvr.record_filter_relation_state()
                        == RecordDvrFilterRelationState::Healthy
                    && dvr.attached_record_filters().contains(&filter_id))
                .then_some(*dvr_id)
            })
            .collect()
    }

    fn attached_record_dvr_ids_for_filter(&self, filter_id: i32) -> Vec<i32> {
        self.dvrs
            .iter()
            .filter_map(|(dvr_id, dvr)| {
                (dvr.kind() == DvrKind::Record
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
        let _wake_was_pending = queue.retry_pending_wake(TUNER_EVENT_DATA_READY).is_err();
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
        let transaction = queue
            .begin_dvr_write(packet.len())
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        let result = FmqDeliveryTxn::new(FmqObjectKind::DvrRecord).commit_payload(
            packet.len(),
            queue
                .write_checked(packet)
                .map_err(|_| FmqFailureKind::WriteFailed),
            queue
                .wake(TUNER_EVENT_DATA_READY)
                .map_err(|_| FmqFailureKind::EventFlagWakeFailed),
        );
        if matches!(
            result.action,
            FmqDeliveryAction::Continue | FmqDeliveryAction::WakePending
        ) {
            transaction
                .commit()
                .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        }
        match result.action {
            FmqDeliveryAction::Continue => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.clear_pending_overflow();
                    dvr.mark_pending_data_ready();
                }
                Ok(RecordDvrMirrorWriteOutcome::Written)
            }
            FmqDeliveryAction::WakePending => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.clear_pending_overflow();
                    dvr.mark_pending_data_ready();
                }
                Ok(RecordDvrMirrorWriteOutcome::WakePending)
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

    fn commit_generated_filter_events(
        &mut self,
        packet: &[u8],
        generated_events: &mut Vec<PipelineGeneratedEvent>,
        packet_pid: crate::packet_pipeline::PacketPid,
        origin: TsInputOrigin,
    ) -> Vec<crate::packet_pipeline::PipelineDiagnostic> {
        let mut diagnostics = Vec::new();
        let mut committed_events = Vec::with_capacity(generated_events.len());
        let mut gates_with_pending_events = BTreeSet::new();
        for event in std::mem::take(generated_events) {
            let prepared_section_delivery = match &event {
                PipelineGeneratedEvent::SectionPayloadReady {
                    filter_id,
                    pid,
                    raw,
                    bytes,
                    ..
                } => match self
                    .filters
                    .get(filter_id)
                    .and_then(|filter| {
                        filter.prepare_section_delivery(origin, *pid, bytes, *raw)
                    })
                {
                    Some(prepared) => Some(prepared),
                    None => continue,
                },
                _ => None,
            };
            let (filter_id, pid, payload, callback_event) = match &event {
                PipelineGeneratedEvent::FilterStatus { .. } => {
                    committed_events.push(event);
                    continue;
                }
                PipelineGeneratedEvent::DataReady { filter_id } => {
                    (*filter_id, packet_pid, Some(packet.to_vec()), false)
                }
                PipelineGeneratedEvent::SectionPayloadReady {
                    filter_id,
                    pid,
                    bytes,
                    ..
                } => (*filter_id, *pid, Some(bytes.clone()), true),
                PipelineGeneratedEvent::PesPacketReady {
                    filter_id,
                    pid,
                    packet,
                    ..
                } => (*filter_id, *pid, Some(packet.raw_bytes.clone()), true),
                PipelineGeneratedEvent::AvMedia { filter_id, .. } => {
                    (*filter_id, packet_pid, None, true)
                }
                PipelineGeneratedEvent::Record { .. }
                | PipelineGeneratedEvent::RecordIndex { .. }
                | PipelineGeneratedEvent::Section { .. }
                | PipelineGeneratedEvent::Pes { .. } => {
                    committed_events.push(event);
                    continue;
                }
            };
            let gate = match self.filter_producer_gates.get(&filter_id).cloned() {
                Some(gate) => gate,
                None => {
                    diagnostics.push(PipelineDiagnostic::filter_queue_payload_delivery_failure(
                        pid,
                        filter_id,
                        DemuxRuntimeError::filter_missing(filter_id),
                    ));
                    continue;
                }
            };
            if let Some(payload) = payload.as_ref() {
                if let Err(error) = self.preflight_filter_queue_payload(filter_id, payload.len()) {
                    if error.is_overflow() {
                        committed_events.push(PipelineGeneratedEvent::FilterStatus {
                            filter_id,
                            status: FilterStatusEvent::Overflow,
                        });
                    }
                    diagnostics.push(PipelineDiagnostic::filter_queue_payload_delivery_failure(
                        pid,
                        filter_id,
                        error.runtime_error(),
                    ));
                    continue;
                }
            }
            let mut permit = match gate.begin_producer() {
                Ok(permit) => permit,
                Err(_) => {
                    diagnostics.push(PipelineDiagnostic::filter_queue_payload_delivery_failure(
                        pid,
                        filter_id,
                        DemuxRuntimeError::queue_runtime_failure(filter_id),
                    ));
                    continue;
                }
            };
            let payload_committed = payload.is_some();
            let payload_result = match payload {
                Some(payload) => self
                    .enqueue_filter_queue_payload_with_permit(filter_id, payload, &mut permit),
                None => Ok(()),
            };
            if let Err(error) = payload_result {
                drop(permit);
                if error.is_overflow() {
                    committed_events.push(PipelineGeneratedEvent::FilterStatus {
                        filter_id,
                        status: FilterStatusEvent::Overflow,
                    });
                }
                diagnostics.push(PipelineDiagnostic::filter_queue_payload_delivery_failure(
                    pid,
                    filter_id,
                    error.runtime_error(),
                ));
                continue;
            }
            let unqueued_event = if callback_event {
                if permit.enqueue_event(event).is_err() {
                    drop(permit);
                    self.quarantine_filter_runtime(filter_id);
                    diagnostics.push(PipelineDiagnostic::filter_queue_payload_delivery_failure(
                        pid,
                        filter_id,
                        DemuxRuntimeError::queue_runtime_failure(filter_id),
                    ));
                    continue;
                }
                None
            } else {
                Some(event)
            };
            let permit_committed = if permit.commit().is_err() {
                self.quarantine_filter_runtime(filter_id);
                diagnostics.push(PipelineDiagnostic::filter_queue_payload_delivery_failure(
                    pid,
                    filter_id,
                    DemuxRuntimeError::queue_runtime_failure(filter_id),
                ));
                false
            } else if callback_event {
                gates_with_pending_events.insert(filter_id);
                true
            } else if let Some(event) = unqueued_event {
                committed_events.push(event);
                true
            } else {
                true
            };
            if permit_committed {
                if let Some(prepared) = prepared_section_delivery {
                    let committed = self
                        .filters
                        .get_mut(&filter_id)
                        .is_some_and(|filter| filter.commit_section_delivery(prepared));
                    if !committed {
                        self.quarantine_filter_runtime(filter_id);
                        diagnostics.push(
                            PipelineDiagnostic::filter_queue_payload_delivery_failure(
                                pid,
                                filter_id,
                                DemuxRuntimeError::queue_runtime_failure(filter_id),
                            ),
                        );
                        continue;
                    }
                }
            }
            if payload_committed && permit_committed {
                match self.committed_filter_status_events(filter_id) {
                    Ok(mut events) => committed_events.append(&mut events),
                    Err(error) => diagnostics.push(
                        PipelineDiagnostic::filter_queue_payload_delivery_failure(
                            pid, filter_id, error,
                        ),
                    ),
                }
            }
        }
        for (filter_id, filter) in &self.filters {
            if filter.state().is_started()
                && filter.delivery_readiness() == FilterDelayReadiness::Ready
            {
                gates_with_pending_events.insert(*filter_id);
            }
        }
        for filter_id in gates_with_pending_events {
            let ready = self
                .filters
                .get(&filter_id)
                .is_some_and(|filter| {
                    filter.state().is_started()
                        && filter.delivery_readiness() == FilterDelayReadiness::Ready
                });
            if !ready {
                continue;
            }
            let pending = self
                .filter_producer_gates
                .get(&filter_id)
                .ok_or(DemuxRuntimeError::filter_missing(filter_id))
                .and_then(|gate| {
                    gate.take_pending_events()
                        .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))
                });
            match pending {
                Ok(mut pending) => {
                    if !pending.is_empty() {
                        if let Some(filter) = self.filters.get_mut(&filter_id) {
                            filter.commit_delivery_batch();
                        }
                        committed_events.append(&mut pending);
                    }
                }
                Err(error) => {
                    self.quarantine_filter_runtime(filter_id);
                    diagnostics.push(PipelineDiagnostic::filter_queue_payload_delivery_failure(
                        packet_pid,
                        filter_id,
                        error,
                    ));
                }
            }
        }
        *generated_events = committed_events;
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
            .available_to_read()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        let mut out = vec![0u8; bytes];
        let read = queue
            .read_into(&mut out)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        out.truncate(read);
        Ok(out)
    }
}
