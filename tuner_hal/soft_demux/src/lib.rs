pub mod record_index;
pub use crate::record_index::{CodecStartCodeScanner, RecordIndexEventBuilder, RecordIndexParser};
pub mod packet_pipeline;
pub mod sections;
pub mod ts_core;

use crate::packet_pipeline::{
    PacketPipeline, PipelineDeliveryAction, PipelineFilterView, PipelineGeneratedEvent, PipelineInputKind, PipelineOpenKind,
};
use crate::sections::{parse_section_header, section_crc_valid};
#[cfg(test)]
use crate::sections::SectionPushOutcome;
use crate::ts_core::PesPacket;
use maleicacid_tuner_hal_common::{
    DEMUX_MAX_AUDIO_FILTERS, DEMUX_MAX_PES_FILTERS,
    DEMUX_MAX_RECORD_FILTERS, DEMUX_MAX_SECTION_FILTERS, DEMUX_MAX_VIDEO_FILTERS,
    MAX_SECTION_FILTER_BYTES, MAX_SECTION_PAYLOAD_BYTES, TS_PACKET_SIZE,
};
use maleicacid_tuner_hal_dvr::{
    DvrQueueDiscipline, DvrQueueModel, FilterQueueDiscipline, FilterQueueModel, QueueKind,
    QueuePolicy,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
#[cfg(test)]
use maleicacid_tuner_hal_dvr::QueueOverflowPolicy;

const FILTER_DELAY_TIME_DISABLED_MS: u64 = 0;
const FILTER_DELAY_DATA_SIZE_DISABLED_BYTES: usize = 0;
const SECTION_TABLE_EXTENSION_ABSENT: u16 = 0;
const SECTION_VERSION_ABSENT: u8 = 0;
const SECTION_NUMBER_ABSENT: u8 = 0;
#[cfg(test)]
const TEST_TS_PACKET_SIZE_I32: i32 = TS_PACKET_SIZE as i32;
#[cfg(test)]
const TEST_TS_PACKET_BUFFER_SIZE: i32 = (TS_PACKET_SIZE * 4) as i32;

static FILTER_STOP_IDEMPOTENT_COUNT: AtomicU64 = AtomicU64::new(0);
static DVR_STOP_IDEMPOTENT_COUNT: AtomicU64 = AtomicU64::new(0);
static SET_DATA_SOURCE_SUCCESS_COUNT: AtomicU64 = AtomicU64::new(0);
static SOURCE_FILTER_DOWNSTREAM_DROP_COUNT: AtomicU64 = AtomicU64::new(0);
static SET_DATA_SOURCE_INVALID_PAIR_COUNT: AtomicU64 = AtomicU64::new(0);
static SOFT_DEMUX_DIAGNOSTIC_COUNTER_SATURATED: AtomicBool = AtomicBool::new(false);

fn should_log_soft_demux_counter(count: u64) -> bool {
    count <= 4 || count.is_power_of_two() || count % 64 == 0
}

fn record_soft_demux_diagnostic(counter: &AtomicU64, name: &str) {
    let total = counter.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
        current.checked_add(1)
    });
    let total = match total {
        Ok(previous) => previous + 1,
        Err(_) => {
            SOFT_DEMUX_DIAGNOSTIC_COUNTER_SATURATED.store(true, Ordering::SeqCst);
            u64::MAX
        }
    };
    if should_log_soft_demux_counter(total) {
        eprintln!("maleicacid-tuner-hal-soft-demux-diagnostic: {name} total={total}");
    }
}

fn increment_diagnostic_counter(value: &mut u64, saturated: &mut bool) {
    match value.checked_add(1) {
        Some(next) => *value = next,
        None => *saturated = true,
    }
}

fn add_diagnostic_counter(value: &mut u64, amount: u64, saturated: &mut bool) {
    match value.checked_add(amount) {
        Some(next) => *value = next,
        None => {
            *value = u64::MAX;
            *saturated = true;
        }
    }
}

fn add_queue_accounting(value: usize, amount: usize, saturated: &mut bool) -> Option<usize> {
    match value.checked_add(amount) {
        Some(next) => Some(next),
        None => {
            *saturated = true;
            None
        }
    }
}

fn sub_queue_accounting(value: usize, amount: usize, saturated: &mut bool) -> usize {
    match value.checked_sub(amount) {
        Some(next) => next,
        None => {
            *saturated = true;
            0
        }
    }
}

