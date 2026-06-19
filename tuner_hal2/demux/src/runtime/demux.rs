use std::collections::{BTreeMap, VecDeque};

use crate::config::{AvStreamTypeConfig, FilterDelayHint, FilterOpenType, OpenFilterRequest};
use crate::packet_pipeline::{
    FilterPipelineConfig, PacketPipeline, PipelineFilterView, PipelineInputKind, PipelineOpenKind,
    PipelineReport, PipelineResetReport,
};
use crate::TsInputOrigin;

use super::dvr::{DvrKind, DvrRuntime};
use super::filter::{FilterRuntime, FilterRuntimeSnapshot, FilterRuntimeState};
use super::source_boundary::SourceBoundaryTxn;

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
    SourceLifecycle,
    SinkLifecycle,
    InvalidSourceSubtype,
    InvalidSinkSubtype,
    PidMismatch,
    PipelineFailed,
    GenerationExhausted,
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
    }

    pub fn register_filter(&mut self, filter: FilterRuntime) -> Result<(), DemuxRuntimeError> {
        if filter.state().is_closed_or_failed() {
            return Err(DemuxRuntimeError::invalid_state(filter.filter_id()));
        }
        self.filters.insert(filter.filter_id(), filter);
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
        self.dvrs.insert(dvr.dvr_id(), dvr);
        Ok(())
    }

    pub fn remove_dvr(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
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
        let queue = self
            .filter_queues
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        queue.clear();
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.clear_queue_marker();
        }
        Ok(())
    }

    pub fn remove_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        self.filter_queues
            .remove(&filter_id)
            .ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.clear_queue_marker();
        }
        Ok(())
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
        Ok(())
    }

    pub fn configure_filter_runtime(
        &mut self,
        filter_id: i32,
        config: FilterPipelineConfig,
    ) -> Result<(), DemuxRuntimeError> {
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
        if filter.queue_present() {
            self.filter_queues.entry(filter_id).or_default();
        } else {
            self.filter_queues.remove(&filter_id);
        }
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
        let filter = self
            .filters
            .get_mut(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        match filter.state() {
            FilterRuntimeState::Started => {
                self.pipeline
                    .stop_filter(filter_id)
                    .map_err(|_| DemuxRuntimeError::pipeline_failed())?;
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
        let filter = self
            .filters
            .get(&filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        match filter.state() {
            FilterRuntimeState::Configured
            | FilterRuntimeState::Started
            | FilterRuntimeState::Stopped => {
                let snapshot = filter.snapshot();
                let origins = [(snapshot.source.origin(), snapshot.tpid.unwrap_or(-1))];
                if snapshot.tpid.is_some() {
                    self.pipeline.flush_filter(filter_id, &origins);
                } else {
                    self.pipeline.clear_filter_state_after_flush(filter_id);
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
        Ok(())
    }

    pub fn disconnect_filter_source(
        &mut self,
        sink_filter_id: i32,
    ) -> Result<(), DemuxRuntimeError> {
        self.filters
            .get_mut(&sink_filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?
            .disconnect_source();
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
        let reset = source_boundary
            .reset_report()
            .cloned()
            .unwrap_or_default();
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
}
