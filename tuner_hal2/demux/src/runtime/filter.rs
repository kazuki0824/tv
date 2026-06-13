use crate::config::FilterOpenType;
use crate::packet_pipeline::{FilterPipelineConfig, PipelineFilterView, PipelineOpenKind};
use crate::TsInputOrigin;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterRuntimeState {
    Open,
    Configured,
    Started,
    Stopped,
    Closing,
    CleanupFailed,
    Closed,
    Failed,
}

impl FilterRuntimeState {
    pub const fn is_started(self) -> bool { matches!(self, Self::Started) }
    pub const fn is_closed_or_failed(self) -> bool { matches!(self, Self::Closing | Self::CleanupFailed | Self::Closed | Self::Failed) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterSource {
    DemuxInput,
    SourceFilter { source_filter_id: i32, source_filter_generation: u64 },
}

impl FilterSource {
    pub const fn origin(self) -> TsInputOrigin {
        match self {
            Self::DemuxInput => TsInputOrigin::Frontend,
            Self::SourceFilter { source_filter_id, source_filter_generation } => {
                TsInputOrigin::SourceFilter { source_filter_id, source_filter_generation }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterRuntimeSnapshot {
    pub state: FilterRuntimeState,
    pub generation: u64,
    pub open_type: FilterOpenType,
    pub open_kind: PipelineOpenKind,
    pub tpid: Option<i32>,
    pub raw: bool,
    pub source: FilterSource,
    pub queue_present: bool,
    pub av_backing_present: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterRuntime {
    filter_id: i32,
    state: FilterRuntimeState,
    generation: u64,
    open_type: FilterOpenType,
    open_kind: PipelineOpenKind,
    tpid: Option<i32>,
    raw: bool,
    source: FilterSource,
    queue_present: bool,
    av_backing_present: bool,
}

impl FilterRuntime {
    pub fn new(filter_id: i32, generation: u64, open_kind: PipelineOpenKind) -> Self {
        let open_type = match open_kind {
            PipelineOpenKind::Raw => FilterOpenType::TsRaw,
            PipelineOpenKind::Av => FilterOpenType::TsVideo,
            PipelineOpenKind::Section => FilterOpenType::TsSection,
            PipelineOpenKind::Pes => FilterOpenType::TsPes,
            PipelineOpenKind::Record => FilterOpenType::TsRecord,
            PipelineOpenKind::Other => FilterOpenType::TsRaw,
        };
        Self {
            filter_id,
            state: FilterRuntimeState::Open,
            generation,
            open_type,
            open_kind,
            tpid: None,
            raw: false,
            source: FilterSource::DemuxInput,
            queue_present: false,
            av_backing_present: false,
        }
    }

    pub fn new_typed(filter_id: i32, generation: u64, open_type: FilterOpenType) -> Self {
        Self {
            filter_id,
            state: FilterRuntimeState::Open,
            generation,
            open_type,
            open_kind: open_type.pipeline_open_kind(),
            tpid: None,
            raw: false,
            source: FilterSource::DemuxInput,
            queue_present: false,
            av_backing_present: false,
        }
    }

    pub fn filter_id(&self) -> i32 { self.filter_id }
    pub fn state(&self) -> FilterRuntimeState { self.state }
    pub fn generation(&self) -> u64 { self.generation }
    pub fn open_type(&self) -> FilterOpenType { self.open_type }
    pub fn open_kind(&self) -> PipelineOpenKind { self.open_kind }
    pub fn tpid(&self) -> Option<i32> { self.tpid }
    pub fn source(&self) -> FilterSource { self.source }
    pub fn queue_present(&self) -> bool { self.queue_present }
    pub fn av_backing_present(&self) -> bool { self.av_backing_present }

    pub fn snapshot(&self) -> FilterRuntimeSnapshot {
        FilterRuntimeSnapshot {
            state: self.state,
            generation: self.generation,
            open_type: self.open_type,
            open_kind: self.open_kind,
            tpid: self.tpid,
            raw: self.raw,
            source: self.source,
            queue_present: self.queue_present,
            av_backing_present: self.av_backing_present,
        }
    }

    pub fn restore(&mut self, snapshot: FilterRuntimeSnapshot) {
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.open_type = snapshot.open_type;
        self.open_kind = snapshot.open_kind;
        self.tpid = snapshot.tpid;
        self.raw = snapshot.raw;
        self.source = snapshot.source;
        self.queue_present = snapshot.queue_present;
        self.av_backing_present = snapshot.av_backing_present;
    }

    pub fn configure_with_generation(&mut self, generation: u64, config: FilterPipelineConfig) {
        self.generation = generation;
        self.tpid = config.tpid;
        self.raw = config.raw;
        self.source = FilterSource::DemuxInput;
        self.queue_present = matches!(self.open_kind, PipelineOpenKind::Raw | PipelineOpenKind::Record | PipelineOpenKind::Section | PipelineOpenKind::Pes);
        self.av_backing_present = matches!(self.open_kind, PipelineOpenKind::Av);
        self.state = FilterRuntimeState::Configured;
    }

    pub fn disconnect_source(&mut self) {
        self.source = FilterSource::DemuxInput;
    }

    pub fn set_source_filter(&mut self, source_filter_id: i32, source_filter_generation: u64) {
        self.source = FilterSource::SourceFilter { source_filter_id, source_filter_generation };
    }

    pub fn clear_queue_marker(&mut self) -> bool {
        let had_queue = self.queue_present;
        self.queue_present = false;
        had_queue
    }

    pub fn clear_av_backing_marker(&mut self) -> bool {
        let had_backing = self.av_backing_present;
        self.av_backing_present = false;
        had_backing
    }

    pub fn pipeline_view(&self) -> PipelineFilterView {
        PipelineFilterView {
            filter_id: self.filter_id,
            tpid: self.tpid,
            started: self.state.is_started(),
            has_upstream: !matches!(self.source, FilterSource::DemuxInput),
            open_kind: self.open_kind,
            section_raw: self.raw && matches!(self.open_kind, PipelineOpenKind::Section),
            pes_raw: self.raw && matches!(self.open_kind, PipelineOpenKind::Pes),
            wants_record_index: matches!(self.open_kind, PipelineOpenKind::Record),
        }
    }

    pub fn mark_started(&mut self) { self.state = FilterRuntimeState::Started; }
    pub fn mark_stopped(&mut self) { self.state = FilterRuntimeState::Stopped; }
    pub fn mark_failed(&mut self) { self.state = FilterRuntimeState::Failed; }
}