fn add_queue_outcome_counter(value: &mut usize, amount: usize, saturated: &mut bool) {
    match value.checked_add(amount) {
        Some(next) => *value = next,
        None => {
            *value = usize::MAX;
            *saturated = true;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxPathDirection {
    Record,
    Playback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum TsInputOrigin {
    Frontend,
    Playback,
    SourceFilter { source_filter_id: i32, source_filter_generation: u64 },
}

impl TsInputOrigin {
    fn allows_record_mirror(self) -> bool {
        matches!(self, TsInputOrigin::Frontend)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AssemblyGeneration {
    Section { pid: i32, generation: u64 },
    Pes { pid: i32, generation: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterOpenType {
    TsRaw,
    TsAudio,
    TsVideo,
    TsSection,
    TsPes,
    TsRecord,
    TsOther,
    NonTs,
}

impl FilterOpenType {
    pub fn is_media(self) -> bool {
        matches!(self, FilterOpenType::TsAudio | FilterOpenType::TsVideo)
    }

    pub fn accepts_config_kind(self, kind: &FilterConfigKind) -> bool {
        match self {
            FilterOpenType::TsAudio | FilterOpenType::TsVideo => {
                matches!(kind, FilterConfigKind::Av { .. })
            }
            FilterOpenType::TsRaw => matches!(kind, FilterConfigKind::Noinit),
            FilterOpenType::TsSection => matches!(kind, FilterConfigKind::Section { .. }),
            FilterOpenType::TsPes => matches!(kind, FilterConfigKind::PesData { .. }),
            FilterOpenType::TsRecord => matches!(kind, FilterConfigKind::Record { .. }),
            FilterOpenType::TsOther | FilterOpenType::NonTs => false,
        }
    }
}


fn pipeline_open_kind(open_type: FilterOpenType) -> PipelineOpenKind {
    match open_type {
        FilterOpenType::TsRaw => PipelineOpenKind::Raw,
        FilterOpenType::TsRecord => PipelineOpenKind::Record,
        FilterOpenType::TsSection => PipelineOpenKind::Section,
        FilterOpenType::TsPes => PipelineOpenKind::Pes,
        FilterOpenType::TsAudio | FilterOpenType::TsVideo => PipelineOpenKind::Av,
        FilterOpenType::TsOther | FilterOpenType::NonTs => PipelineOpenKind::Other,
    }
}
pub const DEMUX_FILTER_MAIN_TYPE_COUNT: usize = 5;
pub const DEMUX_FILTER_MAIN_TYPE_TS_BITS: i32 = 1;
const DEFAULT_DVR_STATUS_CHECK_INTERVAL_MS: i64 = 25;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterLinkagePolicyEntry {
    source_main_type_index: usize,
    source_main_type_bits: i32,
    destination_main_type_bits: i32,
    open_types: &'static [FilterOpenType],
}

const TS_LINKABLE_OPEN_TYPES: &[FilterOpenType] = &[
    FilterOpenType::TsRaw,
    FilterOpenType::TsRecord,
];

pub const FILTER_LINKAGE_POLICY: &[FilterLinkagePolicyEntry] = &[FilterLinkagePolicyEntry {
    source_main_type_index: 0,
    source_main_type_bits: DEMUX_FILTER_MAIN_TYPE_TS_BITS,
    destination_main_type_bits: DEMUX_FILTER_MAIN_TYPE_TS_BITS,
    open_types: TS_LINKABLE_OPEN_TYPES,
}];


pub fn can_link_filter_open_types(source: FilterOpenType, destination: FilterOpenType) -> bool {
    // r50ea48/WP-02: DESIGN_JA.md の SourceFilter 契約に合わせ、setDataSource()
    // 成功条件は raw TS packet を downstream raw TS / record 系へ渡す範囲だけに固定する。
    // section/PES/AV payload の直接多段再配送、および raw TS から section/PES/AV への
    // 再parse chain は本製品では advertised しない。要求された場合は UNAVAILABLE とする。
    matches!(
        (source, destination),
        (FilterOpenType::TsRaw, FilterOpenType::TsRaw)
            | (FilterOpenType::TsRaw, FilterOpenType::TsRecord)
    )
}

pub fn demux_link_caps_for_filter_linkage_policy() -> Vec<i32> {
    let mut link_caps = vec![0; DEMUX_FILTER_MAIN_TYPE_COUNT];
    for entry in FILTER_LINKAGE_POLICY {
        if entry.source_main_type_index < link_caps.len() {
            link_caps[entry.source_main_type_index] |= entry.destination_main_type_bits;
        }
    }
    link_caps
}

pub fn demux_link_caps_for_ts_filter_linkage() -> Vec<i32> {
    demux_link_caps_for_filter_linkage_policy()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FilterDelayHints {
    pub time_delay_ms: Option<u64>,
    pub data_size_delay_bytes: Option<usize>,
}

impl FilterDelayHints {
    pub fn has_active_hint(self) -> bool {
        self.time_delay_ms.unwrap_or(FILTER_DELAY_TIME_DISABLED_MS) > FILTER_DELAY_TIME_DISABLED_MS || self.data_size_delay_bytes.unwrap_or(FILTER_DELAY_DATA_SIZE_DISABLED_BYTES) > FILTER_DELAY_DATA_SIZE_DISABLED_BYTES
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterDelayHintState {
    TimeDelayMs(u64),
    DataSizeDelayBytes(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterDeliveryReadiness {
    Ready,
    WaitingForTime,
    WaitingForDataSize,
    MissingFilter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SectionConditionKind {
    SectionBits,
    TableInfo,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SectionCondition {
    pub filter_bytes: Vec<u8>,
    pub mask_bytes: Vec<u8>,
    pub mode_bytes: Vec<u8>,
    pub table_id: Option<i32>,
    pub version: Option<i32>,
}

impl SectionCondition {
    pub fn validates_section_filter_width(&self) -> bool {
        let max = MAX_SECTION_FILTER_BYTES as usize;
        self.filter_bytes.len() <= max
            && self.mask_bytes.len() <= max
            && self.mode_bytes.len() <= max
            && self.filter_bytes.len() == self.mask_bytes.len()
            && self.filter_bytes.len() == self.mode_bytes.len()
    }

    pub fn matches(&self, payload: &[u8], length_field_bits: i32) -> bool {
        if !self.validates_section_filter_width() {
            return false;
        }
        let Some(header) = parse_section_header(payload, length_field_bits) else {
            return false;
        };
        let payload = &payload[..header.total_length];
        if let Some(table_id) = self.table_id {
            if header.table_id != table_id as u8 {
                return false;
            }
        }
        if let Some(version) = self.version {
            if header.version.map(|v| v as i32) != Some(version) {
                return false;
            }
        }
        let width = self.filter_bytes.len();
        if width > payload.len() {
            return false;
        }
        for index in 0..width {
            let filter = self.filter_bytes[index];
            let mask = self.mask_bytes[index];
            let mode = self.mode_bytes[index];
            let value = payload[index];
            if ((value ^ filter) & mask) != (mode & mask) {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilterConfigKind {
    Noinit,
    Section {
        check_crc: bool,
        repeat: bool,
        raw: bool,
        length_field_bits: i32,
        condition_kind: SectionConditionKind,
        condition: SectionCondition,
    },
    Av {
        passthrough: bool,
        secure_memory: bool,
    },
    PesData {
        stream_id: i32,
        raw: bool,
    },
    Record {
        ts_index_mask: i32,
        sc_index_type: i32,
        sc_index_mask_bits: i32,
    },
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterConfig {
    pub tpid: i32,
    pub main_type_bits: i32,
    pub sub_type_hint: i32,
    pub kind: FilterConfigKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrConfig {
    pub direction: DemuxPathDirection,
    pub status_mask: i32,
    pub low_threshold: i64,
    pub high_threshold: i64,
    pub data_format: i32,
    pub packet_size: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterLifecycleState {
    Open,
    Configured,
    Started,
    Stopped,
    Closed,
    FailedClosed,
}

impl FilterLifecycleState {
    fn is_configured(self) -> bool {
        matches!(self, Self::Configured | Self::Started | Self::Stopped)
    }

    fn is_started(self) -> bool {
        matches!(self, Self::Started)
    }

    fn is_closed_or_failed(self) -> bool {
        matches!(self, Self::Closed | Self::FailedClosed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemuxFilterRecord {
    pub filter_id: i32,
    pub filter_type_bits: i32,
    pub open_type: FilterOpenType,
    pub buffer_size: i32,
    pub lifecycle: FilterLifecycleState,
    pub monitor_event_mask: i32,
    pub ip_cid: Option<i32>,
    pub data_upstream_filter_id: Option<i32>,
    pub pending_start_event: bool,
    pub pending_start_id: i32,
    pub ever_started: bool,
    pub delay_hints: FilterDelayHints,
    pub delivery_not_before: Option<Instant>,
    pub av_stream_type_hint: Option<i32>,
    pub av_stream_kind: Option<AvFilterStreamKind>,
    pub config: Option<FilterConfig>,
    pub queued_bytes: usize,
    pub pending_overflow: bool,
    pub overflow_events: u64,
    pub drop_bytes: u64,
    pub section_drop_events: u64,
    pub stale_partial_discards: u64,
    pub events_emitted: u64,
    pub diagnostic_counter_saturated: bool,
    pub delivery_generation: u64,
}


fn next_filter_delivery_generation_record(filter: &DemuxFilterRecord) -> Result<u64, DemuxConfigError> {
    let next = filter
        .delivery_generation
        .checked_add(1)
        .ok_or(DemuxConfigError::IdExhausted)?;
    if next > i32::MAX as u64 {
        return Err(DemuxConfigError::IdExhausted);
    }
    Ok(next)
}

fn bump_filter_delivery_generation_record(filter: &mut DemuxFilterRecord) -> Result<u64, DemuxConfigError> {
    let next = next_filter_delivery_generation_record(filter)?;
    filter.delivery_generation = next;
    Ok(next)
}

fn mark_filter_generation_exhausted(filter: &mut DemuxFilterRecord) {
    filter.set_lifecycle(FilterLifecycleState::FailedClosed);
    filter.pending_start_event = false;
    filter.pending_overflow = false;
    filter.delivery_not_before = None;
}

impl DemuxFilterRecord {
    fn new(
        filter_id: i32,
        filter_type_bits: i32,
        open_type: FilterOpenType,
        buffer_size: i32,
    ) -> Self {
        Self {
            filter_id,
            filter_type_bits,
            open_type,
            buffer_size,
            lifecycle: FilterLifecycleState::Open,
            monitor_event_mask: 0,
            ip_cid: None,
            data_upstream_filter_id: None,
            pending_start_event: false,
            pending_start_id: 0,
            ever_started: false,
            delay_hints: FilterDelayHints::default(),
            delivery_not_before: None,
            av_stream_type_hint: None,
            av_stream_kind: None,
            config: None,
            queued_bytes: 0,
            pending_overflow: false,
            overflow_events: 0,
            drop_bytes: 0,
            section_drop_events: 0,
            stale_partial_discards: 0,
            events_emitted: 0,
            diagnostic_counter_saturated: false,
            delivery_generation: 0,
        }
    }

    pub fn effective_av_stream_kind(&self) -> Option<AvFilterStreamKind> {
        self.av_stream_kind
            .or_else(|| av_kind_for_filter_open_type(self.open_type))
    }

    pub fn set_lifecycle(&mut self, lifecycle: FilterLifecycleState) {
        self.lifecycle = lifecycle;
    }

    pub fn is_configured_for_api(&self) -> bool {
        self.lifecycle.is_configured() && self.config.is_some()
    }

    pub fn is_started_for_api(&self) -> bool {
        self.lifecycle.is_started()
    }

    pub fn is_closed_or_failed_for_api(&self) -> bool {
        self.lifecycle.is_closed_or_failed()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterSourceSnapshot {
    pub filter_id: i32,
    pub configured: bool,
    pub started: bool,
    pub generation: u64,
    pub tpid: Option<i32>,
    pub open_type: FilterOpenType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrLifecycleState {
    Open,
    Configured,
    Started,
    Stopped,
    Closed,
    FailedClosed,
}

impl DvrLifecycleState {
    fn is_configured(self) -> bool {
        matches!(self, Self::Configured | Self::Started | Self::Stopped)
    }

    fn is_started(self) -> bool {
        matches!(self, Self::Started)
    }

}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemuxDvrRecord {
    pub dvr_id: i32,
    pub direction: DemuxPathDirection,
    pub buffer_size: i32,
    pub lifecycle: DvrLifecycleState,
    pub status_check_interval_hint_ms: i64,
    pub attached_filter_ids: Vec<i32>,
    pub config: Option<DvrConfig>,
    pub queued_bytes: usize,
    pub pending_overflow: bool,
    pub overflow_events: u64,
    pub drop_bytes: u64,
    pub section_drop_events: u64,
    pub stale_partial_discards: u64,
    pub playback_injected_packets: u64,
    pub playback_injected_bytes: u64,
    pub playback_malformed_bytes: u64,
    pub diagnostic_counter_saturated: bool,
}

impl DemuxDvrRecord {
    fn new(dvr_id: i32, direction: DemuxPathDirection, buffer_size: i32) -> Self {
        Self {
            dvr_id,
            direction,
            buffer_size,
            lifecycle: DvrLifecycleState::Open,
            status_check_interval_hint_ms: DEFAULT_DVR_STATUS_CHECK_INTERVAL_MS,
            attached_filter_ids: Vec::new(),
            config: None,
            queued_bytes: 0,
            pending_overflow: false,
            overflow_events: 0,
            drop_bytes: 0,
            section_drop_events: 0,
            stale_partial_discards: 0,
            playback_injected_packets: 0,
            playback_injected_bytes: 0,
            playback_malformed_bytes: 0,
            diagnostic_counter_saturated: false,
        }
    }

    pub fn set_lifecycle(&mut self, lifecycle: DvrLifecycleState) {
        self.lifecycle = lifecycle;
    }

    pub fn is_configured_for_api(&self) -> bool {
        self.lifecycle.is_configured() && self.config.is_some()
    }

    pub fn is_started_for_api(&self) -> bool {
        self.lifecycle.is_started()
    }

}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemuxSnapshot {
    pub demux_id: i32,
    pub frontend_id: Option<i32>,
    pub ci_cam_id: Option<i32>,
    pub filter_ids: Vec<i32>,
    pub dvr_ids: Vec<i32>,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvFilterStreamKind {
    Audio,
    Video,
}

fn pes_stream_id_kind(stream_id: i32) -> Option<AvFilterStreamKind> {
    match stream_id {
        0xc0..=0xdf => Some(AvFilterStreamKind::Audio),
        0xe0..=0xef => Some(AvFilterStreamKind::Video),
        _ => None,
    }
}

fn pes_stream_id_matches_av_kind(stream_id: i32, av_stream_kind: AvFilterStreamKind) -> bool {
    pes_stream_id_kind(stream_id) == Some(av_stream_kind)
}

fn av_kind_for_filter_open_type(open_type: FilterOpenType) -> Option<AvFilterStreamKind> {
    match open_type {
        FilterOpenType::TsAudio => Some(AvFilterStreamKind::Audio),
        FilterOpenType::TsVideo => Some(AvFilterStreamKind::Video),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxConfigError {
    NotFound,
    CapacityExceeded,
    InvalidKind,
    InvalidState,
    Unavailable,
    IdExhausted,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QueuePushOutcome {
    pub accepted_bytes: usize,
    pub accepted_entries: usize,
    pub dropped_bytes: usize,
    pub dropped_entries: usize,
    pub dropped_old: bool,
    pub dropped_new: bool,
    pub overflowed: bool,
    pub counter_saturated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketPushOutcome {
    Consumed,
    DroppedMalformed,
    DroppedTransportError,
    DroppedDuplicate,
    DroppedNoPayload,
    DroppedNoDelivery,
}

impl PacketPushOutcome {
    pub fn is_consumed(self) -> bool {
        matches!(self, Self::Consumed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackInjectionOutcome {
    ConsumedWithDelivery,
    ConsumedNoDelivery,
    Malformed,
    InvalidState,
    InternalError,
}

impl PlaybackInjectionOutcome {
    pub fn is_nonfatal_consumed(self) -> bool {
        matches!(
            self,
            Self::ConsumedWithDelivery | Self::ConsumedNoDelivery | Self::Malformed
        )
    }

    pub fn is_legacy_success(self) -> bool {
        matches!(self, Self::ConsumedWithDelivery | Self::ConsumedNoDelivery)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvPayloadMetadata {
    pub pts_90khz: Option<u64>,
    pub dts_90khz: Option<u64>,
    pub stream_id: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterPayload {
    Bytes(Vec<u8>),
    TsPacket(Vec<u8>),
    RecordPacket(Vec<u8>),
    PesData {
        bytes: Vec<u8>,
        stream_id: i32,
        raw: bool,
        metadata: AvPayloadMetadata,
    },
    AvEs {
        bytes: Vec<u8>,
        metadata: AvPayloadMetadata,
    },
}

impl FilterPayload {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Bytes(bytes) => bytes.as_slice(),
            Self::TsPacket(bytes) => bytes.as_slice(),
            Self::RecordPacket(_) => &[],
            Self::PesData { bytes, .. } => bytes.as_slice(),
            Self::AvEs { bytes, .. } => bytes.as_slice(),
        }
    }

    pub fn event_bytes(&self) -> &[u8] {
        match self {
            Self::RecordPacket(bytes) => bytes.as_slice(),
            _ => self.bytes(),
        }
    }

    pub fn event_len(&self) -> usize {
        self.event_bytes().len()
    }

    pub fn len(&self) -> usize {
        self.bytes().len()
    }

    pub fn av_metadata(&self) -> Option<&AvPayloadMetadata> {
        match self {
            Self::PesData { metadata, .. } | Self::AvEs { metadata, .. } => Some(metadata),
            _ => None,
        }
    }

    pub fn pes_stream_id(&self) -> Option<i32> {
        match self {
            Self::PesData { stream_id, .. } => Some(*stream_id),
            _ => None,
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Self::Bytes(bytes) => bytes,
            Self::TsPacket(bytes) => bytes,
            Self::RecordPacket(_) => Vec::new(),
            Self::PesData { bytes, .. } => bytes,
            Self::AvEs { bytes, .. } => bytes,
        }
    }
}

#[derive(Debug, Default)]
pub struct DemuxCore;


pub struct SoftDemuxConfigureTxn<'a> { demux: &'a mut DemuxCore }
pub(crate) struct SoftDemuxOriginTxn<'a> { demux: &'a mut DemuxCore }
pub struct AvSyncClock;

impl<'a> SoftDemuxConfigureTxn<'a> {
    pub fn new(demux: &'a mut DemuxCore) -> Self { Self { demux } }

    pub fn configure_filter(self, filter_id: i32, summary: FilterConfig) -> Result<(), DemuxConfigError> {
        self.demux.configure_filter_with_summary_result_impl(filter_id, summary)
    }

    pub fn configure_record_pid_set(self, filter_ids: &[i32], pids: &BTreeSet<u16>) -> Result<(), DemuxConfigError> {
        self.demux.configure_record_pid_set_impl(filter_ids, pids)
    }

    pub fn set_data_source(self, filter_id: i32, upstream_filter_id: i32) -> Result<(), DemuxConfigError> {
        self.demux.set_filter_data_source_result_impl(filter_id, upstream_filter_id)
    }

    pub fn restore_data_source(self, filter_id: i32, upstream_filter_id: Option<i32>) -> Result<(), DemuxConfigError> {
        self.demux.restore_filter_data_source_snapshot_impl(filter_id, upstream_filter_id)
    }
}

impl<'a> SoftDemuxOriginTxn<'a> {
    pub fn new(demux: &'a mut DemuxCore) -> Self { Self { demux } }
    pub fn set_source(self, filter_id: i32, upstream_filter_id: i32) -> Result<(), DemuxConfigError> {
        self.demux.set_filter_data_source_result_impl(filter_id, upstream_filter_id)
    }
    pub fn restore_source(self, filter_id: i32, upstream_filter_id: Option<i32>) -> Result<(), DemuxConfigError> {
        self.demux.restore_filter_data_source_snapshot_impl(filter_id, upstream_filter_id)
    }

    fn source_origin(&self, source_filter_id: i32) -> Option<TsInputOrigin> {
        self.demux.source_filter_origin_impl(source_filter_id)
    }

    fn active_origins_for_filter(&self, filter_id: i32) -> Vec<TsInputOrigin> {
        self.demux.active_input_origins_for_filter_impl(filter_id)
    }

    pub fn disconnect_downstreams(self, filter_id: i32) {
        self.demux.disconnect_downstreams_of_impl(filter_id);
    }

    pub fn mark_flush_generation(self, filter_id: i32) {
        self.demux.mark_filter_flush_generation_impl(filter_id);
    }

    pub fn reset_filter_source_origin(self, origin: TsInputOrigin, downstream_filter_id: i32, pid: i32) {
        self.demux.reset_source_origin_partial_state_impl(origin, downstream_filter_id, pid);
    }

    pub fn reset_downstreams_for_source(self, origin: TsInputOrigin, downstreams: &[(i32, i32)]) {
        self.demux.reset_source_filter_downstream_partial_state_impl(origin, downstreams);
    }
}

pub(crate) struct SoftDemuxOriginView<'a> { demux: &'a DemuxCore }

impl<'a> SoftDemuxOriginView<'a> {
    pub fn new(demux: &'a DemuxCore) -> Self { Self { demux } }

    pub fn source_origin(&self, source_filter_id: i32) -> Option<TsInputOrigin> {
        self.demux.source_filter_origin_impl(source_filter_id)
    }

    pub fn active_origins_for_filter(&self, filter_id: i32) -> Vec<TsInputOrigin> {
        self.demux.active_input_origins_for_filter_impl(filter_id)
    }
}

impl AvSyncClock {
    pub fn now_checked(base: i64, elapsed: std::time::Duration) -> Option<i64> {
        let elapsed_ns = elapsed.as_nanos();
        let elapsed_90khz_u128 = elapsed_ns.checked_mul(90_000)? / 1_000_000_000;
        let elapsed_90khz = i64::try_from(elapsed_90khz_u128).ok()?;
        base.checked_add(elapsed_90khz)
    }
}

impl DemuxCore {
    pub const fn new() -> Self {
        Self
    }

    pub fn new_handle(&self, demux_id: i32) -> DemuxHandle {
        DemuxHandle::new(demux_id)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SectionTableProgress {
    last_section_number: Option<u8>,
    seen_sections: BTreeSet<u8>,
}

impl SectionTableProgress {
    fn observe(&mut self, section_number: u8, last_section_number: u8) {
        self.last_section_number = Some(last_section_number);
        self.seen_sections.insert(section_number);
    }

    fn is_complete(&self) -> bool {
        let Some(last) = self.last_section_number else {
            return false;
        };
        (0..=last).all(|n| self.seen_sections.contains(&n))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SectionFilterRuntime {
    completed: bool,
    active_table_key: Option<(u8, u16, u8)>,
    seen_section_keys: BTreeSet<(u8, u16, u8, u8)>,
    table_progress: BTreeMap<(u8, u16, u8), SectionTableProgress>,
}

const AV_SYNC_33BIT_MODULUS: i64 = 1i64 << 33;
const AV_SYNC_33BIT_HALF_RANGE: i64 = 1i64 << 32;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AvSyncTimestampExtender {
    last_raw: Option<u64>,
    epoch: i64,
}

impl AvSyncTimestampExtender {
    fn update_checked(&mut self, raw_33bit: u64) -> Option<i64> {
        let raw = (raw_33bit & ((1u64 << 33) - 1)) as i64;
        if let Some(last_raw) = self.last_raw {
            let last = (last_raw & ((1u64 << 33) - 1)) as i64;
            let diff = raw - last;
            if diff < -AV_SYNC_33BIT_HALF_RANGE {
                self.epoch = self.epoch.checked_add(AV_SYNC_33BIT_MODULUS)?;
            } else if diff > AV_SYNC_33BIT_HALF_RANGE {
                self.epoch = self.epoch.checked_sub(AV_SYNC_33BIT_MODULUS)?;
            }
        }
        self.last_raw = Some(raw as u64);
        self.epoch.checked_add(raw)
    }

    fn reset(&mut self) {
        self.last_raw = None;
        self.epoch = 0;
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AvSyncJitterSmoothingState {
    enabled: bool,
    last_error_90khz: Option<i64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AvSyncPllState {
    enabled: bool,
    rate_ppm: i32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AvSyncState {
    pid: i32,
    pcr_pid: Option<i32>,
    service_clock_id: Option<i32>,
    stream_type_hint: Option<i32>,
    stream_kind: Option<AvFilterStreamKind>,
    jitter_smoothing: AvSyncJitterSmoothingState,
    pll: AvSyncPllState,
}

impl AvSyncState {
    fn new(pid: i32) -> Self {
        Self {
            pid,
            pcr_pid: None,
            service_clock_id: None,
            stream_type_hint: None,
            stream_kind: None,
            jitter_smoothing: AvSyncJitterSmoothingState::default(),
            pll: AvSyncPllState::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FilterCapacityKind {
    TsRaw,
    Section,
    Audio,
    Video,
    Pes,
    Record,
    Other,
}

#[derive(Clone)]
struct DemuxTxnSnapshot {
    filters: BTreeMap<i32, DemuxFilterRecord>,
    filter_queues: BTreeMap<i32, VecDeque<FilterPayload>>,
    section_filter_runtime: BTreeMap<i32, SectionFilterRuntime>,
    packet_pipeline: PacketPipeline,
    av_sync_states: BTreeMap<i32, AvSyncState>,
    av_sync_hw_ids: BTreeMap<i32, i32>,
    av_sync_filter_by_hw_id: BTreeMap<i32, i32>,
    next_av_sync_hw_id: i32,
}

pub struct DemuxHandle {
    demux_id: i32,
    frontend_id: Option<i32>,
    ci_cam_id: Option<i32>,
    filters: BTreeMap<i32, DemuxFilterRecord>,
    filter_queues: BTreeMap<i32, VecDeque<FilterPayload>>,
    section_filter_runtime: BTreeMap<i32, SectionFilterRuntime>,
    dvrs: BTreeMap<i32, DemuxDvrRecord>,
    dvr_queues: BTreeMap<i32, VecDeque<Vec<u8>>>,
    packet_pipeline: PacketPipeline,
    latest_pcr: Option<u64>,
    latest_pcr_instant: Option<Instant>,
    pcr_extender: AvSyncTimestampExtender,
    latest_pcr_90khz: Option<i64>,
    av_sync_states: BTreeMap<i32, AvSyncState>,
    av_sync_hw_ids: BTreeMap<i32, i32>,
    av_sync_filter_by_hw_id: BTreeMap<i32, i32>,
    next_av_sync_hw_id: i32,
    next_filter_id: i32,
    next_dvr_id: i32,
    closed: bool,
}

fn zero_payload_entry_limit(buffer_size: i32) -> usize {
    ((buffer_size.max(0) as usize) / TS_PACKET_SIZE).max(1)
}

impl DemuxHandle {
    pub fn new(demux_id: i32) -> Self {
        Self {
            demux_id,
            frontend_id: None,
            ci_cam_id: None,
            filters: BTreeMap::new(),
            filter_queues: BTreeMap::new(),
            section_filter_runtime: BTreeMap::new(),
            dvrs: BTreeMap::new(),
            dvr_queues: BTreeMap::new(),
            packet_pipeline: PacketPipeline::default(),
            latest_pcr: None,
            latest_pcr_instant: None,
            pcr_extender: AvSyncTimestampExtender::default(),
            latest_pcr_90khz: None,
            av_sync_states: BTreeMap::new(),
            av_sync_hw_ids: BTreeMap::new(),
            av_sync_filter_by_hw_id: BTreeMap::new(),
            next_av_sync_hw_id: 1,
            next_filter_id: 0,
            next_dvr_id: 0,
            closed: false,
        }
    }

    pub fn demux_id(&self) -> i32 {
        self.demux_id
    }

    fn pipeline_filter_views(&self) -> Vec<PipelineFilterView> {
        self.filters
            .iter()
            .map(|(id, filter)| PipelineFilterView {
                filter_id: *id,
                tpid: filter.config.as_ref().map(|config| config.tpid),
                started: filter.is_started_for_api(),
                has_upstream: filter.data_upstream_filter_id.is_some(),
                open_kind: pipeline_open_kind(filter.open_type),
                section_raw: matches!(filter.config.as_ref().map(|config| &config.kind), Some(FilterConfigKind::Section { raw: true, .. })),
                pes_raw: matches!(filter.config.as_ref().map(|config| &config.kind), Some(FilterConfigKind::PesData { raw: true, .. })),
                wants_record_index: self.record_filter_wants_index_events(*id),
            })
            .collect()
    }
    pub fn bind_frontend(&mut self, frontend_id: i32) {
        self.frontend_id = Some(frontend_id);
    }
    pub fn unbind_frontend(&mut self) {
        self.frontend_id = None;
    }
    pub fn frontend_id(&self) -> Option<i32> {
        self.frontend_id
    }
    pub fn connect_ci_cam(&mut self, _ci_cam_id: i32) { /* product HAL では CI CAM を support しないため、state は保存しない。 */
    }
    pub fn disconnect_ci_cam(&mut self) { /* product HAL では CI CAM を support しないため、state は保存しない。 */
    }
    pub fn clear_output_pid_blocks(&mut self) {
        self.packet_pipeline_drop_all_pes();
    }

    pub fn apply_stream_boundary_reset(&mut self) {
        self.packet_pipeline_drop_all_pes();
        self.filter_queues.clear();
        self.section_filter_runtime.clear();
        let filter_ids: Vec<i32> = self.filters.keys().copied().collect();
        for filter_id in filter_ids {
            if let Some(filter) = self.filters.get_mut(&filter_id) {
                filter.queued_bytes = 0;
                filter.pending_start_event = false;
                filter.pending_overflow = false;
                if bump_filter_delivery_generation_record(filter).is_err() {
                    mark_filter_generation_exhausted(filter);
                }
            }
            self.filter_queues.insert(filter_id, VecDeque::new());
            self.section_filter_runtime
                .insert(filter_id, SectionFilterRuntime::default());
        }
        self.dvr_queues.clear();
        let dvr_ids: Vec<i32> = self.dvrs.keys().copied().collect();
        for dvr_id in dvr_ids {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.queued_bytes = 0;
                dvr.pending_overflow = false;
            }
            self.dvr_queues.insert(dvr_id, VecDeque::new());
        }
        // r50dz60/G2-19: a lifecycle boundary must drain the completion buffer exactly once;
        // reset_boundary() accounts for completed residual packets and malformed tail bytes.
        self.packet_pipeline.reset_boundary();
        self.latest_pcr = None;
        self.latest_pcr_instant = None;
        self.pcr_extender.reset();
        self.latest_pcr_90khz = None;
    }

    pub fn next_filter_id_candidate(&self) -> Result<i32, DemuxConfigError> {
        if self.closed {
            return Err(DemuxConfigError::InvalidState);
        }
        self.next_filter_id
            .checked_add(1)
            .ok_or(DemuxConfigError::IdExhausted)
            .map(|_| self.next_filter_id)
    }

    pub fn register_filter_result(
        &mut self,
        filter_type_bits: i32,
        open_type: FilterOpenType,
        buffer_size: i32,
    ) -> Result<DemuxFilterRecord, DemuxConfigError> {
        if self.closed {
            return Err(DemuxConfigError::InvalidState);
        }
        let filter_id = self.next_filter_id;
        self.next_filter_id = self
            .next_filter_id
            .checked_add(1)
            .ok_or(DemuxConfigError::IdExhausted)?;
        let record = DemuxFilterRecord::new(filter_id, filter_type_bits, open_type, buffer_size);
        self.filters.insert(filter_id, record.clone());
        self.filter_queues.insert(filter_id, VecDeque::new());
        self.section_filter_runtime
            .insert(filter_id, SectionFilterRuntime::default());
        Ok(record)
    }


    fn reset_source_origin_partial_state_impl(
        &mut self,
        origin: TsInputOrigin,
        downstream_filter_id: i32,
        pid: i32,
    ) {
        self.packet_pipeline
            .mark_filter_flush_generation_for_origin(downstream_filter_id, pid, origin);
        self.packet_pipeline
            .reset_downstream_assembly_for_origin_pid_filter(origin, pid, downstream_filter_id);
    }

    fn source_filter_downstream_snapshots(&self, filter_id: i32) -> Vec<(i32, i32)> {
        self.filters
            .iter()
            .filter_map(|(id, downstream)| {
                if downstream.data_upstream_filter_id == Some(filter_id) {
                    downstream
                        .config
                        .as_ref()
                        .map(|config| (*id, config.tpid))
                } else {
                    None
                }
            })
            .collect()
    }

    fn reset_source_filter_downstream_partial_state_impl(
        &mut self,
        origin: TsInputOrigin,
        downstreams: &[(i32, i32)],
    ) {
        for (downstream_id, pid) in downstreams.iter().copied() {
            self.reset_source_origin_partial_state_impl(origin, downstream_id, pid);
        }
    }

    fn disconnect_downstreams_of_impl(&mut self, filter_id: i32) {
        let source_origin = self.source_filter_origin_impl(filter_id);
        let downstreams = self.source_filter_downstream_snapshots(filter_id);
        if let Some(origin) = source_origin {
            self.reset_source_filter_downstream_partial_state_impl(origin, &downstreams);
        }
        for (downstream_id, _) in downstreams {
            SoftDemuxOriginTxn::new(self).mark_flush_generation(downstream_id);
            if let Some(downstream) = self.filters.get_mut(&downstream_id) {
                // 表SSOTどおり、既出力 queue / pending event は維持し、新規配送だけ止める。
                // ただし source filter 契約上、接続解除境界では downstream の partial state を破棄する。
                downstream.data_upstream_filter_id = None;
                downstream.set_lifecycle(FilterLifecycleState::Stopped);
                downstream.delivery_not_before = None;
            }
        }
    }

    pub fn unregister_filter(&mut self, filter_id: i32) -> Option<DemuxFilterRecord> {
        let pid = self
            .filters
            .get(&filter_id)
            .and_then(|filter| filter.config.as_ref().map(|config| config.tpid));
        for dvr in self.dvrs.values_mut() {
            dvr.attached_filter_ids.retain(|id| *id != filter_id);
        }
        self.filter_queues.remove(&filter_id);
        self.section_filter_runtime.remove(&filter_id);
        self.av_sync_states.remove(&filter_id);
        if let Some(hw_id) = self.av_sync_hw_ids.remove(&filter_id) {
            self.av_sync_filter_by_hw_id.remove(&hw_id);
        }
        self.packet_pipeline.clear_filter_state(filter_id);
        SoftDemuxOriginTxn::new(self).disconnect_downstreams(filter_id);
        let removed = self.filters.remove(&filter_id);
        if let Some(pid) = pid {
            self.prune_assemblers_for_pid(pid);
        }
        removed
    }

    pub fn filter_ids(&self) -> Vec<i32> {
        self.filters.keys().copied().collect()
    }
    pub fn has_filter(&self, filter_id: i32) -> bool {
        self.filters.contains_key(&filter_id)
    }

    pub fn oversized_section_drop_count(&self) -> u64 {
        self.packet_pipeline.oversized_section_drop_count()
    }

    pub fn stale_partial_section_discard_count(&self) -> u64 {
        self.packet_pipeline.stale_partial_section_discard_count()
    }

    pub fn filter_section_drop_event_count(&self, filter_id: i32) -> Option<u64> {
        self.filters
            .get(&filter_id)
            .map(|filter| filter.section_drop_events)
    }

    pub fn filter_stale_partial_discard_count(&self, filter_id: i32) -> Option<u64> {
        self.filters
            .get(&filter_id)
            .map(|filter| filter.stale_partial_discards)
    }

    #[cfg(test)]
    fn apply_section_push_outcome(&mut self, filter_id: i32, outcome: &SectionPushOutcome) {
        if !outcome.has_drop_or_discard() {
            return;
        }
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.pending_overflow = true;
            add_diagnostic_counter(
                &mut filter.overflow_events,
                outcome
                    .oversized_section_drop_delta
                    .checked_add(outcome.stale_partial_discard_delta)
                    .unwrap_or_else(|| {
                        filter.diagnostic_counter_saturated = true;
                        u64::MAX
                    }),
                &mut filter.diagnostic_counter_saturated,
            );
            add_diagnostic_counter(
                &mut filter.section_drop_events,
                outcome.oversized_section_drop_delta,
                &mut filter.diagnostic_counter_saturated,
            );
            add_diagnostic_counter(
                &mut filter.stale_partial_discards,
                outcome.stale_partial_discard_delta,
                &mut filter.diagnostic_counter_saturated,
            );
            if outcome.oversized_section_counter_saturated || outcome.stale_partial_counter_saturated {
                filter.diagnostic_counter_saturated = true;
            }
        }
    }

    fn configured_filter_counts_against(
        record: &DemuxFilterRecord,
        kind: FilterCapacityKind,
    ) -> bool {
        let Some(cfg) = record.config.as_ref() else {
            return false;
        };
        match kind {
            FilterCapacityKind::TsRaw => matches!(cfg.kind, FilterConfigKind::Noinit),
            FilterCapacityKind::Section => matches!(cfg.kind, FilterConfigKind::Section { .. }),
            FilterCapacityKind::Audio => {
                matches!(cfg.kind, FilterConfigKind::Av { .. })
                    && record.effective_av_stream_kind() == Some(AvFilterStreamKind::Audio)
            }
            FilterCapacityKind::Video => {
                matches!(cfg.kind, FilterConfigKind::Av { .. })
                    && record.effective_av_stream_kind() == Some(AvFilterStreamKind::Video)
            }
            FilterCapacityKind::Pes => matches!(cfg.kind, FilterConfigKind::PesData { .. }),
            FilterCapacityKind::Record => matches!(cfg.kind, FilterConfigKind::Record { .. }),
            FilterCapacityKind::Other => false,
        }
    }

    fn filter_capacity_limit(kind: FilterCapacityKind) -> usize {
        match kind {
            FilterCapacityKind::TsRaw => maleicacid_tuner_hal_common::DEMUX_MAX_TS_FILTERS.max(0) as usize,
            FilterCapacityKind::Section => DEMUX_MAX_SECTION_FILTERS.max(0) as usize,
            FilterCapacityKind::Audio => DEMUX_MAX_AUDIO_FILTERS.max(0) as usize,
            FilterCapacityKind::Video => DEMUX_MAX_VIDEO_FILTERS.max(0) as usize,
            FilterCapacityKind::Pes => DEMUX_MAX_PES_FILTERS.max(0) as usize,
            FilterCapacityKind::Record => DEMUX_MAX_RECORD_FILTERS.max(0) as usize,
            FilterCapacityKind::Other => 0,
        }
    }

    fn filter_kind_capacity(open_type: FilterOpenType, summary: &FilterConfigKind) -> FilterCapacityKind {
        match summary {
            FilterConfigKind::Noinit => FilterCapacityKind::TsRaw,
            FilterConfigKind::Section { .. } => FilterCapacityKind::Section,
            FilterConfigKind::Av { .. } => match av_kind_for_filter_open_type(open_type) {
                Some(AvFilterStreamKind::Audio) => FilterCapacityKind::Audio,
                Some(AvFilterStreamKind::Video) => FilterCapacityKind::Video,
                None => FilterCapacityKind::Other,
            },
            FilterConfigKind::PesData { .. } => FilterCapacityKind::Pes,
            FilterConfigKind::Record { .. } => FilterCapacityKind::Record,
            FilterConfigKind::Other => FilterCapacityKind::Other,
        }
    }

    fn filter_capacity_available(&self, filter_id: i32, kind: FilterCapacityKind) -> bool {
        let limit = Self::filter_capacity_limit(kind);
        if limit == 0 {
            return false;
        }
        let configured = self
            .filters
            .iter()
            .filter(|(id, record)| {
                **id != filter_id && Self::configured_filter_counts_against(record, kind)
            })
            .count();
        configured < limit
    }

    #[cfg(test)]
    pub fn configure_filter(&mut self, filter_id: i32) -> bool {
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.set_lifecycle(FilterLifecycleState::Configured);
            return true;
        }
        false
    }

    fn txn_snapshot(&self) -> DemuxTxnSnapshot {
        DemuxTxnSnapshot {
            filters: self.filters.clone(),
            filter_queues: self.filter_queues.clone(),
            section_filter_runtime: self.section_filter_runtime.clone(),
            packet_pipeline: self.packet_pipeline.clone(),
            av_sync_states: self.av_sync_states.clone(),
            av_sync_hw_ids: self.av_sync_hw_ids.clone(),
            av_sync_filter_by_hw_id: self.av_sync_filter_by_hw_id.clone(),
            next_av_sync_hw_id: self.next_av_sync_hw_id,
        }
    }

    fn restore_txn_snapshot(&mut self, snapshot: DemuxTxnSnapshot) {
        self.filters = snapshot.filters;
        self.filter_queues = snapshot.filter_queues;
        self.section_filter_runtime = snapshot.section_filter_runtime;
        self.packet_pipeline = snapshot.packet_pipeline;
        self.av_sync_states = snapshot.av_sync_states;
        self.av_sync_hw_ids = snapshot.av_sync_hw_ids;
        self.av_sync_filter_by_hw_id = snapshot.av_sync_filter_by_hw_id;
        self.next_av_sync_hw_id = snapshot.next_av_sync_hw_id;
    }

    pub fn validate_filter_configure_result(
        &self,
        filter_id: i32,
        summary: &FilterConfig,
    ) -> Result<(), DemuxConfigError> {
        let Some(existing) = self.filters.get(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if existing.is_started_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        let open_type = existing.open_type;
        if !open_type.accepts_config_kind(&summary.kind) {
            return Err(DemuxConfigError::InvalidKind);
        }
        let kind = Self::filter_kind_capacity(open_type, &summary.kind);
        if kind == FilterCapacityKind::Other {
            return Err(DemuxConfigError::InvalidKind);
        }
        if !self.filter_capacity_available(filter_id, kind) {
            return Err(DemuxConfigError::CapacityExceeded);
        }
        if let FilterConfigKind::Section { condition, .. } = &summary.kind {
            if !condition.validates_section_filter_width() {
                return Err(DemuxConfigError::InvalidKind);
            }
        }
        Ok(())
    }

    pub fn configure_filter_with_summary_result(
        &mut self,
        filter_id: i32,
        summary: FilterConfig,
    ) -> Result<(), DemuxConfigError> {
        SoftDemuxConfigureTxn::new(self).configure_filter(filter_id, summary)
    }

    fn configure_filter_with_summary_result_impl(
        &mut self,
        filter_id: i32,
        summary: FilterConfig,
    ) -> Result<(), DemuxConfigError> {
        self.validate_filter_configure_result(filter_id, &summary)?;
        let snapshot = self.txn_snapshot();
        let Some(existing) = self.filters.get(&filter_id) else {
            self.restore_txn_snapshot(snapshot);
            return Err(DemuxConfigError::NotFound);
        };
        let open_type = existing.open_type;
        let is_av = matches!(&summary.kind, FilterConfigKind::Av { .. });
        let pid = summary.tpid;
        let allocated_av_sync_hw_id = if is_av && !self.av_sync_hw_ids.contains_key(&filter_id) {
            let hw_id = self.next_av_sync_hw_id;
            let next_hw_id = self
                .next_av_sync_hw_id
                .checked_add(1)
                .ok_or(DemuxConfigError::IdExhausted)?;
            Some((hw_id, next_hw_id))
        } else {
            None
        };
        let next_delivery_generation = next_filter_delivery_generation_record(existing)?;
        SoftDemuxOriginTxn::new(self).disconnect_downstreams(filter_id);
        {
            let Some(filter) = self.filters.get_mut(&filter_id) else {
                self.restore_txn_snapshot(snapshot);
                return Err(DemuxConfigError::NotFound);
            };
            filter.set_lifecycle(FilterLifecycleState::Configured);
            filter.config = Some(summary);
            // 再設定は以前の configureAvStreamType() hint だけを無効化する。
            // audio/video routing 種別は open subtype から導出し、hint 未設定でも維持する。
            filter.av_stream_type_hint = None;
            filter.av_stream_kind = None;
            // 再設定 は以前の 上流接続 を無効化する 状態破棄境界 である。
            // 下流フィルタ は condition / PID 変更後に明示的に再接続する必要がある。
            filter.data_upstream_filter_id = None;
            filter.queued_bytes = 0;
            filter.pending_overflow = false;
            filter.pending_start_event = false;
            filter.pending_start_id = 0;
            filter.delivery_not_before = None;
            filter.delivery_generation = next_delivery_generation;
        }
        if is_av {
            let state = self
                .av_sync_states
                .entry(filter_id)
                .or_insert_with(|| AvSyncState::new(pid));
            state.pid = pid;
            state.stream_type_hint = None;
            state.stream_kind = av_kind_for_filter_open_type(open_type);
            if let Some((hw_id, next_hw_id)) = allocated_av_sync_hw_id {
                self.next_av_sync_hw_id = next_hw_id;
                self.av_sync_hw_ids.insert(filter_id, hw_id);
                self.av_sync_filter_by_hw_id.insert(hw_id, filter_id);
            }
        } else {
            self.av_sync_states.remove(&filter_id);
            if let Some(hw_id) = self.av_sync_hw_ids.remove(&filter_id) {
                self.av_sync_filter_by_hw_id.remove(&hw_id);
            }
        }
        self.filter_queues.insert(filter_id, VecDeque::new());
        self.section_filter_runtime
            .insert(filter_id, SectionFilterRuntime::default());
        self.packet_pipeline.clear_filter_state(filter_id);
        Ok(())
    }

    #[cfg(test)]
    pub fn configure_filter_with_summary(&mut self, filter_id: i32, summary: FilterConfig) -> bool {
        self.configure_filter_with_summary_result(filter_id, summary)
            .is_ok()
    }

    pub fn configure_record_pid_filter(
        &mut self,
        filter_id: i32,
        pid: u16,
    ) -> Result<(), DemuxConfigError> {
        self.configure_filter_with_summary_result(
            filter_id,
            FilterConfig {
                tpid: pid as i32,
                main_type_bits: 1,
                sub_type_hint: 0,
                kind: FilterConfigKind::Record {
                    ts_index_mask: 0,
                    sc_index_type: 0,
                    sc_index_mask_bits: 0,
                },
            },
        )
    }

    pub fn configure_record_pid_set(
        &mut self,
        filter_ids: &[i32],
        pids: &BTreeSet<u16>,
    ) -> Result<(), DemuxConfigError> {
        SoftDemuxConfigureTxn::new(self).configure_record_pid_set(filter_ids, pids)
    }

    fn configure_record_pid_set_impl(
        &mut self,
        filter_ids: &[i32],
        pids: &BTreeSet<u16>,
    ) -> Result<(), DemuxConfigError> {
        if filter_ids.len() < pids.len() {
            return Err(DemuxConfigError::CapacityExceeded);
        }
        let target_ids: Vec<i32> = filter_ids.iter().copied().take(pids.len()).collect();
        let snapshot = self.txn_snapshot();
        for filter_id in &target_ids {
            if !self.filters.contains_key(filter_id) {
                self.restore_txn_snapshot(snapshot);
                return Err(DemuxConfigError::NotFound);
            }
        }
        for (filter_id, pid) in target_ids.iter().copied().zip(pids.iter().copied()) {
            if let Err(err) = self.configure_record_pid_filter(filter_id, pid) {
                self.restore_txn_snapshot(snapshot);
                return Err(err);
            }
        }
        Ok(())
    }
    pub fn filter_record(&self, filter_id: i32) -> Option<&DemuxFilterRecord> {
        self.filters.get(&filter_id)
    }

    pub fn filter_generation(&self, filter_id: i32) -> Option<u64> {
        self.filters
            .get(&filter_id)
            .map(|filter| filter.delivery_generation)
    }

    pub fn filter_source_snapshot(&self, filter_id: i32) -> Option<FilterSourceSnapshot> {
        self.filters.get(&filter_id).map(|filter| FilterSourceSnapshot {
            filter_id,
            configured: filter.is_configured_for_api(),
            started: filter.is_started_for_api(),
            generation: filter.delivery_generation,
            tpid: filter.config.as_ref().map(|config| config.tpid),
            open_type: filter.open_type,
        })
    }

    fn source_filter_origin_impl(&self, source_filter_id: i32) -> Option<TsInputOrigin> {
        self.filters.get(&source_filter_id).map(|filter| TsInputOrigin::SourceFilter {
            source_filter_id,
            source_filter_generation: filter.delivery_generation,
        })
    }

    fn source_filter_origins_feeding_filter(&self, filter_id: i32) -> Vec<TsInputOrigin> {
        self.filters
            .get(&filter_id)
            .and_then(|filter| filter.data_upstream_filter_id)
            .and_then(|source_filter_id| SoftDemuxOriginView::new(self).source_origin(source_filter_id))
            .into_iter()
            .collect()
    }

    fn active_input_origins_for_filter_impl(&self, filter_id: i32) -> Vec<TsInputOrigin> {
        if let Some(source_origin) = self.source_filter_origins_feeding_filter(filter_id).into_iter().next() {
            return vec![source_origin];
        }
        let mut origins = Vec::new();
        if self.frontend_id.is_some() {
            origins.push(TsInputOrigin::Frontend);
        }
        if self
            .dvrs
            .values()
            .any(|dvr| dvr.direction == DemuxPathDirection::Playback && dvr.is_started_for_api())
        {
            origins.push(TsInputOrigin::Playback);
        }
        origins
    }

    fn validate_pes_to_av_data_source(
        &self,
        source: &DemuxFilterRecord,
        destination: &DemuxFilterRecord,
        source_config: &FilterConfig,
        destination_config: &FilterConfig,
    ) -> Result<(), DemuxConfigError> {
        if !matches!(source.open_type, FilterOpenType::TsPes)
            || !matches!(destination.open_type, FilterOpenType::TsAudio | FilterOpenType::TsVideo)
        {
            return Ok(());
        }
        let FilterConfigKind::PesData { stream_id, raw } = &source_config.kind else {
            return Err(DemuxConfigError::InvalidKind);
        };
        let FilterConfigKind::Av { passthrough, .. } = &destination_config.kind else {
            return Err(DemuxConfigError::InvalidKind);
        };
        if *passthrough || *raw || *stream_id < 0 {
            return Err(DemuxConfigError::InvalidKind);
        }
        let Some(expected_kind) = av_kind_for_filter_open_type(destination.open_type) else {
            return Err(DemuxConfigError::InvalidKind);
        };
        if !pes_stream_id_matches_av_kind(*stream_id, expected_kind) {
            return Err(DemuxConfigError::InvalidKind);
        }
        Ok(())
    }

    fn validate_pes_to_pes_data_source(
        &self,
        source: &DemuxFilterRecord,
        destination: &DemuxFilterRecord,
        source_config: &FilterConfig,
        destination_config: &FilterConfig,
    ) -> Result<(), DemuxConfigError> {
        if !matches!(source.open_type, FilterOpenType::TsPes)
            || !matches!(destination.open_type, FilterOpenType::TsPes)
        {
            return Ok(());
        }
        let FilterConfigKind::PesData {
            stream_id: source_stream_id,
            raw: source_raw,
        } = &source_config.kind else {
            return Err(DemuxConfigError::InvalidKind);
        };
        let FilterConfigKind::PesData {
            stream_id: destination_stream_id,
            raw: destination_raw,
        } = &destination_config.kind else {
            return Err(DemuxConfigError::InvalidKind);
        };
        if source_raw != destination_raw {
            return Err(DemuxConfigError::InvalidKind);
        }
        if *source_stream_id != -1
            && *destination_stream_id != -1
            && *source_stream_id != *destination_stream_id
        {
            return Err(DemuxConfigError::InvalidKind);
        }
        Ok(())
    }

    pub fn validate_filter_data_source_sink_preconditions(
        &self,
        filter_id: i32,
    ) -> Result<(), DemuxConfigError> {
        let Some(destination) = self.filters.get(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if destination.is_closed_or_failed_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        if destination.is_started_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        Ok(())
    }

    pub fn validate_filter_data_source(
        &self,
        filter_id: i32,
        upstream_filter_id: i32,
    ) -> Result<(), DemuxConfigError> {
        let Some(destination) = self.filters.get(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if destination.is_closed_or_failed_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        if destination.is_started_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        let Some(source) = self.filters.get(&upstream_filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        // DESIGN_JA.md 表1-D-1の優先順に合わせる。sink lifecycleは上で判定済み。
        // source lifecycle は ownership/self-reference/種別互換より先に判定し、
        // 閉鎖済み・runtime failed source を INVALID_ARGUMENT に丸めない。
        if source.is_closed_or_failed_for_api() {
            record_soft_demux_diagnostic(&SET_DATA_SOURCE_INVALID_PAIR_COUNT, "set_data_source_source_lifecycle_invalid");
            return Err(DemuxConfigError::InvalidState);
        }
        if !source.is_configured_for_api() || !destination.is_configured_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        if filter_id == upstream_filter_id {
            record_soft_demux_diagnostic(&SET_DATA_SOURCE_INVALID_PAIR_COUNT, "set_data_source_self_reference");
            return Err(DemuxConfigError::InvalidKind);
        }
        if !can_link_filter_open_types(source.open_type, destination.open_type) {
            record_soft_demux_diagnostic(&SET_DATA_SOURCE_INVALID_PAIR_COUNT, "set_data_source_unavailable_pair");
            return Err(DemuxConfigError::Unavailable);
        }
        let (Some(source_config), Some(destination_config)) =
            (source.config.as_ref(), destination.config.as_ref())
        else {
            return Err(DemuxConfigError::InvalidState);
        };
        if source_config.tpid != destination_config.tpid {
            record_soft_demux_diagnostic(&SET_DATA_SOURCE_INVALID_PAIR_COUNT, "set_data_source_invalid_pair");
            return Err(DemuxConfigError::InvalidKind);
        }
        if let Err(err) = self.validate_pes_to_av_data_source(source, destination, source_config, destination_config) {
            if matches!(err, DemuxConfigError::InvalidKind) {
                record_soft_demux_diagnostic(&SET_DATA_SOURCE_INVALID_PAIR_COUNT, "set_data_source_invalid_pair");
            }
            return Err(err);
        }
        if let Err(err) = self.validate_pes_to_pes_data_source(source, destination, source_config, destination_config) {
            if matches!(err, DemuxConfigError::InvalidKind) {
                record_soft_demux_diagnostic(&SET_DATA_SOURCE_INVALID_PAIR_COUNT, "set_data_source_invalid_pair");
            }
            return Err(err);
        }

        let mut current = Some(upstream_filter_id);
        let mut visited = BTreeSet::new();
        while let Some(source_id) = current {
            if source_id == filter_id {
                record_soft_demux_diagnostic(&SET_DATA_SOURCE_INVALID_PAIR_COUNT, "set_data_source_invalid_pair");
                return Err(DemuxConfigError::InvalidKind);
            }
            if !visited.insert(source_id) {
                record_soft_demux_diagnostic(&SET_DATA_SOURCE_INVALID_PAIR_COUNT, "set_data_source_invalid_pair");
                return Err(DemuxConfigError::InvalidKind);
            }
            current = self
                .filters
                .get(&source_id)
                .and_then(|source| source.data_upstream_filter_id);
        }
        Ok(())
    }

    fn filter_config_pid(&self, filter_id: i32) -> Result<i32, DemuxConfigError> {
        self.filters
            .get(&filter_id)
            .and_then(|filter| filter.config.as_ref().map(|config| config.tpid))
            .ok_or(DemuxConfigError::NotFound)
    }

    fn source_origin_for_snapshot(
        &self,
        upstream_filter_id: Option<i32>,
    ) -> Result<Option<TsInputOrigin>, DemuxConfigError> {
        match upstream_filter_id {
            Some(source_id) => self
                .source_filter_origin_impl(source_id)
                .map(Some)
                .ok_or(DemuxConfigError::NotFound),
            None => Ok(None),
        }
    }

    fn apply_filter_data_source_transition(
        &mut self,
        filter_id: i32,
        upstream_filter_id: Option<i32>,
    ) -> Result<(), DemuxConfigError> {
        let previous_upstream = self.filter_data_source_snapshot(filter_id)?;
        let pid = self.filter_config_pid(filter_id)?;
        let previous_origin = self.source_origin_for_snapshot(previous_upstream)?;
        let next_origin = self.source_origin_for_snapshot(upstream_filter_id)?;
        let next_delivery_generation = {
            let Some(filter) = self.filters.get(&filter_id) else {
                return Err(DemuxConfigError::NotFound);
            };
            next_filter_delivery_generation_record(filter)?
        };

        if let Some(origin) = previous_origin {
            self.reset_source_origin_partial_state_impl(origin, filter_id, pid);
        }
        if let Some(origin) = next_origin {
            self.reset_source_origin_partial_state_impl(origin, filter_id, pid);
        }

        let Some(filter) = self.filters.get_mut(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        filter.data_upstream_filter_id = upstream_filter_id;
        filter.delivery_generation = next_delivery_generation;
        Ok(())
    }

    pub fn set_filter_data_source_result(
        &mut self,
        filter_id: i32,
        upstream_filter_id: i32,
    ) -> Result<(), DemuxConfigError> {
        SoftDemuxConfigureTxn::new(self).set_data_source(filter_id, upstream_filter_id)
    }

    fn set_filter_data_source_result_impl(
        &mut self,
        filter_id: i32,
        upstream_filter_id: i32,
    ) -> Result<(), DemuxConfigError> {
        self.validate_filter_data_source(filter_id, upstream_filter_id)?;
        let snapshot = self.txn_snapshot();
        if let Err(err) = self.apply_filter_data_source_transition(filter_id, Some(upstream_filter_id)) {
            self.restore_txn_snapshot(snapshot);
            return Err(err);
        }
        record_soft_demux_diagnostic(&SET_DATA_SOURCE_SUCCESS_COUNT, "set_data_source_success");
        Ok(())
    }

    pub fn filter_data_source_snapshot(
        &self,
        filter_id: i32,
    ) -> Result<Option<i32>, DemuxConfigError> {
        self.filters
            .get(&filter_id)
            .map(|record| record.data_upstream_filter_id)
            .ok_or(DemuxConfigError::NotFound)
    }

    pub fn restore_filter_data_source_snapshot(
        &mut self,
        filter_id: i32,
        upstream_filter_id: Option<i32>,
    ) -> Result<(), DemuxConfigError> {
        SoftDemuxConfigureTxn::new(self).restore_data_source(filter_id, upstream_filter_id)
    }

    fn restore_filter_data_source_snapshot_impl(
        &mut self,
        filter_id: i32,
        upstream_filter_id: Option<i32>,
    ) -> Result<(), DemuxConfigError> {
        let snapshot = self.txn_snapshot();
        if let Err(err) = self.apply_filter_data_source_transition(filter_id, upstream_filter_id) {
            self.restore_txn_snapshot(snapshot);
            return Err(err);
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn set_filter_data_source(&mut self, filter_id: i32, upstream_filter_id: i32) -> bool {
        self.set_filter_data_source_result(filter_id, upstream_filter_id)
            .is_ok()
    }

    pub fn set_filter_delay_hint(
        &mut self,
        filter_id: i32,
        delay_hint: FilterDelayHintState,
    ) -> bool {
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            if matches!(filter.open_type, FilterOpenType::TsRecord)
                && matches!(delay_hint, FilterDelayHintState::DataSizeDelayBytes(_))
            {
                return false;
            }
            let next_time_delay_ms = match delay_hint {
                FilterDelayHintState::TimeDelayMs(0) => None,
                FilterDelayHintState::TimeDelayMs(ms) => Some(ms),
                FilterDelayHintState::DataSizeDelayBytes(_) => filter.delay_hints.time_delay_ms,
            };
            let next_delivery_not_before = if filter.is_started_for_api() {
                if let Some(ms) = next_time_delay_ms.filter(|ms| *ms > 0) {
                    let Some(deadline) = Instant::now().checked_add(Duration::from_millis(ms)) else {
                        return false;
                    };
                    Some(deadline)
                } else {
                    None
                }
            } else {
                None
            };
            match delay_hint {
                FilterDelayHintState::TimeDelayMs(0) => filter.delay_hints.time_delay_ms = None,
                FilterDelayHintState::TimeDelayMs(ms) => {
                    filter.delay_hints.time_delay_ms = Some(ms)
                }
                FilterDelayHintState::DataSizeDelayBytes(0) => {
                    filter.delay_hints.data_size_delay_bytes = None
                }
                FilterDelayHintState::DataSizeDelayBytes(bytes) => {
                    filter.delay_hints.data_size_delay_bytes = Some(bytes)
                }
            }
            filter.delivery_not_before = next_delivery_not_before;
            return true;
        }
        false
    }

    pub fn set_filter_ip_cid(&mut self, filter_id: i32, ip_cid: i32) -> bool {
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.ip_cid = Some(ip_cid);
            return true;
        }
        false
    }

    pub fn set_filter_monitor_event_mask(
        &mut self,
        filter_id: i32,
        monitor_event_mask: i32,
    ) -> bool {
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.monitor_event_mask = monitor_event_mask;
            return true;
        }
        false
    }

    pub fn validate_filter_av_stream_type_hint_result(
        &self,
        filter_id: i32,
        _av_stream_type_hint: i32,
        av_stream_kind: AvFilterStreamKind,
    ) -> Result<(), DemuxConfigError> {
        let Some(existing) = self.filters.get(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if existing.is_started_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        let Some(cfg) = existing.config.as_ref() else {
            return Err(DemuxConfigError::InvalidState);
        };
        if !matches!(cfg.kind, FilterConfigKind::Av { .. }) {
            return Err(DemuxConfigError::InvalidKind);
        }
        if !matches!(
            (existing.open_type, av_stream_kind),
            (FilterOpenType::TsAudio, AvFilterStreamKind::Audio)
                | (FilterOpenType::TsVideo, AvFilterStreamKind::Video)
        ) {
            return Err(DemuxConfigError::InvalidKind);
        }
        let capacity_kind = match av_stream_kind {
            AvFilterStreamKind::Audio => FilterCapacityKind::Audio,
            AvFilterStreamKind::Video => FilterCapacityKind::Video,
        };
        if !self.filter_capacity_available(filter_id, capacity_kind) {
            return Err(DemuxConfigError::CapacityExceeded);
        }
        Ok(())
    }

    pub fn set_filter_av_stream_type_hint_result(
        &mut self,
        filter_id: i32,
        av_stream_type_hint: i32,
        av_stream_kind: AvFilterStreamKind,
    ) -> Result<(), DemuxConfigError> {
        self.validate_filter_av_stream_type_hint_result(
            filter_id,
            av_stream_type_hint,
            av_stream_kind,
        )?;
        let Some(filter) = self.filters.get_mut(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        filter.av_stream_type_hint = Some(av_stream_type_hint);
        filter.av_stream_kind = Some(av_stream_kind);
        if let Some(state) = self.av_sync_states.get_mut(&filter_id) {
            state.stream_type_hint = Some(av_stream_type_hint);
            state.stream_kind = Some(av_stream_kind);
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn set_filter_av_stream_type_hint(
        &mut self,
        filter_id: i32,
        av_stream_type_hint: i32,
        av_stream_kind: AvFilterStreamKind,
    ) -> bool {
        self.set_filter_av_stream_type_hint_result(filter_id, av_stream_type_hint, av_stream_kind)
            .is_ok()
    }
    pub fn start_filter_result(&mut self, filter_id: i32) -> Result<(), DemuxConfigError> {
        let Some(filter) = self.filters.get_mut(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if !filter.is_configured_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        // AV stream type hint は MediaEvent 解釈の補助情報であり、start() の必須ゲートにしない。
        // 二重 start は表SSOTどおり no-op 成功とし、queue / pending event / assembler を壊さない。
        if filter.is_started_for_api() {
            return Ok(());
        }
        let pending_start_id = if filter.ever_started {
            filter.delivery_generation.max(1) as i32
        } else {
            0
        };
        let delivery_not_before = if let Some(ms) = filter.delay_hints.time_delay_ms.filter(|ms| *ms > 0) {
            let Some(deadline) = Instant::now().checked_add(Duration::from_millis(ms)) else {
                return Err(DemuxConfigError::InvalidState);
            };
            Some(deadline)
        } else {
            None
        };
        filter.pending_start_id = pending_start_id;
        filter.ever_started = true;
        filter.set_lifecycle(FilterLifecycleState::Started);
        filter.delivery_not_before = delivery_not_before;
        self.section_filter_runtime
            .insert(filter_id, SectionFilterRuntime::default());
        Ok(())
    }

    #[cfg(test)]
    pub fn start_filter(&mut self, filter_id: i32) -> bool {
        self.start_filter_result(filter_id).is_ok()
    }

    pub fn set_filter_start_event_pending(&mut self, filter_id: i32, pending: bool) -> bool {
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.pending_start_event = pending;
            return true;
        }
        false
    }

    pub fn take_filter_start_event_id_if_ready(&mut self, filter_id: i32) -> Option<i32> {
        if self.filter_delivery_readiness(filter_id) != FilterDeliveryReadiness::Ready {
            return None;
        }
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            if filter.pending_start_event {
                filter.pending_start_event = false;
                return Some(filter.pending_start_id);
            }
        }
        None
    }

    pub fn take_filter_start_event_if_ready(&mut self, filter_id: i32) -> bool {
        self.take_filter_start_event_id_if_ready(filter_id).is_some()
    }

    pub fn filter_start_event_pending(&self, filter_id: i32) -> Option<bool> {
        self.filters
            .get(&filter_id)
            .map(|filter| filter.pending_start_event)
    }

    pub fn stop_filter_result(&mut self, filter_id: i32) -> Result<(), DemuxConfigError> {
        let Some(filter) = self.filters.get_mut(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if !filter.is_configured_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        if !filter.is_started_for_api() {
            record_soft_demux_diagnostic(&FILTER_STOP_IDEMPOTENT_COUNT, "stop_idempotent");
            return Ok(());
        }
        // 既出力 queue、pending event、assembler/runtime、linkage は flush()/configure()/close() の責務であり、ここでは破棄しない。
        filter.set_lifecycle(FilterLifecycleState::Stopped);
        filter.delivery_not_before = None;
        Ok(())
    }

    #[cfg(test)]
    pub fn stop_filter(&mut self, filter_id: i32) -> bool {
        self.stop_filter_result(filter_id).is_ok()
    }

    pub fn flush_filter_result(&mut self, filter_id: i32) -> Result<(), DemuxConfigError> {
        let old_source_origin = SoftDemuxOriginView::new(self).source_origin(filter_id);
        let downstreams = self.source_filter_downstream_snapshots(filter_id);
        let next_delivery_generation = {
            let Some(filter) = self.filters.get(&filter_id) else {
                return Err(DemuxConfigError::NotFound);
            };
            if !filter.is_configured_for_api() {
                return Err(DemuxConfigError::InvalidState);
            }
            next_filter_delivery_generation_record(filter)?
        };
        let Some(filter) = self.filters.get_mut(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        // linkage / configure 内容 / AV shared backing は維持する。
        filter.queued_bytes = 0;
        filter.pending_start_event = false;
        filter.pending_overflow = false;
        filter.delivery_not_before = None;
        filter.delivery_generation = next_delivery_generation;
        self.filter_queues.insert(filter_id, VecDeque::new());
        self.section_filter_runtime
            .insert(filter_id, SectionFilterRuntime::default());
        if let Some(origin) = old_source_origin {
            self.reset_source_filter_downstream_partial_state_impl(origin, &downstreams);
        }
        SoftDemuxOriginTxn::new(self).mark_flush_generation(filter_id);
        Ok(())
    }

    #[cfg(test)]
    pub fn flush_filter(&mut self, filter_id: i32) -> bool {
        self.flush_filter_result(filter_id).is_ok()
    }

    pub fn inject_payload(&mut self, filter_id: i32, payload: &[u8]) -> bool {
        let Some(filter) = self.filters.get(&filter_id).cloned() else {
            return false;
        };
        if !filter.is_started_for_api() {
            return false;
        }
        let accepted = match filter.config.as_ref() {
            Some(config) if matches!(&config.kind, FilterConfigKind::Section { .. }) => {
                self.filter_accepts_section(filter_id, config.tpid, payload)
            }
            _ => self.payload_matches_filter(&filter, payload),
        };
        if !accepted {
            return false;
        }
        let payload_entry = FilterPayload::Bytes(payload.to_vec());
        if self.push_filter_payload_for_delivery(filter_id, payload_entry.clone()) {
            self.propagate_filter_output(filter_id, &payload_entry);
            true
        } else {
            false
        }
    }

    pub fn push_ts_stream(&mut self, payload: &[u8]) -> usize {
        self.push_ts_stream_from_frontend(payload)
    }

    pub fn push_ts_stream_from_frontend(&mut self, payload: &[u8]) -> usize {
        let packets = self.packet_pipeline.split_ts_bytes(payload, PipelineInputKind::Live);
        let mut pushed = 0usize;
        for packet in packets {
            if self.push_ts_packet_with_origin(&packet, TsInputOrigin::Frontend).is_consumed() {
                pushed += 1;
            }
        }
        pushed
    }


    pub fn push_ts_packet(&mut self, packet: &[u8]) -> bool {
        self.push_ts_packet_with_origin(packet, TsInputOrigin::Frontend).is_consumed()
    }

    pub fn push_ts_packet_record_only(&mut self, packet: &[u8]) -> bool {
        let Some(parsed) = self.packet_pipeline.accept_ts_packet(packet, TsInputOrigin::Frontend) else {
            return false;
        };
        let filter_views = self.pipeline_filter_views();
        let report = self.packet_pipeline.plan_ts_packet_report(&parsed, TsInputOrigin::Frontend, &filter_views);
        let mut retained = false;
        for action in report.delivery_actions.into_iter() {
            match action {
                PipelineDeliveryAction::DvrMirror { dvr_id: filter_id } => {
                    let packet_entry = FilterPayload::RecordPacket(packet.to_vec());
                    if self.mirror_filter_payload_to_record_dvrs(filter_id, &packet_entry) {
                        retained = true;
                    }
                }
                PipelineDeliveryAction::RecordPacket { filter_id } => {
                    let packet_entry = FilterPayload::RecordPacket(packet.to_vec());
                    if self.push_filter_payload_for_delivery(filter_id, packet_entry) {
                        retained = true;
                    }
                }
                _ => {}
            }
        }
        retained
    }


    fn push_ts_packet_with_origin(&mut self, packet: &[u8], origin: TsInputOrigin) -> PacketPushOutcome {
        let parsed = match self.packet_pipeline.accept_ts_packet_with_outcome(packet, origin) {
            crate::packet_pipeline::PacketAcceptOutcome::Accepted(view) => view,
            crate::packet_pipeline::PacketAcceptOutcome::Malformed => return PacketPushOutcome::DroppedMalformed,
            crate::packet_pipeline::PacketAcceptOutcome::TransportError => return PacketPushOutcome::DroppedTransportError,
            crate::packet_pipeline::PacketAcceptOutcome::Duplicate => return PacketPushOutcome::DroppedDuplicate,
            crate::packet_pipeline::PacketAcceptOutcome::NoPayload => return PacketPushOutcome::DroppedNoPayload,
        };

        // r50dz39: 対象選択だけでなく section/PES 組立結果も PacketPipeline report を正とする。
        let filter_views = self.pipeline_filter_views();
        let pipeline_report = self.packet_pipeline.plan_and_assemble_ts_packet_report(&parsed, origin, &filter_views);
        let mut retained_payload = false;
        let mut linked_packet_sources = Vec::new();
        for action in pipeline_report.delivery_actions.iter().cloned() {
            match action {
                PipelineDeliveryAction::RawPacket { filter_id } => {
                    let packet_entry = FilterPayload::TsPacket(packet.to_vec());
                    if self.push_filter_payload_for_delivery(filter_id, packet_entry) {
                        retained_payload = true;
                        linked_packet_sources.push(filter_id);
                    }
                }
                PipelineDeliveryAction::RecordPacket { filter_id } => {
                    let record_entry = FilterPayload::RecordPacket(packet.to_vec());
                    if self.push_filter_payload_for_delivery(filter_id, record_entry) {
                        retained_payload = true;
                    }
                }
                PipelineDeliveryAction::DvrMirror { dvr_id: filter_id } => {
                    let record_entry = FilterPayload::RecordPacket(packet.to_vec());
                    if self.mirror_filter_payload_to_record_dvrs(filter_id, &record_entry) {
                        retained_payload = true;
                    }
                }
                PipelineDeliveryAction::SectionPayload { .. }
                | PipelineDeliveryAction::PesPayload { .. }
                | PipelineDeliveryAction::AvPayload { .. } => {}
            }
        }

        for filter_id in linked_packet_sources {
            self.route_ts_packet_to_downstreams(filter_id, packet, origin);
        }

        if let Some(pcr) = parsed.pcr_90khz {
            if let Some(extended_pcr) = self.pcr_extender.update_checked(pcr) {
                self.latest_pcr = Some(pcr);
                self.latest_pcr_instant = Some(Instant::now());
                self.latest_pcr_90khz = Some(extended_pcr);
            } else {
                self.latest_pcr = None;
                self.latest_pcr_instant = None;
                self.latest_pcr_90khz = None;
                self.pcr_extender.reset();
            }
        }

        for generated in pipeline_report.generated_events.into_iter() {
            match generated {
                PipelineGeneratedEvent::SectionPayloadReady { filter_id, pid, generation, bytes } => {
                    if !self.section_generation_allows_delivery(origin, filter_id, pid, generation) {
                        continue;
                    }
                    if !self.filter_accepts_section(filter_id, pid, &bytes) {
                        continue;
                    }
                    let section_entry = FilterPayload::Bytes(bytes.clone());
                    if self.push_filter_payload_for_delivery(filter_id, section_entry.clone()) {
                        retained_payload = true;
                        self.propagate_filter_output_with_origin_generation(
                            filter_id,
                            &section_entry,
                            origin,
                            Some(AssemblyGeneration::Section { pid, generation }),
                        );
                    }
                }
                PipelineGeneratedEvent::PesPacketReady { filter_id, pid, generation, packet } => {
                    if self.route_pes_packet_for_filter(origin, pid, filter_id, &packet, generation) {
                        retained_payload = true;
                    }
                }
                PipelineGeneratedEvent::DataReady { .. }
                | PipelineGeneratedEvent::Section { .. }
                | PipelineGeneratedEvent::Pes { .. }
                | PipelineGeneratedEvent::Record { .. } => {}
            }
        }
        if retained_payload {
            PacketPushOutcome::Consumed
        } else {
            PacketPushOutcome::DroppedNoDelivery
        }
    }

    pub fn inject_playback_payload_result(
        &mut self,
        dvr_id: i32,
        payload: &[u8],
    ) -> PlaybackInjectionOutcome {
        let Some(dvr) = self.dvrs.get(&dvr_id).cloned() else {
            return PlaybackInjectionOutcome::InvalidState;
        };
        if !dvr.is_started_for_api() || dvr.direction != DemuxPathDirection::Playback {
            return PlaybackInjectionOutcome::InvalidState;
        }
        if payload.len() % TS_PACKET_SIZE != 0 {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                add_diagnostic_counter(
                    &mut dvr.playback_malformed_bytes,
                    payload.len() as u64,
                    &mut dvr.diagnostic_counter_saturated,
                );
            }
            return PlaybackInjectionOutcome::Malformed;
        }

        let mut delivered_packets = 0usize;
        let mut no_delivery_packets = 0usize;
        let mut malformed = 0u64;
        let mut malformed_saturated = false;
        let mut saw_packet = false;
        for chunk in payload.chunks_exact(TS_PACKET_SIZE) {
            saw_packet = true;
            if chunk[0] != 0x47 {
                match malformed.checked_add(TS_PACKET_SIZE as u64) {
                    Some(next) => malformed = next,
                    None => {
                        malformed = u64::MAX;
                        malformed_saturated = true;
                    }
                }
                continue;
            }
            let mut packet = [0u8; TS_PACKET_SIZE];
            packet.copy_from_slice(chunk);
            match self.push_ts_packet_with_origin(&packet, TsInputOrigin::Playback) {
                PacketPushOutcome::Consumed => delivered_packets += 1,
                PacketPushOutcome::DroppedNoDelivery
                | PacketPushOutcome::DroppedDuplicate
                | PacketPushOutcome::DroppedNoPayload => no_delivery_packets += 1,
                PacketPushOutcome::DroppedMalformed
                | PacketPushOutcome::DroppedTransportError => {
                    match malformed.checked_add(TS_PACKET_SIZE as u64) {
                    Some(next) => malformed = next,
                    None => {
                        malformed = u64::MAX;
                        malformed_saturated = true;
                    }
                }
                }
            }
        }

        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            add_diagnostic_counter(
                &mut dvr.playback_malformed_bytes,
                malformed,
                &mut dvr.diagnostic_counter_saturated,
            );
            if malformed_saturated {
                dvr.diagnostic_counter_saturated = true;
            }
            add_diagnostic_counter(
                &mut dvr.playback_injected_packets,
                delivered_packets as u64,
                &mut dvr.diagnostic_counter_saturated,
            );
            add_diagnostic_counter(
                &mut dvr.playback_injected_bytes,
                (delivered_packets * TS_PACKET_SIZE) as u64,
                &mut dvr.diagnostic_counter_saturated,
            );
        }

        if delivered_packets > 0 {
            PlaybackInjectionOutcome::ConsumedWithDelivery
        } else if no_delivery_packets > 0 || (saw_packet && malformed == 0) {
            PlaybackInjectionOutcome::ConsumedNoDelivery
        } else if malformed > 0 {
            PlaybackInjectionOutcome::Malformed
        } else {
            PlaybackInjectionOutcome::ConsumedNoDelivery
        }
    }

    pub fn inject_playback_payload(&mut self, dvr_id: i32, payload: &[u8]) -> bool {
        self.inject_playback_payload_result(dvr_id, payload)
            .is_legacy_success()
    }

    pub fn pop_filter_payload_entry(&mut self, filter_id: i32) -> Option<FilterPayload> {
        let payload = self.filter_queues.get_mut(&filter_id)?.pop_front();
        if let Some(ref entry) = payload {
            if let Some(filter) = self.filters.get_mut(&filter_id) {
                filter.queued_bytes = sub_queue_accounting(
                    filter.queued_bytes,
                    entry.len(),
                    &mut filter.diagnostic_counter_saturated,
                );
            }
        }
        payload
    }

    pub fn pop_filter_payload(&mut self, filter_id: i32) -> Option<Vec<u8>> {
        self.pop_filter_payload_entry(filter_id)
            .map(FilterPayload::into_bytes)
    }

    pub fn next_dvr_id_candidate(&self, direction: DemuxPathDirection) -> Result<i32, DemuxConfigError> {
        if self.closed {
            return Err(DemuxConfigError::InvalidState);
        }
        if self.dvrs.values().any(|dvr| dvr.direction == direction) {
            return Err(DemuxConfigError::InvalidState);
        }
        self.next_dvr_id
            .checked_add(1)
            .ok_or(DemuxConfigError::IdExhausted)
            .map(|_| self.next_dvr_id)
    }

    pub fn register_dvr(
        &mut self,
        direction: DemuxPathDirection,
        buffer_size: i32,
    ) -> Result<DemuxDvrRecord, DemuxConfigError> {
        if self.closed {
            return Err(DemuxConfigError::InvalidState);
        }
        if self.dvrs.values().any(|dvr| dvr.direction == direction) {
            return Err(DemuxConfigError::InvalidState);
        }
        let dvr_id = self.next_dvr_id;
        self.next_dvr_id = self
            .next_dvr_id
            .checked_add(1)
            .ok_or(DemuxConfigError::IdExhausted)?;
        let record = DemuxDvrRecord::new(dvr_id, direction, buffer_size);
        self.dvrs.insert(dvr_id, record.clone());
        self.dvr_queues.insert(dvr_id, VecDeque::new());
        Ok(record)
    }

    pub fn dvr_ids(&self) -> Vec<i32> {
        self.dvrs.keys().copied().collect()
    }
    pub fn has_dvr(&self, dvr_id: i32) -> bool {
        self.dvrs.contains_key(&dvr_id)
    }
    pub fn dvr_record(&self, dvr_id: i32) -> Option<&DemuxDvrRecord> {
        self.dvrs.get(&dvr_id)
    }

    #[cfg(test)]
    fn dvr_record_mut_for_test(&mut self, dvr_id: i32) -> &mut DemuxDvrRecord {
        self.dvrs
            .get_mut(&dvr_id)
            .expect("test用DVR recordが存在する必要があります")
    }

    #[cfg(test)]
    fn filter_record_mut_for_test(&mut self, filter_id: i32) -> &mut DemuxFilterRecord {
        self.filters
            .get_mut(&filter_id)
            .expect("test用filter recordが存在する必要があります")
    }

    fn reset_dvr_runtime_for_configure(&mut self, dvr_id: i32) {
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.status_check_interval_hint_ms = DEFAULT_DVR_STATUS_CHECK_INTERVAL_MS;
            dvr.queued_bytes = 0;
            dvr.pending_overflow = false;
            dvr.overflow_events = 0;
            dvr.drop_bytes = 0;
            dvr.diagnostic_counter_saturated = false;
            dvr.section_drop_events = 0;
            dvr.stale_partial_discards = 0;
            dvr.playback_injected_packets = 0;
            dvr.playback_injected_bytes = 0;
            dvr.playback_malformed_bytes = 0;
        }
        self.dvr_queues.insert(dvr_id, VecDeque::new());
    }

    pub fn validate_dvr_configure_result(
        &self,
        dvr_id: i32,
        summary: &DvrConfig,
    ) -> Result<(), DemuxConfigError> {
        let Some(dvr) = self.dvrs.get(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if dvr.direction != summary.direction {
            return Err(DemuxConfigError::InvalidKind);
        }
        if dvr.is_started_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        Ok(())
    }

    pub fn configure_dvr_with_summary_result(
        &mut self,
        dvr_id: i32,
        summary: DvrConfig,
    ) -> Result<(), DemuxConfigError> {
        self.validate_dvr_configure_result(dvr_id, &summary)?;
        self.reset_dvr_runtime_for_configure(dvr_id);
        let Some(dvr) = self.dvrs.get_mut(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        dvr.set_lifecycle(DvrLifecycleState::Configured);
        dvr.config = Some(summary);
        Ok(())
    }

    pub fn attach_filter_to_dvr_result(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), DemuxConfigError> {
        let Some(dvr) = self.dvrs.get(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        let Some(filter) = self.filters.get(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if dvr.direction != DemuxPathDirection::Record {
            return Err(DemuxConfigError::InvalidState);
        }
        if !dvr.is_configured_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        if !filter.is_configured_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        if !matches!(
            filter.config.as_ref().map(|config| &config.kind),
            Some(FilterConfigKind::Record { .. })
        ) {
            return Err(DemuxConfigError::InvalidKind);
        }
        let Some(dvr) = self.dvrs.get_mut(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if !dvr.attached_filter_ids.contains(&filter_id) {
            dvr.attached_filter_ids.push(filter_id);
        }
        Ok(())
    }

    pub fn detach_filter_from_dvr_result(
        &mut self,
        dvr_id: i32,
        filter_id: i32,
    ) -> Result<(), DemuxConfigError> {
        let Some(dvr) = self.dvrs.get_mut(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if dvr.direction != DemuxPathDirection::Record {
            return Err(DemuxConfigError::InvalidState);
        }
        if !dvr.is_configured_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        if !dvr.attached_filter_ids.contains(&filter_id) {
            return Err(DemuxConfigError::InvalidState);
        }
        dvr.attached_filter_ids.retain(|id| *id != filter_id);
        Ok(())
    }

    fn record_dvr_has_attached_record_filter(&self, dvr: &DemuxDvrRecord) -> bool {
        dvr.attached_filter_ids.iter().any(|filter_id| {
            self.filters.get(filter_id).map_or(false, |filter| {
                filter.is_configured_for_api()
                    && filter.is_started_for_api()
                    && matches!(
                        filter.config.as_ref().map(|config| &config.kind),
                        Some(FilterConfigKind::Record { .. })
                    )
            })
        })
    }

    pub fn start_dvr_result(&mut self, dvr_id: i32) -> Result<(), DemuxConfigError> {
        let Some(dvr) = self.dvrs.get(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if !dvr.is_configured_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        if dvr.direction == DemuxPathDirection::Record
            && !self.record_dvr_has_attached_record_filter(dvr)
        {
            return Err(DemuxConfigError::InvalidState);
        }
        let Some(dvr) = self.dvrs.get_mut(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if dvr.is_started_for_api() {
            return Ok(());
        }
        dvr.set_lifecycle(DvrLifecycleState::Started);
        Ok(())
    }

    pub fn stop_dvr_result(&mut self, dvr_id: i32) -> Result<(), DemuxConfigError> {
        let Some(dvr) = self.dvrs.get_mut(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if !dvr.is_configured_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        if !dvr.is_started_for_api() {
            record_soft_demux_diagnostic(&DVR_STOP_IDEMPOTENT_COUNT, "dvr_stop_idempotent");
            return Ok(());
        }
        dvr.set_lifecycle(DvrLifecycleState::Stopped);
        Ok(())
    }

    pub fn flush_dvr_result(&mut self, dvr_id: i32) -> Result<(), DemuxConfigError> {
        let Some(direction) = self.dvrs.get(&dvr_id).map(|dvr| dvr.direction) else {
            return Err(DemuxConfigError::NotFound);
        };
        let Some(dvr) = self.dvrs.get_mut(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if !dvr.is_configured_for_api() {
            return Err(DemuxConfigError::InvalidState);
        }
        match direction {
            DemuxPathDirection::Record => {
                dvr.queued_bytes = 0;
                dvr.pending_overflow = false;
                dvr.overflow_events = 0;
                dvr.drop_bytes = 0;
                dvr.diagnostic_counter_saturated = false;
                dvr.section_drop_events = 0;
                dvr.stale_partial_discards = 0;
                self.dvr_queues.insert(dvr_id, VecDeque::new());
            }
            DemuxPathDirection::Playback => {
                dvr.playback_injected_packets = 0;
                dvr.playback_injected_bytes = 0;
                dvr.playback_malformed_bytes = 0;
            }
        }
        Ok(())
    }

    pub fn set_dvr_status_check_interval_hint(&mut self, dvr_id: i32, interval_ms: i64) -> bool {
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.status_check_interval_hint_ms = interval_ms;
            return true;
        }
        false
    }

    pub fn unregister_dvr(&mut self, dvr_id: i32) -> Option<DemuxDvrRecord> {
        self.dvr_queues.remove(&dvr_id);
        self.dvrs.remove(&dvr_id)
    }

    pub fn pop_dvr_payload(&mut self, dvr_id: i32) -> Option<Vec<u8>> {
        let payload = self.dvr_queues.get_mut(&dvr_id)?.pop_front();
        if let Some(ref bytes) = payload {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.queued_bytes = sub_queue_accounting(
                    dvr.queued_bytes,
                    bytes.len(),
                    &mut dvr.diagnostic_counter_saturated,
                );
            }
        }
        payload
    }

    pub fn current_fill_bytes(&self, dvr_id: i32) -> Option<usize> {
        self.dvrs.get(&dvr_id).map(|d| d.queued_bytes)
    }

    pub fn current_filter_fill_bytes(&self, filter_id: i32) -> Option<usize> {
        self.filters.get(&filter_id).map(|f| f.queued_bytes)
    }

    pub fn take_filter_pending_overflow(&mut self, filter_id: i32) -> bool {
        let Some(filter) = self.filters.get_mut(&filter_id) else {
            return false;
        };
        let pending = filter.pending_overflow;
        filter.pending_overflow = false;
        pending
    }

    pub fn take_dvr_pending_overflow(&mut self, dvr_id: i32) -> bool {
        let Some(dvr) = self.dvrs.get_mut(&dvr_id) else {
            return false;
        };
        let pending = dvr.pending_overflow;
        dvr.pending_overflow = false;
        pending
    }

    pub fn playback_diagnostics(&self, dvr_id: i32) -> Option<(u64, u64, u64)> {
        self.dvrs.get(&dvr_id).map(|d| {
            (
                d.playback_injected_packets,
                d.playback_injected_bytes,
                d.playback_malformed_bytes,
            )
        })
    }

    pub fn has_filter_payload_ready(&self, filter_id: i32) -> bool {
        let queued_bytes_ready = self
            .filters
            .get(&filter_id)
            .map_or(false, |filter| filter.queued_bytes > 0);
        let queued_entry_ready = self
            .filter_queues
            .get(&filter_id)
            .map_or(false, |queue| !queue.is_empty());
        queued_bytes_ready || queued_entry_ready
    }

    pub fn current_filter_queue_entries(&self, filter_id: i32) -> Option<usize> {
        self.filter_queues.get(&filter_id).map(|queue| queue.len())
    }

    pub fn dvr_threshold_state(
        &self,
        dvr_id: i32,
    ) -> Option<(usize, Option<usize>, Option<usize>, usize)> {
        let dvr = self.dvrs.get(&dvr_id)?;
        let cfg = dvr.config.as_ref();
        Some((
            dvr.queued_bytes,
            cfg.and_then(|c| usize::try_from(c.low_threshold).ok()),
            cfg.and_then(|c| usize::try_from(c.high_threshold).ok()),
            dvr.buffer_size.max(0) as usize,
        ))
    }

    pub fn filter_queue_model(&self, filter_id: i32) -> Option<FilterQueueModel> {
        let filter = self.filters.get(&filter_id)?;
        let discipline = match filter.config.as_ref().map(|cfg| &cfg.kind) {
            Some(FilterConfigKind::Section { .. }) => FilterQueueDiscipline::SectionReassembled,
            Some(FilterConfigKind::Av { .. }) => FilterQueueDiscipline::AvMediaEvent,
            Some(FilterConfigKind::Record { .. }) => FilterQueueDiscipline::RecordEventMetadata,
            _ => FilterQueueDiscipline::PacketPassthrough,
        };
        let policy = match discipline {
            FilterQueueDiscipline::RecordEventMetadata => {
                QueuePolicy::bounded_metadata_entries(zero_payload_entry_limit(filter.buffer_size))
            }
            _ if filter.open_type.is_media() => {
                QueuePolicy::bounded_drop_old(filter.buffer_size.max(0) as usize)
            }
            _ => QueuePolicy::bounded_drop_new(filter.buffer_size.max(0) as usize),
        };
        Some(FilterQueueModel {
            queue_kind: QueueKind::FilterOutput,
            discipline,
            policy,
        })
    }

    pub fn dvr_queue_model(&self, dvr_id: i32) -> Option<DvrQueueModel> {
        let dvr = self.dvrs.get(&dvr_id)?;
        Some(DvrQueueModel {
            queue_kind: match dvr.direction {
                DemuxPathDirection::Record => QueueKind::DvrRecord,
                DemuxPathDirection::Playback => QueueKind::DvrPlayback,
            },
            discipline: match dvr.direction {
                DemuxPathDirection::Record => DvrQueueDiscipline::PacketPassthrough,
                DemuxPathDirection::Playback => DvrQueueDiscipline::PlaybackReinject,
            },
            policy: match dvr.direction {
                DemuxPathDirection::Record => {
                    QueuePolicy::bounded_drop_new(dvr.buffer_size.max(0) as usize)
                }
                DemuxPathDirection::Playback => {
                    QueuePolicy::producer_backpressure(dvr.buffer_size.max(0) as usize)
                }
            },
        })
    }

    pub fn snapshot_filter_queue_bytes(&self, filter_id: i32) -> Option<Vec<u8>> {
        let queue = self.filter_queues.get(&filter_id)?;
        let mut out = Vec::new();
        for payload in queue {
            out.extend_from_slice(payload.bytes());
        }
        Some(out)
    }

    pub fn snapshot_dvr_queue_bytes(&self, dvr_id: i32) -> Option<Vec<u8>> {
        let queue = self.dvr_queues.get(&dvr_id)?;
        let mut out = Vec::new();
        for payload in queue {
            out.extend_from_slice(payload);
        }
        Some(out)
    }

    pub fn filter_delivery_readiness(&self, filter_id: i32) -> FilterDeliveryReadiness {
        let Some(filter) = self.filters.get(&filter_id) else {
            return FilterDeliveryReadiness::MissingFilter;
        };
        let has_time_delay = filter.delay_hints.time_delay_ms.unwrap_or(FILTER_DELAY_TIME_DISABLED_MS) > FILTER_DELAY_TIME_DISABLED_MS;
        let has_data_size_delay = filter.delay_hints.data_size_delay_bytes.unwrap_or(FILTER_DELAY_DATA_SIZE_DISABLED_BYTES) > FILTER_DELAY_DATA_SIZE_DISABLED_BYTES;
        if !has_time_delay && !has_data_size_delay {
            return FilterDeliveryReadiness::Ready;
        }

        let time_ready = has_time_delay
            && filter
                .delivery_not_before
                .map(|期限| Instant::now() >= 期限)
                .unwrap_or(true);
        let data_ready = has_data_size_delay
            && filter.queued_bytes >= filter.delay_hints.data_size_delay_bytes.unwrap_or(FILTER_DELAY_DATA_SIZE_DISABLED_BYTES);

        if has_time_delay && has_data_size_delay {
            if time_ready || data_ready {
                return FilterDeliveryReadiness::Ready;
            }
            return FilterDeliveryReadiness::WaitingForTime;
        }
        if has_time_delay {
            if time_ready {
                FilterDeliveryReadiness::Ready
            } else {
                FilterDeliveryReadiness::WaitingForTime
            }
        } else if data_ready {
            FilterDeliveryReadiness::Ready
        } else {
            FilterDeliveryReadiness::WaitingForDataSize
        }
    }

    pub fn drain_filter_payloads_for_delivery(&mut self, filter_id: i32) -> Vec<FilterPayload> {
        if self
            .filters
            .get(&filter_id)
            .map_or(true, |filter| !filter.is_started_for_api())
        {
            return Vec::new();
        }
        if self.filter_delivery_readiness(filter_id) != FilterDeliveryReadiness::Ready {
            return Vec::new();
        }
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.delivery_not_before = None;
        }
        self.drain_filter_payloads(filter_id)
    }

    pub fn drain_filter_payloads(&mut self, filter_id: i32) -> Vec<FilterPayload> {
        let mut out = Vec::new();
        while let Some(payload) = self.pop_filter_payload_entry(filter_id) {
            out.push(payload);
        }
        out
    }

    pub fn drain_dvr_payloads(&mut self, dvr_id: i32) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(payload) = self.pop_dvr_payload(dvr_id) {
            out.push(payload);
        }
        out
    }

    pub fn av_sync_hw_id_for(&self, filter_id: i32) -> Option<i32> {
        let filter = self.filters.get(&filter_id)?;
        let config = filter.config.as_ref()?;
        if !matches!(config.kind, FilterConfigKind::Av { .. }) {
            return None;
        }
        if !self.av_sync_states.contains_key(&filter_id) {
            return None;
        }
        // AOSP CTS は INVALID_AV_SYNC_ID を許容するが、有効 ID を返す場合は
        // getAvSyncTime(id) が有効な現在 timestamp を返すことを期待する。
        // software demux 実装では A/V sync clock は PCR から初期化されるため、
        // PCR 由来の元クロックが存在するまで同期 ID を公開しない。
        if self.latest_pcr_90khz.is_none() || self.latest_pcr_instant.is_none() {
            return None;
        }
        self.av_sync_hw_ids.get(&filter_id).copied()
    }

    pub fn source_time_now(&self) -> Option<i64> {
        let base = self.latest_pcr_90khz?;
        let instant = self.latest_pcr_instant?;
        let elapsed_ns = instant.elapsed().as_nanos();
        let elapsed_90khz_u128 = elapsed_ns.checked_mul(90_000)? / 1_000_000_000;
        let elapsed_90khz = i64::try_from(elapsed_90khz_u128).ok()?;
        base.checked_add(elapsed_90khz)
    }

    pub fn av_sync_time_now(&self, av_sync_hw_id: i32) -> Option<i64> {
        if av_sync_hw_id <= 0 {
            return None;
        }
        let filter_id = *self.av_sync_filter_by_hw_id.get(&av_sync_hw_id)?;
        if self.av_sync_hw_id_for(filter_id)? != av_sync_hw_id {
            return None;
        }
        self.source_time_now()
    }

    pub fn close(&mut self) {
        self.packet_pipeline_drop_all_pes();
        self.closed = true;
        self.av_sync_states.clear();
        self.av_sync_hw_ids.clear();
        self.av_sync_filter_by_hw_id.clear();
        self.filters.clear();
        self.filter_queues.clear();
        self.section_filter_runtime.clear();
        self.dvrs.clear();
        self.dvr_queues.clear();
        // r50dz60/G2-19: a lifecycle boundary must drain the completion buffer exactly once;
        // reset_boundary() accounts for completed residual packets and malformed tail bytes.
        self.packet_pipeline.reset_boundary();
        self.latest_pcr = None;
        self.latest_pcr_instant = None;
        self.pcr_extender.reset();
        self.latest_pcr_90khz = None;
        self.frontend_id = None;
        self.ci_cam_id = None;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn snapshot(&self) -> DemuxSnapshot {
        DemuxSnapshot {
            demux_id: self.demux_id,
            frontend_id: self.frontend_id,
            ci_cam_id: self.ci_cam_id,
            filter_ids: self.filter_ids(),
            dvr_ids: self.dvr_ids(),
            closed: self.closed,
        }
    }

    fn route_pes_packet_for_filter(
        &mut self,
        origin: TsInputOrigin,
        pid: i32,
        filter_id: i32,
        pes: &PesPacket,
        generation: u64,
    ) -> bool {
        let Some(filter) = self.filters.get(&filter_id).cloned() else {
            return false;
        };
        if !filter.is_started_for_api()
            || !self.filter_accepts_pes(&filter, pid, pes)
            || !self.pes_generation_allows_delivery(origin, filter_id, pid, generation)
        {
            return false;
        }
        let kind = filter.config.as_ref().map(|c| c.kind.clone());
        let metadata = AvPayloadMetadata {
            pts_90khz: pes.pts_90khz,
            dts_90khz: pes.dts_90khz,
            stream_id: pes.stream_id as i32,
        };
        let payload = match kind {
            Some(FilterConfigKind::PesData { raw: true, .. }) => FilterPayload::PesData {
                bytes: pes.raw_bytes.clone(),
                stream_id: pes.stream_id as i32,
                raw: true,
                metadata: metadata.clone(),
            },
            Some(FilterConfigKind::PesData { raw: false, .. }) => FilterPayload::PesData {
                bytes: pes.payload.clone(),
                stream_id: pes.stream_id as i32,
                raw: false,
                metadata: metadata.clone(),
            },
            Some(FilterConfigKind::Av {
                passthrough: false, ..
            }) => FilterPayload::AvEs {
                bytes: pes.payload.clone(),
                metadata,
            },
            Some(FilterConfigKind::Av { .. }) => FilterPayload::Bytes(pes.raw_bytes.clone()),
            _ => FilterPayload::Bytes(pes.raw_bytes.clone()),
        };
        if self.push_filter_payload_for_delivery(filter_id, payload.clone()) {
            self.propagate_filter_output_with_origin_generation(
                filter_id,
                &payload,
                origin,
                Some(AssemblyGeneration::Pes { pid, generation }),
            );
            true
        } else {
            false
        }
    }

    fn record_filter_wants_index_events(&self, filter_id: i32) -> bool {
        self.filters
            .get(&filter_id)
            .and_then(|filter| filter.config.as_ref())
            .map_or(false, |config| match &config.kind {
                FilterConfigKind::Record {
                    ts_index_mask,
                    sc_index_mask_bits,
                    ..
                } => *ts_index_mask != 0 || *sc_index_mask_bits != 0,
                _ => false,
            })
    }

    fn mirror_filter_payload_to_record_dvrs(&mut self, filter_id: i32, payload: &FilterPayload) -> bool {
        let bytes = match payload {
            FilterPayload::TsPacket(bytes) | FilterPayload::RecordPacket(bytes) => bytes,
            _ => return false,
        };
        if bytes.len() != maleicacid_tuner_hal_common::TS_PACKET_SIZE {
            return false;
        }
        let attached: Vec<i32> = self
            .dvrs
            .iter()
            .filter(|(_, dvr)| {
                dvr.is_started_for_api()
                    && dvr.direction == DemuxPathDirection::Record
                    && dvr.attached_filter_ids.contains(&filter_id)
            })
            .map(|(id, _)| *id)
            .collect();
        let mut retained = false;
        for dvr_id in attached {
            let outcome = self.push_dvr_payload(dvr_id, bytes);
            if Self::payload_retained_after_push(&outcome) {
                retained = true;
            }
        }
        retained
    }

    fn propagate_filter_output(&mut self, upstream_filter_id: i32, payload: &FilterPayload) {
        self.propagate_filter_output_with_origin(
            upstream_filter_id,
            payload,
            TsInputOrigin::Frontend,
        );
    }

    fn propagate_filter_output_with_origin(
        &mut self,
        upstream_filter_id: i32,
        payload: &FilterPayload,
        origin: TsInputOrigin,
    ) {
        self.propagate_filter_output_with_origin_generation(
            upstream_filter_id,
            payload,
            origin,
            None,
        );
    }

    fn propagate_filter_output_with_origin_generation(
        &mut self,
        upstream_filter_id: i32,
        payload: &FilterPayload,
        origin: TsInputOrigin,
        _generation: Option<AssemblyGeneration>,
    ) {
        // DESIGN_JA.md: SourceFilter 経由の再投入対象は raw TS packet だけに固定する。
        // section/PES/AV payload の直接多段再配送は行わない。
        let source_is_ts_packet_source = self
            .filters
            .get(&upstream_filter_id)
            .map(|filter| matches!(filter.open_type, FilterOpenType::TsRaw))
            .unwrap_or(false);
        if source_is_ts_packet_source {
            if let FilterPayload::TsPacket(packet) = payload {
                self.route_ts_packet_to_downstreams(upstream_filter_id, packet, origin);
            }
        }
    }

    fn route_ts_packet_to_downstreams(
        &mut self,
        upstream_filter_id: i32,
        packet: &[u8],
        _origin: TsInputOrigin,
    ) {
        let Some(packet_origin) = SoftDemuxOriginView::new(self).source_origin(upstream_filter_id) else {
            return;
        };
        let downstreams: Vec<i32> = self
            .filters
            .iter()
            .filter(|(_, filter)| {
                filter.is_started_for_api() && filter.data_upstream_filter_id == Some(upstream_filter_id)
            })
            .map(|(id, _)| *id)
            .collect();
        if downstreams.is_empty() {
            // DESIGN_JA.md WP-02: downstream 未接続時は source origin の continuity / assembler を進めない。
            return;
        }

        let downstream_views: Vec<PipelineFilterView> = downstreams
            .iter()
            .filter_map(|downstream_id| {
                let filter = self.filters.get(downstream_id)?;
                let config = filter.config.as_ref()?;
                Some(PipelineFilterView {
                    filter_id: *downstream_id,
                    tpid: Some(config.tpid),
                    started: filter.is_started_for_api(),
                    // source filter 経由で渡されたpacketを、ここではdownstream集合の入口として扱う。
                    has_upstream: false,
                    open_kind: pipeline_open_kind(filter.open_type),
                    section_raw: matches!(filter.config.as_ref().map(|config| &config.kind), Some(FilterConfigKind::Section { raw: true, .. })),
                    pes_raw: matches!(filter.config.as_ref().map(|config| &config.kind), Some(FilterConfigKind::PesData { raw: true, .. })),
                    wants_record_index: self.record_filter_wants_index_events(*downstream_id),
                })
            })
            .collect();
        if downstream_views.is_empty() {
            // 設定不完全な downstream だけの場合も source origin 状態を汚染しない。
            return;
        }

        let Some(parsed) = self.packet_pipeline.accept_ts_packet(packet, packet_origin) else {
            return;
        };
        let pipeline_report = self.packet_pipeline.plan_and_assemble_ts_packet_report(&parsed, packet_origin, &downstream_views);

        for action in pipeline_report.delivery_actions.iter().cloned() {
            match action {
                PipelineDeliveryAction::RawPacket { filter_id: downstream_id } => {
                    let packet_entry = FilterPayload::TsPacket(packet.to_vec());
                    if self.push_filter_payload_for_delivery(downstream_id, packet_entry) {
                        if let Some(origin) = SoftDemuxOriginView::new(self).source_origin(downstream_id) {
                            self.route_ts_packet_to_downstreams(downstream_id, packet, origin);
                        }
                    }
                }
                PipelineDeliveryAction::RecordPacket { filter_id: downstream_id } => {
                    let record_entry = FilterPayload::RecordPacket(packet.to_vec());
                    if !self.push_filter_payload_for_delivery(downstream_id, record_entry) {
                        record_soft_demux_diagnostic(&SOURCE_FILTER_DOWNSTREAM_DROP_COUNT, "source_filter_downstream_record_drop");
                    }
                }
                PipelineDeliveryAction::DvrMirror { dvr_id: downstream_id } => {
                    let record_entry = FilterPayload::RecordPacket(packet.to_vec());
                    if !self.mirror_filter_payload_to_record_dvrs(downstream_id, &record_entry) {
                        record_soft_demux_diagnostic(&SOURCE_FILTER_DOWNSTREAM_DROP_COUNT, "source_filter_downstream_dvr_mirror_drop");
                    }
                }
                PipelineDeliveryAction::SectionPayload { .. }
                | PipelineDeliveryAction::PesPayload { .. }
                | PipelineDeliveryAction::AvPayload { .. } => {}
            }
        }

        for generated in pipeline_report.generated_events.into_iter() {
            match generated {
                PipelineGeneratedEvent::SectionPayloadReady { .. }
                | PipelineGeneratedEvent::PesPacketReady { .. } => {
                    // WP-02: source filter linkage では raw TS packet の再投入だけを正式対応とし、
                    // source origin 由来の section/PES payload を直接多段配送しない。
                }
                PipelineGeneratedEvent::DataReady { .. }
                | PipelineGeneratedEvent::Section { .. }
                | PipelineGeneratedEvent::Pes { .. }
                | PipelineGeneratedEvent::Record { .. } => {}
            }
        }
    }

    #[cfg(test)]
    fn payload_entry_matches_filter(&self, filter: &DemuxFilterRecord, payload: &FilterPayload) -> bool {
        let Some(config) = filter.config.as_ref() else {
            return false;
        };
        match (&config.kind, payload) {
            (FilterConfigKind::Noinit, FilterPayload::TsPacket(bytes)) => bytes.len() == TS_PACKET_SIZE,
            (FilterConfigKind::Record { .. }, FilterPayload::TsPacket(bytes))
            | (FilterConfigKind::Record { .. }, FilterPayload::RecordPacket(bytes)) => bytes.len() == TS_PACKET_SIZE,
            (FilterConfigKind::Section { .. }, FilterPayload::Bytes(_)) => false,
            (
                FilterConfigKind::PesData { stream_id, raw },
                FilterPayload::PesData {
                    stream_id: payload_stream_id,
                    raw: payload_raw,
                    ..
                },
            ) => *raw == *payload_raw && (*stream_id == -1 || *stream_id == *payload_stream_id),
            (FilterConfigKind::Av { .. }, FilterPayload::AvEs { metadata, .. }) => {
                matches!(filter.effective_av_stream_kind(), Some(kind) if pes_stream_id_matches_av_kind(metadata.stream_id, kind))
            }
            (
                FilterConfigKind::Av { .. },
                FilterPayload::PesData {
                    stream_id,
                    raw,
                    metadata,
                    ..
                },
            ) => {
                !*raw
                    && *stream_id >= 0
                    && matches!(
                        filter.effective_av_stream_kind(),
                        Some(kind)
                            if metadata.stream_id == *stream_id
                                && pes_stream_id_matches_av_kind(*stream_id, kind)
                    )
            }
            _ => false,
        }
    }

    fn payload_matches_filter(&self, filter: &DemuxFilterRecord, payload: &[u8]) -> bool {
        filter.config.as_ref().map_or(false, |config| {
            Self::section_payload_matches_config(config, payload)
        })
    }

    fn section_payload_matches_config(config: &FilterConfig, payload: &[u8]) -> bool {
        let FilterConfigKind::Section {
            condition,
            check_crc,
            length_field_bits,
            ..
        } = &config.kind
        else {
            return false;
        };
        if *check_crc && !section_crc_valid(payload, *length_field_bits) {
            return false;
        }
        condition.matches(payload, *length_field_bits)
    }

    fn pid_has_started_section_filter(&self, pid: i32) -> bool {
        self.filters.values().any(|filter| {
            filter.data_upstream_filter_id.is_none()
                && filter.is_started_for_api()
                && filter.config.as_ref().map_or(false, |config| {
                    config.tpid == pid && matches!(&config.kind, FilterConfigKind::Section { .. })
                })
        })
    }

    fn pid_has_started_pes_or_av_filter(&self, pid: i32) -> bool {
        self.filters.values().any(|filter| {
            filter.data_upstream_filter_id.is_none()
                && filter.is_started_for_api()
                && filter.config.as_ref().map_or(false, |config| {
                    config.tpid == pid
                        && matches!(
                            &config.kind,
                            FilterConfigKind::PesData { .. } | FilterConfigKind::Av { .. }
                        )
                })
        })
    }

    fn prune_assemblers_for_pid(&mut self, pid: i32) {
        if !self.pid_has_started_section_filter(pid) {
            let section_filter_ids_for_pid: Vec<i32> = self
                .filters
                .iter()
                .filter_map(|(filter_id, filter)| {
                    let matches_pid = filter.config.as_ref().map_or(false, |config| {
                        config.tpid == pid
                            && matches!(config.kind, FilterConfigKind::Section { .. })
                    });
                    matches_pid.then_some(*filter_id)
                })
                .collect();
            self.packet_pipeline.remove_section_for_filter_ids_all_origins(&section_filter_ids_for_pid);
        }
        if !self.pid_has_started_pes_or_av_filter(pid) {
            let pes_filter_ids_for_pid: Vec<i32> = self
                .filters
                .iter()
                .filter_map(|(filter_id, filter)| {
                    let matches_pid = filter.config.as_ref().map_or(false, |config| {
                        config.tpid == pid
                            && matches!(
                                config.kind,
                                FilterConfigKind::PesData { .. } | FilterConfigKind::Av { .. }
                            )
                    });
                    matches_pid.then_some(*filter_id)
                })
                .collect();
            self.packet_pipeline.remove_pes_for_filter_ids_all_origins(&pes_filter_ids_for_pid);
        }
    }

    fn section_generation_allows_delivery(
        &self,
        origin: TsInputOrigin,
        filter_id: i32,
        pid: i32,
        generation: u64,
    ) -> bool {
        self.packet_pipeline.section_generation_allows_delivery(origin, filter_id, pid, generation)
    }

    fn pes_generation_allows_delivery(
        &self,
        origin: TsInputOrigin,
        filter_id: i32,
        pid: i32,
        generation: u64,
    ) -> bool {
        self.packet_pipeline.pes_generation_allows_delivery(origin, filter_id, pid, generation)
    }

    fn mark_filter_flush_generation_impl(&mut self, filter_id: i32) {
        let Some(pid) = self
            .filters
            .get(&filter_id)
            .and_then(|filter| filter.config.as_ref().map(|config| config.tpid))
        else {
            return;
        };
        let origins: Vec<(TsInputOrigin, i32)> = self
            .active_input_origins_for_filter_impl(filter_id)
            .into_iter()
            .map(|origin| (origin, pid))
            .collect();
        self.packet_pipeline.flush_filter(filter_id, &origins);
    }


    fn filter_accepts_pes(&self, filter: &DemuxFilterRecord, pid: i32, pes: &PesPacket) -> bool {
        let Some(config) = filter.config.as_ref() else {
            return false;
        };
        if config.tpid != pid {
            return false;
        }
        match &config.kind {
            FilterConfigKind::PesData { stream_id, .. } => {
                *stream_id == -1 || *stream_id == pes.stream_id as i32
            }
            FilterConfigKind::Av { .. } => matches!(
                filter.effective_av_stream_kind(),
                Some(kind) if pes_stream_id_matches_av_kind(pes.stream_id as i32, kind)
            ),
            _ => false,
        }
    }

    fn filter_accepts_section(&mut self, filter_id: i32, pid: i32, payload: &[u8]) -> bool {
        let Some(filter) = self.filters.get(&filter_id) else {
            return false;
        };
        let Some(config) = filter.config.as_ref() else {
            return false;
        };
        if config.tpid != pid {
            return false;
        }
        let FilterConfigKind::Section {
            check_crc,
            repeat,
            raw,
            length_field_bits,
            condition_kind,
            condition,
        } = &config.kind
        else {
            return false;
        };
        let Some(header) = parse_section_header(payload, *length_field_bits) else {
            return false;
        };
        if header.total_length > MAX_SECTION_PAYLOAD_BYTES || payload.len() < header.total_length {
            return false;
        }
        let payload = &payload[..header.total_length];
        if *raw {
            // r50dz53/G1-18: raw section は table/version/CRC condition を配送条件にしない。
            // ただし section_length で確定した完全 section だけを受理する。
            return true;
        }
        if *check_crc && !section_crc_valid(payload, *length_field_bits) {
            return false;
        }
        if !condition.matches(payload, *length_field_bits) {
            return false;
        }
        if *repeat {
            return true;
        }
        let table_extension = header.table_id_extension.unwrap_or(SECTION_TABLE_EXTENSION_ABSENT);
        let version = header.version.unwrap_or(SECTION_VERSION_ABSENT);
        let section_number = header.section_number.unwrap_or(SECTION_NUMBER_ABSENT);
        let last_section_number = header.last_section_number.unwrap_or(section_number);
        let section_key = (header.table_id, table_extension, version, section_number);
        let table_key = (header.table_id, table_extension, version);
        let runtime = self.section_filter_runtime.entry(filter_id).or_default();
        if runtime.completed {
            return false;
        }
        match condition_kind {
            SectionConditionKind::SectionBits => {
                runtime.completed = true;
                true
            }
            SectionConditionKind::TableInfo => {
                match runtime.active_table_key {
                    Some(active) if active != table_key => return false,
                    Some(_) => {}
                    None => runtime.active_table_key = Some(table_key),
                }
                if !runtime.seen_section_keys.insert(section_key) {
                    return false;
                }
                let progress = runtime.table_progress.entry(table_key).or_default();
                progress.observe(section_number, last_section_number);
                if progress.is_complete() {
                    runtime.completed = true;
                }
                true
            }
        }
    }

    fn packet_pipeline_drop_all_pes(&mut self) {
        self.packet_pipeline.drop_all_pes();
    }


    fn payload_retained_after_push(outcome: &QueuePushOutcome) -> bool {
        outcome.accepted_entries > 0 && !outcome.dropped_new
    }

    fn push_filter_payload_for_delivery(&mut self, filter_id: i32, payload: FilterPayload) -> bool {
        let outcome = self.push_filter_payload(filter_id, payload);
        if Self::payload_retained_after_push(&outcome) {
            if let Some(filter_mut) = self.filters.get_mut(&filter_id) {
                filter_mut.events_emitted += 1;
            }
            true
        } else {
            false
        }
    }


    fn push_filter_payload(&mut self, filter_id: i32, payload: FilterPayload) -> QueuePushOutcome {
        let payload_len = payload.len();
        let Some(filter) = self.filters.get(&filter_id) else {
            return QueuePushOutcome::default();
        };
        let max_bytes = filter.buffer_size.max(0) as usize;
        let drop_old_policy = self
            .filters
            .get(&filter_id)
            .map(|f| f.open_type.is_media())
            .unwrap_or(false);
        let mut outcome = QueuePushOutcome::default();
        if drop_old_policy && max_bytes > 0 && payload_len > max_bytes {
            if let Some(filter) = self.filters.get_mut(&filter_id) {
                filter.pending_overflow = true;
                increment_diagnostic_counter(&mut filter.overflow_events, &mut filter.diagnostic_counter_saturated);
                add_diagnostic_counter(&mut filter.drop_bytes, payload_len as u64, &mut filter.diagnostic_counter_saturated);
            }
            outcome.dropped_bytes = payload_len;
            outcome.dropped_entries = 1;
            outcome.dropped_new = true;
            outcome.overflowed = true;
            return outcome;
        }
        if !drop_old_policy {
            let queued = filter.queued_bytes;
            let queued_entries = self
                .filter_queues
                .get(&filter_id)
                .map(|queue| queue.len())
                .unwrap_or_default();
            let is_record_metadata_event = matches!(payload, FilterPayload::RecordPacket(_));
            let max_zero_payload_entries = zero_payload_entry_limit(max_bytes as i32);
            if is_record_metadata_event && queued_entries >= max_zero_payload_entries {
                if let Some(filter) = self.filters.get_mut(&filter_id) {
                    filter.pending_overflow = true;
                    increment_diagnostic_counter(&mut filter.overflow_events, &mut filter.diagnostic_counter_saturated);
                }
                outcome.dropped_entries = 1;
                outcome.dropped_new = true;
                outcome.overflowed = true;
                return outcome;
            }
            let queued_overflowed = queued.checked_add(payload_len).is_none();
            if max_bytes > 0 && queued.checked_add(payload_len).map_or(true, |next| next > max_bytes) {
                if let Some(filter) = self.filters.get_mut(&filter_id) {
                    if queued_overflowed {
                        filter.diagnostic_counter_saturated = true;
                        outcome.counter_saturated = true;
                    }
                    filter.pending_overflow = true;
                    increment_diagnostic_counter(&mut filter.overflow_events, &mut filter.diagnostic_counter_saturated);
                    add_diagnostic_counter(&mut filter.drop_bytes, payload_len as u64, &mut filter.diagnostic_counter_saturated);
                }
                outcome.dropped_bytes = payload_len;
                outcome.dropped_entries = 1;
                outcome.dropped_new = true;
                outcome.overflowed = true;
                return outcome;
            }
        }
        let queue_was_empty = self
            .filter_queues
            .get(&filter_id)
            .map(|queue| queue.is_empty())
            .unwrap_or(true);
        let Some(next_queued) = self
            .filters
            .get(&filter_id)
            .and_then(|filter| {
                let mut saturated = false;
                let next = add_queue_accounting(filter.queued_bytes, payload_len, &mut saturated);
                if saturated {
                    // Queue accounting overflow is an input overflow, not a value to clamp and reuse.
                    None
                } else {
                    next
                }
            })
        else {
            if let Some(filter) = self.filters.get_mut(&filter_id) {
                filter.pending_overflow = true;
                filter.diagnostic_counter_saturated = true;
                increment_diagnostic_counter(&mut filter.overflow_events, &mut filter.diagnostic_counter_saturated);
                add_diagnostic_counter(&mut filter.drop_bytes, payload_len as u64, &mut filter.diagnostic_counter_saturated);
            }
            outcome.dropped_bytes = payload_len;
            outcome.dropped_entries = 1;
            outcome.dropped_new = true;
            outcome.overflowed = true;
            outcome.counter_saturated = true;
            return outcome;
        };
        let queue = self.filter_queues.entry(filter_id).or_default();
        queue.push_back(payload);
        outcome.accepted_entries = 1;
        outcome.accepted_bytes = payload_len;
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            if queue_was_empty && filter.is_started_for_api() {
                filter.delivery_not_before = filter
                    .delay_hints
                    .time_delay_ms
                    .filter(|ms| *ms > 0)
                    .map(|ms| Instant::now() + Duration::from_millis(ms));
            }
            filter.queued_bytes = next_queued;
            if max_bytes > 0 && drop_old_policy {
                while filter.queued_bytes > max_bytes {
                    if let Some(removed) = queue.pop_front() {
                        let removed_len = removed.len();
                        filter.queued_bytes = sub_queue_accounting(
                            filter.queued_bytes,
                            removed_len,
                            &mut filter.diagnostic_counter_saturated,
                        );
                        add_queue_outcome_counter(
                            &mut outcome.dropped_bytes,
                            removed_len,
                            &mut outcome.counter_saturated,
                        );
                        add_queue_outcome_counter(
                            &mut outcome.dropped_entries,
                            1,
                            &mut outcome.counter_saturated,
                        );
                        outcome.dropped_old = true;
                    } else {
                        filter.diagnostic_counter_saturated = true;
                        break;
                    }
                }
            }
            if outcome.counter_saturated {
                filter.diagnostic_counter_saturated = true;
            }
            if outcome.dropped_entries > 0 {
                outcome.overflowed = true;
                filter.pending_overflow = true;
                increment_diagnostic_counter(&mut filter.overflow_events, &mut filter.diagnostic_counter_saturated);
                add_diagnostic_counter(&mut filter.drop_bytes, outcome.dropped_bytes as u64, &mut filter.diagnostic_counter_saturated);
            }
        }
        outcome
    }

    fn push_dvr_payload(&mut self, dvr_id: i32, payload: &[u8]) -> QueuePushOutcome {
        let Some(dvr) = self.dvrs.get(&dvr_id) else {
            return QueuePushOutcome::default();
        };
        let max_bytes = dvr.buffer_size.max(0) as usize;
        let mut outcome = QueuePushOutcome::default();
        let dvr_queued_overflowed = self
            .dvrs
            .get(&dvr_id)
            .map(|d| d.queued_bytes.checked_add(payload.len()).is_none())
            .unwrap_or(false);
        if max_bytes > 0
            && self
                .dvrs
                .get(&dvr_id)
                .map(|d| d.queued_bytes.checked_add(payload.len()).map_or(true, |next| next > max_bytes))
                .unwrap_or(false)
        {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                if dvr_queued_overflowed {
                    dvr.diagnostic_counter_saturated = true;
                    outcome.counter_saturated = true;
                }
                dvr.pending_overflow = true;
                increment_diagnostic_counter(&mut dvr.overflow_events, &mut dvr.diagnostic_counter_saturated);
                add_diagnostic_counter(&mut dvr.drop_bytes, payload.len() as u64, &mut dvr.diagnostic_counter_saturated);
            }
            outcome.dropped_bytes = payload.len();
            outcome.dropped_entries = 1;
            outcome.dropped_new = true;
            outcome.overflowed = true;
            return outcome;
        }
        let Some(next_queued) = self
            .dvrs
            .get(&dvr_id)
            .and_then(|dvr| {
                let mut saturated = false;
                let next = add_queue_accounting(dvr.queued_bytes, payload.len(), &mut saturated);
                if saturated { None } else { next }
            })
        else {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.pending_overflow = true;
                dvr.diagnostic_counter_saturated = true;
                increment_diagnostic_counter(&mut dvr.overflow_events, &mut dvr.diagnostic_counter_saturated);
                add_diagnostic_counter(&mut dvr.drop_bytes, payload.len() as u64, &mut dvr.diagnostic_counter_saturated);
            }
            outcome.dropped_bytes = payload.len();
            outcome.dropped_entries = 1;
            outcome.dropped_new = true;
            outcome.overflowed = true;
            outcome.counter_saturated = true;
            return outcome;
        };
        let queue = self.dvr_queues.entry(dvr_id).or_default();
        queue.push_back(payload.to_vec());
        outcome.accepted_entries = 1;
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.queued_bytes = next_queued;
            outcome.accepted_bytes = payload.len();
        }
        outcome
    }
}


#[cfg(test)]
mod pes_metadata_tests {
    use super::{AvPayloadMetadata, FilterPayload};

    #[test]
    fn pes_payload_preserves_stream_id_even_when_payload_is_es_only() {
        let payload = FilterPayload::PesData {
            bytes: vec![0x00, 0x11, 0x22, 0x33],
            stream_id: 0xbd,
            raw: false,
            metadata: AvPayloadMetadata {
                pts_90khz: Some(90_000),
                dts_90khz: Some(89_000),
                stream_id: 0xbd,
            },
        };
        assert_eq!(payload.pes_stream_id(), Some(0xbd));
        assert_eq!(payload.bytes(), &[0x00, 0x11, 0x22, 0x33]);
        let metadata = payload.av_metadata().expect("PES payload should retain timing metadata");
        assert_eq!(metadata.pts_90khz, Some(90_000));
        assert_eq!(metadata.dts_90khz, Some(89_000));
    }
}

#[cfg(test)]
mod record_dvr_tests {
    use super::{AvPayloadMetadata, FilterPayload};
    use maleicacid_tuner_hal_common::TS_PACKET_SIZE;

    #[test]
    fn record_dvr_filters_only_ts_packets_by_type() {
        let ts = FilterPayload::TsPacket(vec![0x47; TS_PACKET_SIZE]);
        let pes = FilterPayload::PesData {
            bytes: vec![1, 2, 3],
            stream_id: 0xbd,
            raw: false,
            metadata: AvPayloadMetadata {
                pts_90khz: None,
                dts_90khz: None,
                stream_id: 0xbd,
            },
        };
        let section = FilterPayload::Bytes(vec![0x00, 0xb0, 0x0d]);
        assert_eq!(ts.bytes().len(), TS_PACKET_SIZE);
        assert!(!matches!(pes, FilterPayload::TsPacket(_)));
        assert!(!matches!(section, FilterPayload::TsPacket(_)));
    }
}

#[cfg(test)]
mod pes_lifecycle_tests {
    use super::{DemuxHandle, TsInputOrigin};

    fn length_zero_video_pes_payload(byte: u8) -> Vec<u8> {
        let mut bytes = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00];
        bytes.extend_from_slice(&[byte; 16]);
        bytes
    }

    #[test]
    fn lifecycle_drop_clears_pending_length_zero_pes_without_emit() {
        let mut demux = DemuxHandle::new(0);
        let pid = 0x0100u16;
        let pending = length_zero_video_pes_payload(0xaa);
        let emitted = demux.packet_pipeline.test_assemble_pes_for_filter(
            TsInputOrigin::Frontend,
            pid,
            true,
            &pending,
        );
        assert!(emitted.is_empty());
        assert!(demux.packet_pipeline.has_pending_pes());
        demux.packet_pipeline_drop_all_pes();
        assert!(!demux.packet_pipeline.has_pending_pes());
    }

    #[test]
    fn same_stream_next_pusi_emits_previous_length_zero_pes() {
        let mut demux = DemuxHandle::new(0);
        let pid = 0x0100u16;
        let first = length_zero_video_pes_payload(0xaa);
        let second = length_zero_video_pes_payload(0xbb);
        let emitted_first = demux.packet_pipeline.test_assemble_pes_for_filter(
            TsInputOrigin::Frontend,
            pid,
            true,
            &first,
        );
        assert!(emitted_first.is_empty());
        let emitted_second = demux.packet_pipeline.test_assemble_pes_for_filter(
            TsInputOrigin::Frontend,
            pid,
            true,
            &second,
        );
        assert_eq!(emitted_second.len(), 1);
        assert_eq!(emitted_second[0].stream_id, 0xe0);
        assert!(emitted_second[0].payload.iter().all(|b| *b == 0xaa));
    }
}

#[cfg(test)]
mod record_dvr_negative_tests {
    use super::{
        AvPayloadMetadata, DemuxHandle, DemuxPathDirection, DvrConfig, FilterOpenType,
        FilterPayload,
    };

    #[test]
    fn record_dvr_rejects_section_pes_and_av_payloads() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        demux
            .configure_record_pid_filter(filter.filter_id, 0x0100)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 0,
                high_threshold: 0,
                data_format: 0,
                packet_size: 188,
            }
        ), Ok(()));
        assert_eq!(demux.attach_filter_to_dvr_result(dvr.dvr_id, filter.filter_id), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        demux.mirror_filter_payload_to_record_dvrs(
            filter.filter_id,
            &FilterPayload::Bytes(vec![0x00, 0xb0, 0x0d]),
        );
        demux.mirror_filter_payload_to_record_dvrs(
            filter.filter_id,
            &FilterPayload::PesData {
                bytes: vec![1, 2, 3],
                stream_id: 0xbd,
                raw: false,
                metadata: AvPayloadMetadata {
                    pts_90khz: None,
                    dts_90khz: None,
                    stream_id: 0xbd,
                },
            },
        );
        demux.mirror_filter_payload_to_record_dvrs(
            filter.filter_id,
            &FilterPayload::AvEs {
                bytes: vec![4, 5, 6],
                metadata: AvPayloadMetadata {
                    pts_90khz: None,
                    dts_90khz: None,
                    stream_id: 0xe0,
                },
            },
        );
        assert!(demux.pop_dvr_payload(dvr.dvr_id).is_none());
    }
}

#[cfg(test)]
mod filter_capacity_tests {
    use super::{
        AvFilterStreamKind, DemuxConfigError, DemuxHandle, FilterConfig, FilterConfigKind,
        FilterOpenType, SectionCondition, SectionConditionKind,
    };
    use maleicacid_tuner_hal_common::{
        DEMUX_MAX_AUDIO_FILTERS, DEMUX_MAX_SECTION_FILTERS, DEMUX_MAX_VIDEO_FILTERS,
    };

    fn av_config() -> FilterConfig {
        FilterConfig {
            tpid: 0x0100,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Av {
                passthrough: false,
                secure_memory: false,
            },
        }
    }

    fn section_config() -> FilterConfig {
        FilterConfig {
            tpid: 0x0000,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    #[test]
    fn audio_and_video_capacity_are_enforced_independently() {
        let mut demux = DemuxHandle::new(0);
        let mut audio_ids = Vec::new();
        for _ in 0..DEMUX_MAX_AUDIO_FILTERS {
            let filter = demux
                .register_filter_result(1, FilterOpenType::TsAudio, 4096)
                .expect("test setup should register filter");
            assert!(demux.configure_filter_with_summary(filter.filter_id, av_config()));
            audio_ids.push(filter.filter_id);
        }
        let extra_audio = demux
            .register_filter_result(1, FilterOpenType::TsAudio, 4096)
            .expect("test setup should register filter");
        assert_eq!(
            demux.configure_filter_with_summary_result(extra_audio.filter_id, av_config()),
            Err(DemuxConfigError::CapacityExceeded)
        );

        for _ in 0..DEMUX_MAX_VIDEO_FILTERS {
            let filter = demux
                .register_filter_result(1, FilterOpenType::TsVideo, 4096)
                .expect("test setup should register filter");
            assert!(demux.configure_filter_with_summary(filter.filter_id, av_config()));
        }
        let extra_video = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        assert_eq!(
            demux.configure_filter_with_summary_result(extra_video.filter_id, av_config()),
            Err(DemuxConfigError::CapacityExceeded)
        );
        assert_eq!(audio_ids.len(), DEMUX_MAX_AUDIO_FILTERS as usize);
    }

    #[test]
    fn av_stream_type_must_match_open_subtype() {
        let mut demux = DemuxHandle::new(0);
        let audio = demux
            .register_filter_result(1, FilterOpenType::TsAudio, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(audio.filter_id, av_config()));
        assert!(demux.set_filter_av_stream_type_hint(
            audio.filter_id,
            2,
            AvFilterStreamKind::Audio
        ));
        assert!(!demux.set_filter_av_stream_type_hint(
            audio.filter_id,
            2,
            AvFilterStreamKind::Video
        ));

        let video = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(video.filter_id, av_config()));
        assert!(demux.set_filter_av_stream_type_hint(
            video.filter_id,
            2,
            AvFilterStreamKind::Video
        ));
        assert!(!demux.set_filter_av_stream_type_hint(
            video.filter_id,
            2,
            AvFilterStreamKind::Audio
        ));
    }

    #[test]
    fn av_filter_can_start_without_configure_av_stream_type() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, av_config()));
        assert_eq!(demux.start_filter_result(filter.filter_id), Ok(()));
    }

    #[test]
    fn av_filter_default_stream_kind_comes_from_open_subtype_without_configure_av_stream_type() {
        let mut demux = DemuxHandle::new(0);
        let audio = demux
            .register_filter_result(1, FilterOpenType::TsAudio, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(audio.filter_id, av_config()));
        let audio_record = demux.filter_record(audio.filter_id).unwrap();
        assert_eq!(audio_record.av_stream_kind, None);
        assert_eq!(audio_record.effective_av_stream_kind(), Some(AvFilterStreamKind::Audio));

        let video = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(video.filter_id, av_config()));
        let video_record = demux.filter_record(video.filter_id).unwrap();
        assert_eq!(video_record.av_stream_kind, None);
        assert_eq!(video_record.effective_av_stream_kind(), Some(AvFilterStreamKind::Video));
    }

    #[test]
    fn av_再設定_does_not_require_configure_av_stream_type_again_for_start() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, av_config()));
        assert!(demux.set_filter_av_stream_type_hint(
            filter.filter_id,
            2,
            AvFilterStreamKind::Video
        ));
        assert_eq!(demux.start_filter_result(filter.filter_id), Ok(()));
        assert_eq!(
            demux.set_filter_av_stream_type_hint_result(
                filter.filter_id,
                2,
                AvFilterStreamKind::Video
            ),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(
            demux.configure_filter_with_summary_result(filter.filter_id, av_config()),
            Err(DemuxConfigError::InvalidState)
        );
        assert!(demux.stop_filter(filter.filter_id));
        assert!(demux.configure_filter_with_summary(filter.filter_id, av_config()));
        assert_eq!(demux.start_filter_result(filter.filter_id), Ok(()));
    }

    #[test]
    fn unregister_releases_section_capacity() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config()));
        for _ in 1..DEMUX_MAX_SECTION_FILTERS {
            let f = demux
                .register_filter_result(1, FilterOpenType::TsSection, 4096)
                .expect("test setup should register filter");
            assert!(demux.configure_filter_with_summary(f.filter_id, section_config()));
        }
        let extra = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(!demux.configure_filter_with_summary(extra.filter_id, section_config()));
        assert!(demux.unregister_filter(filter.filter_id).is_some());
        assert!(demux.configure_filter_with_summary(extra.filter_id, section_config()));
    }
}

#[cfg(test)]
mod duplicate_packet_tests {
    use super::*;

    fn packet(pid: u16, cc: u8) -> [u8; TS_PACKET_SIZE] {
        let mut p = [0xff; TS_PACKET_SIZE];
        p[0] = 0x47;
        p[1] = ((pid >> 8) as u8) & 0x1f;
        p[2] = pid as u8;
        p[3] = 0x10 | (cc & 0x0f);
        p[4] = 0x00;
        p
    }

    #[test]
    fn duplicate_ts_packet_is_not_returned_as_record_filter_payload_without_index_event() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(
            filter.filter_id,
            FilterConfig {
                tpid: pid as i32,
                main_type_bits: 1,
                sub_type_hint: 0,
                kind: FilterConfigKind::Record {
                    ts_index_mask: 0,
                    sc_index_type: 0,
                    sc_index_mask_bits: 0
                },
            }
        ));
        demux.start_filter(filter.filter_id);
        assert!(demux.push_ts_packet(&packet(pid, 0)));
        assert!(demux.push_ts_packet(&packet(pid, 0)));
        assert!(demux.drain_filter_payloads(filter.filter_id).is_empty());
    }

    #[test]
    fn duplicate_ts_packet_reaches_record_dvr_queue() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(
            filter.filter_id,
            FilterConfig {
                tpid: pid as i32,
                main_type_bits: 1,
                sub_type_hint: 0,
                kind: FilterConfigKind::Record {
                    ts_index_mask: 0,
                    sc_index_type: 0,
                    sc_index_mask_bits: 0
                },
            }
        ));
        assert!(demux.start_filter(filter.filter_id));
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 0,
                high_threshold: 0,
                data_format: 0,
                packet_size: 188,
            }
        ), Ok(()));
        assert_eq!(demux.attach_filter_to_dvr_result(dvr.dvr_id, filter.filter_id), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));

        let first = packet(pid, 0);
        let duplicate = packet(pid, 0);
        assert!(demux.push_ts_packet(&first));
        assert!(demux.push_ts_packet(&duplicate));
        assert_eq!(
            demux.pop_dvr_payload(dvr.dvr_id).as_deref(),
            Some(&first[..])
        );
        assert_eq!(
            demux.pop_dvr_payload(dvr.dvr_id).as_deref(),
            Some(&duplicate[..])
        );
        assert!(demux.pop_dvr_payload(dvr.dvr_id).is_none());
    }

    fn section_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn section_packet(pid: u16, cc: u8, section: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut p = [0xff; TS_PACKET_SIZE];
        p[0] = 0x47;
        p[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        p[2] = pid as u8;
        p[3] = 0x10 | (cc & 0x0f);
        p[4] = 0x00;
        let end = 5 + section.len();
        p[5..end].copy_from_slice(section);
        p
    }

    #[test]
    fn duplicate_ts_packet_is_not_double_inserted_into_section_assembly() {
        let pid = 0x0100;
        let section = vec![0x80, 0x30, 0x03, 0xaa, 0xbb, 0xcc];
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(pid)));
        assert!(demux.start_filter(filter.filter_id));
        let packet = section_packet(pid, 0, &section);
        assert!(demux.push_ts_packet(&packet));
        assert!(demux.push_ts_packet(&packet));
        let queued = demux.drain_filter_payloads(filter.filter_id);
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].bytes(), &section[..]);
    }

    #[test]
    fn section_condition_longer_than_payload_does_not_match_prefix_only() {
        let condition = SectionCondition {
            filter_bytes: vec![0x42, 0x30, 0x00, 0xff],
            mask_bytes: vec![0xff, 0xff, 0xff, 0xff],
            mode_bytes: vec![0x00, 0x00, 0x00, 0x00],
            table_id: None,
            version: None,
        };
        let valid_short_section = [0x42, 0x30, 0x00];
        assert!(!condition.matches(&valid_short_section, 12));

        let exact_condition = SectionCondition {
            filter_bytes: vec![0x42, 0x30, 0x00],
            mask_bytes: vec![0xff, 0xff, 0xff],
            mode_bytes: vec![0x00, 0x00, 0x00],
            table_id: None,
            version: None,
        };
        assert!(exact_condition.matches(&valid_short_section, 12));
    }

    #[test]
    fn section_condition_mode_bit_one_means_negative_match() {
        let section = [0x42, 0x30, 0x00];
        let positive = SectionCondition {
            filter_bytes: vec![0x42],
            mask_bytes: vec![0xff],
            mode_bytes: vec![0x00],
            table_id: None,
            version: None,
        };
        assert!(positive.matches(&section, 12));
        let negative = SectionCondition {
            filter_bytes: vec![0x43],
            mask_bytes: vec![0x01],
            mode_bytes: vec![0x01],
            table_id: None,
            version: None,
        };
        assert!(negative.matches(&section, 12));
        let negative_miss = SectionCondition {
            filter_bytes: vec![0x42],
            mask_bytes: vec![0x01],
            mode_bytes: vec![0x01],
            table_id: None,
            version: None,
        };
        assert!(!negative_miss.matches(&section, 12));
    }

    #[test]
    fn section_condition_width_limit_tracks_capability() {
        let mut c = SectionCondition::default();
        c.filter_bytes = vec![0; MAX_SECTION_FILTER_BYTES as usize];
        c.mask_bytes = vec![0xff; MAX_SECTION_FILTER_BYTES as usize];
        c.mode_bytes = vec![0; MAX_SECTION_FILTER_BYTES as usize];
        assert!(c.validates_section_filter_width());
        c.mask_bytes = vec![0; MAX_SECTION_FILTER_BYTES as usize + 1];
        assert!(!c.validates_section_filter_width());
    }

    #[test]
    fn section_condition_mask_zero_is_dont_care() {
        let section = [0x42, 0x30, 0x00];
        let c = SectionCondition {
            filter_bytes: vec![0xff],
            mask_bytes: vec![0x00],
            mode_bytes: vec![0x00],
            table_id: None,
            version: None,
        };
        assert!(c.matches(&section, 12));
    }

    #[test]
    fn section_condition_rejects_length_mismatch() {
        let c = SectionCondition {
            filter_bytes: vec![0x42],
            mask_bytes: vec![0xff],
            mode_bytes: Vec::new(),
            table_id: None,
            version: None,
        };
        assert!(!c.validates_section_filter_width());
        assert!(!c.matches(&[0x42, 0x30, 0x00], 12));
    }
}

#[cfg(test)]
mod record_pid_set_tests {
    use super::*;

    #[test]
    fn can_configure_record_pid_set() {
        let mut demux = DemuxHandle::new(0);
        let f1 = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter")
            .filter_id;
        let f2 = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter")
            .filter_id;
        let mut pids = BTreeSet::new();
        pids.insert(0x0101);
        pids.insert(0x0102);
        demux.configure_record_pid_set(&[f1, f2], &pids).unwrap();
        assert!(matches!(
            demux
                .filter_record(f1)
                .unwrap()
                .config
                .as_ref()
                .unwrap()
                .kind,
            FilterConfigKind::Record { .. }
        ));
        assert!(matches!(
            demux
                .filter_record(f2)
                .unwrap()
                .config
                .as_ref()
                .unwrap()
                .kind,
            FilterConfigKind::Record { .. }
        ));
    }
}

#[cfg(test)]
mod transport_error_indicator_tests {
    use super::*;
    use maleicacid_tuner_hal_common::TS_PACKET_SIZE;

    fn record_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Record {
                ts_index_mask: 0,
                sc_index_type: 0,
                sc_index_mask_bits: 0,
            },
        }
    }

    fn packet(pid: u16, cc: u8, tei: bool) -> [u8; TS_PACKET_SIZE] {
        let mut p = [0xff; TS_PACKET_SIZE];
        p[0] = 0x47;
        p[1] = ((pid >> 8) as u8) & 0x1f;
        if tei {
            p[1] |= 0x80;
        }
        p[2] = pid as u8;
        p[3] = 0x10 | (cc & 0x0f);
        p[4] = 0x00;
        p
    }

    #[test]
    fn ts_packet_view_exposes_transport_error_indicator() {
        let tei_packet = packet(0x0100, 0, true);
        let parsed = PacketPipeline::validate_packet(&tei_packet).expect("packet parses");
        assert!(parsed.transport_error_indicator);

        let clean_packet = packet(0x0100, 0, false);
        let parsed = PacketPipeline::validate_packet(&clean_packet).expect("packet parses");
        assert!(!parsed.transport_error_indicator);
    }

    #[test]
    fn tei_packet_is_kept_for_record_dvr_without_record_filter_payload_when_index_disabled() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, record_config(pid)));
        demux.start_filter(filter.filter_id);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 0,
                high_threshold: 0,
                data_format: 0,
                packet_size: 188,
            }
        ), Ok(()));
        assert_eq!(demux.attach_filter_to_dvr_result(dvr.dvr_id, filter.filter_id), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));

        let tei = packet(pid, 0, true);
        assert!(demux.push_ts_packet(&tei));
        assert!(demux.drain_filter_payloads(filter.filter_id).is_empty());
        assert_eq!(demux.pop_dvr_payload(dvr.dvr_id).as_deref(), Some(&tei[..]));
    }

    #[test]
    fn tei_packet_does_not_create_record_filter_payload_when_index_disabled() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, record_config(pid)));
        demux.start_filter(filter.filter_id);

        assert!(demux.push_ts_packet(&packet(pid, 0, false)));
        assert!(demux.push_ts_packet(&packet(pid, 1, true)));
        assert!(demux.push_ts_packet(&packet(pid, 1, false)));
        assert!(demux.drain_filter_payloads(filter.filter_id).is_empty());
    }

    fn section_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn section_packet(pid: u16, cc: u8, tei: bool, section: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut p = [0xff; TS_PACKET_SIZE];
        p[0] = 0x47;
        p[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        if tei {
            p[1] |= 0x80;
        }
        p[2] = pid as u8;
        p[3] = 0x10 | (cc & 0x0f);
        p[4] = 0x00;
        let end = 5 + section.len();
        p[5..end].copy_from_slice(section);
        p
    }

    #[test]
    fn tei_packet_does_not_enter_section_assembly() {
        let pid = 0x0100;
        let section = vec![0x80, 0x30, 0x03, 0xaa, 0xbb, 0xcc];
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(pid)));
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.push_ts_packet(&section_packet(pid, 0, true, &section)));
        assert!(demux.drain_filter_payloads(filter.filter_id).is_empty());
    }
}

#[cfg(test)]
mod parser_policy_pes_av_tests {
    use super::*;
    use maleicacid_tuner_hal_common::TS_PACKET_SIZE;

    fn pes_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::PesData {
                stream_id: 0xe0,
                raw: true,
            },
        }
    }

    fn av_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Av {
                passthrough: false,
                secure_memory: false,
            },
        }
    }

    fn pes_payload(byte: u8) -> Vec<u8> {
        let mut bytes = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00];
        bytes.extend_from_slice(&[byte; 16]);
        bytes
    }

    fn pes_ts_packet(pid: u16, cc: u8, tei: bool, payload_byte: u8) -> [u8; TS_PACKET_SIZE] {
        let payload = pes_payload(payload_byte);
        let mut p = [0xff; TS_PACKET_SIZE];
        p[0] = 0x47;
        p[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        if tei {
            p[1] |= 0x80;
        }
        p[2] = pid as u8;
        p[3] = 0x10 | (cc & 0x0f);
        let end = 4 + payload.len();
        p[4..end].copy_from_slice(&payload);
        p
    }

    fn configure_started_pes_and_av_filters(demux: &mut DemuxHandle, pid: u16) -> (i32, i32) {
        let pes = demux
            .register_filter_result(1, FilterOpenType::TsPes, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(pes.filter_id, pes_config(pid)));
        assert!(demux.start_filter(pes.filter_id));

        let av = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(av.filter_id, av_config(pid)));
        assert_eq!(demux.filter_record(av.filter_id).unwrap().av_stream_kind, None);
        assert!(demux.start_filter(av.filter_id));
        (pes.filter_id, av.filter_id)
    }

    #[test]
    fn pes_stream_id_minus_one_is_wildcard_but_zero_is_exact() {
        let pid = 0x0100;
        let pes = PesPacket {
            pid,
            stream_id: 0xe0,
            pts_90khz: None,
            dts_90khz: None,
            data_alignment_indicator: false,
            raw_bytes: vec![0x00, 0x00, 0x01, 0xe0],
            payload: vec![0xaa],
        };
        let mut wildcard = DemuxHandle::new(0);
        let wildcard_filter = wildcard
            .register_filter_result(1, FilterOpenType::TsPes, 4096)
            .expect("test setup should register filter");
        assert!(wildcard.configure_filter_with_summary(
            wildcard_filter.filter_id,
            FilterConfig {
                kind: FilterConfigKind::PesData {
                    stream_id: -1,
                    raw: true
                },
                ..pes_config(pid)
            },
        ));
        assert!(wildcard.filter_accepts_pes(
            wildcard.filter_record(wildcard_filter.filter_id).unwrap(),
            pid as i32,
            &pes,
        ));

        let mut zero_exact = DemuxHandle::new(0);
        let zero_filter = zero_exact
            .register_filter_result(1, FilterOpenType::TsPes, 4096)
            .expect("test setup should register filter");
        assert!(zero_exact.configure_filter_with_summary(
            zero_filter.filter_id,
            FilterConfig {
                kind: FilterConfigKind::PesData {
                    stream_id: 0,
                    raw: true
                },
                ..pes_config(pid)
            },
        ));
        assert!(!zero_exact.filter_accepts_pes(
            zero_exact.filter_record(zero_filter.filter_id).unwrap(),
            pid as i32,
            &pes,
        ));
    }

    #[test]
    fn av_filter_accepts_pes_by_open_subtype_without_configure_av_stream_type() {
        let pid = 0x0100;
        let video_pes = PesPacket {
            pid,
            stream_id: 0xe0,
            pts_90khz: None,
            dts_90khz: None,
            data_alignment_indicator: false,
            raw_bytes: vec![0x00, 0x00, 0x01, 0xe0],
            payload: vec![0xaa],
        };
        let audio_pes = PesPacket {
            stream_id: 0xc0,
            raw_bytes: vec![0x00, 0x00, 0x01, 0xc0],
            ..video_pes.clone()
        };

        let mut demux = DemuxHandle::new(0);
        let audio = demux
            .register_filter_result(1, FilterOpenType::TsAudio, 4096)
            .expect("test setup should register filter");
        let video = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(audio.filter_id, av_config(pid)));
        assert!(demux.configure_filter_with_summary(video.filter_id, av_config(pid)));

        assert!(demux.filter_accepts_pes(
            demux.filter_record(audio.filter_id).unwrap(),
            pid as i32,
            &audio_pes,
        ));
        assert!(!demux.filter_accepts_pes(
            demux.filter_record(audio.filter_id).unwrap(),
            pid as i32,
            &video_pes,
        ));
        assert!(demux.filter_accepts_pes(
            demux.filter_record(video.filter_id).unwrap(),
            pid as i32,
            &video_pes,
        ));
        assert!(!demux.filter_accepts_pes(
            demux.filter_record(video.filter_id).unwrap(),
            pid as i32,
            &audio_pes,
        ));
    }

    #[test]
    fn tei_packet_does_not_enter_pes_or_av_assembly() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let (pes_filter, av_filter) = configure_started_pes_and_av_filters(&mut demux, pid);

        assert!(demux.push_ts_packet(&pes_ts_packet(pid, 0, true, 0xaa)));
        assert!(demux.push_ts_packet(&pes_ts_packet(pid, 0, false, 0xbb)));

        assert!(demux.drain_filter_payloads(pes_filter).is_empty());
        assert!(demux.drain_filter_payloads(av_filter).is_empty());
    }

    #[test]
    fn duplicate_packet_is_not_double_inserted_into_pes_or_av_assembly() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let (pes_filter, av_filter) = configure_started_pes_and_av_filters(&mut demux, pid);

        let first = pes_ts_packet(pid, 0, false, 0xaa);
        assert!(demux.push_ts_packet(&first));
        assert!(demux.push_ts_packet(&first));
        assert!(demux.push_ts_packet(&pes_ts_packet(pid, 1, false, 0xbb)));

        let pes_payloads = demux.drain_filter_payloads(pes_filter);
        let av_payloads = demux.drain_filter_payloads(av_filter);
        assert_eq!(pes_payloads.len(), 1);
        assert_eq!(av_payloads.len(), 1);
        assert!(pes_payloads[0].bytes().contains(&0xaa));
        assert!(!pes_payloads[0].bytes().contains(&0xbb));
        assert!(av_payloads[0].bytes().contains(&0xaa));
        assert!(!av_payloads[0].bytes().contains(&0xbb));
    }
}

#[cfg(test)]
mod av_sync_tests {
    use super::*;
    use maleicacid_tuner_hal_common::TS_PACKET_SIZE;
    use std::time::{Duration, Instant};

    fn av_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Av {
                passthrough: false,
                secure_memory: false,
            },
        }
    }

    fn section_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn pcr_packet(pid: u16, cc: u8, pcr_base: u64) -> [u8; TS_PACKET_SIZE] {
        let base = pcr_base & ((1u64 << 33) - 1);
        let mut packet = [0xff; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = pid as u8;
        packet[3] = 0x20 | (cc & 0x0f);
        packet[4] = 7;
        packet[5] = 0x10;
        packet[6] = (base >> 25) as u8;
        packet[7] = (base >> 17) as u8;
        packet[8] = (base >> 9) as u8;
        packet[9] = (base >> 1) as u8;
        packet[10] = ((base & 0x01) as u8) << 7;
        packet[11] = 0;
        packet
    }

    fn configure_started_av_filter(
        demux: &mut DemuxHandle,
        pid: u16,
        kind: AvFilterStreamKind,
    ) -> i32 {
        let open_type = match kind {
            AvFilterStreamKind::Audio => FilterOpenType::TsAudio,
            AvFilterStreamKind::Video => FilterOpenType::TsVideo,
        };
        let filter = demux
            .register_filter_result(1, open_type, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, av_config(pid)));
        assert!(demux.set_filter_av_stream_type_hint(filter.filter_id, 2, kind));
        assert!(demux.start_filter(filter.filter_id));
        filter.filter_id
    }

    #[test]
    fn av_sync_hw_id_is_assigned_only_to_av_filters() {
        let mut demux = DemuxHandle::new(3);
        let av_filter_id =
            configure_started_av_filter(&mut demux, 0x0100, AvFilterStreamKind::Video);
        assert_eq!(demux.av_sync_hw_id_for(av_filter_id), None);
        assert!(demux.push_ts_packet(&pcr_packet(0x0100, 0, 1_000)));
        assert_eq!(
            demux.av_sync_hw_id_for(av_filter_id),
            Some((3 << 16) | av_filter_id)
        );

        let section = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(section.filter_id, section_config(0x0000)));
        assert_eq!(demux.av_sync_hw_id_for(section.filter_id), None);
    }

    #[test]
    fn av_sync_time_returns_interpolated_pcr_clock() {
        let mut demux = DemuxHandle::new(0);
        let av_filter_id =
            configure_started_av_filter(&mut demux, 0x0100, AvFilterStreamKind::Video);
        assert_eq!(demux.av_sync_hw_id_for(av_filter_id), None);

        assert!(demux.push_ts_packet(&pcr_packet(0x0100, 0, 123_456)));
        let sync_id = demux.av_sync_hw_id_for(av_filter_id).unwrap();
        let immediate = demux.av_sync_time_now(sync_id).unwrap();
        assert!(immediate >= 123_456);

        demux.latest_pcr_instant = Some(Instant::now() - Duration::from_millis(1_000));
        assert_eq!(demux.av_sync_time_now(sync_id), Some(123_456 + 90_000));
    }

    #[test]
    fn av_sync_time_requires_pcr_clock_even_when_pts_is_seen() {
        let mut demux = DemuxHandle::new(1);
        let av_filter_id =
            configure_started_av_filter(&mut demux, 0x0100, AvFilterStreamKind::Audio);
        assert_eq!(demux.av_sync_hw_id_for(av_filter_id), None);
        let pes = PesPacket {
            pid: 0x0100,
            stream_id: 0xc0,
            pts_90khz: Some(9_000),
            dts_90khz: None,
            data_alignment_indicator: false,
            raw_bytes: vec![0x00, 0x00, 0x01, 0xc0],
            payload: vec![0xaa, 0xbb],
        };
        demux.route_pes_packet_for_filter(TsInputOrigin::Frontend, 0x0100, av_filter_id, &pes, 0);
        assert_eq!(demux.av_sync_hw_id_for(av_filter_id), None);
    }

    #[test]
    fn av_sync_time_rejects_unknown_closed_and_wrong_demux_ids() {
        let mut demux = DemuxHandle::new(2);
        let av_filter_id =
            configure_started_av_filter(&mut demux, 0x0100, AvFilterStreamKind::Video);
        assert!(demux.push_ts_packet(&pcr_packet(0x0100, 0, 1_000)));
        let sync_id = demux.av_sync_hw_id_for(av_filter_id).unwrap();
        assert_eq!(demux.av_sync_time_now((3 << 16) | av_filter_id), None);
        assert!(demux.unregister_filter(av_filter_id).is_some());
        assert_eq!(demux.av_sync_time_now(sync_id), None);
    }

    #[test]
    fn av_sync_pcr_wrap_is_extended_monotonically() {
        let mut demux = DemuxHandle::new(0);
        let av_filter_id =
            configure_started_av_filter(&mut demux, 0x0100, AvFilterStreamKind::Video);
        let near_wrap = (1u64 << 33) - 10;
        assert!(demux.push_ts_packet(&pcr_packet(0x0100, 0, near_wrap)));
        let sync_id = demux.av_sync_hw_id_for(av_filter_id).unwrap();
        assert!(demux.av_sync_time_now(sync_id).unwrap() >= near_wrap as i64);
        assert!(demux.push_ts_packet(&pcr_packet(0x0100, 1, 20)));
        assert!(demux.av_sync_time_now(sync_id).unwrap() >= (1i64 << 33) + 20);
    }
}

#[cfg(test)]
mod section_payload_cap_tests {
    use super::*;

    fn section_config_for_pid(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: DEMUX_FILTER_MAIN_TYPE_TS_BITS,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn section_packet(pid: u16, continuity_counter: u8, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = (pid & 0xff) as u8;
        packet[3] = 0x10 | (continuity_counter & 0x0f);
        let copy_len = payload.len().min(TS_PACKET_SIZE - 4);
        packet[4..4 + copy_len].copy_from_slice(&payload[..copy_len]);
        packet
    }

    fn started_section_filter(demux: &mut DemuxHandle, pid: u16) -> DemuxFilterRecord {
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("section filter registration should succeed");
        assert!(demux
            .configure_filter_with_summary(filter.filter_id, section_config_for_pid(pid as i32),));
        assert!(demux.start_filter(filter.filter_id));
        filter
    }

    #[test]
    fn section_pipeline_oversized_drop_reports_outcome() {
        let mut demux = DemuxHandle::new(9);
        assert!(!demux.packet_pipeline.test_record_oversized_section_drop(TsInputOrigin::Frontend, 0x0123));
        let outcome = demux.packet_pipeline.test_assemble_section_for_filter(TsInputOrigin::Frontend, 0x0123, false, &[]);
        assert!(outcome.is_empty());
        assert_eq!(demux.oversized_section_drop_count(), 1);
    }

    #[test]
    fn stale_partial_discard_sets_filter_pending_overflow() {
        let mut demux = DemuxHandle::new(9);
        let pid = 0x0123;
        let filter = started_section_filter(&mut demux, pid);
        let stale = [0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1];
        let replacement = [0x42, 0xf0, 0x05, 0x00, 0x01, 0xc1, 0x00, 0x00];
        let mut first = vec![0x00];
        first.extend_from_slice(&stale);
        assert!(demux.push_ts_packet(&section_packet(pid, 0, &first)));
        let mut second = vec![0x00];
        second.extend_from_slice(&replacement);
        assert!(demux.push_ts_packet(&section_packet(pid, 1, &second)));
        assert_eq!(
            demux.filter_stale_partial_discard_count(filter.filter_id),
            Some(1)
        );
        assert!(demux.take_filter_pending_overflow(filter.filter_id));
        assert!(!demux.take_filter_pending_overflow(filter.filter_id));
    }

    #[test]
    fn section_drop_sets_filter_pending_overflow() {
        let mut demux = DemuxHandle::new(9);
        let pid = 0x0123;
        let filter = started_section_filter(&mut demux, pid);
        let outcome = SectionPushOutcome {
            sections: Vec::new(),
            oversized_section_drop_delta: 1,
            stale_partial_discard_delta: 0,
            oversized_section_counter_saturated: false,
            stale_partial_counter_saturated: false,
        };

        demux.apply_section_push_outcome(filter.filter_id, &outcome);

        assert_eq!(
            demux.filter_section_drop_event_count(filter.filter_id),
            Some(1)
        );
        assert_eq!(
            demux.filter_stale_partial_discard_count(filter.filter_id),
            Some(0)
        );
        assert!(demux.take_filter_pending_overflow(filter.filter_id));
        assert!(!demux.take_filter_pending_overflow(filter.filter_id));
    }

    #[test]
    fn demux_exposes_oversized_section_drop_counter() {
        let mut demux = DemuxHandle::new(9);
        let pid = 0x0123;
        assert!(!demux.packet_pipeline.test_record_oversized_section_drop(TsInputOrigin::Frontend, pid));
        assert_eq!(demux.oversized_section_drop_count(), 1);
    }

    #[test]
    fn demux_exposes_stale_partial_section_discard_counter() {
        let mut demux = DemuxHandle::new(9);
        let pid = 0x0123;
        let stale = [0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1];
        let replacement = [0x42, 0xf0, 0x05, 0x00, 0x01, 0xc1, 0x00, 0x00];
        let mut first = vec![0x00];
        first.extend_from_slice(&stale);
        assert!(demux.packet_pipeline.test_assemble_section_for_filter(TsInputOrigin::Frontend, pid, true, &first).is_empty());
        let mut second = vec![0x00];
        second.extend_from_slice(&replacement);
        assert_eq!(
            demux.packet_pipeline.test_assemble_section_for_filter(TsInputOrigin::Frontend, pid, true, &second),
            vec![replacement.to_vec()]
        );
        assert_eq!(demux.stale_partial_section_discard_count(), 1);
    }
}

#[cfg(test)]
mod stream_boundary_reset_tests {
    use super::*;

    #[test]
    fn apply_stream_boundary_reset_keeps_configuration_but_drops_runtime_state() {
        let mut demux = DemuxHandle::new(7);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        demux
            .configure_record_pid_filter(filter.filter_id, 0x0100)
            .unwrap();
        assert!(demux.start_filter(filter.filter_id));
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 0,
                high_threshold: 0,
                data_format: 0,
                packet_size: 188,
            }
        ), Ok(()));
        assert_eq!(demux.attach_filter_to_dvr_result(dvr.dvr_id, filter.filter_id), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        demux
            .filter_queues
            .get_mut(&filter.filter_id)
            .unwrap()
            .push_back(FilterPayload::Bytes(vec![1, 2, 3]));
        demux
            .filters
            .get_mut(&filter.filter_id)
            .unwrap()
            .queued_bytes = 3;
        demux
            .dvr_queues
            .get_mut(&dvr.dvr_id)
            .unwrap()
            .push_back(vec![4, 5]);
        demux.dvrs.get_mut(&dvr.dvr_id).unwrap().queued_bytes = 2;
        demux.packet_pipeline.test_seed_section(TsInputOrigin::Frontend, 0x100);
        demux.packet_pipeline.test_seed_pes(TsInputOrigin::Frontend, 0x100);
        demux.latest_pcr = Some(123);
        demux.latest_pcr_instant = Some(Instant::now());
        demux.latest_pcr_90khz = Some(123);

        demux.apply_stream_boundary_reset();

        assert!(demux.has_filter(filter.filter_id));
        assert!(demux.filter_record(filter.filter_id).unwrap().lifecycle.is_started());
        assert!(demux.has_dvr(dvr.dvr_id));
        assert!(demux.dvr_record(dvr.dvr_id).unwrap().lifecycle.is_started());
        assert_eq!(demux.current_filter_fill_bytes(filter.filter_id), Some(0));
        assert_eq!(demux.current_fill_bytes(dvr.dvr_id), Some(0));
        assert!(demux
            .filter_queues
            .get(&filter.filter_id)
            .unwrap()
            .is_empty());
        assert!(demux.dvr_queues.get(&dvr.dvr_id).unwrap().is_empty());
        assert!(!demux.packet_pipeline.has_pending_section());
        assert!(!demux.packet_pipeline.has_pending_pes());
        assert_eq!(demux.latest_pcr, None);
        assert_eq!(demux.latest_pcr_instant, None);
        assert_eq!(demux.latest_pcr_90khz, None);
    }
}

#[cfg(test)]
mod dvr_capacity_tests {
    use super::{DemuxConfigError, DemuxHandle, DemuxPathDirection};

    #[test]
    fn register_dvr_rejects_second_same_direction() {
        let mut demux = DemuxHandle::new(42);
        let first = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert_eq!(first.direction, DemuxPathDirection::Record);
        assert_eq!(
            demux
                .register_dvr(DemuxPathDirection::Record, 4096)
                .unwrap_err(),
            DemuxConfigError::InvalidState
        );
    }

    #[test]
    fn register_dvr_allows_one_record_and_one_playback() {
        let mut demux = DemuxHandle::new(42);
        let record = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        let playback = demux
            .register_dvr(DemuxPathDirection::Playback, 4096)
            .unwrap();
        assert_ne!(record.dvr_id, playback.dvr_id);
    }

    #[test]
    fn unregister_dvr_releases_direction_capacity() {
        let mut demux = DemuxHandle::new(42);
        let record = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert!(demux.unregister_dvr(record.dvr_id).is_some());
        assert!(demux.register_dvr(DemuxPathDirection::Record, 4096).is_ok());
    }
}

#[cfg(test)]
mod delay_hint_delivery_tests {
    use super::{
        DemuxHandle, FilterConfig, FilterConfigKind, FilterDelayHintState, FilterDeliveryReadiness,
        FilterOpenType, FilterPayload, SectionCondition, SectionConditionKind,
    };
    use std::thread;
    use std::time::Duration;

    fn section_config(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    #[test]
    fn data_size_delay_holds_payload_until_threshold() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(0x100)));
        assert!(demux.set_filter_delay_hint(
            filter.filter_id,
            FilterDelayHintState::DataSizeDelayBytes(5)
        ));
        assert!(demux.start_filter(filter.filter_id));
        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert_eq!(
            demux.filter_delivery_readiness(filter.filter_id),
            FilterDeliveryReadiness::WaitingForDataSize
        );
        assert!(demux
            .drain_filter_payloads_for_delivery(filter.filter_id)
            .is_empty());
        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![4, 5]));
        assert_eq!(
            demux.filter_delivery_readiness(filter.filter_id),
            FilterDeliveryReadiness::Ready
        );
        let out = demux.drain_filter_payloads_for_delivery(filter.filter_id);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].bytes(), &[1, 2, 3]);
        assert_eq!(out[1].bytes(), &[4, 5]);
    }

    #[test]
    fn time_delay_holds_payload_until_期限() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(0x100)));
        assert!(
            demux.set_filter_delay_hint(filter.filter_id, FilterDelayHintState::TimeDelayMs(20))
        );
        assert!(demux.start_filter(filter.filter_id));
        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![1]));
        assert_eq!(
            demux.filter_delivery_readiness(filter.filter_id),
            FilterDeliveryReadiness::WaitingForTime
        );
        assert!(demux
            .drain_filter_payloads_for_delivery(filter.filter_id)
            .is_empty());
        thread::sleep(Duration::from_millis(25));
        assert_eq!(
            demux.filter_delivery_readiness(filter.filter_id),
            FilterDeliveryReadiness::Ready
        );
        let out = demux.drain_filter_payloads_for_delivery(filter.filter_id);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bytes(), &[1]);
    }

    #[test]
    fn time_delay_rearms_for_each_queue_まとまり() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(0x100)));
        assert!(
            demux.set_filter_delay_hint(filter.filter_id, FilterDelayHintState::TimeDelayMs(20))
        );
        assert!(demux.start_filter(filter.filter_id));

        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![1]));
        assert_eq!(
            demux.filter_delivery_readiness(filter.filter_id),
            FilterDeliveryReadiness::WaitingForTime
        );
        thread::sleep(Duration::from_millis(25));
        assert_eq!(
            demux
                .drain_filter_payloads_for_delivery(filter.filter_id)
                .len(),
            1
        );

        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![2]));
        assert_eq!(
            demux.filter_delivery_readiness(filter.filter_id),
            FilterDeliveryReadiness::WaitingForTime
        );
        assert!(demux
            .drain_filter_payloads_for_delivery(filter.filter_id)
            .is_empty());
        thread::sleep(Duration::from_millis(25));
        let out = demux.drain_filter_payloads_for_delivery(filter.filter_id);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bytes(), &[2]);
    }
}

