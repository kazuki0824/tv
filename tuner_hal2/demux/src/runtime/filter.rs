use crate::config::{
    AvStreamTypeConfig, FilterDelayHint, FilterDelayHints, FilterOpenType, OpenFilterRequest,
};
use crate::packet_pipeline::{FilterPipelineConfig, PipelineFilterView, PipelineOpenKind};
use crate::runtime::{FilterStatusEvent, FilterWatermarkClassifier};
use crate::TsInputOrigin;
use std::time::{Duration, Instant};

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
    pub const fn is_started(self) -> bool {
        matches!(self, Self::Started)
    }
    pub const fn is_closed_or_failed(self) -> bool {
        matches!(
            self,
            Self::Closing | Self::CleanupFailed | Self::Closed | Self::Failed
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterSource {
    DemuxInput,
    SourceFilter {
        source_filter_id: i32,
        source_filter_generation: u64,
    },
}

impl FilterSource {
    pub const fn source_filter_origin(self) -> Option<TsInputOrigin> {
        match self {
            Self::DemuxInput => None,
            Self::SourceFilter {
                source_filter_id,
                source_filter_generation,
            } => Some(TsInputOrigin::SourceFilter {
                source_filter_id,
                source_filter_generation,
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterRuntimeSnapshot {
    pub state: FilterRuntimeState,
    pub generation: u64,
    pub open_type: FilterOpenType,
    pub open_kind: PipelineOpenKind,
    pub buffer_size: i32,
    pub callback_present: bool,
    pub pipeline_config: Option<FilterPipelineConfig>,
    pub tpid: Option<i32>,
    pub pes_stream_id: Option<i32>,
    pub raw: bool,
    pub source: FilterSource,
    pub source_relation_generation: u64,
    pub queue_present: bool,
    pub av_backing_present: bool,
    pub av_stream_type_hint: Option<AvStreamTypeConfig>,
    pub delay_hints: FilterDelayHints,
    pub queued_bytes: usize,
    pub delivery_not_before: Option<Instant>,
    pub callback_unhealthy: bool,
    pub last_watermark_status: Option<FilterStatusEvent>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilterRuntime {
    filter_id: i32,
    state: FilterRuntimeState,
    generation: u64,
    open_type: FilterOpenType,
    open_kind: PipelineOpenKind,
    buffer_size: i32,
    callback_present: bool,
    pipeline_config: Option<FilterPipelineConfig>,
    tpid: Option<i32>,
    pes_stream_id: Option<i32>,
    raw: bool,
    source: FilterSource,
    source_relation_generation: u64,
    queue_present: bool,
    av_backing_present: bool,
    av_stream_type_hint: Option<AvStreamTypeConfig>,
    delay_hints: FilterDelayHints,
    queued_bytes: usize,
    delivery_not_before: Option<Instant>,
    callback_unhealthy: bool,
    last_watermark_status: Option<FilterStatusEvent>,
}

impl FilterRuntime {
    #[cfg(test)]
    pub(crate) fn new(filter_id: i32, generation: u64, open_kind: PipelineOpenKind) -> Self {
        let open_type = match open_kind {
            PipelineOpenKind::Raw => FilterOpenType::TsRaw,
            PipelineOpenKind::Pcr => FilterOpenType::TsPcr,
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
            buffer_size: 0,
            callback_present: false,
            pipeline_config: None,
            tpid: None,
            pes_stream_id: None,
            raw: false,
            source: FilterSource::DemuxInput,
            source_relation_generation: 1,
            queue_present: false,
            av_backing_present: false,
            av_stream_type_hint: None,
            delay_hints: FilterDelayHints::default(),
            queued_bytes: 0,
            delivery_not_before: None,
            callback_unhealthy: false,
            last_watermark_status: None,
        }
    }

    pub(crate) fn new_typed(filter_id: i32, generation: u64, open_type: FilterOpenType) -> Self {
        Self {
            filter_id,
            state: FilterRuntimeState::Open,
            generation,
            open_type,
            open_kind: open_type.pipeline_open_kind(),
            buffer_size: 0,
            callback_present: false,
            pipeline_config: None,
            tpid: None,
            pes_stream_id: None,
            raw: false,
            source: FilterSource::DemuxInput,
            source_relation_generation: 1,
            queue_present: false,
            av_backing_present: false,
            av_stream_type_hint: None,
            delay_hints: FilterDelayHints::default(),
            queued_bytes: 0,
            delivery_not_before: None,
            callback_unhealthy: false,
            last_watermark_status: None,
        }
    }

    pub(crate) fn new_open_request(
        filter_id: i32,
        generation: u64,
        request: &OpenFilterRequest,
    ) -> Self {
        let mut runtime = Self::new_typed(filter_id, generation, request.open_type);
        runtime.buffer_size = request.buffer_size;
        runtime.callback_present = request.callback_present;
        runtime.queue_present = runtime.supports_normal_fmq_queue() && request.buffer_size > 0;
        runtime
    }

    pub fn filter_id(&self) -> i32 {
        self.filter_id
    }
    pub fn state(&self) -> FilterRuntimeState {
        self.state
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
    }
    pub fn open_kind(&self) -> PipelineOpenKind {
        self.open_kind
    }
    pub fn buffer_size(&self) -> i32 {
        self.buffer_size
    }
    pub fn queue_present(&self) -> bool {
        self.queue_present
    }
    pub fn supports_normal_fmq_queue(&self) -> bool {
        self.open_type.supports_normal_fmq_queue()
    }
    pub fn allows_queue_desc(&self) -> bool {
        match self.state {
            FilterRuntimeState::Open => self.supports_normal_fmq_queue(),
            FilterRuntimeState::Configured
            | FilterRuntimeState::Started
            | FilterRuntimeState::Stopped => self.queue_present,
            FilterRuntimeState::Closing
            | FilterRuntimeState::CleanupFailed
            | FilterRuntimeState::Closed
            | FilterRuntimeState::Failed => false,
        }
    }
    pub fn av_backing_present(&self) -> bool {
        self.av_backing_present
    }
    #[cfg(test)]
    pub fn delivery_readiness(&self) -> crate::config::FilterDelayReadiness {
        self.delay_hints.delivery_readiness(
            self.delivery_not_before
                .map(|deadline| {
                    if deadline <= Instant::now() {
                        u64::MAX
                    } else {
                        0
                    }
                })
                .unwrap_or(u64::MAX),
            self.queued_bytes,
        )
    }
    pub fn snapshot(&self) -> FilterRuntimeSnapshot {
        FilterRuntimeSnapshot {
            state: self.state,
            generation: self.generation,
            open_type: self.open_type,
            open_kind: self.open_kind,
            buffer_size: self.buffer_size,
            callback_present: self.callback_present,
            pipeline_config: self.pipeline_config.clone(),
            tpid: self.tpid,
            pes_stream_id: self.pes_stream_id,
            raw: self.raw,
            source: self.source,
            source_relation_generation: self.source_relation_generation,
            queue_present: self.queue_present,
            av_backing_present: self.av_backing_present,
            av_stream_type_hint: self.av_stream_type_hint,
            delay_hints: self.delay_hints,
            queued_bytes: self.queued_bytes,
            delivery_not_before: self.delivery_not_before,
            callback_unhealthy: self.callback_unhealthy,
            last_watermark_status: self.last_watermark_status,
        }
    }

    pub fn restore(&mut self, snapshot: FilterRuntimeSnapshot) {
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.open_type = snapshot.open_type;
        self.open_kind = snapshot.open_kind;
        self.buffer_size = snapshot.buffer_size;
        self.callback_present = snapshot.callback_present;
        self.pipeline_config = snapshot.pipeline_config;
        self.tpid = snapshot.tpid;
        self.pes_stream_id = snapshot.pes_stream_id;
        self.raw = snapshot.raw;
        self.source = snapshot.source;
        self.source_relation_generation = snapshot.source_relation_generation;
        self.queue_present = snapshot.queue_present;
        self.av_backing_present = snapshot.av_backing_present;
        self.av_stream_type_hint = snapshot.av_stream_type_hint;
        self.delay_hints = snapshot.delay_hints;
        self.queued_bytes = snapshot.queued_bytes;
        self.delivery_not_before = snapshot.delivery_not_before;
        self.callback_unhealthy = snapshot.callback_unhealthy;
        self.last_watermark_status = snapshot.last_watermark_status;
    }

    pub fn configure_with_generation(
        &mut self,
        generation: u64,
        config: FilterPipelineConfig,
        pes_stream_id: Option<i32>,
    ) {
        self.generation = generation;
        self.pipeline_config = Some(config.clone());
        self.tpid = config.tpid;
        self.pes_stream_id = pes_stream_id;
        self.raw = config.raw;
        self.queue_present = self.supports_normal_fmq_queue();
        self.av_backing_present = matches!(self.open_kind, PipelineOpenKind::Av);
        self.queued_bytes = 0;
        self.delivery_not_before = None;
        self.last_watermark_status = None;
        self.state = FilterRuntimeState::Configured;
    }

    pub(crate) fn configured_matches(
        &self,
        config: &FilterPipelineConfig,
        pes_stream_id: Option<i32>,
    ) -> bool {
        self.pipeline_config.as_ref() == Some(config) && self.pes_stream_id == pes_stream_id
    }

    pub(crate) fn accepts_pes_stream_id(&self, stream_id: u8) -> bool {
        match self.pes_stream_id {
            None | Some(crate::config::PES_STREAM_ID_WILDCARD) => true,
            Some(configured) => configured == i32::from(stream_id),
        }
    }

    pub fn set_av_stream_type_hint(&mut self, config: AvStreamTypeConfig) {
        self.av_stream_type_hint = Some(config);
    }

    pub fn set_delay_hint(&mut self, hint: FilterDelayHint) {
        match hint {
            FilterDelayHint::TimeDelayMs(0) => self.delay_hints.time_delay_ms = None,
            FilterDelayHint::TimeDelayMs(ms) => self.delay_hints.time_delay_ms = Some(ms),
            FilterDelayHint::DataSizeDelayBytes(0) => {
                self.delay_hints.data_size_delay_bytes = None;
            }
            FilterDelayHint::DataSizeDelayBytes(bytes) => {
                self.delay_hints.data_size_delay_bytes = Some(bytes);
            }
        }
        self.rearm_delivery_deadline_if_needed();
    }

    pub fn note_payload_queued(&mut self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let was_empty = self.queued_bytes == 0;
        self.queued_bytes = self.queued_bytes.saturating_add(bytes);
        if was_empty {
            self.rearm_delivery_deadline_if_needed();
        }
    }

    pub fn clear_queued_payload_state(&mut self) {
        self.queued_bytes = 0;
        self.delivery_not_before = None;
        self.last_watermark_status = None;
    }

    pub(crate) fn classify_watermark_transition(
        &mut self,
        capacity_bytes: usize,
        readable_bytes: usize,
    ) -> Option<FilterStatusEvent> {
        let status = FilterWatermarkClassifier::classify(capacity_bytes, readable_bytes)?;
        if self.last_watermark_status == Some(status) {
            return None;
        }
        self.last_watermark_status = Some(status);
        Some(status)
    }

    pub const fn source_relation_generation(&self) -> u64 {
        self.source_relation_generation
    }

    pub(crate) fn prepare_next_source_relation_generation(&self) -> Option<u64> {
        self.source_relation_generation
            .checked_add(1)
            .filter(|generation| *generation != 0)
    }

    pub(crate) fn disconnect_source(
        &mut self,
        expected_generation: u64,
        next_generation: u64,
    ) -> bool {
        if self.source_relation_generation != expected_generation
            || expected_generation.checked_add(1) != Some(next_generation)
            || next_generation == 0
        {
            return false;
        }
        self.source = FilterSource::DemuxInput;
        self.source_relation_generation = next_generation;
        true
    }

    pub(crate) fn set_source_filter(
        &mut self,
        expected_generation: u64,
        next_generation: u64,
        source_filter_id: i32,
        source_filter_generation: u64,
    ) -> bool {
        if self.source_relation_generation != expected_generation
            || expected_generation.checked_add(1) != Some(next_generation)
            || next_generation == 0
        {
            return false;
        }
        self.source = FilterSource::SourceFilter {
            source_filter_id,
            source_filter_generation,
        };
        self.source_relation_generation = next_generation;
        true
    }

    #[cfg(test)]
    pub fn set_source_filter_for_test(
        &mut self,
        source_filter_id: i32,
        source_filter_generation: u64,
    ) -> bool {
        let Some(next_generation) = self.prepare_next_source_relation_generation() else {
            return false;
        };
        self.set_source_filter(
            self.source_relation_generation,
            next_generation,
            source_filter_id,
            source_filter_generation,
        )
    }

    #[cfg(test)]
    pub(crate) fn set_source_relation_generation_for_test(&mut self, generation: u64) {
        self.source_relation_generation = generation;
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
            source_filter: match self.source {
                FilterSource::DemuxInput => None,
                FilterSource::SourceFilter {
                    source_filter_id,
                    source_filter_generation,
                } => Some((source_filter_id, source_filter_generation)),
            },
            open_kind: self.open_kind,
            section_raw: self.raw && matches!(self.open_kind, PipelineOpenKind::Section),
            pes_raw: self.raw && matches!(self.open_kind, PipelineOpenKind::Pes),
            wants_record_index: matches!(self.open_kind, PipelineOpenKind::Record),
        }
    }

    pub fn mark_started(&mut self) {
        self.state = FilterRuntimeState::Started;
        self.rearm_delivery_deadline_if_needed();
    }
    pub fn mark_stopped(&mut self) {
        self.state = FilterRuntimeState::Stopped;
        self.delivery_not_before = None;
    }
    pub fn mark_failed(&mut self) {
        self.state = FilterRuntimeState::Failed;
        self.delivery_not_before = None;
    }

    pub fn mark_callback_unhealthy(&mut self) {
        self.callback_unhealthy = true;
    }

    fn rearm_delivery_deadline_if_needed(&mut self) {
        if !self.state.is_started() || self.queued_bytes == 0 {
            self.delivery_not_before = None;
            return;
        }
        let Some(delay_ms) = self
            .delay_hints
            .time_delay_ms
            .filter(|delay_ms| *delay_ms > 0)
        else {
            self.delivery_not_before = None;
            return;
        };
        self.delivery_not_before = Instant::now().checked_add(Duration::from_millis(delay_ms));
    }
}
