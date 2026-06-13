use std::collections::{BTreeMap, VecDeque};

use crate::packet_pipeline::{FilterPipelineConfig, PacketPipeline, PipelineFilterView, PipelineInputKind, PipelineOpenKind, PipelineReport, PipelineResetReport};
use crate::TsInputOrigin;

use super::dvr::{DvrKind, DvrRuntime};
use super::filter::{FilterRuntime, FilterRuntimeSnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxRuntimeState { Open, Closing, CleanupFailed, Closed, Quarantined }

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DemuxRuntimeError { pub kind: DemuxRuntimeErrorKind, pub id: Option<i32> }

impl DemuxRuntimeError {
    pub const fn filter_missing(filter_id: i32) -> Self { Self { kind: DemuxRuntimeErrorKind::FilterMissing, id: Some(filter_id) } }
    pub const fn dvr_missing(dvr_id: i32) -> Self { Self { kind: DemuxRuntimeErrorKind::DvrMissing, id: Some(dvr_id) } }
    pub const fn queue_missing(filter_id: i32) -> Self { Self { kind: DemuxRuntimeErrorKind::QueueMissing, id: Some(filter_id) } }
    pub const fn invalid_state(id: i32) -> Self { Self { kind: DemuxRuntimeErrorKind::InvalidState, id: Some(id) } }
    pub const fn source_lifecycle(filter_id: i32) -> Self { Self { kind: DemuxRuntimeErrorKind::SourceLifecycle, id: Some(filter_id) } }
    pub const fn sink_lifecycle(filter_id: i32) -> Self { Self { kind: DemuxRuntimeErrorKind::SinkLifecycle, id: Some(filter_id) } }
    pub const fn invalid_source_subtype(filter_id: i32) -> Self { Self { kind: DemuxRuntimeErrorKind::InvalidSourceSubtype, id: Some(filter_id) } }
    pub const fn invalid_sink_subtype(filter_id: i32) -> Self { Self { kind: DemuxRuntimeErrorKind::InvalidSinkSubtype, id: Some(filter_id) } }
    pub const fn pid_mismatch(filter_id: i32) -> Self { Self { kind: DemuxRuntimeErrorKind::PidMismatch, id: Some(filter_id) } }
    pub const fn pipeline_failed() -> Self { Self { kind: DemuxRuntimeErrorKind::PipelineFailed, id: None } }
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
        Self { demux_id, state: DemuxRuntimeState::Open, generation, pipeline: PacketPipeline::default(), filters: BTreeMap::new(), dvrs: BTreeMap::new(), filter_queues: BTreeMap::new() }
    }
    pub fn demux_id(&self) -> i32 { self.demux_id }
    pub fn state(&self) -> DemuxRuntimeState { self.state }
    pub fn generation(&self) -> u64 { self.generation }
    pub fn pipeline(&self) -> &PacketPipeline { &self.pipeline }
    pub fn pipeline_mut(&mut self) -> &mut PacketPipeline { &mut self.pipeline }
    pub fn filter(&self, filter_id: i32) -> Option<&FilterRuntime> { self.filters.get(&filter_id) }
    pub fn filter_mut(&mut self, filter_id: i32) -> Option<&mut FilterRuntime> { self.filters.get_mut(&filter_id) }
    pub fn dvr(&self, dvr_id: i32) -> Option<&DvrRuntime> { self.dvrs.get(&dvr_id) }
    pub fn dvr_mut(&mut self, dvr_id: i32) -> Option<&mut DvrRuntime> { self.dvrs.get_mut(&dvr_id) }

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
        if filter.state().is_closed_or_failed() { return Err(DemuxRuntimeError::invalid_state(filter.filter_id())); }
        self.filters.insert(filter.filter_id(), filter);
        Ok(())
    }

    pub fn remove_filter(&mut self, filter_id: i32) -> Result<FilterRuntimeSnapshot, DemuxRuntimeError> {
        self.filter_queues.remove(&filter_id);
        self.filters.remove(&filter_id).map(|filter| filter.snapshot()).ok_or(DemuxRuntimeError::filter_missing(filter_id))
    }

    pub fn register_dvr(&mut self, dvr: DvrRuntime) {
        self.dvrs.insert(dvr.dvr_id(), dvr);
    }

    pub fn create_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        if !self.filters.contains_key(&filter_id) { return Err(DemuxRuntimeError::filter_missing(filter_id)); }
        self.filter_queues.entry(filter_id).or_default();
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            let mut snapshot = filter.snapshot();
            snapshot.queue_present = true;
            filter.restore(snapshot);
        }
        Ok(())
    }

    pub fn queue_exists(&self, filter_id: i32) -> bool { self.filter_queues.contains_key(&filter_id) }

    pub fn clear_existing_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        let queue = self.filter_queues.get_mut(&filter_id).ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        queue.clear();
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.clear_queue_marker();
        }
        Ok(())
    }

    pub fn remove_filter_queue(&mut self, filter_id: i32) -> Result<(), DemuxRuntimeError> {
        self.filter_queues.remove(&filter_id).ok_or(DemuxRuntimeError::queue_missing(filter_id))?;
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.clear_queue_marker();
        }
        Ok(())
    }

    pub fn filter_snapshot(&self, filter_id: i32) -> Result<FilterRuntimeSnapshot, DemuxRuntimeError> {
        self.filters.get(&filter_id).map(FilterRuntime::snapshot).ok_or(DemuxRuntimeError::filter_missing(filter_id))
    }

    pub fn restore_filter_snapshot(&mut self, filter_id: i32, snapshot: FilterRuntimeSnapshot) -> Result<(), DemuxRuntimeError> {
        self.filters.get_mut(&filter_id).ok_or(DemuxRuntimeError::filter_missing(filter_id))?.restore(snapshot);
        Ok(())
    }

    pub fn configure_filter_runtime(&mut self, filter_id: i32, config: FilterPipelineConfig) -> Result<(), DemuxRuntimeError> {
        let filter = self.filters.get_mut(&filter_id).ok_or(DemuxRuntimeError::filter_missing(filter_id))?;
        filter.configure(config.clone());
        self.pipeline.configure_filter(filter_id, config).map_err(|_| DemuxRuntimeError::pipeline_failed())
    }

    pub fn configure_dvr_runtime(&mut self, dvr_id: i32) -> Result<(), DemuxRuntimeError> {
        let dvr = self.dvrs.get_mut(&dvr_id).ok_or(DemuxRuntimeError::dvr_missing(dvr_id))?;
        dvr.configure();
        Ok(())
    }

    pub fn disconnect_filter_source(&mut self, sink_filter_id: i32) -> Result<(), DemuxRuntimeError> {
        self.filters.get_mut(&sink_filter_id).ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?.disconnect_source();
        Ok(())
    }

    pub fn set_filter_source_non_null(&mut self, sink_filter_id: i32, source_filter_id: i32) -> Result<PipelineResetReport, DemuxRuntimeError> {
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
        if matches!(sink_snapshot.open_kind, PipelineOpenKind::Raw | PipelineOpenKind::Record | PipelineOpenKind::Other) {
            return Err(DemuxRuntimeError::invalid_sink_subtype(sink_filter_id));
        }
        if sink_snapshot.tpid.is_some() && source_snapshot.tpid.is_some() && sink_snapshot.tpid != source_snapshot.tpid {
            return Err(DemuxRuntimeError::pid_mismatch(source_filter_id));
        }
        let reset = self.reset_generation_boundary();
        self.filters
            .get_mut(&sink_filter_id)
            .ok_or(DemuxRuntimeError::filter_missing(sink_filter_id))?
            .set_source_filter(source_filter_id, source_snapshot.generation);
        Ok(reset)
    }

    pub fn reset_generation_boundary(&mut self) -> PipelineResetReport {
        self.generation = self.generation.saturating_add(1);
        self.pipeline.reset_boundary()
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
        self.filters.values().map(FilterRuntime::pipeline_view).collect()
    }

    pub fn push_ts_packet_from_origin(&mut self, packet: &[u8], origin: TsInputOrigin) -> PipelineReport {
        let kind = match origin {
            TsInputOrigin::Frontend => PipelineInputKind::Live,
            TsInputOrigin::Playback => PipelineInputKind::Playback,
            TsInputOrigin::SourceFilter { source_filter_id, source_filter_generation } => PipelineInputKind::SourceFilter { source_filter_id, source_filter_generation },
        };
        let view_report = self.pipeline.push_ts_packet(packet, kind);
        if view_report.accepted_packets == 0 { return view_report; }
        let Some(view) = self.pipeline.inspect_ts_packet(packet) else { return view_report; };
        let filters = self.filter_views();
        self.pipeline.plan_and_assemble_ts_packet_report(&view, origin, &filters)
    }

    pub fn open_filter_runtime(filter_id: i32, generation: u64, kind: PipelineOpenKind, config: Option<FilterPipelineConfig>) -> FilterRuntime {
        let mut runtime = FilterRuntime::new(filter_id, generation, kind);
        if let Some(config) = config { runtime.configure(config); }
        runtime
    }

    pub fn open_record_dvr_runtime(dvr_id: i32, generation: u64) -> DvrRuntime {
        DvrRuntime::new(dvr_id, DvrKind::Record, generation)
    }
}