#[cfg(test)]
mod filter_contract_tests {
    use super::{
        AvFilterStreamKind, DemuxHandle, FilterConfig, FilterConfigKind, FilterDelayHintState,
        FilterDeliveryReadiness, FilterOpenType, FilterPayload, SectionCondition,
        SectionConditionKind,
    };
    use std::thread;
    use std::time::Duration;

    fn av_config() -> FilterConfig {
        FilterConfig {
            tpid: 0x0100,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Av {
                passthrough: false,
                secure_memory: false,
            },
        }
    }

    fn section_config() -> FilterConfig {
        FilterConfig {
            tpid: 0x0000,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn pes_config() -> FilterConfig {
        FilterConfig {
            tpid: 0x0100,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::PesData {
                stream_id: 0xbd,
                raw: false,
            },
        }
    }

    fn record_config() -> FilterConfig {
        FilterConfig {
            tpid: 0x0100,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Record {
                ts_index_mask: 0,
                sc_index_type: 0,
                sc_index_mask_bits: 0,
            },
        }
    }

    #[test]
    fn open_subtype_and_config_kind_must_match() {
        let mut demux = DemuxHandle::new(0);
        let audio = demux
            .register_filter_result(1, FilterOpenType::TsAudio, 4096)
            .expect("test setup should register filter");
        let video = demux
            .register_filter_result(1, FilterOpenType::TsVideo, 4096)
            .expect("test setup should register filter");
        let section = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let pes = demux
            .register_filter_result(1, FilterOpenType::TsPes, 4096)
            .expect("test setup should register filter");
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");

        assert!(demux.configure_filter_with_summary(audio.filter_id, av_config()));
        assert!(demux.set_filter_av_stream_type_hint(
            audio.filter_id,
            2,
            AvFilterStreamKind::Audio
        ));
        assert!(demux.configure_filter_with_summary(video.filter_id, av_config()));
        assert!(demux.set_filter_av_stream_type_hint(
            video.filter_id,
            2,
            AvFilterStreamKind::Video
        ));
        assert!(demux.configure_filter_with_summary(section.filter_id, section_config()));
        assert!(demux.configure_filter_with_summary(pes.filter_id, pes_config()));
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config()));

        let mut mismatch = DemuxHandle::new(1);
        let audio_as_section = mismatch
            .register_filter_result(1, FilterOpenType::TsAudio, 4096)
            .expect("test setup should register filter");
        let section_as_av = mismatch
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        let record_as_pes = mismatch
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        let pes_as_record = mismatch
            .register_filter_result(1, FilterOpenType::TsPes, 4096)
            .expect("test setup should register filter");
        assert!(
            !mismatch.configure_filter_with_summary(audio_as_section.filter_id, section_config())
        );
        assert!(!mismatch.configure_filter_with_summary(section_as_av.filter_id, av_config()));
        assert!(!mismatch.configure_filter_with_summary(record_as_pes.filter_id, pes_config()));
        assert!(!mismatch.configure_filter_with_summary(pes_as_record.filter_id, record_config()));
    }

