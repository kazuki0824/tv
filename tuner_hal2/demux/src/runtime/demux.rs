use std::collections::{BTreeMap, VecDeque};

use maleicacid_tuner_hal2_control_core::{
    FmqDeliveryAction, FmqDeliveryTxn, FmqFailureKind, FmqObjectKind,
};

use crate::config::{
    AvStreamTypeConfig, FilterDelayHint, FilterDelayReadiness, FilterOpenType, OpenFilterRequest,
};
use crate::packet_pipeline::{
    FilterPipelineConfig, PacketPipeline, PipelineDeliveryAction, PipelineFilterView,
    PipelineGeneratedEvent, PipelineInputKind, PipelineOpenKind, PipelineReport,
    PipelineResetReport,
};
use crate::TsInputOrigin;

use super::dvr::{DvrKind, DvrRuntime, DvrRuntimeSnapshot};
use super::filter::{FilterRuntime, FilterRuntimeSnapshot, FilterRuntimeState};
use super::queue_runtime::{QueueDescriptorSnapshot, QueueRuntime, QueueRuntimeError};
use super::source_boundary::SourceBoundaryTxn;

const TUNER_EVENT_DATA_READY: u32 = 1 << 0;

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
    SourceLifecycle,
    SinkLifecycle,
    InvalidSourceSubtype,
    InvalidSinkSubtype,
    PidMismatch,
    PipelineFailed,
    GenerationExhausted,
    QueueRuntimeFailure,
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
}

pub fn next_generation(current: u64) -> Result<u64, DemuxRuntimeError> {
    current
        .checked_add(1)
        .ok_or(DemuxRuntimeError::generation_exhausted(None))
}

#[derive(Clone, Debug)]
pub struct DemuxRuntimeSnapshot {
    pub state: DemuxRuntimeState,
    pub generation: u64,
    pub pipeline: PacketPipeline,
    pub filters: BTreeMap<i32, FilterRuntime>,
    pub dvrs: BTreeMap<i32, DvrRuntime>,
    pub filter_queues: BTreeMap<i32, VecDeque<Vec<u8>>>,
}

