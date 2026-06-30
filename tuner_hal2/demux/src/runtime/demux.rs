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
use crate::config::{
    AvStreamTypeConfig, ConfigInputPid, FilterDelayHint, FilterDelayReadiness, FilterOpenType,
    OpenFilterRequest,
};
use crate::packet_pipeline::{
    FilterPipelineConfig, PacketPipeline, PipelineDeliveryAction, PipelineFilterView,
    PipelineGeneratedEvent, PipelineInputKind, PipelineOpenKind, PipelineReport,
    PipelineResetReport,
};
use crate::TsInputOrigin;

use super::dvr::{DvrKind, DvrRuntime, DvrRuntimeSnapshot, DvrStatusEvent};
use super::filter::{FilterRuntime, FilterRuntimeSnapshot, FilterRuntimeState};
use super::queue_runtime::{QueueDescriptorSnapshot, QueueRuntime, QueueRuntimeError};
use super::source_boundary::apply_filter_source_boundary_change;

const TUNER_EVENT_DATA_READY: u32 = 1 << 0;
const MAX_FILTER_DELAY_MS: u64 = 10_000;

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
    pub state: DemuxRuntimeState,
    pub generation: u64,
    pub pipeline: PacketPipeline,
    pub filters: BTreeMap<i32, FilterRuntime>,
    pub dvrs: BTreeMap<i32, DvrRuntime>,
    pub filter_av_stale_data_ids: BTreeMap<i32, BTreeSet<AvDataId>>,
    #[cfg(test)]
    pub filter_queue_mirror: BTreeMap<i32, VecDeque<Vec<u8>>>,
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
    pub fn pipeline(&self) -> &PacketPipeline {
        &self.pipeline
    }
    pub fn pipeline_mut(&mut self) -> &mut PacketPipeline {
        &mut self.pipeline
    }
    pub fn filter(&self, filter_id: i32) -> Option<&FilterRuntime> {
        self.filters.get(&filter_id)
    }
    pub fn filter_mut(&mut self, filter_id: i32) -> Option<&mut FilterRuntime> {
        self.filters.get_mut(&filter_id)
    }
    pub fn dvr(&self, dvr_id: i32) -> Option<&DvrRuntime> {
        self.dvrs.get(&dvr_id)
    }
    pub fn dvr_mut(&mut self, dvr_id: i32) -> Option<&mut DvrRuntime> {
        self.dvrs.get_mut(&dvr_id)
    }
    pub fn mark_dvr_callback_unhealthy(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        dvr.mark_callback_unhealthy();
        Ok(())
    }
    pub fn mark_filter_callback_unhealthy(
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

    pub fn export_filter_av_shared_handle(
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

    pub fn release_filter_av_handle(
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
        let fallback_outcome = AvHandleReleaseTxn::classify(AvHandleReleaseInput {
            has_fd,
            data_id,
            client_state: ClientHandleState::NotExported,
            filter_state,
            shared_handle_exported: false,
            data_id_state: if known_stale_data_id {
                AvDataIdState::Stale
            } else {
                match filter_state {
                    AvFilterReleaseState::Closed if av_data_id > 0 => AvDataIdState::Stale,
                    _ => AvDataIdState::Unknown,
                }
            },
        });
        match fallback_outcome {
            AvHandleReleaseOutcome::ClientHandleReleaseAfterClose
            | AvHandleReleaseOutcome::StaleReleaseAfterClose { .. }
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

    pub fn restore(&mut self, snapshot: DemuxRuntimeSnapshot) -> Result<(), DemuxRuntimeError> {
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

    pub fn register_filter(&mut self, filter: FilterRuntime) -> Result<(), DemuxRuntimeError> {
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

    pub fn remove_filter(
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

    pub fn register_dvr(&mut self, dvr: DvrRuntime) -> Result<(), DemuxRuntimeError> {
        if dvr.state().is_closed_or_failed() {
            return Err(DemuxRuntimeError::invalid_state(dvr.dvr_id()));
        }
        let dvr_id = dvr.dvr_id();
        self.dvr_queue_runtimes.remove(&dvr_id);
        self.dvrs.insert(dvr_id, dvr);
        self.rebuild_dvr_queue_runtime(dvr_id)?;
        Ok(())
    }

    pub fn remove_dvr(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        if !self.dvrs.contains_key(&dvr_id) {
            return Err(DemuxRuntimeError::dvr_missing(dvr_id));
        }
        self.dvr_queue_runtimes.remove(&dvr_id);
        self.dvrs.remove(&dvr_id);
        Ok(())
    }

    pub fn create_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
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

    pub fn clear_existing_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
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

    pub fn remove_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        if !self.queue_exists(filter_id) {
            return Err(DemuxRuntimeError::queue_missing(filter_id));
        }
        self.filter_queue_runtimes.remove(&filter_id);
        #[cfg(test)]
        {
            self.filter_queue_mirror.remove(&filter_id);
        }
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.clear_queue_marker();
            filter.clear_queued_payload_state();
        }
        Ok(())
    }

    pub fn enqueue_filter_queue_payload(
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

    pub fn filter_delivery_readiness(
        &self,
        filter_id: i32,
    ) -> Result<FilterDelayReadiness, DemuxRuntimeError> {
        self.filters
            .get(&filter_id)
            .map(FilterRuntime::delivery_readiness)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))
    }

    pub fn drain_filter_queue_for_delivery(
        &mut self,
        filter_id: i32,
    ) -> Result<Vec<Vec<u8>>, DemuxRuntimeError> {
        let readiness = self.filter_delivery_readiness(filter_id)?;
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
    pub fn snapshot_filter_queue_bytes(&self, filter_id: i32) -> Option<Vec<u8>> {
        let queue = self.filter_queue_mirror.get(&filter_id)?;
        let mut out = Vec::new();
        for payload in queue {
            out.extend_from_slice(payload);
        }
        Some(out)
    }

    #[cfg(test)]
    pub fn mark_filter_av_shared_handle_exported_for_test(
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
    pub fn allocate_filter_av_payload_for_test(
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
    pub fn filter_av_active_slot_count_for_test(&self, filter_id: i32) -> Option<usize> {
        self.filter_av_backings
            .get(&filter_id)
            .map(AvSharedBacking::active_slot_count)
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

    pub fn restore_filter_snapshot(
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

    pub fn restore_dvr_snapshot(
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

    pub fn configure_filter_runtime(
        &mut self,
        filter_id: i32,
        config: FilterPipelineConfig,
    ) -> Result<(), DemuxRuntimeError> {
        self.drop_filter_av_backing_to_stale(filter_id);
        let av_backing_present = {
            let filter = self
                .filters
                .get_mut(&filter_id)
                .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
            let next = match next_generation(filter.generation()) {
                Ok(next) => next,
                Err(_) => {
                    filter.mark_failed();
                    return Err(DemuxRuntimeError::generation_exhausted(Some(filter_id)));
                }
            };
            filter.configure_with_generation(next, config.clone());
            filter.clear_queued_payload_state();
            filter.av_backing_present()
        };
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
        self.rebuild_filter_queue_runtime(filter_id)?;
        self.pipeline
            .configure_filter(filter_id, config)
            .map_err(|_| DemuxRuntimeError::pipeline_failed())
    }

    pub fn start_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let filter = self
            .filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        match filter.state() {
            FilterRuntimeState::Configured | FilterRuntimeState::Stopped => {
                self.pipeline
                    .start_filter(filter_id)
                    .map_err(|_| DemuxRuntimeError::pipeline_failed())?;
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

    pub fn stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let state = self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        match state.state() {
            FilterRuntimeState::Started => {
                self.pipeline
                    .stop_filter(filter_id)
                    .map_err(|_| DemuxRuntimeError::pipeline_failed())?;
                self.clear_filter_queue_runtime(filter_id)?;
                #[cfg(test)]
                {
                    if let Some(queue) = self.filter_queue_mirror.get_mut(&filter_id) {
                        queue.clear();
                    }
                }
                let filter = self
                    .filters
                    .get_mut(&filter_id)
                    .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
                filter.clear_queued_payload_state();
                filter.mark_stopped();
                Ok(())
            }
            FilterRuntimeState::Configured | FilterRuntimeState::Stopped => Ok(()),
            FilterRuntimeState::Open => Ok(()),
            FilterRuntimeState::Closing
            | FilterRuntimeState::CleanupFailed
            | FilterRuntimeState::Closed
            | FilterRuntimeState::Failed => Err(DemuxRuntimeError::sink_lifecycle(filter_id)),
        }
    }

    pub fn flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let snapshot = self.filter_snapshot(filter_id)?;
        match snapshot.state {
            FilterRuntimeState::Configured
            | FilterRuntimeState::Started
            | FilterRuntimeState::Stopped => {
                if let Some(tpid) = snapshot.tpid.and_then(ConfigInputPid::validate_tpid) {
                    let origins = [(snapshot.source.origin(), tpid)];
                    self.pipeline.flush_filter(filter_id, &origins);
                } else {
                    self.pipeline.clear_filter_state_after_flush(filter_id);
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
                if let Some(backing) = self.filter_av_backings.get_mut(&filter_id) {
                    backing.flush_slots_keep_exported_handle();
                }
                Ok(())
            }
            FilterRuntimeState::Open => Err(DemuxRuntimeError::invalid_state(filter_id)),
            FilterRuntimeState::Closing
            | FilterRuntimeState::CleanupFailed
            | FilterRuntimeState::Closed
            | FilterRuntimeState::Failed => Err(DemuxRuntimeError::sink_lifecycle(filter_id)),
        }
    }

    pub fn configure_filter_av_stream_type(
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

    pub fn set_filter_delay_hint(
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

    pub fn configure_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        let next = match next_generation(dvr.generation()) {
            Ok(next) => next,
            Err(_) => {
                dvr.mark_failed();
                return Err(DemuxRuntimeError::generation_exhausted(Some(dvr_id)));
            }
        };
        dvr.configure_with_generation(next);
        self.rebuild_dvr_queue_runtime(dvr_id)?;
        Ok(())
    }

    pub fn attach_dvr_filter(
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

    pub fn detach_dvr_filter(
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

    pub fn start_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let dvr = self
            .dvrs
            .get_mut(&dvr_id)
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        if dvr.callback_unhealthy() {
            return Err(DemuxRuntimeError::invalid_state(dvr_id));
        }
        match dvr.state() {
            super::dvr::DvrRuntimeState::Configured | super::dvr::DvrRuntimeState::Stopped => {
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

    pub fn stop_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
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

    pub fn flush_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
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

    pub fn set_dvr_status_check_interval(
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

    pub fn write_playback_dvr_queue_bytes(
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

    pub fn disconnect_filter_source(
        &mut self,
        sink_filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        let (_source_boundary, outcome) =
            apply_filter_source_boundary_change(self, sink_filter_id, None);
        outcome.map(|_| ())
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

    pub fn set_filter_source_non_null(
        &mut self,
        sink_filter_id: i32,
        source_filter_id: i32,
    ) -> Result<PipelineResetReport, DemuxRuntimeError> {
        let sink_snapshot = self.filter_snapshot(sink_filter_id)?;
        let source_snapshot = self.filter_snapshot(source_filter_id)?;
        if sink_snapshot.state.is_closed_or_failed() {
            return Err(DemuxRuntimeError::sink_lifecycle(sink_filter_id));
        }
        if source_snapshot.state.is_closed_or_failed() {
            return Err(DemuxRuntimeError::source_lifecycle(source_filter_id));
        }
        if source_snapshot.open_kind != PipelineOpenKind::Raw {
            return Err(DemuxRuntimeError::invalid_source_subtype(source_filter_id));
        }
        if matches!(sink_snapshot.open_kind, PipelineOpenKind::Other) {
            return Err(DemuxRuntimeError::invalid_sink_subtype(sink_filter_id));
        }
        if sink_snapshot.tpid.is_some()
            && source_snapshot.tpid.is_some()
            && sink_snapshot.tpid != source_snapshot.tpid
        {
            return Err(DemuxRuntimeError::pid_mismatch(source_filter_id));
        }
        let (source_boundary, outcome) = apply_filter_source_boundary_change(
            self,
            sink_filter_id,
            Some((source_filter_id, source_snapshot.generation)),
        );
        outcome?;
        let reset = source_boundary.reset_report().cloned().unwrap_or_default();
        Ok(reset)
    }

    pub fn reset_generation_boundary(&mut self) -> Result<PipelineResetReport, DemuxRuntimeError> {
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

    pub fn quarantine(&mut self) {
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

    pub fn push_ts_packet_from_origin(
        &mut self,
        packet: &[u8],
        origin: TsInputOrigin,
    ) -> PipelineReport {
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
        let validated = match crate::packet_pipeline::ValidatedTsPacket::validate(packet) {
            Ok(validated) => validated,
            Err(_) => return self.pipeline.push_ts_packet(packet, kind),
        };
        let mut report = self.pipeline.push_validated_ts_packet(&validated, kind);
        if report.accepted_packets == 0 {
            return report;
        }
        let filters = self.filter_views();
        let downstream = self
            .pipeline
            .plan_and_assemble_ts_packet_report_after_preflight(
                &validated,
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

    pub fn open_filter_runtime(
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

    pub fn open_filter_runtime_typed(
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

    pub fn open_filter_runtime_from_request(
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

    pub fn open_record_dvr_runtime(dvr_id: i32, generation: u64) -> DvrRuntime {
        DvrRuntime::new(dvr_id, DvrKind::Record, generation)
    }

    pub fn open_dvr_runtime(
        dvr_id: i32,
        generation: u64,
        kind: DvrKind,
        buffer_size: i32,
        callback_present: bool,
    ) -> DvrRuntime {
        DvrRuntime::new_open_request(dvr_id, kind, generation, buffer_size, callback_present)
    }

    pub fn export_filter_queue_descriptor(
        &self,
        filter_id: i32,
    ) -> Result<QueueDescriptorSnapshot, QueueDescriptorQueryError> {
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
        let queue = self
            .filter_queue_runtimes
            .get(&filter_id)
            .ok_or(QueueDescriptorQueryError::RuntimeMissing(filter_id))?;
        queue
            .export_descriptor()
            .map_err(QueueDescriptorQueryError::Runtime)
    }

    pub fn export_dvr_queue_descriptor(
        &self,
        dvr_id: i32,
    ) -> Result<QueueDescriptorSnapshot, QueueDescriptorQueryError> {
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
        let queue = self
            .dvr_queue_runtimes
            .get(&dvr_id)
            .ok_or(QueueDescriptorQueryError::RuntimeMissing(dvr_id))?;
        queue
            .export_descriptor()
            .map_err(QueueDescriptorQueryError::Runtime)
    }

    fn clear_filter_queue_runtime(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        if let Some(queue) = self.filter_queue_runtimes.get_mut(&filter_id) {
            queue
                .clear()
                .map_err(|_| DemuxRuntimeError::queue_runtime_failure(filter_id))?;
        }
        Ok(())
    }

    pub fn clear_dvr_queue_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        if let Some(queue) = self.dvr_queue_runtimes.get_mut(&dvr_id) {
            queue
                .clear()
                .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
        }
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
        if self.filter_queue_runtimes.contains_key(&filter_id) {
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
        if self.dvr_queue_runtimes.contains_key(&dvr_id) {
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
                if let Err(error) = self.try_write_record_dvr_packet(dvr_id, packet) {
                    diagnostics.push(
                        crate::packet_pipeline::PipelineDiagnostic::record_dvr_mirror_failure(
                            pid, filter_id, dvr_id, error,
                        ),
                    );
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
    ) -> Result<(), DemuxRuntimeError> {
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
            return Ok(());
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
                Ok(())
            }
            FmqDeliveryAction::Overflow => {
                if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                    dvr.mark_pending_overflow();
                }
                Ok(())
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

    pub fn read_record_dvr_queue_bytes(&self, dvr_id: i32) -> Result<Vec<u8>, DemuxRuntimeError> {
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