    #[test]
    fn record_filter_rejects_data_size_delay_but_accepts_time_delay() {
        let mut demux = DemuxHandle::new(0);
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config()));
        assert!(demux.set_filter_delay_hint(record.filter_id, FilterDelayHintState::TimeDelayMs(10)));
        assert!(!demux.set_filter_delay_hint(
            record.filter_id,
            FilterDelayHintState::DataSizeDelayBytes(0)
        ));
        assert!(!demux.set_filter_delay_hint(
            record.filter_id,
            FilterDelayHintState::DataSizeDelayBytes(188)
        ));
        assert_eq!(
            demux.filter_record(record.filter_id).unwrap().delay_hints.data_size_delay_bytes,
            None
        );
    }

    #[test]
    fn time_and_data_size_delay_uses_or_condition() {
        let mut demux = DemuxHandle::new(0);
        let by_time = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(by_time.filter_id, section_config()));
        assert!(
            demux.set_filter_delay_hint(by_time.filter_id, FilterDelayHintState::TimeDelayMs(20))
        );
        assert!(demux.set_filter_delay_hint(
            by_time.filter_id,
            FilterDelayHintState::DataSizeDelayBytes(64)
        ));
        assert!(demux.start_filter(by_time.filter_id));
        demux.push_filter_payload(by_time.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert_eq!(
            demux.filter_delivery_readiness(by_time.filter_id),
            FilterDeliveryReadiness::WaitingForTime
        );
        thread::sleep(Duration::from_millis(25));
        assert_eq!(
            demux.filter_delivery_readiness(by_time.filter_id),
            FilterDeliveryReadiness::Ready
        );
        assert_eq!(
            demux
                .drain_filter_payloads_for_delivery(by_time.filter_id)
                .len(),
            1
        );

        let by_size = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(by_size.filter_id, section_config()));
        assert!(demux
            .set_filter_delay_hint(by_size.filter_id, FilterDelayHintState::TimeDelayMs(10_000)));
        assert!(demux.set_filter_delay_hint(
            by_size.filter_id,
            FilterDelayHintState::DataSizeDelayBytes(3)
        ));
        assert!(demux.start_filter(by_size.filter_id));
        demux.push_filter_payload(by_size.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert_eq!(
            demux.filter_delivery_readiness(by_size.filter_id),
            FilterDeliveryReadiness::Ready
        );
        assert_eq!(
            demux
                .drain_filter_payloads_for_delivery(by_size.filter_id)
                .len(),
            1
        );
    }
}

