use crate::av::{
    AudioMediaFrame, AudioTimestampAssociation, AudioTimestampAssociationFailure,
    AvMediaEventMetadata,
};
use crate::config::{
    AvStreamTypeConfig, FilterDelayHint, FilterDelayHints, FilterOpenType, OpenFilterRequest,
    SectionConditionKind, SectionRuntimeConfig,
};
use crate::packet_pipeline::{
    FilterPipelineConfig, PacketPid, PipelineFilterView, PipelineOpenKind,
};
use crate::sections::{parse_raw_section_framing, parse_section_header, section_crc_valid};
use crate::runtime::{
    WatermarkClassifier, WatermarkDecision, WatermarkPolicy, WatermarkQueueSnapshot,
};
use crate::ts_core::PesPacket;
use crate::TsInputOrigin;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterStatusEvent {
    DataReady,
    LowWater,
    HighWater,
    Overflow,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SectionTableInstanceKey {
    origin: TsInputOrigin,
    filter_generation: u64,
    pid: i32,
    table_id: u8,
    table_id_extension: Option<u16>,
    version: Option<u8>,
    current_next: Option<bool>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SectionDeliveryState {
    target: Option<SectionTableInstanceKey>,
    target_last_section: Option<u8>,
    delivered_sections: [u64; 4],
    completed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreparedSectionDelivery {
    Repeat,
    SectionBitsOneShot,
    TableInfo {
        key: SectionTableInstanceKey,
        section_number: u8,
        last_section_number: u8,
    },
}

fn section_bits_match(section: &[u8], config: &SectionRuntimeConfig) -> bool {
    let mut has_negative_bits = false;
    let mut negative_bit_mismatched = false;
    for (index, mask) in config.condition.mask.iter().copied().enumerate() {
        if mask == 0 {
            continue;
        }
        let Some(data) = section.get(index).copied() else {
            return false;
        };
        let Some(filter) = config.condition.filter.get(index).copied() else {
            return false;
        };
        let Some(mode) = config.condition.mode.get(index).copied() else {
            return false;
        };
        let differing_bits = data ^ filter;
        let positive_mask = mask & !mode;
        if differing_bits & positive_mask != 0 {
            return false;
        }
        let negative_mask = mask & mode;
        if negative_mask != 0 {
            has_negative_bits = true;
            negative_bit_mismatched |= differing_bits & negative_mask != 0;
        }
    }
    !has_negative_bits || negative_bit_mismatched
}

fn section_was_delivered(delivered: &[u64; 4], section_number: u8) -> bool {
    let number = usize::from(section_number);
    (delivered[number / 64] & (1_u64 << (number % 64))) != 0
}

fn mark_section_delivered(delivered: &mut [u64; 4], section_number: u8) {
    let number = usize::from(section_number);
    delivered[number / 64] |= 1_u64 << (number % 64);
}

fn all_sections_delivered(delivered: &[u64; 4], last_section_number: u8) -> bool {
    (0..=last_section_number).all(|section_number| {
        section_was_delivered(delivered, section_number)
    })
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
    pub(crate) section_config: Option<SectionRuntimeConfig>,
    section_delivery_state: SectionDeliveryState,
    pub tpid: Option<i32>,
    pub pes_stream_id: Option<i32>,
    pub raw: bool,
    pub source: FilterSource,
    pub source_relation_generation: u64,
    pub queue_present: bool,
    pub av_backing_present: bool,
    pub av_stream_type_hint: Option<AvStreamTypeConfig>,
    pub(crate) audio_timestamp_association: AudioTimestampAssociation,
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
    section_config: Option<SectionRuntimeConfig>,
    section_delivery_state: SectionDeliveryState,
    tpid: Option<i32>,
    pes_stream_id: Option<i32>,
    raw: bool,
    source: FilterSource,
    source_relation_generation: u64,
    queue_present: bool,
    av_backing_present: bool,
    av_stream_type_hint: Option<AvStreamTypeConfig>,
    audio_timestamp_association: AudioTimestampAssociation,
    delay_hints: FilterDelayHints,
    queued_bytes: usize,
    delivery_not_before: Option<Instant>,
    callback_unhealthy: bool,
    last_watermark_status: Option<FilterStatusEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedAvMediaPayload {
    pub(crate) payload: Vec<u8>,
    pub(crate) metadata: AvMediaEventMetadata,
}

impl From<AudioMediaFrame> for PreparedAvMediaPayload {
    fn from(frame: AudioMediaFrame) -> Self {
        Self {
            payload: frame.payload,
            metadata: frame.metadata,
        }
    }
}

impl FilterRuntime {
    #[cfg(test)]
    pub(crate) fn new(filter_id: i32, generation: u64, open_kind: PipelineOpenKind) -> Self {
        let open_type = match open_kind {
            PipelineOpenKind::Other => FilterOpenType::TsUndefined,
            PipelineOpenKind::Raw => FilterOpenType::TsRaw,
            PipelineOpenKind::Pcr => FilterOpenType::TsPcr,
            PipelineOpenKind::Av => FilterOpenType::TsVideo,
            PipelineOpenKind::Section => FilterOpenType::TsSection,
            PipelineOpenKind::Pes => FilterOpenType::TsPes,
            PipelineOpenKind::Record => FilterOpenType::TsRecord,
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
            section_config: None,
            section_delivery_state: SectionDeliveryState::default(),
            tpid: None,
            pes_stream_id: None,
            raw: false,
            source: FilterSource::DemuxInput,
            source_relation_generation: 1,
            queue_present: false,
            av_backing_present: false,
            av_stream_type_hint: None,
            audio_timestamp_association: AudioTimestampAssociation::default(),
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
            section_config: None,
            section_delivery_state: SectionDeliveryState::default(),
            tpid: None,
            pes_stream_id: None,
            raw: false,
            source: FilterSource::DemuxInput,
            source_relation_generation: 1,
            queue_present: false,
            av_backing_present: false,
            av_stream_type_hint: None,
            audio_timestamp_association: AudioTimestampAssociation::default(),
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
            section_config: self.section_config.clone(),
            section_delivery_state: self.section_delivery_state.clone(),
            tpid: self.tpid,
            pes_stream_id: self.pes_stream_id,
            raw: self.raw,
            source: self.source,
            source_relation_generation: self.source_relation_generation,
            queue_present: self.queue_present,
            av_backing_present: self.av_backing_present,
            av_stream_type_hint: self.av_stream_type_hint,
            audio_timestamp_association: self.audio_timestamp_association.clone(),
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
        self.section_config = snapshot.section_config;
        self.section_delivery_state = snapshot.section_delivery_state;
        self.tpid = snapshot.tpid;
        self.pes_stream_id = snapshot.pes_stream_id;
        self.raw = snapshot.raw;
        self.source = snapshot.source;
        self.source_relation_generation = snapshot.source_relation_generation;
        self.queue_present = snapshot.queue_present;
        self.av_backing_present = snapshot.av_backing_present;
        self.av_stream_type_hint = snapshot.av_stream_type_hint;
        self.audio_timestamp_association = snapshot.audio_timestamp_association;
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
        self.section_config = None;
        self.section_delivery_state = SectionDeliveryState::default();
        self.tpid = config.tpid;
        self.pes_stream_id = pes_stream_id;
        self.raw = config.raw;
        self.queue_present = self.supports_normal_fmq_queue();
        self.av_backing_present = matches!(self.open_kind, PipelineOpenKind::Av);
        self.av_stream_type_hint = None;
        self.audio_timestamp_association.reset();
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

    pub(crate) fn set_section_runtime_config(&mut self, config: Option<SectionRuntimeConfig>) {
        self.section_config = config;
        self.section_delivery_state = SectionDeliveryState::default();
    }

    pub(crate) fn prepare_section_delivery(
        &self,
        origin: TsInputOrigin,
        pid: PacketPid,
        section: &[u8],
        raw: bool,
    ) -> Option<PreparedSectionDelivery> {
        let config = self.section_config.as_ref()?;
        let header = if raw && !config.check_crc {
            parse_raw_section_framing(section, config.length_field_bits)?
        } else {
            parse_section_header(section, config.length_field_bits)?
        };
        if config.check_crc && !section_crc_valid(section, config.length_field_bits) {
            return None;
        }
        match config.condition.kind {
            SectionConditionKind::SectionBits => {
                if !section_bits_match(&section[..header.total_length], config)
                    || !config.repeat && self.section_delivery_state.completed
                {
                    return None;
                }
                Some(if config.repeat {
                    PreparedSectionDelivery::Repeat
                } else {
                    PreparedSectionDelivery::SectionBitsOneShot
                })
            }
            SectionConditionKind::TableInfo => {
                if Some(i32::from(header.table_id)) != config.condition.table_id {
                    return None;
                }
                if config.condition.version.is_some()
                    && header.version.map(i32::from) != config.condition.version
                {
                    return None;
                }
                if config.repeat {
                    return Some(PreparedSectionDelivery::Repeat);
                }
                if self.section_delivery_state.completed {
                    return None;
                }
                let section_number = header.section_number.unwrap_or(0);
                let last_section_number = header.last_section_number.unwrap_or(0);
                if section_number > last_section_number {
                    return None;
                }
                let key = SectionTableInstanceKey {
                    origin,
                    filter_generation: self.generation,
                    pid: pid.to_i32_for_aidl_boundary(),
                    table_id: header.table_id,
                    table_id_extension: header.table_id_extension,
                    version: header.version,
                    current_next: header.current_next_indicator,
                };
                if self
                    .section_delivery_state
                    .target
                    .is_some_and(|target| target != key)
                    || self
                        .section_delivery_state
                        .target_last_section
                        .is_some_and(|last| last != last_section_number)
                    || section_was_delivered(
                        &self.section_delivery_state.delivered_sections,
                        section_number,
                    )
                {
                    return None;
                }
                Some(PreparedSectionDelivery::TableInfo {
                    key,
                    section_number,
                    last_section_number,
                })
            }
        }
    }

    pub(crate) fn commit_section_delivery(&mut self, prepared: PreparedSectionDelivery) -> bool {
        match prepared {
            PreparedSectionDelivery::Repeat => true,
            PreparedSectionDelivery::SectionBitsOneShot => {
                if self.section_delivery_state.completed {
                    return false;
                }
                self.section_delivery_state.completed = true;
                true
            }
            PreparedSectionDelivery::TableInfo {
                key,
                section_number,
                last_section_number,
            } => {
                if self.section_delivery_state.completed
                    || self
                        .section_delivery_state
                        .target
                        .is_some_and(|target| target != key)
                    || self
                        .section_delivery_state
                        .target_last_section
                        .is_some_and(|last| last != last_section_number)
                    || section_was_delivered(
                        &self.section_delivery_state.delivered_sections,
                        section_number,
                    )
                {
                    return false;
                }
                self.section_delivery_state.target.get_or_insert(key);
                self.section_delivery_state
                    .target_last_section
                    .get_or_insert(last_section_number);
                mark_section_delivered(
                    &mut self.section_delivery_state.delivered_sections,
                    section_number,
                );
                self.section_delivery_state.completed = all_sections_delivered(
                    &self.section_delivery_state.delivered_sections,
                    last_section_number,
                );
                true
            }
        }
    }

    pub(crate) fn reset_section_delivery_state(&mut self) {
        self.section_delivery_state = SectionDeliveryState::default();
    }

    pub(crate) fn accepts_pes_stream_id(&self, stream_id: u8) -> bool {
        match self.pes_stream_id {
            None | Some(crate::config::PES_STREAM_ID_WILDCARD) => true,
            Some(configured) => configured == i32::from(stream_id),
        }
    }

    pub fn set_av_stream_type_hint(&mut self, config: AvStreamTypeConfig) {
        self.av_stream_type_hint = Some(config);
        self.audio_timestamp_association.reset();
    }

    pub(crate) fn prepare_av_media_payloads(
        &mut self,
        packet: PesPacket,
        origin: TsInputOrigin,
    ) -> Result<Vec<PreparedAvMediaPayload>, AudioTimestampAssociationFailure> {
        if self.open_type != FilterOpenType::TsAudio {
            let metadata = AvMediaEventMetadata::from_pes(
                packet.stream_id,
                packet.pts_90khz,
                packet.dts_90khz,
                packet.is_pes_private_data,
            );
            return Ok(vec![PreparedAvMediaPayload {
                payload: packet.payload,
                metadata,
            }]);
        }
        self.audio_timestamp_association
            .extract(packet, origin)
            .map(|frames| {
                frames
                    .into_iter()
                    .map(PreparedAvMediaPayload::from)
                    .collect()
            })
    }

    pub(crate) fn reset_audio_timestamp_association(&mut self) {
        self.audio_timestamp_association.reset();
    }

    pub(crate) fn reset_audio_timestamp_association_for_origin(&mut self, origin: TsInputOrigin) {
        self.audio_timestamp_association.reset_if_origin(origin);
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
        if capacity_bytes == 0 || readable_bytes > capacity_bytes {
            return None;
        }

        let quarter = capacity_bytes / 4;
        let remainder = capacity_bytes % 4;
        let policy = WatermarkPolicy::OccupancyBand {
            low: quarter + usize::from(remainder != 0),
            high: quarter * 3 + (remainder * 3 + 3) / 4,
        };
        let snapshot = WatermarkQueueSnapshot::new(readable_bytes, capacity_bytes - readable_bytes);
        let status = match WatermarkClassifier::new(policy).classify(snapshot) {
            WatermarkDecision::Low => FilterStatusEvent::LowWater,
            WatermarkDecision::High => FilterStatusEvent::HighWater,
            WatermarkDecision::Empty
            | WatermarkDecision::Full
            | WatermarkDecision::NoTransition => return None,
        };
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
        self.audio_timestamp_association.reset();
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
        self.audio_timestamp_association.reset();
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
        self.reset_section_delivery_state();
        self.audio_timestamp_association.reset();
        self.state = FilterRuntimeState::Started;
        self.rearm_delivery_deadline_if_needed();
    }
    pub fn mark_stopped(&mut self) {
        self.reset_section_delivery_state();
        self.audio_timestamp_association.reset();
        self.state = FilterRuntimeState::Stopped;
        self.delivery_not_before = None;
    }
    pub fn mark_failed(&mut self) {
        self.audio_timestamp_association.reset();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn long_section(
        table_id: u8,
        version: u8,
        section_number: u8,
        last_section_number: u8,
    ) -> Vec<u8> {
        let mut section = vec![
            table_id,
            0xb0,
            9,
            0x12,
            0x34,
            0xc1 | ((version & 0x1f) << 1),
            section_number,
            last_section_number,
        ];
        let crc = crate::sections::crc32_mpeg(&section);
        section.extend_from_slice(&crc.to_be_bytes());
        section
    }

    fn section_runtime(
        condition: crate::config::SectionCondition,
        check_crc: bool,
        repeat: bool,
    ) -> FilterRuntime {
        let mut runtime = FilterRuntime::new(1, 7, PipelineOpenKind::Section);
        runtime.set_section_runtime_config(Some(SectionRuntimeConfig {
            check_crc,
            repeat,
            length_field_bits: 12,
            condition,
        }));
        runtime.mark_started();
        runtime
    }

    #[test]
    fn section_bits_one_shot_advances_only_after_commit() {
        let mut runtime = section_runtime(
            crate::config::SectionCondition {
                kind: SectionConditionKind::SectionBits,
                filter: vec![0x42],
                mask: vec![0xff],
                mode: vec![0],
                table_id: None,
                version: None,
            },
            true,
            false,
        );
        let section = long_section(0x42, 3, 0, 0);
        let origin = TsInputOrigin::frontend(9);
        let pid =
            PacketPid::from_config_pid(crate::config::ConfigInputPid::validate_tpid(0x11).unwrap());
        let prepared = runtime
            .prepare_section_delivery(origin, pid, &section, false)
            .unwrap();
        assert!(runtime
            .prepare_section_delivery(origin, pid, &section, false)
            .is_some());
        assert!(runtime.commit_section_delivery(prepared));
        assert!(runtime
            .prepare_section_delivery(origin, pid, &section, false)
            .is_none());
    }

    #[test]
    fn section_bits_negative_mode_requires_a_selected_mismatch() {
        let runtime = section_runtime(
            crate::config::SectionCondition {
                kind: SectionConditionKind::SectionBits,
                filter: vec![0x42],
                mask: vec![0xff],
                mode: vec![0xff],
                table_id: None,
                version: None,
            },
            false,
            true,
        );
        let origin = TsInputOrigin::frontend(9);
        let pid =
            PacketPid::from_config_pid(crate::config::ConfigInputPid::validate_tpid(0x11).unwrap());
        assert!(runtime
            .prepare_section_delivery(origin, pid, &long_section(0x42, 0, 0, 0), false)
            .is_none());
        assert!(runtime
            .prepare_section_delivery(origin, pid, &long_section(0x43, 0, 0, 0), false)
            .is_some());
    }

    #[test]
    fn table_info_one_shot_tracks_one_versioned_table_instance() {
        let mut runtime = section_runtime(
            crate::config::SectionCondition {
                kind: SectionConditionKind::TableInfo,
                filter: vec![0x42],
                mask: vec![0xff],
                mode: vec![0],
                table_id: Some(0x42),
                version: None,
            },
            true,
            false,
        );
        let origin = TsInputOrigin::frontend(9);
        let pid =
            PacketPid::from_config_pid(crate::config::ConfigInputPid::validate_tpid(0x11).unwrap());
        let section_one = runtime
            .prepare_section_delivery(origin, pid, &long_section(0x42, 3, 1, 1), false)
            .unwrap();
        assert!(runtime.commit_section_delivery(section_one));
        assert!(runtime
            .prepare_section_delivery(origin, pid, &long_section(0x42, 4, 0, 1), false)
            .is_none());
        let section_zero = runtime
            .prepare_section_delivery(origin, pid, &long_section(0x42, 3, 0, 1), false)
            .unwrap();
        assert!(runtime.commit_section_delivery(section_zero));
        assert!(runtime
            .prepare_section_delivery(origin, pid, &long_section(0x42, 3, 0, 1), false)
            .is_none());
    }

    #[test]
    fn section_crc_failure_is_discarded() {
        let runtime = section_runtime(
            crate::config::SectionCondition {
                kind: SectionConditionKind::SectionBits,
                filter: Vec::new(),
                mask: Vec::new(),
                mode: Vec::new(),
                table_id: None,
                version: None,
            },
            true,
            true,
        );
        let origin = TsInputOrigin::frontend(9);
        let pid =
            PacketPid::from_config_pid(crate::config::ConfigInputPid::validate_tpid(0x11).unwrap());
        let mut section = long_section(0x42, 0, 0, 0);
        section[8] ^= 1;
        assert!(runtime
            .prepare_section_delivery(origin, pid, &section, false)
            .is_none());
    }

    #[test]
    fn filter_projects_strict_rounded_watermark_decisions() {
        let mut runtime = FilterRuntime::new(1, 1, PipelineOpenKind::Raw);

        assert_eq!(
            runtime.classify_watermark_transition(10, 2),
            Some(FilterStatusEvent::LowWater)
        );
        assert_eq!(runtime.classify_watermark_transition(10, 2), None);
        assert_eq!(runtime.classify_watermark_transition(10, 3), None);
        assert_eq!(runtime.classify_watermark_transition(10, 8), None);
        assert_eq!(
            runtime.classify_watermark_transition(10, 9),
            Some(FilterStatusEvent::HighWater)
        );
    }

    #[test]
    fn filter_non_divisible_capacity_uses_ceiling_thresholds() {
        let mut runtime = FilterRuntime::new(1, 1, PipelineOpenKind::Raw);

        assert_eq!(
            runtime.classify_watermark_transition(5, 1),
            Some(FilterStatusEvent::LowWater)
        );
        assert_eq!(runtime.classify_watermark_transition(5, 2), None);
        assert_eq!(runtime.classify_watermark_transition(5, 4), None);
        assert_eq!(
            runtime.classify_watermark_transition(5, 5),
            Some(FilterStatusEvent::HighWater)
        );
    }
}
