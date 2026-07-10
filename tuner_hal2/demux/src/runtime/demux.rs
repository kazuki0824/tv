#[cfg(test)]
use std::collections::VecDeque;
use std::collections::{BTreeMap, BTreeSet};

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
use crate::config::{AvStreamTypeConfig, ConfigInputPid, FilterDelayHint, OpenFilterRequest};
use crate::packet_pipeline::{
    FilterPipelineConfig, PacketPipeline, PipelineBoundaryReason, PipelineDeliveryAction,
    PipelineFilterView, PipelineGeneratedEvent, PipelineInputKind, PipelineOpenKind,
    PipelineReport, PipelineResetReport,
};
use crate::TsInputOrigin;

use super::dvr::{DvrKind, DvrRuntime, DvrRuntimeSnapshot, DvrStatusEvent};
use super::filter::{FilterRuntime, FilterRuntimeSnapshot, FilterRuntimeState};
use super::queue_runtime::{
    QueueDescriptorExportPlan, QueueDescriptorExportTarget, QueueRuntime, QueueRuntimeError,
};
use super::source_boundary::{
    apply_filter_source_boundary_change, connect_filter_source_boundary_change,
    SourceBoundaryReport,
};

const TUNER_EVENT_DATA_READY: u32 = 1 << 0;
const MAX_FILTER_DELAY_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordDvrMirrorWriteOutcome {
    Written,
    Overflow,
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

    fn failed(&mut self, step: FilterRuntimeOperationStep, error: DemuxRuntimeErrorKind) {
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
    filter_av_stale_data_ids: BTreeMap<i32, BTreeSet<AvDataId>>,
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
}

impl DvrStatusReportingRequest {
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
pub struct DemuxGenerationBoundaryRequest {
    reason: PipelineBoundaryReason,
}

impl DemuxGenerationBoundaryRequest {
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
    has_fd: bool,
    av_data_id: i64,
}

impl FilterAvHandleReleaseRequest {
    pub const fn new(filter_id: i32, has_fd: bool, av_data_id: i64) -> Self {
        Self {
            filter_id,
            has_fd,
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
    rollback_snapshots: BTreeMap<u64, DemuxRuntimeSnapshot>,
    next_rollback_token_id: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaybackConsumeReport {
    pub bytes_read: usize,
    pub completed_packets: usize,
    pub malformed_bytes: usize,
    pub packet_reports: Vec<PipelineReport>,
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
            rollback_snapshots: BTreeMap::new(),
            next_rollback_token_id: 1,
        }
    }
    pub fn demux_id(&self) -> i32 {
        self.demux_id
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

    pub fn snapshot(&self) -> DemuxRuntimeSnapshot {
        DemuxRuntimeSnapshot {
            state: self.state,
            generation: self.generation,
            pipeline: self.pipeline.clone(),
            filters: self.filters.clone(),
            dvrs: self.dvrs.clone(),
            filter_av_stale_data_ids: self.filter_av_stale_data_ids.clone(),
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
        let generation = snapshot.generation;
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
        if snapshot.generation != token.generation {
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
        let filter_queue_runtimes =
            Self::build_filter_queue_runtimes_for_snapshot(&snapshot.filters)?;
        let dvr_queue_runtimes = Self::build_dvr_queue_runtimes_for_snapshot(&snapshot.dvrs)?;
        let mut filter_av_backings = std::mem::take(&mut self.filter_av_backings);
        filter_av_backings.retain(|filter_id, _| {
            snapshot
                .filters
                .get(filter_id)
                .map(|filter| filter.av_backing_present())
                .unwrap_or(false)
        });

        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.pipeline = snapshot.pipeline;
        self.filters = snapshot.filters;
        self.dvrs = snapshot.dvrs;
        #[cfg(test)]
        {
            self.filter_queue_mirror = snapshot.filter_queue_mirror;
        }
        self.filter_queue_runtimes = filter_queue_runtimes;
        self.dvr_queue_runtimes = dvr_queue_runtimes;
        self.filter_av_backings = filter_av_backings;
        self.filter_av_stale_data_ids = snapshot.filter_av_stale_data_ids;
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
        self.filter_queue_runtimes.remove(&filter_id);
        #[cfg(test)]
        {
            self.filter_queue_mirror.remove(&filter_id);
        }
        self.filter_av_backings.remove(&filter_id);
        self.filter_av_stale_data_ids.remove(&filter_id);
        self.filters.insert(filter_id, filter);
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
        self.pipeline
            .remove_filter(filter_id)
            .map_err(|_| DemuxRuntimeError::pipeline_failed())?;
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
        self.rebuild_filter_queue_runtime(filter_id)?;
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
            filter.clear_queue_marker();
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
        )
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
            ReuseAndClear,
            Replace(QueueRuntime),
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
                    return Err(DemuxRuntimeError::generation_exhausted(Some(filter_id)));
                }
            };
            let queue_action = if filter.supports_normal_fmq_queue() && filter.buffer_size() > 0 {
                match self.filter_queue_runtimes.get(&filter_id) {
                    Some(queue) if queue.capacity_matches_buffer_size(filter.buffer_size()) => {
                        QueueConfigureAction::ReuseAndClear
                    }
                    _ => QueueConfigureAction::Replace(
                        QueueRuntime::new(filter.buffer_size(), true)
                            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?,
                    ),
                }
            } else {
                QueueConfigureAction::Remove
            };
            (
                next,
                queue_action,
                matches!(filter.open_kind(), PipelineOpenKind::Av),
            )
        };
        let old_pipeline = self.pipeline.clone();
        if self
            .pipeline
            .configure_filter(filter_id, config.clone())
            .is_err()
        {
            self.pipeline = old_pipeline;
            return Err(DemuxRuntimeError::pipeline_failed());
        }
        if let QueueConfigureAction::ReuseAndClear = queue_action {
            if let Err(error) = self.clear_filter_queue_runtime(filter_id) {
                self.pipeline = old_pipeline;
                return Err(error);
            }
        }
        self.drop_filter_av_backing_to_stale(filter_id);
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
                .insert(filter_id, AvSharedBacking::default());
        } else {
            self.filter_av_backings.remove(&filter_id);
        }
        match queue_action {
            QueueConfigureAction::Remove => {
                self.filter_queue_runtimes.remove(&filter_id);
            }
            QueueConfigureAction::ReuseAndClear => {}
            QueueConfigureAction::Replace(queue) => {
                self.filter_queue_runtimes.insert(filter_id, queue);
            }
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
                let old_pipeline = self.pipeline.clone();
                if let Err(_error) = self.pipeline.stop_filter(filter_id) {
                    let error = DemuxRuntimeError::pipeline_failed();
                    report.failed(FilterRuntimeOperationStep::PipelineStop, error.kind);
                    report.finish(FilterRuntimeOperationOutcome::Failed {
                        failed_step: FilterRuntimeOperationStep::PipelineStop,
                    });
                    return (report, Err(error));
                }
                report.succeeded(FilterRuntimeOperationStep::PipelineStop);
                if snapshot.queue_present {
                    if let Err(error) = self.clear_filter_queue_runtime(filter_id) {
                        report.failed(FilterRuntimeOperationStep::QueueClear, error.kind);
                        self.pipeline = old_pipeline;
                        report.succeeded(FilterRuntimeOperationStep::PipelineRollback);
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
                        report.finish(FilterRuntimeOperationOutcome::RolledBack {
                            failed_step: FilterRuntimeOperationStep::QueueClear,
                            rollback_step: FilterRuntimeOperationStep::PipelineRollback,
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
                        report.failed(FilterRuntimeOperationStep::QueuedPayloadClear, error.kind);
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
                report.failed(FilterRuntimeOperationStep::ValidateState, error.kind);
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
                report.failed(FilterRuntimeOperationStep::ValidateState, error.kind);
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
                let old_pipeline = self.pipeline.clone();
                if let Some(tpid) = snapshot.tpid.and_then(ConfigInputPid::validate_tpid) {
                    let origins = [(snapshot.source.origin(), tpid)];
                    self.pipeline.flush_filter(filter_id, &origins);
                } else {
                    self.pipeline.clear_filter_state_after_flush(filter_id);
                }
                report.succeeded(FilterRuntimeOperationStep::PipelineFlush);
                if snapshot.queue_present {
                    if let Err(error) = self.clear_filter_queue_runtime(filter_id) {
                        report.failed(FilterRuntimeOperationStep::QueueClear, error.kind);
                        self.pipeline = old_pipeline;
                        report.succeeded(FilterRuntimeOperationStep::PipelineRollback);
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
                        report.finish(FilterRuntimeOperationOutcome::RolledBack {
                            failed_step: FilterRuntimeOperationStep::QueueClear,
                            rollback_step: FilterRuntimeOperationStep::PipelineRollback,
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
                report.failed(FilterRuntimeOperationStep::ValidateState, error.kind);
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
                report.failed(FilterRuntimeOperationStep::ValidateState, error.kind);
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
        let (next, replacement_queue) = {
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
            let replacement_queue = QueueRuntime::new(dvr.buffer_size(), true)
                .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
            (next, replacement_queue)
        };
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        dvr.configure_with_generation(next);
        self.dvr_queue_runtimes.insert(dvr_id, replacement_queue);
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
        let available_to_write = queue
            .available_to_write()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        let capacity = usize::try_from(dvr.buffer_size())
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        let fill_bytes = capacity.saturating_sub(available_to_write);
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
    ) -> Result<(), DemuxRuntimeError> {
        self.flush_dvr_runtime(request.dvr_id)
    }

    pub(crate) fn flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let state = self
            .dvrs
            .get(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        match state.state() {
            super::dvr::DvrRuntimeState::Configured
            | super::dvr::DvrRuntimeState::Started
            | super::dvr::DvrRuntimeState::Stopped => {
                self.clear_dvr_queue_runtime(dvr_id)?;
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    if dvr.kind() == DvrKind::Playback {
                        dvr.clear_playback_completion();
                    }
                }
                Ok(())
            }
            super::dvr::DvrRuntimeState::Open => Err(DemuxRuntimeError::invalid_state(dvr_id)),
            super::dvr::DvrRuntimeState::Closing
            | super::dvr::DvrRuntimeState::CleanupFailed
            | super::dvr::DvrRuntimeState::Closed
            | super::dvr::DvrRuntimeState::Failed => Err(DemuxRuntimeError::invalid_state(dvr_id)),
        }
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
        let result = FmqDeliveryTxn::new(FmqObjectKind::DvrPlayback).commit_payload(
            data.len(),
            queue
                .write_checked(data)
                .map_err(|_| FmqFailureKind::WriteFailed),
            queue
                .wake(TUNER_EVENT_DATA_READY)
                .map_err(|_| FmqFailureKind::EventFlagWakeFailed),
        );
        match result.action {
            FmqDeliveryAction::Continue => Ok(result.bytes),
            FmqDeliveryAction::Overflow => Ok(0),
            FmqDeliveryAction::RuntimeFailed(_) => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_failed();
                }
                Err(DemuxRuntimeError::queue_runtime_failure(dvr_id))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn consume_playback_dvr_queue_for_test(
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
        let mut packet_reports = Vec::with_capacity(drain.packets.len());
        for packet in &drain.packets {
            packet_reports.push(self.push_ts_packet_from_origin(packet, TsInputOrigin::Playback));
        }
        Ok(PlaybackConsumeReport {
            bytes_read: read,
            completed_packets: drain.packets.len(),
            malformed_bytes: drain.malformed_bytes,
            packet_reports,
        })
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
                return Err(DemuxRuntimeError::generation_exhausted(Some(self.demux_id)));
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
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        queue
            .clear()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        Ok(())
    }

    pub(crate) fn clear_dvr_queue_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let queue = self
            .dvr_queue_runtimes
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::queue_missing(dvr_id))?;
        queue
            .clear()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        Ok(())
    }

    fn build_filter_queue_runtimes_for_snapshot(
        filters: &BTreeMap<i32, FilterRuntime>,
    ) -> Result<BTreeMap<i32, QueueRuntime>, DemuxRuntimeError> {
        let mut runtimes = BTreeMap::new();
        for (filter_id, filter) in filters {
            if Self::should_keep_filter_queue_runtime(filter) {
                let queue = QueueRuntime::new(filter.buffer_size(), true)
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
                let queue = QueueRuntime::new(dvr.buffer_size(), true)
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
        let queue = QueueRuntime::new(filter.buffer_size(), true)
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
        let queue = QueueRuntime::new(dvr.buffer_size(), true)
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        self.dvr_queue_runtimes.insert(dvr_id, queue);
        Ok(())
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

    fn should_keep_dvr_queue_runtime(dvr: &DvrRuntime) -> bool {
        !dvr.state().is_closed_or_failed() && dvr.buffer_size() > 0
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