#[cfg(test)]
mod start_event_delay_tests {
    use super::{
        DemuxHandle, FilterConfig, FilterConfigKind, FilterDelayHintState, FilterOpenType,
        FilterPayload, SectionCondition, SectionConditionKind,
    };
    use std::thread;
    use std::time::Duration;

    fn raw_config() -> FilterConfig {
        FilterConfig {
            tpid: 0x0100,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Noinit,
        }
    }

    fn section_config() -> FilterConfig {
        FilterConfig {
            tpid: 0x0000,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }


    #[test]
    fn pending_start_event_is_delivered_after_time_delay_becomes_ready() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config()));
        assert!(
            demux.set_filter_delay_hint(filter.filter_id, FilterDelayHintState::TimeDelayMs(20))
        );
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.set_filter_start_event_pending(filter.filter_id, true));

        assert!(!demux.take_filter_start_event_if_ready(filter.filter_id));
        assert_eq!(
            demux.filter_start_event_pending(filter.filter_id),
            Some(true)
        );
        thread::sleep(Duration::from_millis(25));
        assert!(demux.take_filter_start_event_if_ready(filter.filter_id));
        assert_eq!(
            demux.filter_start_event_pending(filter.filter_id),
            Some(false)
        );
        assert!(!demux.take_filter_start_event_if_ready(filter.filter_id));
    }

    #[test]
    fn pending_start_event_is_delivered_after_data_size_delay_becomes_ready() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config()));
        assert!(demux.set_filter_delay_hint(
            filter.filter_id,
            FilterDelayHintState::DataSizeDelayBytes(5)
        ));
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.set_filter_start_event_pending(filter.filter_id, true));

        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert!(!demux.take_filter_start_event_if_ready(filter.filter_id));
        assert_eq!(
            demux.filter_start_event_pending(filter.filter_id),
            Some(true)
        );
        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![4, 5]));
        assert!(demux.take_filter_start_event_if_ready(filter.filter_id));
        assert_eq!(
            demux.filter_start_event_pending(filter.filter_id),
            Some(false)
        );
    }

    #[test]
    fn pending_start_event_is_preserved_by_stop_and_cleared_by_flush_and_stream_boundary_reset() {
        let mut demux = DemuxHandle::new(0);
        let stopped = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(stopped.filter_id, section_config()));
        assert!(demux.start_filter(stopped.filter_id));
        assert!(demux.set_filter_start_event_pending(stopped.filter_id, true));
        assert!(demux.stop_filter(stopped.filter_id));
        assert_eq!(
            demux.filter_start_event_pending(stopped.filter_id),
            Some(true)
        );

        let flushed = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(flushed.filter_id, section_config()));
        assert!(demux.start_filter(flushed.filter_id));
        assert!(demux.set_filter_start_event_pending(flushed.filter_id, true));
        assert!(demux.flush_filter(flushed.filter_id));
        assert_eq!(
            demux.filter_start_event_pending(flushed.filter_id),
            Some(false)
        );

        let reset = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(reset.filter_id, section_config()));
        assert!(demux.start_filter(reset.filter_id));
        assert!(demux.set_filter_start_event_pending(reset.filter_id, true));
        demux.apply_stream_boundary_reset();
        assert_eq!(
            demux.filter_start_event_pending(reset.filter_id),
            Some(false)
        );
    }

    #[test]
    fn stop_filter_preserves_queued_payload_until_flush() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config()));
        assert!(demux.start_filter(filter.filter_id));
        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert_eq!(
            demux
                .filter_record(filter.filter_id)
                .map(|record| record.queued_bytes),
            Some(3)
        );

        assert!(demux.stop_filter(filter.filter_id));

        assert_eq!(
            demux.filter_record(filter.filter_id).map(|record| (
                record.lifecycle.is_started(),
                record.queued_bytes,
                record.pending_overflow
            )),
            Some((false, 3, false))
        );
        assert_eq!(demux.drain_filter_payloads(filter.filter_id), vec![FilterPayload::Bytes(vec![1, 2, 3])]);
        assert_eq!(
            demux
                .filter_record(filter.filter_id)
                .map(|record| record.queued_bytes),
            Some(0)
        );
    }

    #[test]
    fn 再設定_clears_old_linkage_and_queued_payload() {
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsRaw, 4096)
            .expect("test setup should register filter");
        let downstream = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config()));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, section_config()));
        assert!(demux.start_filter(source.filter_id));
        assert!(demux.set_filter_data_source(downstream.filter_id, source.filter_id));
        demux.push_filter_payload(downstream.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert_eq!(
            demux
                .filter_record(downstream.filter_id)
                .map(|record| (record.data_upstream_filter_id, record.queued_bytes)),
            Some((Some(source.filter_id), 3))
        );

        assert!(demux.configure_filter_with_summary(downstream.filter_id, section_config()));

        assert_eq!(
            demux
                .filter_record(downstream.filter_id)
                .map(|record| (record.data_upstream_filter_id, record.queued_bytes)),
            Some((None, 0))
        );
        assert!(demux.drain_filter_payloads(downstream.filter_id).is_empty());
    }

    #[test]
    fn upstream_unregister_stops_downstream_but_preserves_existing_queue() {
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsRaw, 4096)
            .expect("test setup should register filter");
        let downstream = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config()));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, section_config()));
        assert!(demux.start_filter(source.filter_id));
        assert!(demux.set_filter_data_source(downstream.filter_id, source.filter_id));
        assert!(demux.start_filter(downstream.filter_id));
        demux.push_filter_payload(downstream.filter_id, FilterPayload::Bytes(vec![4, 5, 6]));

        assert!(demux.unregister_filter(source.filter_id).is_some());

        assert_eq!(
            demux.filter_record(downstream.filter_id).map(|record| (
                record.data_upstream_filter_id,
                record.lifecycle.is_started(),
                record.queued_bytes,
                record.pending_overflow
            )),
            Some((None, false, 3, false))
        );
        assert_eq!(
            demux.drain_filter_payloads_for_delivery(downstream.filter_id),
            vec![FilterPayload::Bytes(vec![4, 5, 6])]
        );
        assert!(demux.drain_filter_payloads(downstream.filter_id).is_empty());
    }
}