#[derive(Debug)]
pub struct DemuxRuntime {
    demux_id: i32,
    state: DemuxRuntimeState,
    generation: u64,
    pipeline: PacketPipeline,
    filters: BTreeMap<i32, FilterRuntime>,
    dvrs: BTreeMap<i32, DvrRuntime>,
    filter_queues: BTreeMap<i32, VecDeque<Vec<u8>>>,
    filter_queue_runtimes: BTreeMap<i32, QueueRuntime>,
    dvr_queue_runtimes: BTreeMap<i32, QueueRuntime>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlaybackConsumeReport {
    pub bytes_read: usize,
    pub completed_packets: usize,
    pub malformed_bytes: usize,
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
            filter_queues: BTreeMap::new(),
            filter_queue_runtimes: BTreeMap::new(),
            dvr_queue_runtimes: BTreeMap::new(),
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

    pub fn snapshot(&self) -> DemuxRuntimeSnapshot {
        DemuxRuntimeSnapshot {
            state: self.state,
            generation: self.generation,
            pipeline: self.pipeline.clone(),
            filters: self.filters.clone(),
            dvrs: self.dvrs.clone(),
            filter_queues: self.filter_queues.clone(),
        }
    }

    pub fn restore(&mut self, snapshot: DemuxRuntimeSnapshot) {
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.pipeline = snapshot.pipeline;
        self.filters = snapshot.filters;
        self.dvrs = snapshot.dvrs;
        self.filter_queues = snapshot.filter_queues;
        let stale_filter_queue_ids: Vec<i32> = self
            .filter_queue_runtimes
            .keys()
            .copied()
            .filter(|filter_id| !self.filter_should_keep_queue_runtime(*filter_id))
            .collect();
        for filter_id in stale_filter_queue_ids {
            self.filter_queue_runtimes.remove(&filter_id);
        }
        let stale_dvr_queue_ids: Vec<i32> = self
            .dvr_queue_runtimes
            .keys()
            .copied()
            .filter(|dvr_id| !self.dvr_should_keep_queue_runtime(*dvr_id))
            .collect();
        for dvr_id in stale_dvr_queue_ids {
            self.dvr_queue_runtimes.remove(&dvr_id);
        }
        let filter_ids: Vec<i32> = self.filters.keys().copied().collect();
        for filter_id in filter_ids {
            let _ = self.rebuild_filter_queue_runtime(filter_id);
        }
        let dvr_ids: Vec<i32> = self.dvrs.keys().copied().collect();
        for dvr_id in dvr_ids {
            let _ = self.rebuild_dvr_queue_runtime(dvr_id);
        }
    }

    pub fn register_filter(&mut self, filter: FilterRuntime) -> Result<(), DemuxRuntimeError> {
        if filter.state().is_closed_or_failed() {
            return Err(DemuxRuntimeError::invalid_state(filter.filter_id()));
        }
        let filter_id = filter.filter_id();
        self.filter_queue_runtimes.remove(&filter_id);
        self.filters.insert(filter_id, filter);
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
        self.filter_queues.remove(&filter_id);
        self.filter_queue_runtimes.remove(&filter_id);
        self.pipeline
            .remove_filter(filter_id)
            .map_err(|_| DemuxRuntimeError::pipeline_failed())?;
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
        self.dvr_queue_runtimes.remove(&dvr_id);
        self.dvrs
            .remove(&dvr_id)
            .map(|_| ())
            .ok_or(DemuxRuntimeError::dvr_missing(dvr_id))
    }

    pub fn create_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        if !self.filters.contains_key(&filter_id) {
            return Err(DemuxRuntimeError::filter_missing(filter_id));
        }
        self.filter_queues.entry(filter_id).or_default();
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            let mut snapshot = filter.snapshot();
            snapshot.queue_present = true;
            filter.restore(snapshot);
        }
        Ok(())
    }

    pub fn queue_exists(&self, filter_id: i32) -> bool {
        self.filter_queues.contains_key(&filter_id)
    }

    pub fn clear_existing_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        {
            let queue = self
                .filter_queues
                .get_mut(&filter_id)
                .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
            queue.clear();
        }
        self.clear_filter_queue_runtime(filter_id)?;
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.clear_queue_marker();
            filter.clear_queued_payload_state();
        }
        Ok(())
    }

    pub fn remove_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        self.filter_queues
            .remove(&filter_id)
            .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        self.filter_queue_runtimes.remove(&filter_id);
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
        let queue = self
            .filter_queues
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        let payload_len = payload.len();
        queue.push_back(payload);
        let filter = self
            .filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        filter.note_payload_queued(payload_len);
        Ok(())
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
            .filter_queues
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        let drained: Vec<Vec<u8>> = queue.drain(..).collect();
        let filter = self
            .filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        filter.clear_queued_payload_state();
        Ok(drained)
    }

    pub fn snapshot_filter_queue_bytes(&self, filter_id: i32) -> Option<Vec<u8>> {
        let queue = self.filter_queues.get(&filter_id)?;
        let mut out = Vec::new();
        for payload in queue {
            out.extend_from_slice(payload);
        }
        Some(out)
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
        let queue_present = {
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
            filter.queue_present()
        };
        if queue_present {
            self.filter_queues.entry(filter_id).or_default();
        } else {
            self.filter_queues.remove(&filter_id);
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
                if let Some(queue) = self.filter_queues.get_mut(&filter_id) {
                    queue.clear();
                }
                self.clear_filter_queue_runtime(filter_id)?;
                let filter = self
                    .filters
                    .get_mut(&filter_id)
                    .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
                filter.clear_queued_payload_state();
                filter.mark_stopped();
                Ok(())
            }
            FilterRuntimeState::Configured | FilterRuntimeState::Stopped => Ok(()),
            FilterRuntimeState::Open => Err(DemuxRuntimeError::invalid_state(filter_id)),
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
                let origins = [(snapshot.source.origin(), snapshot.tpid.unwrap_or(-1))];
                if snapshot.tpid.is_some() {
                    self.pipeline.flush_filter(filter_id, &origins);
                } else {
                    self.pipeline.clear_filter_state_after_flush(filter_id);
                }
                if let Some(queue) = self.filter_queues.get_mut(&filter_id) {
                    queue.clear();
                }
                self.clear_filter_queue_runtime(filter_id)?;
                if let Some(filter) = self.filters.get_mut(&filter_id) {
                    filter.clear_queued_payload_state();
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
        Ok(())
    }

    pub fn set_filter_delay_hint(
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
                if dvr.kind() != DvrKind::Record {
                    return Err(DemuxRuntimeError::invalid_state(dvr_id));
                }
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
                if dvr.kind() != DvrKind::Record {
                    return Err(DemuxRuntimeError::invalid_state(dvr_id));
                }
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
        match dvr.state() {
            super::dvr::DvrRuntimeState::Configured | super::dvr::DvrRuntimeState::Stopped => {
                if dvr.kind() == DvrKind::Record && !dvr.has_attached_record_filters() {
                    return Err(DemuxRuntimeError::invalid_state(dvr_id));
                }
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
            super::dvr::DvrRuntimeState::Open => Err(DemuxRuntimeError::invalid_state(dvr_id)),
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
        for packet in &drain.packets {
            let _ = self.push_ts_packet_from_origin(packet, TsInputOrigin::Playback);
        }
        Ok(PlaybackConsumeReport {
            bytes_read: read,
            completed_packets: drain.packets.len(),
            malformed_bytes: drain.malformed_bytes,
        })
    }

    pub fn disconnect_filter_source(
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
        if matches!(
            sink_snapshot.open_kind,
            PipelineOpenKind::Raw | PipelineOpenKind::Record | PipelineOpenKind::Other
        ) {
            return Err(DemuxRuntimeError::invalid_sink_subtype(sink_filter_id));
        }
        if sink_snapshot.tpid.is_some()
            && source_snapshot.tpid.is_some()
            && sink_snapshot.tpid != source_snapshot.tpid
        {
            return Err(DemuxRuntimeError::pid_mismatch(source_filter_id));
        }
        let (source_boundary, outcome) = SourceBoundaryTxn::new(sink_filter_id).apply(self);
        outcome?;
        let reset = source_boundary.reset_report().cloned().unwrap_or_default();
        self.filters
            .get_mut(&sink_filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?
            .set_source_filter(source_filter_id, source_snapshot.generation);
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
        let mut report = self.pipeline.push_ts_packet(packet, kind);
        if report.accepted_packets == 0 {
            return report;
        }
        let Some(view) = self.pipeline.inspect_ts_packet(packet) else {
            return report;
        };
        let filters = self.filter_views();
        let downstream = self
            .pipeline
            .plan_and_assemble_ts_packet_report_after_preflight(
                &view,
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
        self.mirror_record_dvr_packets(packet, &report.delivery_actions);
        self.enqueue_queue_payloads_from_generated_events(packet, &report.generated_events);
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

    fn filter_should_keep_queue_runtime(&self, filter_id: i32) -> bool {
        self.filters
            .get(&filter_id)
            .is_some_and(Self::should_keep_filter_queue_runtime)
    }

    fn dvr_should_keep_queue_runtime(&self, dvr_id: i32) -> bool {
        self.dvrs
            .get(&dvr_id)
            .is_some_and(Self::should_keep_dvr_queue_runtime)
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
    ) {
        for action in delivery_actions {
            let PipelineDeliveryAction::DvrMirror { dvr_id: filter_id } = *action else {
                continue;
            };
            let target_ids = self.record_dvr_target_ids_for_filter(filter_id);
            for dvr_id in target_ids {
                let _ = self.try_write_record_dvr_packet(dvr_id, packet);
            }
        }
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
        let available = queue
            .available_to_write()
            .map_err(|_| DemuxRuntimeError::queue_runtime_failure(dvr_id))?;
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
    ) {
        for event in generated_events {
            let result = match event {
                PipelineGeneratedEvent::DataReady { filter_id }
                | PipelineGeneratedEvent::Record { filter_id } => {
                    self.enqueue_filter_queue_payload(*filter_id, packet.to_vec())
                }
                PipelineGeneratedEvent::SectionPayloadReady {
                    filter_id, bytes, ..
                } => self.enqueue_filter_queue_payload(*filter_id, bytes.clone()),
                PipelineGeneratedEvent::PesPacketReady {
                    filter_id, packet, ..
                } => self.enqueue_filter_queue_payload(*filter_id, packet.raw_bytes.clone()),
                PipelineGeneratedEvent::Section { .. } | PipelineGeneratedEvent::Pes { .. } => {
                    continue;
                }
            };
            if result.is_err() {
                continue;
            }
        }
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