#[cfg(test)]
mod arbitrary_pid_section_delivery_tests {
    use super::*;

    fn section_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn section_packet(pid: u16, section: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xff; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = 0x10;
        packet[4] = 0x00;
        let end = 5 + section.len();
        packet[5..end].copy_from_slice(section);
        packet
    }

    #[test]
    fn arbitrary_pid_private_section_is_delivered_without_hal_si_semantics() {
        let pid = 0x1abc;
        let private_section = vec![0x80, 0x30, 0x03, 0xaa, 0xbb, 0xcc];
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(pid)));
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.push_ts_packet(&section_packet(pid, &private_section)));
        assert_eq!(
            demux.pop_filter_payload(filter.filter_id),
            Some(private_section)
        );
    }
}

#[cfg(test)]
mod filter_dvr_state_contract_tests {
    use super::*;

    fn record_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Record {
                ts_index_mask: 0,
                sc_index_type: 0,
                sc_index_mask_bits: 0,
            },
        }
    }

    fn raw_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Noinit,
        }
    }

    fn section_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn av_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Av {
                passthrough: false,
                secure_memory: false,
            },
        }
    }

    fn pes_config(pid: u16) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::PesData {
                stream_id: 0xe0,
                raw: false,
            },
        }
    }

    fn dvr_config(direction: DemuxPathDirection) -> DvrConfig {
        DvrConfig {
            direction,
            status_mask: 0,
            low_threshold: 0,
            high_threshold: 4096,
            data_format: 0,
            packet_size: 188,
        }
    }

    #[test]
    fn filter_start_requires_configuration() {
        let mut demux = DemuxHandle::new(0);
        assert_eq!(
            demux.start_filter_result(999),
            Err(DemuxConfigError::NotFound)
        );
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert_eq!(
            demux.start_filter_result(filter.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(0x0000)));
        assert_eq!(demux.start_filter_result(filter.filter_id), Ok(()));
        assert!(demux.filter_record(filter.filter_id).unwrap().lifecycle.is_started());
    }

    #[test]
    fn record_dvr_attach_requires_configured_record_dvr_and_record_filter() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, 999),
            Err(DemuxConfigError::NotFound)
        );

        let record_before_dvr_config = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(
            record_before_dvr_config.filter_id,
            record_config(0x0102)
        ));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record_before_dvr_config.filter_id),
            Err(DemuxConfigError::InvalidState)
        );

        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));

        let section = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(section.filter_id, section_config(0x0000)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, section.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );

        let audio = demux
            .register_filter_result(1, FilterOpenType::TsAudio, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(audio.filter_id, av_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, audio.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );

        let pes = demux
            .register_filter_result(1, FilterOpenType::TsPes, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(pes.filter_id, pes_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, pes.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );

        let unconfigured_record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, unconfigured_record.filter_id),
            Err(DemuxConfigError::InvalidState)
        );

        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0101)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert_eq!(
            demux.dvr_record(dvr.dvr_id).unwrap().attached_filter_ids,
            vec![record.filter_id]
        );
    }

    #[test]
    fn playback_dvr_rejects_filter_attach() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, 4096)
            .unwrap();
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, 999),
            Err(DemuxConfigError::NotFound)
        );
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
    }

    #[test]
    fn configured_filter_and_dvr_stop_are_idempotent_before_start() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(0x0000)));
        assert_eq!(demux.stop_filter_result(filter.filter_id), Ok(()));

        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, 4096)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ), Ok(()));
        assert_eq!(demux.stop_dvr_result(dvr.dvr_id), Ok(()));
    }

    #[test]
    fn playback_dvr_detach_filter_is_invalid_state() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, 4096)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ), Ok(()));
        assert_eq!(
            demux.detach_filter_from_dvr_result(dvr.dvr_id, 123),
            Err(DemuxConfigError::InvalidState)
        );
    }

    #[test]
    fn record_dvr_detach_unattached_filter_is_invalid_state() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            dvr_config(DemuxPathDirection::Record)
        ), Ok(()));
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.detach_filter_from_dvr_result(dvr.dvr_id, record.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
    }

    #[test]
    fn dvr_start_enforces_direction_specific_prerequisites() {
        let mut demux = DemuxHandle::new(0);
        assert_eq!(demux.start_dvr_result(999), Err(DemuxConfigError::NotFound));

        let record_dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert_eq!(
            demux.start_dvr_result(record_dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(demux.configure_dvr_with_summary_result(record_dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));
        assert_eq!(
            demux.start_dvr_result(record_dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert!(!demux.dvr_record(record_dvr.dvr_id).unwrap().lifecycle.is_started());

        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(record_dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert_eq!(
            demux.start_dvr_result(record_dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert!(demux.start_filter(record.filter_id));
        assert_eq!(demux.start_dvr_result(record_dvr.dvr_id), Ok(()));
        assert!(demux.dvr_record(record_dvr.dvr_id).unwrap().lifecycle.is_started());

        let playback_dvr = demux
            .register_dvr(DemuxPathDirection::Playback, 4096)
            .unwrap();
        assert_eq!(
            demux.start_dvr_result(playback_dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(demux.configure_dvr_with_summary_result(
            playback_dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ), Ok(()));
        assert_eq!(demux.start_dvr_result(playback_dvr.dvr_id), Ok(()));
        assert!(demux.dvr_record(playback_dvr.dvr_id).unwrap().lifecycle.is_started());
    }

    #[test]
    fn register_dvr_rejects_id_overflow() {
        let mut demux = DemuxHandle::new(0);
        demux.next_dvr_id = i32::MAX;
        let first = demux.register_dvr(DemuxPathDirection::Record, 4096).unwrap();
        assert_eq!(first.dvr_id, i32::MAX);
        assert_eq!(
            demux.register_dvr(DemuxPathDirection::Playback, 4096),
            Err(DemuxConfigError::IdExhausted)
        );
    }

    #[test]
    fn dvr_flush_distinguishes_missing_and_unconfigured_state() {
        let mut demux = DemuxHandle::new(0);
        assert_eq!(demux.flush_dvr_result(999), Err(DemuxConfigError::NotFound));
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(
            demux.flush_dvr_result(dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            dvr_config(DemuxPathDirection::Record)
        ), Ok(()));
        assert_eq!(demux.flush_dvr_result(dvr.dvr_id), Ok(()));
    }

    #[test]
    fn dvr_reconfigure_resets_status_interval_and_runtime() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ), Ok(()));
        assert!(demux.set_dvr_status_check_interval_hint(dvr.dvr_id, 250));
        assert_eq!(
            demux.dvr_record(dvr.dvr_id)
                .map(|record| record.status_check_interval_hint_ms),
            Some(250)
        );
        assert_eq!(demux.configure_dvr_with_summary_result(
            dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ), Ok(()));
        assert_eq!(
            demux.dvr_record(dvr.dvr_id)
                .map(|record| record.status_check_interval_hint_ms),
            Some(DEFAULT_DVR_STATUS_CHECK_INTERVAL_MS)
        );
        assert_eq!(demux.playback_diagnostics(dvr.dvr_id), Some((0, 0, 0)));
    }

    #[test]
    fn record_dvr_start_rejects_after_last_filter_detached() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        assert_eq!(demux.stop_dvr_result(dvr.dvr_id), Ok(()));
        assert_eq!(demux.detach_filter_from_dvr_result(dvr.dvr_id, record.filter_id), Ok(()));
        assert_eq!(
            demux.start_dvr_result(dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
    }

    #[test]
    fn record_dvr_detach_stops_delivery_for_detached_pid() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));

        demux.mirror_filter_payload_to_record_dvrs(
            record.filter_id,
            &FilterPayload::TsPacket(ts_packet(0x0100, 0xaa).to_vec()),
        );
        assert_eq!(demux.drain_dvr_payloads(dvr.dvr_id).len(), 1);

        assert_eq!(demux.detach_filter_from_dvr_result(dvr.dvr_id, record.filter_id), Ok(()));
        demux.mirror_filter_payload_to_record_dvrs(
            record.filter_id,
            &FilterPayload::TsPacket(ts_packet(0x0100, 0xbb).to_vec()),
        );
        assert!(demux.drain_dvr_payloads(dvr.dvr_id).is_empty());
    }

    #[test]
    fn record_dvr_start_revalidates_attached_filter_state() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record.filter_id),
            Ok(())
        );

        let removed = demux
            .unregister_filter(record.filter_id)
            .expect("record filter should be present");
        assert_eq!(
            demux.start_dvr_result(dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );

        let stale_id = removed.filter_id;
        demux
            .dvr_record_mut_for_test(dvr.dvr_id)
            .attached_filter_ids
            .push(stale_id);
        assert_eq!(
            demux.start_dvr_result(dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );

        let record2 = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record2.filter_id, record_config(0x0101)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record2.filter_id),
            Ok(())
        );
        demux.filter_record_mut_for_test(record2.filter_id).config = None;
        assert_eq!(
            demux.start_dvr_result(dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );

        demux.filter_record_mut_for_test(record2.filter_id).config = Some(section_config(0x0000));
        assert_eq!(
            demux.start_dvr_result(dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
    }

    fn ts_packet(pid: u16, fill: u8) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [fill; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        packet[2] = (pid & 0xff) as u8;
        packet[3] = 0x10;
        packet
    }

    #[test]
    fn ts_raw_filter_receives_only_matching_pid_packet() {
        let mut demux = DemuxHandle::new(0);
        let raw = demux
            .register_filter_result(1, FilterOpenType::TsRaw, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(raw.filter_id, raw_config(0x0100)));
        assert!(demux.start_filter(raw.filter_id));

        let matching = ts_packet(0x0100, 0xaa);
        let non_matching = ts_packet(0x0101, 0xbb);
        assert!(demux.push_ts_packet(&matching));
        assert!(demux.push_ts_packet(&non_matching));

        let payloads = demux.drain_filter_payloads(raw.filter_id);
        assert_eq!(payloads, vec![FilterPayload::Bytes(matching.to_vec())]);
    }

    #[test]
    fn record_filter_capacity_supports_one_service_pid_set() {
        let mut demux = DemuxHandle::new(0);
        let mut filters = Vec::new();
        for index in 0..maleicacid_tuner_hal_common::DEMUX_MAX_RECORD_FILTERS {
            let filter = demux
                .register_filter_result(1, FilterOpenType::TsRecord, 4096)
                .expect("test setup should register filter");
            assert!(demux.configure_filter_with_summary(
                filter.filter_id,
                record_config(0x0100 + index as u16)
            ));
            filters.push(filter.filter_id);
        }
        let extra = demux
            .register_filter_result(1, FilterOpenType::TsRecord, 4096)
            .expect("test setup should register filter");
        assert_eq!(
            demux.configure_record_pid_filter(extra.filter_id, 0x1fff),
            Err(DemuxConfigError::CapacityExceeded)
        );
        assert_eq!(filters.len(), 32);
    }

    #[test]
    fn internal_filter_overflow_sets_pending_overflow() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 3)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(0x0000)));
        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert!(!demux.take_filter_pending_overflow(filter.filter_id));
        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![4, 5]));
        assert!(demux.take_filter_pending_overflow(filter.filter_id));
        assert!(!demux.take_filter_pending_overflow(filter.filter_id));
        assert!(demux.filter_record(filter.filter_id).unwrap().drop_bytes > 0);
    }

    #[test]
    fn non_media_filter_overflow_drops_new_but_media_filter_drops_old() {
        let mut non_media = DemuxHandle::new(0);
        let raw = non_media
            .register_filter_result(1, FilterOpenType::TsRaw, 3)
            .expect("test setup should register filter");
        assert!(non_media.configure_filter_with_summary(raw.filter_id, raw_config(0x0100)));
        let first = non_media.push_filter_payload(raw.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert!(!first.overflowed);
        let second = non_media.push_filter_payload(raw.filter_id, FilterPayload::Bytes(vec![4]));
        assert!(second.dropped_new);
        assert!(second.overflowed);
        let kept = non_media.drain_filter_payloads(raw.filter_id);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].bytes(), &[1, 2, 3]);

        let mut media = DemuxHandle::new(1);
        let video = media
            .register_filter_result(1, FilterOpenType::TsVideo, 3)
            .expect("test setup should register filter");
        assert!(media.configure_filter_with_summary(video.filter_id, av_config(0x0100)));
        let first = media.push_filter_payload(video.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert!(!first.overflowed);
        let second = media.push_filter_payload(video.filter_id, FilterPayload::Bytes(vec![4]));
        assert!(second.dropped_old);
        assert!(second.overflowed);
        let kept = media.drain_filter_payloads(video.filter_id);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].bytes(), &[4]);
    }

    #[test]
    fn record_dvr_overflow_drops_new_packet_and_reports_pending_overflow() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_SIZE_I32)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));
        assert_eq!(
            demux
                .push_dvr_payload(dvr.dvr_id, &ts_packet(0x0100, 0xaa))
                .accepted_bytes,
            TS_PACKET_SIZE
        );
        let overflow = demux.push_dvr_payload(dvr.dvr_id, &ts_packet(0x0101, 0xbb));
        assert!(overflow.overflowed);
        assert!(demux.take_dvr_pending_overflow(dvr.dvr_id));
        let queued = demux.drain_dvr_payloads(dvr.dvr_id);
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0][4], 0xaa);
    }

    #[test]
    fn frontend_ts_still_mirrors_to_started_record_dvr() {
        let mut demux = DemuxHandle::new(0);

        let record_dvr = demux
            .register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(record_dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(record_dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert_eq!(demux.start_filter_result(record.filter_id), Ok(()));
        assert_eq!(demux.start_dvr_result(record_dvr.dvr_id), Ok(()));

        let packet = ts_packet(0x0100, 0xee);
        assert!(demux.push_ts_packet(&packet));

        let queued = demux.drain_dvr_payloads(record_dvr.dvr_id);
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0], packet.to_vec());
    }

    #[test]
    fn playback_injection_does_not_enqueue_to_dvr_output_queue() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback)), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        let packet = ts_packet(0x0100, 0xcc);
        assert!(demux.inject_playback_payload(dvr.dvr_id, &packet));
        assert!(demux.drain_dvr_payloads(dvr.dvr_id).is_empty());
        assert_eq!(
            demux.playback_diagnostics(dvr.dvr_id),
            Some((1, TS_PACKET_SIZE as u64, 0))
        );
    }

    #[test]
    fn playback_injection_does_not_mirror_to_started_record_dvr() {
        let mut demux = DemuxHandle::new(0);

        let record_dvr = demux
            .register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(record_dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));
        let record = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(record_dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert_eq!(demux.start_filter_result(record.filter_id), Ok(()));
        assert_eq!(demux.start_dvr_result(record_dvr.dvr_id), Ok(()));

        let playback_dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(
            playback_dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ), Ok(()));
        assert_eq!(demux.start_dvr_result(playback_dvr.dvr_id), Ok(()));

        let packet = ts_packet(0x0100, 0xdd);
        assert!(demux.inject_playback_payload(playback_dvr.dvr_id, &packet));

        assert!(demux.drain_dvr_payloads(playback_dvr.dvr_id).is_empty());
        assert!(demux.drain_dvr_payloads(record_dvr.dvr_id).is_empty());
        assert_eq!(
            demux.playback_diagnostics(playback_dvr.dvr_id),
            Some((1, TS_PACKET_SIZE as u64, 0))
        );
    }

    #[test]
    fn playback_injection_rejects_unaligned_payload_after_consumer_residual_boundary() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback)), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        assert!(!demux.inject_playback_payload(dvr.dvr_id, &[0x00, 0x01, 0x02]));
        assert_eq!(demux.playback_diagnostics(dvr.dvr_id), Some((0, 0, 3)));
        assert!(demux.drain_dvr_payloads(dvr.dvr_id).is_empty());
    }

    fn section_packet(pid: u16, section: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xff; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = 0x10;
        packet[4] = 0x00;
        let end = 5 + section.len();
        packet[5..end].copy_from_slice(section);
        packet
    }

    fn pes_payload(byte: u8) -> Vec<u8> {
        let mut bytes = vec![0x00, 0x00, 0x01, 0xe0, 0x00, 0x00, 0x80, 0x00, 0x00];
        bytes.extend_from_slice(&[byte; 16]);
        bytes
    }

    fn pes_ts_packet(pid: u16, cc: u8, _tei: bool, payload_byte: u8) -> [u8; TS_PACKET_SIZE] {
        let payload = pes_payload(payload_byte);
        let mut packet = [0xff; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x40 | (((pid >> 8) as u8) & 0x1f);
        packet[2] = pid as u8;
        packet[3] = 0x10 | (cc & 0x0f);
        let end = 4 + payload.len();
        packet[4..end].copy_from_slice(&payload);
        packet
    }

    fn ts_payload_packet(pid: u16, cc: u8, pusi: bool, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0u8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if pusi {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        packet[3] = 0x30 | (cc & 0x0f);
        let adaptation_len = TS_PACKET_SIZE - 5 - payload.len();
        packet[4] = adaptation_len as u8;
        let payload_start = 5 + adaptation_len;
        packet[payload_start..payload_start + payload.len()].copy_from_slice(payload);
        packet
    }

    #[test]
    fn frontend_and_playback_same_pid_do_not_share_continuity_or_section_state() {
        let pid = 0x0120;
        let section = vec![0x80, 0x30, 0x03, 0x10, 0x20, 0x30];
        let mut demux = DemuxHandle::new(0);
        let section_filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let playback_dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert!(demux.configure_filter_with_summary(section_filter.filter_id, section_config(pid)));
        assert_eq!(demux.configure_dvr_with_summary_result(
            playback_dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ), Ok(()));
        assert!(demux.start_filter(section_filter.filter_id));
        assert_eq!(demux.start_dvr_result(playback_dvr.dvr_id), Ok(()));

        assert!(demux.push_ts_packet(&ts_payload_packet(
            pid,
            0,
            true,
            &[0x00, section[0], section[1], section[2], section[3], section[4], section[5]]
        )));
        assert!(demux.inject_playback_payload(
            playback_dvr.dvr_id,
            &ts_payload_packet(
                pid,
                0,
                true,
                &[0x00, section[0], section[1], section[2], section[3], section[4], section[5]]
            )
        ));

        let payloads = demux.drain_filter_payloads(section_filter.filter_id);
        assert_eq!(payloads.len(), 2);
        assert_eq!(payloads[0].bytes(), section.as_slice());
        assert_eq!(payloads[1].bytes(), section.as_slice());
    }

    #[test]
    fn record_only_packet_does_not_feed_section_assembly() {
        let pid = 0x0121;
        let section = vec![0x80, 0x30, 0x03, 0x44, 0x55, 0x66];
        let mut demux = DemuxHandle::new(0);
        let section_filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let record_filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let record_dvr = demux
            .register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert!(demux.configure_filter_with_summary(section_filter.filter_id, section_config(pid)));
        assert!(demux.configure_filter_with_summary(record_filter.filter_id, record_config(pid)));
        assert_eq!(demux.configure_dvr_with_summary_result(record_dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));
        assert_eq!(demux.attach_filter_to_dvr_result(record_dvr.dvr_id, record_filter.filter_id), Ok(()));
        assert!(demux.start_filter(section_filter.filter_id));
        assert!(demux.start_filter(record_filter.filter_id));
        assert_eq!(demux.start_dvr_result(record_dvr.dvr_id), Ok(()));

        assert!(demux.push_ts_packet_record_only(&ts_payload_packet(
            pid,
            0,
            true,
            &[0x00, section[0], section[1], section[2], section[3], section[4], section[5]]
        )));

        assert!(demux
            .drain_filter_payloads(section_filter.filter_id)
            .is_empty());
        assert!(demux
            .drain_filter_payloads(record_filter.filter_id)
            .is_empty());
        assert_eq!(demux.drain_dvr_payloads(record_dvr.dvr_id).len(), 1);
    }

    #[test]
    fn record_filter_dvr_delivery_does_not_write_ts_packet_to_filter_fmq() {
        let pid = 0x0124;
        let packet = ts_payload_packet(pid, 0, true, &[0x00, 0x80, 0x30, 0x03, 0x44, 0x55, 0x66]);
        let mut demux = DemuxHandle::new(0);
        let record_filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let record_dvr = demux
            .register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert!(demux.configure_filter_with_summary(record_filter.filter_id, record_config(pid)));
        assert_eq!(demux.configure_dvr_with_summary_result(record_dvr.dvr_id, dvr_config(DemuxPathDirection::Record)), Ok(()));
        assert_eq!(demux.attach_filter_to_dvr_result(record_dvr.dvr_id, record_filter.filter_id), Ok(()));
        assert!(demux.start_filter(record_filter.filter_id));
        assert_eq!(demux.start_dvr_result(record_dvr.dvr_id), Ok(()));

        assert!(demux.push_ts_packet(&packet));

        assert!(demux
            .drain_filter_payloads(record_filter.filter_id)
            .is_empty());
        assert_eq!(demux.drain_dvr_payloads(record_dvr.dvr_id), vec![packet.to_vec()]);
    }

    #[test]
    fn record_filter_index_event_payload_does_not_expose_ts_packet_as_fmq_bytes() {
        let pid = 0x0125;
        let mut config = record_config(pid);
        if let FilterConfigKind::Record { ts_index_mask, .. } = &mut config.kind {
            *ts_index_mask = 1;
        }
        let packet = ts_payload_packet(pid, 0, true, &[0x00, 0x80, 0x30, 0x03, 0x44, 0x55, 0x66]);
        let mut demux = DemuxHandle::new(0);
        let record_filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(record_filter.filter_id, config));
        assert!(demux.start_filter(record_filter.filter_id));

        assert!(demux.push_ts_packet(&packet));

        let payload = demux
            .pop_filter_payload_entry(record_filter.filter_id)
            .expect("record index metadata should be queued");
        assert!(payload.bytes().is_empty());
        assert_eq!(payload.event_bytes(), &packet[..]);
        assert_eq!(demux.current_filter_fill_bytes(record_filter.filter_id), Some(0));
    }

    #[test]
    fn record_filter_index_event_is_not_counted_as_fmq_payload_bytes() {
        let packet = ts_payload_packet(
            0x0125,
            0,
            true,
            &[0x00, 0x80, 0x30, 0x03, 0x44, 0x55, 0x66],
        );
        let payload = FilterPayload::RecordPacket(packet.to_vec());
        assert_eq!(payload.len(), 0);
        assert!(payload.bytes().is_empty());
        assert_eq!(payload.event_len(), TS_PACKET_SIZE);
        assert_eq!(payload.event_bytes(), &packet[..]);
    }

    #[test]
    fn zero_byte_record_events_are_limited_by_filter_queue_entry_count() {
        let mut demux = DemuxHandle::new(0);
        let record_filter = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TS_PACKET_SIZE as i32)
            .expect("test setup should register filter");
        let packet = ts_payload_packet(
            0x0125,
            0,
            true,
            &[0x00, 0x80, 0x30, 0x03, 0x44, 0x55, 0x66],
        );

        let first = demux.push_filter_payload(
            record_filter.filter_id,
            FilterPayload::RecordPacket(packet.to_vec()),
        );
        assert_eq!(first.dropped_entries, 0);
        assert!(!first.overflowed);
        assert_eq!(demux.current_filter_fill_bytes(record_filter.filter_id), Some(0));
        assert_eq!(demux.current_filter_queue_entries(record_filter.filter_id), Some(1));
        assert!(demux.has_filter_payload_ready(record_filter.filter_id));

        let second = demux.push_filter_payload(
            record_filter.filter_id,
            FilterPayload::RecordPacket(packet.to_vec()),
        );
        assert_eq!(second.dropped_entries, 1);
        assert!(second.dropped_new);
        assert!(second.overflowed);
        assert_eq!(demux.current_filter_fill_bytes(record_filter.filter_id), Some(0));
        let record = demux.filter_record(record_filter.filter_id).unwrap();
        assert!(record.pending_overflow);
        assert_eq!(record.overflow_events, 1);
        assert_eq!(demux.current_filter_queue_entries(record_filter.filter_id), Some(1));
    }

    #[test]
    fn filter_queue_model_matches_payload_discipline_and_overflow_policy() {
        let mut demux = DemuxHandle::new(0);
        let cases = [
            (
                FilterOpenType::TsRaw,
                raw_config(0x0125),
                FilterQueueDiscipline::PacketPassthrough,
                None,
            ),
            (
                FilterOpenType::TsSection,
                section_config(0x0125),
                FilterQueueDiscipline::SectionReassembled,
                None,
            ),
            (
                FilterOpenType::TsPes,
                pes_config(0x0125),
                FilterQueueDiscipline::PacketPassthrough,
                None,
            ),
            (
                FilterOpenType::TsAudio,
                av_config(0x0125),
                FilterQueueDiscipline::AvMediaEvent,
                None,
            ),
            (
                FilterOpenType::TsVideo,
                av_config(0x0125),
                FilterQueueDiscipline::AvMediaEvent,
                None,
            ),
            (
                FilterOpenType::TsRecord,
                record_config(0x0125),
                FilterQueueDiscipline::RecordEventMetadata,
                Some(2),
            ),
        ];
        for (open_type, config, discipline, bounded_entries) in cases {
            let filter = demux
                .register_filter_result(1, open_type, (TS_PACKET_SIZE * 2) as i32)
                .expect("test setup should register filter");
            assert!(demux.configure_filter_with_summary(filter.filter_id, config));
            let model = demux
                .filter_queue_model(filter.filter_id)
                .expect("filter queue model should exist");
            assert_eq!(model.discipline, discipline);
            assert_eq!(model.policy.bounded_entries, bounded_entries);
            let expected_policy = match discipline {
                FilterQueueDiscipline::RecordEventMetadata => {
                    QueueOverflowPolicy::MetadataEntryDropNew
                }
                _ if open_type.is_media() => QueueOverflowPolicy::DropOld,
                _ => QueueOverflowPolicy::DropNew,
            };
            assert_eq!(model.policy.overflow_policy, expected_policy);
            if matches!(discipline, FilterQueueDiscipline::RecordEventMetadata) {
                assert_eq!(model.policy.bounded_bytes, 0);
            } else {
                assert_eq!(model.policy.bounded_bytes, (TS_PACKET_SIZE * 2) as usize);
            }
        }
    }

    #[test]
    fn dvr_queue_model_distinguishes_record_output_and_playback_input_policy() {
        let mut demux = DemuxHandle::new(0);
        let record = demux
            .register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register record dvr");
        let playback = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register playback dvr");

        let record_model = demux
            .dvr_queue_model(record.dvr_id)
            .expect("record dvr model should exist");
        assert_eq!(record_model.queue_kind, QueueKind::DvrRecord);
        assert_eq!(record_model.discipline, DvrQueueDiscipline::PacketPassthrough);
        assert_eq!(record_model.policy.overflow_policy, QueueOverflowPolicy::DropNew);

        let playback_model = demux
            .dvr_queue_model(playback.dvr_id)
            .expect("playback dvr model should exist");
        assert_eq!(playback_model.queue_kind, QueueKind::DvrPlayback);
        assert_eq!(playback_model.discipline, DvrQueueDiscipline::PlaybackReinject);
        assert_eq!(
            playback_model.policy.overflow_policy,
            QueueOverflowPolicy::ProducerBackpressure
        );
    }

    #[test]
    fn filter_flush_generation_suppresses_stale_partial_section_without_breaking_peer_filter() {
        let pid = 0x0120;
        let section = vec![0x80, 0x30, 0x03, 0xaa, 0xbb, 0xcc];
        let mut demux = DemuxHandle::new(0);
        let flushed = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let peer = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(flushed.filter_id, section_config(pid)));
        assert!(demux.configure_filter_with_summary(peer.filter_id, section_config(pid)));
        assert!(demux.start_filter(flushed.filter_id));
        assert!(demux.start_filter(peer.filter_id));

        assert!(demux.push_ts_packet(&ts_payload_packet(
            pid,
            0,
            true,
            &[0x00, section[0], section[1]]
        )));
        assert!(demux.flush_filter(flushed.filter_id));
        assert!(demux.push_ts_packet(&ts_payload_packet(pid, 1, false, &section[2..])));

        assert!(demux.drain_filter_payloads(flushed.filter_id).is_empty());
        let peer_payloads = demux.drain_filter_payloads(peer.filter_id);
        assert_eq!(peer_payloads.len(), 1);
        assert_eq!(peer_payloads[0].bytes(), &section[..]);

        assert!(demux.push_ts_packet(&section_packet(pid, &section)));
        let flushed_payloads = demux.drain_filter_payloads(flushed.filter_id);
        assert_eq!(flushed_payloads.len(), 1);
        assert_eq!(flushed_payloads[0].bytes(), &section[..]);
    }

    #[test]
    fn link_caps_policy_advertises_ts_main_type_only() {
        let link_caps = demux_link_caps_for_filter_linkage_policy();
        assert_eq!(demux_link_caps_for_ts_filter_linkage(), link_caps);
        assert_eq!(link_caps.len(), DEMUX_FILTER_MAIN_TYPE_COUNT);
        assert_eq!(link_caps[0], DEMUX_FILTER_MAIN_TYPE_TS_BITS);
        assert!(link_caps[1..].iter().all(|bits| *bits == 0));
        for destination in TS_LINKABLE_OPEN_TYPES {
            assert!(can_link_filter_open_types(FilterOpenType::TsRaw, *destination));
            assert!(!can_link_filter_open_types(FilterOpenType::TsRecord, *destination));
        }
        assert!(!can_link_filter_open_types(
            FilterOpenType::TsSection,
            FilterOpenType::TsSection
        ));
        assert!(!can_link_filter_open_types(FilterOpenType::TsPes, FilterOpenType::TsPes));
        assert!(!can_link_filter_open_types(FilterOpenType::TsPes, FilterOpenType::TsVideo));
        assert!(!can_link_filter_open_types(FilterOpenType::TsPes, FilterOpenType::TsAudio));
        assert!(!can_link_filter_open_types(FilterOpenType::TsAudio, FilterOpenType::TsAudio));
        assert!(!can_link_filter_open_types(FilterOpenType::TsVideo, FilterOpenType::TsVideo));
        assert!(!can_link_filter_open_types(FilterOpenType::TsSection, FilterOpenType::TsRaw));
        assert!(!can_link_filter_open_types(FilterOpenType::TsAudio, FilterOpenType::TsRecord));
        assert!(!can_link_filter_open_types(
            FilterOpenType::NonTs,
            FilterOpenType::TsSection
        ));
        assert!(!can_link_filter_open_types(
            FilterOpenType::TsRaw,
            FilterOpenType::NonTs
        ));
    }

    #[test]
    fn set_filter_data_source_rejects_unadvertised_non_ts_linkage_without_mutating_graph() {
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(0, FilterOpenType::NonTs, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn downstream_flush_generation_suppresses_stale_linkage_section() {
        let pid = 0x0121;
        let section = vec![0x80, 0x30, 0x03, 0x11, 0x22, 0x33];
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsRaw, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let downstream = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(pid)));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, section_config(pid)));
        assert!(demux.start_filter(source.filter_id));
        assert!(demux.set_filter_data_source(downstream.filter_id, source.filter_id));
        assert!(demux.start_filter(downstream.filter_id));

        assert!(demux.push_ts_packet(&ts_payload_packet(
            pid,
            0,
            true,
            &[0x00, section[0], section[1]]
        )));
        assert!(demux.flush_filter(downstream.filter_id));
        assert!(demux.push_ts_packet(&ts_payload_packet(pid, 1, false, &section[2..])));

        assert_eq!(demux.drain_filter_payloads(source.filter_id).len(), 1);
        assert!(demux.drain_filter_payloads(downstream.filter_id).is_empty());
    }

    #[test]
    fn filter_flush_generation_suppresses_stale_partial_pes_and_av() {
        let pid = 0x0122;
        let payload = pes_payload(0x5a);
        let mut demux = DemuxHandle::new(0);
        let pes = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let av = demux
            .register_filter_result(1, FilterOpenType::TsVideo, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(pes.filter_id, pes_config(pid)));
        assert!(demux.configure_filter_with_summary(av.filter_id, av_config(pid)));
        assert!(demux.set_filter_av_stream_type_hint(
            av.filter_id,
            0xe0,
            AvFilterStreamKind::Video
        ));
        assert!(demux.start_filter(pes.filter_id));
        assert!(demux.start_filter(av.filter_id));

        assert!(demux.push_ts_packet(&ts_payload_packet(pid, 0, true, &payload[..5])));
        assert!(demux.flush_filter(pes.filter_id));
        assert!(demux.flush_filter(av.filter_id));
        assert!(demux.push_ts_packet(&ts_payload_packet(pid, 1, false, &payload[5..])));

        assert!(demux.drain_filter_payloads(pes.filter_id).is_empty());
        assert!(demux.drain_filter_payloads(av.filter_id).is_empty());

        assert!(demux.push_ts_packet(&pes_ts_packet(pid, 2, false, 0x6b)));
        assert_eq!(demux.drain_filter_payloads(pes.filter_id).len(), 1);
        assert_eq!(demux.drain_filter_payloads(av.filter_id).len(), 1);
    }

    #[test]
    fn playback_injection_accepts_only_consumer_aligned_packet_stream() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback)), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        let p0 = ts_packet(0x0100, 0x45);
        let p1 = ts_packet(0x0101, 0x46);
        let mut data = Vec::new();
        data.extend_from_slice(&p0);
        data.extend_from_slice(&p1);
        assert!(demux.inject_playback_payload(dvr.dvr_id, &data));
        assert_eq!(
            demux.playback_diagnostics(dvr.dvr_id),
            Some((2, (TS_PACKET_SIZE * 2) as u64, 0))
        );

        assert!(!demux.inject_playback_payload(dvr.dvr_id, &p0[..17]));
        assert_eq!(demux.flush_dvr_result(dvr.dvr_id), Ok(()));
        assert_eq!(demux.playback_diagnostics(dvr.dvr_id), Some((0, 0, 0)));
    }

    #[test]
    fn playback_no_payload_packet_not_counted_consumed() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback)), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = 0x01;
        packet[2] = 0x00;
        packet[3] = 0x20; // adaptation only, no payload
        packet[4] = 183;
        assert!(!demux.inject_playback_payload(dvr.dvr_id, &packet));
        assert_eq!(demux.playback_diagnostics(dvr.dvr_id), Some((0, 0, 0)));
    }

    #[test]
    fn playback_duplicate_packet_counted_dropped() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback)), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        let packet = ts_packet(0x0100, 0x45);
        let mut data = Vec::new();
        data.extend_from_slice(&packet);
        data.extend_from_slice(&packet);
        assert!(demux.inject_playback_payload(dvr.dvr_id, &data));
        assert_eq!(demux.playback_diagnostics(dvr.dvr_id), Some((1, TS_PACKET_SIZE as u64, 0)));
    }

    #[test]
    fn playback_malformed_packet_not_counted_consumed() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback)), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        let malformed = [0u8; TS_PACKET_SIZE];
        assert!(!demux.inject_playback_payload(dvr.dvr_id, &malformed));
        assert_eq!(
            demux.playback_diagnostics(dvr.dvr_id),
            Some((0, 0, TS_PACKET_SIZE as u64))
        );
    }

    #[test]
    fn playback_valid_packet_without_delivery_is_nonfatal_no_delivery() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback)), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));

        let packet = ts_packet(0x0100, 0x45);
        assert_eq!(
            demux.inject_playback_payload_result(dvr.dvr_id, &packet),
            PlaybackInjectionOutcome::ConsumedNoDelivery
        );
        assert_eq!(demux.playback_diagnostics(dvr.dvr_id), Some((0, 0, 0)));
    }

    #[test]
    fn playback_malformed_packet_is_nonfatal_malformed_result() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TEST_TS_PACKET_BUFFER_SIZE)
            .unwrap();
        assert_eq!(demux.configure_dvr_with_summary_result(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback)), Ok(()));
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));

        let malformed = [0u8; TS_PACKET_SIZE];
        assert_eq!(
            demux.inject_playback_payload_result(dvr.dvr_id, &malformed),
            PlaybackInjectionOutcome::Malformed
        );
        assert_eq!(
            demux.playback_diagnostics(dvr.dvr_id),
            Some((0, 0, TS_PACKET_SIZE as u64))
        );
    }

    #[test]
    fn set_filter_data_source_rejects_pid_mismatch_without_mutating_graph() {
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsRaw, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(0x0123)));
        assert!(demux.configure_filter_with_summary(destination.filter_id, section_config(0x0124)));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn set_filter_data_source_rejects_record_filter_as_source_without_mutating_graph() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsRecord, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(source.filter_id, record_config(pid)));
        assert!(demux.configure_filter_with_summary(destination.filter_id, section_config(pid)));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn set_filter_data_source_rejects_av_filter_as_source_without_mutating_graph() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsVideo, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(source.filter_id, av_config(pid)));
        assert!(demux.configure_filter_with_summary(destination.filter_id, pes_config(pid)));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            None
        );
    }

    fn pes_config_with(pid: u16, stream_id: i32, raw: bool) -> FilterConfig {
        FilterConfig {
            kind: FilterConfigKind::PesData { stream_id, raw },
            ..pes_config(pid)
        }
    }

    fn configured_pes_to_video_av_pair(
        demux: &mut DemuxHandle,
        pid: u16,
        stream_id: i32,
        raw: bool,
        set_av_stream_type: bool,
    ) -> (i32, i32) {
        let source = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsVideo, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(
            source.filter_id,
            pes_config_with(pid, stream_id, raw)
        ));
        assert!(demux.configure_filter_with_summary(destination.filter_id, av_config(pid)));
        if set_av_stream_type {
            assert!(demux.set_filter_av_stream_type_hint(
                destination.filter_id,
                0xe0,
                AvFilterStreamKind::Video
            ));
        }
        (source.filter_id, destination.filter_id)
    }

    #[test]
    fn set_filter_data_source_rejects_nonraw_video_pes_to_video_av() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let (source, destination) = configured_pes_to_video_av_pair(&mut demux, pid, 0xe0, false, true);
        assert_eq!(
            demux.set_filter_data_source_result(destination, source),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux.filter_record(destination).unwrap().data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn set_filter_data_source_rejects_pes_to_pes_even_when_raw_mode_matches() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let raw_source = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let raw_destination = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(raw_source.filter_id, pes_config_with(pid, 0xe0, true)));
        assert!(demux.configure_filter_with_summary(raw_destination.filter_id, pes_config_with(pid, 0xe0, true)));
        assert_eq!(
            demux.set_filter_data_source_result(raw_destination.filter_id, raw_source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );

        let nonraw_source = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let nonraw_destination = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(nonraw_source.filter_id, pes_config_with(pid, 0xe0, false)));
        assert!(demux.configure_filter_with_summary(nonraw_destination.filter_id, pes_config_with(pid, -1, false)));
        assert_eq!(
            demux.set_filter_data_source_result(nonraw_destination.filter_id, nonraw_source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );

        let wildcard_source = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let explicit_destination = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(wildcard_source.filter_id, pes_config_with(pid, -1, false)));
        assert!(demux.configure_filter_with_summary(explicit_destination.filter_id, pes_config_with(pid, 0xe0, false)));
        assert_eq!(
            demux.set_filter_data_source_result(explicit_destination.filter_id, wildcard_source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
    }

    #[test]
    fn set_filter_data_source_rejects_pes_to_pes_when_raw_mode_differs() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(source.filter_id, pes_config_with(pid, 0xe0, true)));
        assert!(demux.configure_filter_with_summary(destination.filter_id, pes_config_with(pid, 0xe0, false)));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux.filter_record(destination.filter_id).unwrap().data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn set_filter_data_source_rejects_pes_to_pes_when_explicit_stream_ids_differ() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(source.filter_id, pes_config_with(pid, 0xe0, false)));
        assert!(demux.configure_filter_with_summary(destination.filter_id, pes_config_with(pid, 0xc0, false)));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux.filter_record(destination.filter_id).unwrap().data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn pes_destination_payload_guard_rejects_raw_mode_mismatch() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(filter.filter_id, pes_config_with(pid, 0xe0, false)));
        let record = demux.filter_record(filter.filter_id).unwrap().clone();
        let metadata = AvPayloadMetadata {
            pts_90khz: Some(90_000),
            dts_90khz: None,
            stream_id: 0xe0,
        };
        let nonraw_video = FilterPayload::PesData {
            bytes: vec![1, 2, 3],
            stream_id: 0xe0,
            raw: false,
            metadata: metadata.clone(),
        };
        let raw_video = FilterPayload::PesData {
            bytes: vec![0, 0, 1, 0xe0, 1, 2, 3],
            stream_id: 0xe0,
            raw: true,
            metadata,
        };
        assert!(demux.payload_entry_matches_filter(&record, &nonraw_video));
        assert!(!demux.payload_entry_matches_filter(&record, &raw_video));
    }

    #[test]
    fn set_filter_data_source_rejects_nonraw_audio_pes_to_audio_av_without_configure_av_stream_type() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsAudio, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(source.filter_id, pes_config_with(pid, 0xc0, false)));
        assert!(demux.configure_filter_with_summary(destination.filter_id, av_config(pid)));
        assert_eq!(demux.filter_record(destination.filter_id).unwrap().av_stream_kind, None);
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux.filter_record(destination.filter_id).unwrap().data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn set_filter_data_source_rejects_raw_pes_to_video_av() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let (source, destination) = configured_pes_to_video_av_pair(&mut demux, pid, 0xe0, true, true);
        assert_eq!(
            demux.set_filter_data_source_result(destination, source),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux.filter_record(destination).unwrap().data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn set_filter_data_source_rejects_wildcard_pes_to_video_av() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let (source, destination) = configured_pes_to_video_av_pair(&mut demux, pid, -1, false, true);
        assert_eq!(
            demux.set_filter_data_source_result(destination, source),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux.filter_record(destination).unwrap().data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn set_filter_data_source_rejects_audio_pes_to_video_av() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let (source, destination) = configured_pes_to_video_av_pair(&mut demux, pid, 0xc0, false, true);
        assert_eq!(
            demux.set_filter_data_source_result(destination, source),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux.filter_record(destination).unwrap().data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn set_filter_data_source_rejects_pes_to_av_before_configure_av_stream_type() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let (source, destination) = configured_pes_to_video_av_pair(&mut demux, pid, 0xe0, false, false);
        assert_eq!(
            demux.set_filter_data_source_result(destination, source),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(
            demux.filter_record(destination).unwrap().data_upstream_filter_id,
            None
        );
    }

    #[test]
    fn av_destination_payload_guard_rejects_raw_or_wrong_stream_pes() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let av = demux
            .register_filter_result(1, FilterOpenType::TsVideo, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(av.filter_id, av_config(pid)));
        assert_eq!(demux.filter_record(av.filter_id).unwrap().av_stream_kind, None);
        let record = demux.filter_record(av.filter_id).unwrap().clone();
        let video_metadata = AvPayloadMetadata {
            pts_90khz: Some(90_000),
            dts_90khz: None,
            stream_id: 0xe0,
        };
        let audio_metadata = AvPayloadMetadata {
            pts_90khz: Some(90_000),
            dts_90khz: None,
            stream_id: 0xc0,
        };
        let nonraw_video = FilterPayload::PesData {
            bytes: vec![1, 2, 3],
            stream_id: 0xe0,
            raw: false,
            metadata: video_metadata.clone(),
        };
        let raw_video = FilterPayload::PesData {
            bytes: vec![0, 0, 1, 0xe0, 1, 2, 3],
            stream_id: 0xe0,
            raw: true,
            metadata: video_metadata,
        };
        let nonraw_audio = FilterPayload::PesData {
            bytes: vec![4, 5, 6],
            stream_id: 0xc0,
            raw: false,
            metadata: audio_metadata,
        };
        assert!(demux.payload_entry_matches_filter(&record, &nonraw_video));
        assert!(!demux.payload_entry_matches_filter(&record, &raw_video));
        assert!(!demux.payload_entry_matches_filter(&record, &nonraw_audio));
    }


    #[test]
    fn set_filter_data_source_rejects_self_cycle_cycle_and_started_rewire_without_mutating_graph() {
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsRaw, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsRaw, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert_eq!(
            demux.set_filter_data_source_result(source.filter_id, source.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(0x0123)));
        assert!(demux.configure_filter_with_summary(destination.filter_id, raw_config(0x0123)));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Ok(())
        );
        assert!(demux.start_filter(source.filter_id));
        assert!(demux.start_filter(destination.filter_id));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(
            demux
                .filter_record(destination.filter_id)
                .unwrap()
                .data_upstream_filter_id,
            Some(source.filter_id)
        );
    }

    #[test]
    fn set_filter_data_source_order_prioritizes_started_sink_before_source_state() {
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsRaw, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert!(demux.configure_filter_with_summary(destination.filter_id, section_config(0x0123)));
        assert_eq!(demux.start_filter_result(destination.filter_id), Ok(()));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(demux.filter_record(destination.filter_id).unwrap().data_upstream_filter_id, None);
    }

    #[test]
    fn set_filter_data_source_order_prioritizes_self_reference_before_configuration() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsRaw, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        assert_eq!(
            demux.set_filter_data_source_result(filter.filter_id, filter.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );
        assert_eq!(demux.filter_record(filter.filter_id).unwrap().data_upstream_filter_id, None);
    }

    #[test]
    fn set_filter_data_source_order_rejects_failed_source_before_pair_compatibility() {
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsVideo, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsPes, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("test setup should register filter");
        demux.filters
            .get_mut(&source.filter_id)
            .expect("source exists")
            .set_lifecycle(FilterLifecycleState::FailedClosed);
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(demux.filter_record(destination.filter_id).unwrap().data_upstream_filter_id, None);
    }

    #[test]
    fn closed_demux_rejects_new_filter_and_dvr_registration() {
        let mut demux = DemuxHandle::new(0);
        demux.close();
        assert_eq!(
            demux.register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(
            demux.register_dvr(DemuxPathDirection::Record, TEST_TS_PACKET_BUFFER_SIZE),
            Err(DemuxConfigError::InvalidState)
        );
    }
}

#[cfg(test)]
mod r50de_phase3_4_tests {
    use super::*;

    fn versioned_section(version: u8, section_number: u8) -> Vec<u8> {
        vec![
            0x00,
            0xb0,
            0x05,
            0x00,
            0x01,
            0xc0 | ((version & 0x1f) << 1) | 0x01,
            section_number,
            section_number,
        ]
    }

    fn non_repeat_section_config(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: false,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    #[test]
    fn section_bits_repeat_false_delivers_first_match_then_stops_even_new_version() {
        let mut demux = DemuxHandle::new(1);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        assert!(demux.configure_filter_with_summary(
            filter.filter_id,
            non_repeat_section_config(0x0000),
        ));
        let v1 = versioned_section(1, 0);
        let v1_dup = versioned_section(1, 0);
        let v2 = versioned_section(2, 0);
        assert!(demux.filter_accepts_section(filter.filter_id, 0x0000, &v1));
        assert!(!demux.filter_accepts_section(filter.filter_id, 0x0000, &v1_dup));
        assert!(!demux.filter_accepts_section(filter.filter_id, 0x0000, &v2));
    }

    #[test]
    fn section_bits_repeat_true_repeats_matching_sections() {
        let mut demux = DemuxHandle::new(1);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        let mut config = non_repeat_section_config(0x0000);
        if let FilterConfigKind::Section { repeat, .. } = &mut config.kind {
            *repeat = true;
        }
        assert!(demux.configure_filter_with_summary(filter.filter_id, config));
        let v1 = versioned_section(1, 0);
        let v1_dup = versioned_section(1, 0);
        let v2 = versioned_section(2, 0);
        assert!(demux.filter_accepts_section(filter.filter_id, 0x0000, &v1));
        assert!(demux.filter_accepts_section(filter.filter_id, 0x0000, &v1_dup));
        assert!(demux.filter_accepts_section(filter.filter_id, 0x0000, &v2));
    }

    fn non_repeat_table_info_config(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: false,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::TableInfo,
                condition: SectionCondition::default(),
            },
        }
    }

    fn versioned_table_section(version: u8, section_number: u8, last_section_number: u8) -> Vec<u8> {
        vec![
            0x42,
            0xb0,
            0x05,
            0x00,
            0x01,
            0xc0 | ((version & 0x1f) << 1) | 0x01,
            section_number,
            last_section_number,
        ]
    }

    #[test]
    fn table_info_repeat_false_delivers_until_table_complete_then_stops() {
        let mut demux = DemuxHandle::new(1);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        assert!(demux.configure_filter_with_summary(
            filter.filter_id,
            non_repeat_table_info_config(0x0011),
        ));
        let section0 = versioned_table_section(1, 0, 1);
        let section1 = versioned_table_section(1, 1, 1);
        let section0_dup = versioned_table_section(1, 0, 1);
        let next_version = versioned_table_section(2, 0, 0);
        assert!(demux.filter_accepts_section(filter.filter_id, 0x0011, &section0));
        assert!(!demux.filter_accepts_section(filter.filter_id, 0x0011, &section0_dup));
        assert!(demux.filter_accepts_section(filter.filter_id, 0x0011, &section1));
        assert!(!demux.filter_accepts_section(filter.filter_id, 0x0011, &next_version));
    }

    #[test]
    fn table_info_repeat_false_wildcard_latches_first_version_until_complete() {
        let mut demux = DemuxHandle::new(1);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        assert!(demux.configure_filter_with_summary(
            filter.filter_id,
            non_repeat_table_info_config(0x0011),
        ));
        let v1_section0 = versioned_table_section(1, 0, 1);
        let v2_section0 = versioned_table_section(2, 0, 0);
        let v1_section1 = versioned_table_section(1, 1, 1);
        assert!(demux.filter_accepts_section(filter.filter_id, 0x0011, &v1_section0));
        assert!(!demux.filter_accepts_section(filter.filter_id, 0x0011, &v2_section0));
        assert!(demux.filter_accepts_section(filter.filter_id, 0x0011, &v1_section1));
    }


    #[test]
    fn downstream_section_filter_repeat_false_uses_runtime_gate() {
        let mut demux = DemuxHandle::new(1);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        let downstream = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        let mut source_config = non_repeat_section_config(0x0000);
        if let FilterConfigKind::Section { repeat, .. } = &mut source_config.kind {
            *repeat = true;
        }
        assert!(demux.configure_filter_with_summary(source.filter_id, source_config));
        assert!(demux.configure_filter_with_summary(
            downstream.filter_id,
            non_repeat_section_config(0x0000),
        ));
        assert!(demux.start_filter(source.filter_id));
        assert!(demux.set_filter_data_source(downstream.filter_id, source.filter_id));
        assert!(demux.start_filter(downstream.filter_id));

        assert!(demux.inject_payload(source.filter_id, &versioned_section(1, 0)));
        assert!(demux.inject_payload(source.filter_id, &versioned_section(2, 0)));
        assert_eq!(demux.current_filter_queue_entries(source.filter_id), Some(2));
        assert_eq!(demux.current_filter_queue_entries(downstream.filter_id), Some(1));
    }

    #[test]
    fn start_id_is_zero_on_first_start_and_nonzero_after_restart_boundary() {
        let mut demux = DemuxHandle::new(1);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        assert!(demux.configure_filter_with_summary(
            filter.filter_id,
            non_repeat_section_config(0x0000),
        ));
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.set_filter_start_event_pending(filter.filter_id, true));
        assert_eq!(demux.take_filter_start_event_id_if_ready(filter.filter_id), Some(0));
        assert!(demux.stop_filter(filter.filter_id));
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.set_filter_start_event_pending(filter.filter_id, true));
        let restart_id = demux.take_filter_start_event_id_if_ready(filter.filter_id);
        assert!(restart_id.is_some_and(|id| id > 0));
    }
}

#[cfg(all(test, loom))]
mod loom_state_transition_tests {
    use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use loom::sync::{Arc, Mutex};
    use loom::thread;

    #[test]
    fn filter_stop_preserves_queue_unless_flush_wins() {
        loom::model(|| {
            let queue = Arc::new(Mutex::new(vec![0x47u8]));
            let stopped = Arc::new(AtomicBool::new(false));
            let flushed = Arc::new(AtomicBool::new(false));

            let stop_queue = Arc::clone(&queue);
            let stop_flag = Arc::clone(&stopped);
            let stop_thread = thread::spawn(move || {
                stop_flag.store(true, Ordering::SeqCst);
                assert_eq!(crate::packet_pipeline::lock_test_mutex(&stop_queue).len(), 1);
            });

            let flush_queue = Arc::clone(&queue);
            let flush_flag = Arc::clone(&flushed);
            let flush_thread = thread::spawn(move || {
                flush_flag.store(true, Ordering::SeqCst);
                crate::packet_pipeline::lock_test_mutex(&flush_queue).clear();
            });

            stop_thread.join().unwrap();
            flush_thread.join().unwrap();
            if crate::packet_pipeline::lock_test_mutex(&queue).is_empty() {
                assert!(flushed.load(Ordering::SeqCst));
            }
        });
    }

    #[test]
    fn source_generation_change_blocks_new_delivery_without_clearing_existing_queue() {
        loom::model(|| {
            let queue = Arc::new(Mutex::new(vec![188usize]));
            let source_generation = Arc::new(AtomicUsize::new(1));
            let registered_generation = 1usize;

            let generation_for_close = Arc::clone(&source_generation);
            let close_thread = thread::spawn(move || {
                generation_for_close.fetch_add(1, Ordering::SeqCst);
            });

            let queue_for_delivery = Arc::clone(&queue);
            let generation_for_delivery = Arc::clone(&source_generation);
            let delivery_thread = thread::spawn(move || {
                if generation_for_delivery.load(Ordering::SeqCst) == registered_generation {
                    crate::packet_pipeline::lock_test_mutex(&queue_for_delivery).push(188);
                }
            });

            close_thread.join().unwrap();
            delivery_thread.join().unwrap();
            let len = crate::packet_pipeline::lock_test_mutex(&queue).len();
            assert!(len == 1 || len == 2);
        });
    }

    #[test]
    fn dvr_stop_preserves_playback_input_until_flush() {
        loom::model(|| {
            let playback_input_bytes = Arc::new(AtomicUsize::new(188));
            let stopped = Arc::new(AtomicBool::new(false));
            let flushed = Arc::new(AtomicBool::new(false));

            let bytes_for_stop = Arc::clone(&playback_input_bytes);
            let stopped_flag = Arc::clone(&stopped);
            let stop_thread = thread::spawn(move || {
                stopped_flag.store(true, Ordering::SeqCst);
                assert_eq!(bytes_for_stop.load(Ordering::SeqCst), 188);
            });

            let bytes_for_flush = Arc::clone(&playback_input_bytes);
            let flushed_flag = Arc::clone(&flushed);
            let flush_thread = thread::spawn(move || {
                flushed_flag.store(true, Ordering::SeqCst);
                bytes_for_flush.store(0, Ordering::SeqCst);
            });

            stop_thread.join().unwrap();
            flush_thread.join().unwrap();
            if playback_input_bytes.load(Ordering::SeqCst) == 0 {
                assert!(flushed.load(Ordering::SeqCst));
            }
        });
    }
}

#[cfg(test)]
mod r50dz40_wp03_datasource_txn_tests {
    use super::*;

    #[test]
    fn data_source_snapshot_restore_keeps_previous_upstream_on_failed_transaction() {
        let mut demux = DemuxHandle::new(0);
        let source = demux
            .register_filter_result(1, FilterOpenType::TsRaw, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("source registration");
        let destination = demux
            .register_filter_result(1, FilterOpenType::TsSection, TEST_TS_PACKET_BUFFER_SIZE)
            .expect("destination registration");
        assert_eq!(demux.filter_data_source_snapshot(destination.filter_id), Ok(None));
        demux.restore_filter_data_source_snapshot(destination.filter_id, Some(source.filter_id))
            .expect("snapshot restore should be an atomic rollback primitive");
        assert_eq!(
            demux.filter_record(destination.filter_id).unwrap().data_upstream_filter_id,
            Some(source.filter_id)
        );
        demux.restore_filter_data_source_snapshot(destination.filter_id, None)
            .expect("snapshot restore should be reversible");
        assert_eq!(
            demux.filter_record(destination.filter_id).unwrap().data_upstream_filter_id,
            None
        );
    }
}


#[cfg(test)]
mod r50dz52_g1_18_tests {
    use super::*;

    fn section_config(pid: u16, raw: bool, table_id: Option<i32>) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: true,
                repeat: true,
                raw,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition {
                    table_id,
                    ..SectionCondition::default()
                },
            },
        }
    }

    fn complete_private_section(table_id: u8) -> Vec<u8> {
        vec![table_id, 0x30, 0x05, 0xaa, 0xbb, 0xcc, 0xdd, 0xee]
    }

    #[test]
    fn raw_section_ignores_condition_only_after_complete_section_boundary() {
        let pid = 0x0123u16;
        let section = complete_private_section(0x42);

        let mut raw_demux = DemuxHandle::new(0);
        let raw_filter = raw_demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        assert!(raw_demux.configure_filter_with_summary(
            raw_filter.filter_id,
            section_config(pid, true, Some(0x99)),
        ));
        assert!(raw_demux.filter_accepts_section(raw_filter.filter_id, pid as i32, &section));
        assert!(!raw_demux.filter_accepts_section(raw_filter.filter_id, pid as i32, &section[..4]));

        let mut nonraw_demux = DemuxHandle::new(0);
        let nonraw_filter = nonraw_demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .unwrap();
        assert!(nonraw_demux.configure_filter_with_summary(
            nonraw_filter.filter_id,
            section_config(pid, false, Some(0x99)),
        ));
        assert!(!nonraw_demux.filter_accepts_section(nonraw_filter.filter_id, pid as i32, &section));
    }
}

#[cfg(test)]
mod raw_section_contract_tests {
    use super::{DemuxHandle, FilterConfig, FilterConfigKind, FilterOpenType, SectionCondition, SectionConditionKind};
    use maleicacid_tuner_hal_common::TS_PACKET_SIZE;

    fn section_config(pid: u16, raw: bool) -> FilterConfig {
        FilterConfig {
            tpid: pid as i32,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: true,
                raw,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn ts_payload_packet(pid: u16, payload_unit_start: bool, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if payload_unit_start {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        packet[3] = 0x10;
        packet[4..4 + payload.len()].copy_from_slice(payload);
        packet
    }

    #[test]
    fn c04_c16_raw_section_rejects_unparseable_payload_without_section_header() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("filter registration");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(pid, true)));
        assert!(demux.start_filter(filter.filter_id));
        let payload = [0x01, 0x02];
        let packet = ts_payload_packet(pid, true, &payload);
        assert!(demux.push_ts_packet(&packet));
        assert!(demux.drain_filter_payloads(filter.filter_id).is_empty());
    }

    #[test]
    fn c04_c16_nonraw_section_still_requires_parseable_section_header() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(0);
        let filter = demux
            .register_filter_result(1, FilterOpenType::TsSection, 4096)
            .expect("filter registration");
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(pid, false)));
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.push_ts_packet(&ts_payload_packet(pid, true, &[0x01, 0x02])));
        assert!(demux.drain_filter_payloads(filter.filter_id).is_empty());
    }
}


#[cfg(test)]
mod r50ea26_source_filter_boundary_tests {
    use super::*;

    fn raw_config(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Noinit,
        }
    }

    fn non_repeat_section_config(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: false,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn pes_config(pid: i32, stream_id: i32, raw: bool) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::PesData { stream_id, raw },
        }
    }

    fn ts_payload_packet(pid: u16, cc: u8, pusi: bool, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if pusi {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        packet[3] = 0x10 | (cc & 0x0f);
        packet[4..4 + payload.len()].copy_from_slice(payload);
        packet
    }

    #[test]
    fn set_data_source_rejects_section_payload_direct_chain_as_unavailable() {
        let mut demux = DemuxHandle::new(1);
        let source = demux.register_filter_result(1, FilterOpenType::TsSection, 4096).unwrap();
        let destination = demux.register_filter_result(2, FilterOpenType::TsSection, 4096).unwrap();
        assert!(demux.configure_filter_with_summary(source.filter_id, non_repeat_section_config(0x0100)));
        assert!(demux.configure_filter_with_summary(destination.filter_id, non_repeat_section_config(0x0100)));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
    }

    #[test]
    fn set_data_source_rejects_pes_payload_direct_chain_as_unavailable() {
        let mut demux = DemuxHandle::new(1);
        let source = demux.register_filter_result(1, FilterOpenType::TsPes, 4096).unwrap();
        let destination = demux.register_filter_result(2, FilterOpenType::TsPes, 4096).unwrap();
        assert!(demux.configure_filter_with_summary(source.filter_id, pes_config(0x0100, 0xe0, false)));
        assert!(demux.configure_filter_with_summary(destination.filter_id, pes_config(0x0100, 0xe0, false)));
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
    }

    #[test]
    fn source_filter_origin_generation_separates_new_source_epoch() {
        let mut demux = DemuxHandle::new(1);
        let source = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        let downstream = demux.register_filter_result(2, FilterOpenType::TsRaw, 4096).unwrap();
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(0x0100)));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, raw_config(0x0100)));
        assert!(demux.start_filter(source.filter_id));
        assert!(demux.set_filter_data_source(downstream.filter_id, source.filter_id));
        assert!(demux.start_filter(downstream.filter_id));

        assert!(demux.push_ts_packet(&ts_payload_packet(0x0100, 0, true, &[0x01, 0x02])));
        assert_eq!(demux.current_filter_queue_entries(downstream.filter_id), Some(1));
        assert!(demux.flush_filter(source.filter_id));
        assert!(demux.push_ts_packet(&ts_payload_packet(0x0100, 0, true, &[0x03, 0x04])));
        assert_eq!(demux.current_filter_queue_entries(downstream.filter_id), Some(2));
    }
}

#[cfg(test)]
mod r50ea48_source_filter_state_ownership_tests {
    use super::*;

    fn raw_config(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Noinit,
        }
    }

    fn record_config(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Record {
                ts_index_mask: 0,
                sc_index_type: 0,
                sc_index_mask_bits: 0,
            },
        }
    }

    fn section_config(pid: i32) -> FilterConfig {
        FilterConfig {
            tpid: pid,
            main_type_bits: 1,
            sub_type_hint: 0,
            kind: FilterConfigKind::Section {
                check_crc: false,
                repeat: false,
                raw: false,
                length_field_bits: 12,
                condition_kind: SectionConditionKind::SectionBits,
                condition: SectionCondition::default(),
            },
        }
    }

    fn ts_packet(pid: u16, cc: u8, pusi: bool, payload: &[u8]) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if pusi {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        packet[3] = 0x10 | (cc & 0x0f);
        packet[4..4 + payload.len()].copy_from_slice(payload);
        packet
    }

    #[test]
    fn source_filter_without_downstream_does_not_advance_origin_continuity() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(1);
        let source = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        let downstream = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(pid)));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, raw_config(pid)));
        assert!(demux.start_filter(source.filter_id));

        let first = ts_packet(pid as u16, 0, true, &[0x01, 0x02]);
        demux.propagate_filter_output(source.filter_id, &FilterPayload::TsPacket(first.to_vec()));
        assert!(demux.drain_filter_payloads(downstream.filter_id).is_empty());

        assert_eq!(demux.set_filter_data_source_result(downstream.filter_id, source.filter_id), Ok(()));
        assert!(demux.start_filter(downstream.filter_id));
        let second = ts_packet(pid as u16, 0, true, &[0x03, 0x04]);
        demux.propagate_filter_output(source.filter_id, &FilterPayload::TsPacket(second.to_vec()));
        assert_eq!(demux.current_filter_queue_entries(downstream.filter_id), Some(1));
    }

    #[test]
    fn source_filter_raw_to_record_is_supported() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(1);
        let source = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        let record = demux.register_filter_result(1, FilterOpenType::TsRecord, 4096).unwrap();
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(pid)));
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(pid)));
        assert_eq!(demux.set_filter_data_source_result(record.filter_id, source.filter_id), Ok(()));
        assert!(demux.start_filter(source.filter_id));
        assert!(demux.start_filter(record.filter_id));
        assert!(demux.push_ts_packet(&ts_packet(pid as u16, 0, true, &[0x05, 0x06])));
        assert_eq!(demux.current_filter_queue_entries(record.filter_id), Some(1));
    }

    #[test]
    fn source_filter_raw_to_section_is_unavailable() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(1);
        let source = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        let section = demux.register_filter_result(1, FilterOpenType::TsSection, 4096).unwrap();
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(pid)));
        assert!(demux.configure_filter_with_summary(section.filter_id, section_config(pid)));
        assert_eq!(
            demux.set_filter_data_source_result(section.filter_id, source.filter_id),
            Err(DemuxConfigError::Unavailable)
        );
        assert_eq!(demux.filter_record(section.filter_id).unwrap().data_upstream_filter_id, None);
    }

    #[test]
    fn source_filter_flush_preserves_record_downstream_and_resets_origin_state() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(1);
        let source = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        let record = demux.register_filter_result(1, FilterOpenType::TsRecord, 4096).unwrap();
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(pid)));
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(pid)));
        assert_eq!(demux.set_filter_data_source_result(record.filter_id, source.filter_id), Ok(()));
        assert!(demux.start_filter(source.filter_id));
        assert!(demux.start_filter(record.filter_id));

        let first = ts_packet(pid as u16, 0, true, &[0x01, 0x02]);
        demux.propagate_filter_output(source.filter_id, &FilterPayload::TsPacket(first.to_vec()));
        assert_eq!(demux.current_filter_queue_entries(record.filter_id), Some(1));

        assert!(demux.flush_filter(source.filter_id));
        assert_eq!(demux.filter_record(record.filter_id).unwrap().data_upstream_filter_id, Some(source.filter_id));

        // source flush advances the SourceFilter origin generation and resets the old origin state.
        // The same continuity counter must therefore be accepted in the new source epoch.
        let second = ts_packet(pid as u16, 0, true, &[0x03, 0x04]);
        demux.propagate_filter_output(source.filter_id, &FilterPayload::TsPacket(second.to_vec()));
        assert_eq!(demux.current_filter_queue_entries(record.filter_id), Some(2));
    }

    #[test]
    fn source_filter_reconfigure_disconnects_downstream() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(1);
        let source = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        let downstream = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(pid)));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, raw_config(pid)));
        assert_eq!(demux.set_filter_data_source_result(downstream.filter_id, source.filter_id), Ok(()));
        assert_eq!(demux.filter_record(downstream.filter_id).unwrap().data_upstream_filter_id, Some(source.filter_id));
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(pid + 1)));
        assert_eq!(demux.filter_record(downstream.filter_id).unwrap().data_upstream_filter_id, None);
    }

    #[test]
    fn source_filter_close_disconnects_downstream() {
        let pid = 0x0123;
        let mut demux = DemuxHandle::new(1);
        let source = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        let downstream = demux.register_filter_result(1, FilterOpenType::TsRaw, 4096).unwrap();
        assert!(demux.configure_filter_with_summary(source.filter_id, raw_config(pid)));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, raw_config(pid)));
        assert_eq!(demux.set_filter_data_source_result(downstream.filter_id, source.filter_id), Ok(()));
        assert!(demux.unregister_filter(source.filter_id).is_some());
        assert_eq!(demux.filter_record(downstream.filter_id).unwrap().data_upstream_filter_id, None);
    }
}
