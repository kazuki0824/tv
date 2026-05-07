pub mod sections;
pub mod ts_core;

use crate::sections::{parse_section_header, section_crc_valid, SectionAssembler};
use crate::ts_core::{
    ContinuityOutcome, ContinuityTracker, PesAssembler, PesPacket, TsPacketResyncBuffer,
};
use maleicacid_tuner_hal_common::{
    DEMUX_MAX_AUDIO_FILTERS, DEMUX_MAX_FILTERS_PER_DEMUX, DEMUX_MAX_PES_FILTERS,
    DEMUX_MAX_RECORD_FILTERS, DEMUX_MAX_SECTION_FILTERS, DEMUX_MAX_VIDEO_FILTERS,
    MAX_SECTION_FILTER_BYTES, MAX_SECTION_PAYLOAD_BYTES, TS_PACKET_SIZE,
};
use maleicacid_tuner_hal_dvr::{
    DvrQueueDiscipline, DvrQueueModel, FilterQueueDiscipline, FilterQueueModel, QueueKind,
    QueuePolicy,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxPathDirection {
    Record,
    Playback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum TsInputOrigin {
    Frontend,
    Playback,
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
            FilterOpenType::TsSection => matches!(kind, FilterConfigKind::Section { .. }),
            FilterOpenType::TsPes => matches!(kind, FilterConfigKind::PesData { .. }),
            FilterOpenType::TsRecord => matches!(kind, FilterConfigKind::Record { .. }),
            FilterOpenType::TsOther | FilterOpenType::NonTs => false,
        }
    }
}

pub const DEMUX_FILTER_MAIN_TYPE_COUNT: usize = 5;
pub const DEMUX_FILTER_MAIN_TYPE_TS_BITS: i32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterLinkagePolicyEntry {
    source_main_type_index: usize,
    source_main_type_bits: i32,
    destination_main_type_bits: i32,
    open_types: &'static [FilterOpenType],
}

const TS_LINKABLE_OPEN_TYPES: &[FilterOpenType] = &[
    FilterOpenType::TsAudio,
    FilterOpenType::TsVideo,
    FilterOpenType::TsSection,
    FilterOpenType::TsPes,
    FilterOpenType::TsRecord,
];

pub const FILTER_LINKAGE_POLICY: &[FilterLinkagePolicyEntry] = &[FilterLinkagePolicyEntry {
    source_main_type_index: 0,
    source_main_type_bits: DEMUX_FILTER_MAIN_TYPE_TS_BITS,
    destination_main_type_bits: DEMUX_FILTER_MAIN_TYPE_TS_BITS,
    open_types: TS_LINKABLE_OPEN_TYPES,
}];

fn linkage_policy_for_open_type(
    open_type: FilterOpenType,
) -> Option<&'static FilterLinkagePolicyEntry> {
    FILTER_LINKAGE_POLICY
        .iter()
        .find(|entry| entry.open_types.contains(&open_type))
}

pub fn can_link_filter_open_types(source: FilterOpenType, destination: FilterOpenType) -> bool {
    let Some(source_entry) = linkage_policy_for_open_type(source) else {
        return false;
    };
    let Some(destination_entry) = linkage_policy_for_open_type(destination) else {
        return false;
    };
    if source_entry.source_main_type_bits == 0 {
        return false;
    }
    (demux_link_caps_for_filter_linkage_policy()[source_entry.source_main_type_index]
        & destination_entry.source_main_type_bits)
        != 0
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
        self.time_delay_ms.unwrap_or(0) > 0 || self.data_size_delay_bytes.unwrap_or(0) > 0
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
    }

    pub fn matches(&self, payload: &[u8]) -> bool {
        let Some(header) = parse_section_header(payload, 12) else {
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
        let width = self
            .filter_bytes
            .len()
            .max(self.mask_bytes.len())
            .max(self.mode_bytes.len());
        if width > payload.len() {
            return false;
        }
        for index in 0..width {
            let filter = *self.filter_bytes.get(index).unwrap_or(&0);
            let mask = *self.mask_bytes.get(index).unwrap_or(&0x00);
            let mode = *self.mode_bytes.get(index).unwrap_or(&0x00);
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemuxFilterRecord {
    pub filter_id: i32,
    pub filter_type_bits: i32,
    pub open_type: FilterOpenType,
    pub buffer_size: i32,
    pub configured: bool,
    pub started: bool,
    pub monitor_event_mask: i32,
    pub ip_cid: Option<i32>,
    pub data_source_filter_id: Option<i32>,
    pub pending_start_event: bool,
    pub delay_hints: FilterDelayHints,
    pub delivery_not_before: Option<Instant>,
    pub av_stream_type_hint: Option<i32>,
    pub av_stream_kind: Option<AvFilterStreamKind>,
    pub config: Option<FilterConfig>,
    pub queued_bytes: usize,
    pub pending_overflow: bool,
    pub overflow_events: u64,
    pub drop_bytes: u64,
    pub events_emitted: u64,
    pub delivery_generation: u64,
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
            configured: false,
            started: false,
            monitor_event_mask: 0,
            ip_cid: None,
            data_source_filter_id: None,
            pending_start_event: false,
            delay_hints: FilterDelayHints::default(),
            delivery_not_before: None,
            av_stream_type_hint: None,
            av_stream_kind: None,
            config: None,
            queued_bytes: 0,
            pending_overflow: false,
            overflow_events: 0,
            drop_bytes: 0,
            events_emitted: 0,
            delivery_generation: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemuxDvrRecord {
    pub dvr_id: i32,
    pub direction: DemuxPathDirection,
    pub buffer_size: i32,
    pub configured: bool,
    pub started: bool,
    pub status_check_interval_hint_ms: i64,
    pub attached_filter_ids: Vec<i32>,
    pub config: Option<DvrConfig>,
    pub queued_bytes: usize,
    pub pending_overflow: bool,
    pub overflow_events: u64,
    pub drop_bytes: u64,
    pub playback_injected_packets: u64,
    pub playback_injected_bytes: u64,
    pub playback_malformed_bytes: u64,
}

impl DemuxDvrRecord {
    fn new(dvr_id: i32, direction: DemuxPathDirection, buffer_size: i32) -> Self {
        Self {
            dvr_id,
            direction,
            buffer_size,
            configured: false,
            started: false,
            status_check_interval_hint_ms: 25,
            attached_filter_ids: Vec::new(),
            config: None,
            queued_bytes: 0,
            pending_overflow: false,
            overflow_events: 0,
            drop_bytes: 0,
            playback_injected_packets: 0,
            playback_injected_bytes: 0,
            playback_malformed_bytes: 0,
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemuxConfigError {
    NotFound,
    CapacityExceeded,
    InvalidKind,
    InvalidState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QueuePushOutcome {
    pub accepted_bytes: usize,
    pub dropped_bytes: usize,
    pub dropped_entries: usize,
    pub dropped_old: bool,
    pub dropped_new: bool,
    pub overflowed: bool,
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
    PesData {
        bytes: Vec<u8>,
        stream_id: i32,
        raw: bool,
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
            Self::PesData { bytes, .. } => bytes.as_slice(),
            Self::AvEs { bytes, .. } => bytes.as_slice(),
        }
    }

    pub fn len(&self) -> usize {
        self.bytes().len()
    }

    pub fn av_metadata(&self) -> Option<&AvPayloadMetadata> {
        match self {
            Self::AvEs { metadata, .. } => Some(metadata),
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
            Self::PesData { bytes, .. } => bytes,
            Self::AvEs { bytes, .. } => bytes,
        }
    }
}

#[derive(Debug, Default)]
pub struct DemuxCore;

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
    seen_bytes: BTreeSet<Vec<u8>>,
    table_progress: BTreeMap<(u8, u16, u8), SectionTableProgress>,
    finished: bool,
}

const AV_SYNC_33BIT_MODULUS: i64 = 1i64 << 33;
const AV_SYNC_33BIT_HALF_RANGE: i64 = 1i64 << 32;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AvSyncTimestampExtender {
    last_raw: Option<u64>,
    epoch: i64,
}

impl AvSyncTimestampExtender {
    fn update(&mut self, raw_33bit: u64) -> i64 {
        let raw = (raw_33bit & ((1u64 << 33) - 1)) as i64;
        if let Some(last_raw) = self.last_raw {
            let last = (last_raw & ((1u64 << 33) - 1)) as i64;
            let diff = raw - last;
            if diff < -AV_SYNC_33BIT_HALF_RANGE {
                self.epoch = self.epoch.saturating_add(AV_SYNC_33BIT_MODULUS);
            } else if diff > AV_SYNC_33BIT_HALF_RANGE {
                self.epoch = self.epoch.saturating_sub(AV_SYNC_33BIT_MODULUS);
            }
        }
        self.last_raw = Some(raw as u64);
        self.epoch.saturating_add(raw)
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
    Section,
    Audio,
    Video,
    Pes,
    Record,
    AvAny,
    Other,
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
    section_assemblers: BTreeMap<(TsInputOrigin, i32), SectionAssembler>,
    pes_assemblers: BTreeMap<(TsInputOrigin, i32), PesAssembler>,
    section_assembler_generations: BTreeMap<(TsInputOrigin, i32), u64>,
    pes_assembler_generations: BTreeMap<(TsInputOrigin, i32), u64>,
    filter_section_flush_generations: BTreeMap<(TsInputOrigin, i32, i32), u64>,
    filter_pes_flush_generations: BTreeMap<(TsInputOrigin, i32, i32), u64>,
    continuity_trackers: BTreeMap<TsInputOrigin, ContinuityTracker>,
    latest_pcr: Option<u64>,
    latest_pcr_instant: Option<Instant>,
    pcr_extender: AvSyncTimestampExtender,
    latest_pcr_90khz: Option<i64>,
    av_sync_states: BTreeMap<i32, AvSyncState>,
    resync: TsPacketResyncBuffer,
    next_filter_id: i32,
    next_dvr_id: i32,
    closed: bool,
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
            section_assemblers: BTreeMap::new(),
            pes_assemblers: BTreeMap::new(),
            section_assembler_generations: BTreeMap::new(),
            pes_assembler_generations: BTreeMap::new(),
            filter_section_flush_generations: BTreeMap::new(),
            filter_pes_flush_generations: BTreeMap::new(),
            continuity_trackers: BTreeMap::new(),
            latest_pcr: None,
            latest_pcr_instant: None,
            pcr_extender: AvSyncTimestampExtender::default(),
            latest_pcr_90khz: None,
            av_sync_states: BTreeMap::new(),
            resync: TsPacketResyncBuffer::default(),
            next_filter_id: 0,
            next_dvr_id: 0,
            closed: false,
        }
    }

    pub fn demux_id(&self) -> i32 {
        self.demux_id
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
    pub fn connect_ci_cam(&mut self, _ci_cam_id: i32) { /* CI CAM is unsupported in product HAL; state is intentionally not saved. */
    }
    pub fn disconnect_ci_cam(&mut self) { /* CI CAM is unsupported in product HAL; state is intentionally not saved. */
    }
    pub fn clear_output_pid_blocks(&mut self) {
        self.drop_all_pes_assemblers();
    }

    pub fn reset_for_stream_boundary(&mut self) {
        self.drop_all_pes_assemblers();
        self.filter_queues.clear();
        self.section_filter_runtime.clear();
        let filter_ids: Vec<i32> = self.filters.keys().copied().collect();
        for filter_id in filter_ids {
            if let Some(filter) = self.filters.get_mut(&filter_id) {
                filter.queued_bytes = 0;
                filter.pending_start_event = false;
                filter.pending_overflow = false;
                filter.delivery_generation = filter.delivery_generation.saturating_add(1);
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
        self.section_assemblers.clear();
        self.pes_assemblers.clear();
        self.section_assembler_generations.clear();
        self.pes_assembler_generations.clear();
        self.filter_section_flush_generations.clear();
        self.filter_pes_flush_generations.clear();
        self.continuity_trackers.clear();
        self.latest_pcr = None;
        self.latest_pcr_instant = None;
        self.pcr_extender.reset();
        self.latest_pcr_90khz = None;
        self.resync = TsPacketResyncBuffer::default();
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
        self.next_filter_id += 1;
        let record = DemuxFilterRecord::new(filter_id, filter_type_bits, open_type, buffer_size);
        self.filters.insert(filter_id, record.clone());
        self.filter_queues.insert(filter_id, VecDeque::new());
        self.section_filter_runtime
            .insert(filter_id, SectionFilterRuntime::default());
        Ok(record)
    }

    pub fn register_filter(
        &mut self,
        filter_type_bits: i32,
        open_type: FilterOpenType,
        buffer_size: i32,
    ) -> DemuxFilterRecord {
        self.register_filter_result(filter_type_bits, open_type, buffer_size)
            .expect("register_filter called on a closed or invalid DemuxHandle; production callers must use register_filter_result")
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
        self.section_assemblers
            .retain(|(_, stored_filter_id), _| *stored_filter_id != filter_id);
        self.pes_assemblers
            .retain(|(_, stored_filter_id), _| *stored_filter_id != filter_id);
        self.filter_section_flush_generations
            .retain(|(_, id, _), _| *id != filter_id);
        self.filter_pes_flush_generations
            .retain(|(_, id, _), _| *id != filter_id);
        let downstream_ids: Vec<i32> = self
            .filters
            .iter()
            .filter_map(|(id, downstream)| {
                (downstream.data_source_filter_id == Some(filter_id)).then_some(*id)
            })
            .collect();
        for downstream_id in downstream_ids {
            if let Some(downstream) = self.filters.get_mut(&downstream_id) {
                downstream.data_source_filter_id = None;
                downstream.started = false;
                downstream.queued_bytes = 0;
                downstream.pending_overflow = false;
                downstream.pending_start_event = false;
                downstream.delivery_not_before = None;
                downstream.delivery_generation = downstream.delivery_generation.saturating_add(1);
            }
            self.filter_queues.insert(downstream_id, VecDeque::new());
            self.section_filter_runtime
                .insert(downstream_id, SectionFilterRuntime::default());
            self.section_assemblers
                .retain(|(_, stored_filter_id), _| *stored_filter_id != downstream_id);
            self.pes_assemblers
                .retain(|(_, stored_filter_id), _| *stored_filter_id != downstream_id);
            self.filter_section_flush_generations
                .retain(|(_, id, _), _| *id != downstream_id);
            self.filter_pes_flush_generations
                .retain(|(_, id, _), _| *id != downstream_id);
        }
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
        self.section_assemblers
            .values()
            .map(|assembler| assembler.oversized_section_drops())
            .sum()
    }

    pub fn stale_partial_section_discard_count(&self) -> u64 {
        self.section_assemblers
            .values()
            .map(|assembler| assembler.stale_partial_section_discards())
            .sum()
    }

    fn configured_filter_counts_against(
        record: &DemuxFilterRecord,
        kind: FilterCapacityKind,
    ) -> bool {
        let Some(cfg) = record.config.as_ref() else {
            return false;
        };
        match kind {
            FilterCapacityKind::Section => matches!(cfg.kind, FilterConfigKind::Section { .. }),
            FilterCapacityKind::Audio => {
                matches!(cfg.kind, FilterConfigKind::Av { .. })
                    && record.av_stream_kind == Some(AvFilterStreamKind::Audio)
            }
            FilterCapacityKind::Video => {
                matches!(cfg.kind, FilterConfigKind::Av { .. })
                    && record.av_stream_kind == Some(AvFilterStreamKind::Video)
            }
            FilterCapacityKind::Pes => matches!(cfg.kind, FilterConfigKind::PesData { .. }),
            FilterCapacityKind::Record => matches!(cfg.kind, FilterConfigKind::Record { .. }),
            FilterCapacityKind::AvAny => matches!(cfg.kind, FilterConfigKind::Av { .. }),
            FilterCapacityKind::Other => false,
        }
    }

    fn filter_capacity_limit(kind: FilterCapacityKind) -> usize {
        match kind {
            FilterCapacityKind::Section => DEMUX_MAX_SECTION_FILTERS.max(0) as usize,
            FilterCapacityKind::Audio => DEMUX_MAX_AUDIO_FILTERS.max(0) as usize,
            FilterCapacityKind::Video => DEMUX_MAX_VIDEO_FILTERS.max(0) as usize,
            FilterCapacityKind::Pes => DEMUX_MAX_PES_FILTERS.max(0) as usize,
            FilterCapacityKind::Record => DEMUX_MAX_RECORD_FILTERS.max(0) as usize,
            FilterCapacityKind::AvAny => DEMUX_MAX_FILTERS_PER_DEMUX,
            FilterCapacityKind::Other => 0,
        }
    }

    fn filter_kind_capacity(summary: &FilterConfigKind) -> FilterCapacityKind {
        match summary {
            FilterConfigKind::Section { .. } => FilterCapacityKind::Section,
            FilterConfigKind::Av { .. } => FilterCapacityKind::AvAny,
            FilterConfigKind::PesData { .. } => FilterCapacityKind::Pes,
            FilterConfigKind::Record { .. } => FilterCapacityKind::Record,
            FilterConfigKind::Noinit | FilterConfigKind::Other => FilterCapacityKind::Other,
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

    pub fn configure_filter(&mut self, filter_id: i32) -> bool {
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.configured = true;
            return true;
        }
        false
    }

    pub fn configure_filter_with_summary_result(
        &mut self,
        filter_id: i32,
        summary: FilterConfig,
    ) -> Result<(), DemuxConfigError> {
        if !self.filters.contains_key(&filter_id) {
            return Err(DemuxConfigError::NotFound);
        }
        let kind = Self::filter_kind_capacity(&summary.kind);
        if kind == FilterCapacityKind::Other {
            return Err(DemuxConfigError::InvalidKind);
        }
        let Some(existing) = self.filters.get(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if existing.started {
            return Err(DemuxConfigError::InvalidState);
        }
        let open_type = existing.open_type;
        if !open_type.accepts_config_kind(&summary.kind) {
            return Err(DemuxConfigError::InvalidKind);
        }
        if !self.filter_capacity_available(filter_id, kind) {
            return Err(DemuxConfigError::CapacityExceeded);
        }
        let is_av = matches!(&summary.kind, FilterConfigKind::Av { .. });
        let pid = summary.tpid;
        {
            let Some(filter) = self.filters.get_mut(&filter_id) else {
                return Err(DemuxConfigError::NotFound);
            };
            filter.configured = true;
            filter.config = Some(summary);
            // 再設定は以前の AV ストリーム種別紐付けを無効化する。
            // AV filter は start() 前に configureAvStreamType() を再度受け取る必要がある。
            filter.av_stream_type_hint = None;
            filter.av_stream_kind = None;
            // Phase 4 clean boundary: reconfigure invalidates any previous upstream linkage.
            // A downstream filter must be explicitly re-linked after its condition/PID changes.
            filter.data_source_filter_id = None;
            filter.queued_bytes = 0;
            filter.pending_overflow = false;
            filter.pending_start_event = false;
            filter.delivery_not_before = None;
            filter.delivery_generation = filter.delivery_generation.saturating_add(1);
        }
        if is_av {
            let state = self
                .av_sync_states
                .entry(filter_id)
                .or_insert_with(|| AvSyncState::new(pid));
            state.pid = pid;
            state.stream_type_hint = None;
            state.stream_kind = None;
        } else {
            self.av_sync_states.remove(&filter_id);
        }
        self.filter_queues.insert(filter_id, VecDeque::new());
        self.section_filter_runtime
            .insert(filter_id, SectionFilterRuntime::default());
        self.section_assemblers
            .retain(|(_, stored_filter_id), _| *stored_filter_id != filter_id);
        self.pes_assemblers
            .retain(|(_, stored_filter_id), _| *stored_filter_id != filter_id);
        self.filter_section_flush_generations
            .retain(|(_, id, _), _| *id != filter_id);
        self.filter_pes_flush_generations
            .retain(|(_, id, _), _| *id != filter_id);
        Ok(())
    }

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
        if filter_ids.len() < pids.len() {
            return Err(DemuxConfigError::CapacityExceeded);
        }
        for (filter_id, pid) in filter_ids.iter().copied().zip(pids.iter().copied()) {
            self.configure_record_pid_filter(filter_id, pid)?;
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

    pub fn validate_filter_data_source(
        &self,
        filter_id: i32,
        upstream_filter_id: i32,
    ) -> Result<(), DemuxConfigError> {
        let Some(destination) = self.filters.get(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        let Some(source) = self.filters.get(&upstream_filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if !can_link_filter_open_types(source.open_type, destination.open_type) {
            return Err(DemuxConfigError::InvalidKind);
        }
        if filter_id == upstream_filter_id {
            return Err(DemuxConfigError::InvalidKind);
        }
        if destination.started {
            return Err(DemuxConfigError::InvalidState);
        }

        let mut current = Some(upstream_filter_id);
        let mut visited = BTreeSet::new();
        while let Some(source_id) = current {
            if source_id == filter_id {
                return Err(DemuxConfigError::InvalidKind);
            }
            if !visited.insert(source_id) {
                return Err(DemuxConfigError::InvalidKind);
            }
            current = self
                .filters
                .get(&source_id)
                .and_then(|source| source.data_source_filter_id);
        }
        Ok(())
    }

    pub fn set_filter_data_source_result(
        &mut self,
        filter_id: i32,
        upstream_filter_id: i32,
    ) -> Result<(), DemuxConfigError> {
        self.validate_filter_data_source(filter_id, upstream_filter_id)?;
        let Some(filter) = self.filters.get_mut(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        filter.data_source_filter_id = Some(upstream_filter_id);
        Ok(())
    }

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
            filter.delivery_not_before = if filter.started {
                filter
                    .delay_hints
                    .time_delay_ms
                    .filter(|ms| *ms > 0)
                    .map(|ms| Instant::now() + Duration::from_millis(ms))
            } else {
                None
            };
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

    pub fn set_filter_av_stream_type_hint_result(
        &mut self,
        filter_id: i32,
        av_stream_type_hint: i32,
        av_stream_kind: AvFilterStreamKind,
    ) -> Result<(), DemuxConfigError> {
        let Some(existing) = self.filters.get(&filter_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if existing.started {
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
        if !filter.configured || filter.config.is_none() {
            return Err(DemuxConfigError::InvalidState);
        }
        if matches!(
            filter.config.as_ref().map(|cfg| &cfg.kind),
            Some(FilterConfigKind::Av { .. })
        ) && (filter.av_stream_type_hint.is_none() || filter.av_stream_kind.is_none())
        {
            return Err(DemuxConfigError::InvalidState);
        }
        filter.started = true;
        filter.delivery_not_before = filter
            .delay_hints
            .time_delay_ms
            .filter(|ms| *ms > 0)
            .map(|ms| Instant::now() + Duration::from_millis(ms));
        self.section_filter_runtime
            .insert(filter_id, SectionFilterRuntime::default());
        Ok(())
    }

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

    pub fn take_filter_start_event_if_ready(&mut self, filter_id: i32) -> bool {
        if self.filter_delivery_readiness(filter_id) != FilterDeliveryReadiness::Ready {
            return false;
        }
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            if filter.pending_start_event {
                filter.pending_start_event = false;
                return true;
            }
        }
        false
    }

    pub fn filter_start_event_pending(&self, filter_id: i32) -> Option<bool> {
        self.filters
            .get(&filter_id)
            .map(|filter| filter.pending_start_event)
    }

    pub fn stop_filter(&mut self, filter_id: i32) -> bool {
        let pid = self
            .filters
            .get(&filter_id)
            .and_then(|filter| filter.config.as_ref().map(|config| config.tpid));
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.started = false;
            filter.queued_bytes = 0;
            filter.pending_start_event = false;
            filter.pending_overflow = false;
            filter.delivery_not_before = None;
            filter.delivery_generation = filter.delivery_generation.saturating_add(1);
            self.filter_queues.insert(filter_id, VecDeque::new());
            self.section_filter_runtime
                .insert(filter_id, SectionFilterRuntime::default());
            if let Some(pid) = pid {
                self.prune_assemblers_for_pid(pid);
            }
            return true;
        }
        false
    }

    pub fn flush_filter(&mut self, filter_id: i32) -> bool {
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            filter.queued_bytes = 0;
            filter.pending_start_event = false;
            filter.pending_overflow = false;
            filter.delivery_not_before = None;
            filter.delivery_generation = filter.delivery_generation.saturating_add(1);
            self.filter_queues.insert(filter_id, VecDeque::new());
            self.section_filter_runtime
                .insert(filter_id, SectionFilterRuntime::default());
            self.mark_filter_flush_generation(filter_id);
            return true;
        }
        false
    }

    pub fn inject_payload(&mut self, filter_id: i32, payload: &[u8]) -> bool {
        let Some(filter) = self.filters.get(&filter_id).cloned() else {
            return false;
        };
        if !filter.started {
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
        self.push_filter_payload(filter_id, payload_entry.clone());
        self.propagate_filter_output(filter_id, &payload_entry);
        if let Some(filter_mut) = self.filters.get_mut(&filter_id) {
            filter_mut.events_emitted += 1;
        }
        true
    }

    pub fn push_ts_stream(&mut self, payload: &[u8]) -> usize {
        self.push_ts_stream_from_frontend(payload)
    }

    pub fn push_ts_stream_from_frontend(&mut self, payload: &[u8]) -> usize {
        let packets = self.resync.push(payload);
        let mut pushed = 0usize;
        for packet in packets {
            if self.push_ts_packet_with_origin(&packet, TsInputOrigin::Frontend) {
                pushed += 1;
            }
        }
        pushed
    }

    fn push_ts_stream_from_playback(&mut self, payload: &[u8]) -> usize {
        let mut pushed = 0usize;
        for packet in payload.chunks_exact(TS_PACKET_SIZE) {
            if self.push_ts_packet_with_origin(packet, TsInputOrigin::Playback) {
                pushed += 1;
            }
        }
        pushed
    }

    pub fn push_ts_packet(&mut self, packet: &[u8]) -> bool {
        self.push_ts_packet_with_origin(packet, TsInputOrigin::Frontend)
    }

    pub fn push_ts_packet_record_only(&mut self, packet: &[u8]) -> bool {
        let Some(parsed) = TsPacketView::parse(packet) else {
            return false;
        };
        let packet_targets: Vec<i32> = self
            .filters
            .iter()
            .filter(|(_, f)| {
                f.started
                    && f.config.as_ref().map_or(false, |config| {
                        config.tpid == parsed.pid
                            && matches!(config.kind, FilterConfigKind::Record { .. })
                    })
            })
            .map(|(id, _)| *id)
            .collect();
        for filter_id in packet_targets {
            let packet_entry = FilterPayload::TsPacket(packet.to_vec());
            self.push_filter_payload(filter_id, packet_entry.clone());
            self.mirror_filter_payload_to_record_dvrs(filter_id, &packet_entry);
            if let Some(filter_mut) = self.filters.get_mut(&filter_id) {
                filter_mut.events_emitted += 1;
            }
        }
        true
    }

    fn push_ts_packet_with_origin(&mut self, packet: &[u8], origin: TsInputOrigin) -> bool {
        let Some(parsed) = TsPacketView::parse(packet) else {
            return false;
        };

        // Raw TS delivery は section/PES/AV assembly policy から意図的に分離する。
        // AOSP は RECORD/TS filter に TEI/continuity discard policy を要求しておらず、
        // この製品では診断と DVR のため受信した 188-byte TS packet stream を保持する。
        let packet_targets: Vec<i32> = self
            .filters
            .iter()
            .filter(|(_, f)| f.started && self.filter_accepts_packet(f, parsed.pid))
            .map(|(id, _)| *id)
            .collect();
        for filter_id in packet_targets {
            let packet_entry = FilterPayload::TsPacket(packet.to_vec());
            self.push_filter_payload(filter_id, packet_entry.clone());
            if origin.allows_record_mirror() {
                self.mirror_filter_payload_to_record_dvrs(filter_id, &packet_entry);
            }
            self.propagate_filter_output_with_origin(filter_id, &packet_entry, origin);
            if let Some(filter_mut) = self.filters.get_mut(&filter_id) {
                filter_mut.events_emitted += 1;
            }
        }

        if parsed.transport_error_indicator {
            return true;
        }
        let continuity = self.continuity_trackers.entry(origin).or_default().observe(
            parsed.pid as u16,
            parsed.continuity_counter,
            parsed.payload.is_some(),
        );
        if matches!(continuity, ContinuityOutcome::Duplicate) {
            return true;
        }
        if matches!(continuity, ContinuityOutcome::Discontinuity) {
            self.remove_section_assemblers_for_origin_pid(origin, parsed.pid);
            self.remove_pes_assemblers_for_origin_pid(origin, parsed.pid);
        }

        if let Some(pcr) = parsed.pcr_90khz {
            self.latest_pcr = Some(pcr);
            self.latest_pcr_instant = Some(Instant::now());
            self.latest_pcr_90khz = Some(self.pcr_extender.update(pcr));
        }

        if let Some(payload) = parsed.payload {
            if self.pid_has_started_section_filter(parsed.pid) {
                let section_generation = if parsed.payload_unit_start {
                    self.bump_section_generation(origin, parsed.pid)
                } else {
                    self.current_section_generation(origin, parsed.pid)
                };
                let section_filter_ids: Vec<i32> = self
                    .filters
                    .iter()
                    .filter_map(|(id, filter)| {
                        let is_target = filter.started
                            && filter.config.as_ref().map_or(false, |config| {
                                config.tpid == parsed.pid
                                    && matches!(config.kind, FilterConfigKind::Section { .. })
                            });
                        is_target.then_some(*id)
                    })
                    .collect();
                for filter_id in section_filter_ids {
                    let sections = self
                        .section_assemblers
                        .entry((origin, filter_id))
                        .or_default()
                        .push_payload(parsed.payload_unit_start, payload);
                    for section in sections {
                        if !self.section_generation_allows_delivery(
                            origin,
                            filter_id,
                            parsed.pid,
                            section_generation,
                        ) {
                            continue;
                        }
                        if !self.filter_accepts_section(filter_id, parsed.pid, &section) {
                            continue;
                        }
                        let section_entry = FilterPayload::Bytes(section.clone());
                        self.push_filter_payload(filter_id, section_entry.clone());
                        self.propagate_filter_output_with_origin_generation(
                            filter_id,
                            &section_entry,
                            origin,
                            Some(AssemblyGeneration::Section {
                                pid: parsed.pid,
                                generation: section_generation,
                            }),
                        );
                        if let Some(filter_mut) = self.filters.get_mut(&filter_id) {
                            filter_mut.events_emitted += 1;
                        }
                    }
                }
            } else {
                self.remove_section_assemblers_for_origin_pid(origin, parsed.pid);
            }

            if self.pid_has_started_pes_or_av_filter(parsed.pid) {
                let pes_generation = if parsed.payload_unit_start {
                    self.bump_pes_generation(origin, parsed.pid)
                } else {
                    self.current_pes_generation(origin, parsed.pid)
                };
                let pes_filter_ids: Vec<i32> = self
                    .filters
                    .iter()
                    .filter_map(|(id, filter)| {
                        let is_target = filter.started
                            && filter.config.as_ref().map_or(false, |config| {
                                config.tpid == parsed.pid
                                    && matches!(
                                        config.kind,
                                        FilterConfigKind::PesData { .. }
                                            | FilterConfigKind::Av { .. }
                                    )
                            });
                        is_target.then_some(*id)
                    })
                    .collect();
                for filter_id in pes_filter_ids {
                    let pes_packets = self
                        .pes_assemblers
                        .entry((origin, filter_id))
                        .or_default()
                        .push(parsed.pid as u16, parsed.payload_unit_start, payload);
                    for pes in pes_packets {
                        self.route_pes_packet_for_filter(
                            origin,
                            parsed.pid,
                            filter_id,
                            &pes,
                            pes_generation,
                        );
                    }
                }
            } else {
                self.remove_pes_assemblers_for_origin_pid(origin, parsed.pid);
            }
        }
        true
    }

    pub fn inject_playback_payload(&mut self, dvr_id: i32, payload: &[u8]) -> bool {
        let Some(dvr) = self.dvrs.get(&dvr_id).cloned() else {
            return false;
        };
        if !dvr.started || dvr.direction != DemuxPathDirection::Playback {
            return false;
        }
        if payload.len() % TS_PACKET_SIZE != 0 {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.playback_malformed_bytes = dvr
                    .playback_malformed_bytes
                    .saturating_add(payload.len() as u64);
            }
            return false;
        }
        let mut pushed = 0usize;
        let mut malformed = 0u64;
        for chunk in payload.chunks_exact(TS_PACKET_SIZE) {
            if chunk[0] != 0x47 {
                malformed = malformed.saturating_add(TS_PACKET_SIZE as u64);
                continue;
            }
            let mut packet = [0u8; TS_PACKET_SIZE];
            packet.copy_from_slice(chunk);
            if self.push_ts_packet_with_origin(&packet, TsInputOrigin::Playback) {
                pushed += 1;
            }
        }
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.playback_malformed_bytes = dvr.playback_malformed_bytes.saturating_add(malformed);
            dvr.playback_injected_packets =
                dvr.playback_injected_packets.saturating_add(pushed as u64);
            dvr.playback_injected_bytes = dvr
                .playback_injected_bytes
                .saturating_add((pushed * TS_PACKET_SIZE) as u64);
        }
        true
    }

    pub fn pop_filter_payload_entry(&mut self, filter_id: i32) -> Option<FilterPayload> {
        let payload = self.filter_queues.get_mut(&filter_id)?.pop_front();
        if let Some(ref entry) = payload {
            if let Some(filter) = self.filters.get_mut(&filter_id) {
                filter.queued_bytes = filter.queued_bytes.saturating_sub(entry.len());
            }
        }
        payload
    }

    pub fn pop_filter_payload(&mut self, filter_id: i32) -> Option<Vec<u8>> {
        self.pop_filter_payload_entry(filter_id)
            .map(FilterPayload::into_bytes)
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
            return Err(DemuxConfigError::CapacityExceeded);
        }
        let dvr_id = self.next_dvr_id;
        self.next_dvr_id += 1;
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
            .expect("test DVR record must exist")
    }

    #[cfg(test)]
    fn filter_record_mut_for_test(&mut self, filter_id: i32) -> &mut DemuxFilterRecord {
        self.filters
            .get_mut(&filter_id)
            .expect("test filter record must exist")
    }

    pub fn configure_dvr(&mut self, dvr_id: i32) -> bool {
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.configured = true;
            return true;
        }
        false
    }

    pub fn configure_dvr_with_summary_result(
        &mut self,
        dvr_id: i32,
        summary: DvrConfig,
    ) -> Result<(), DemuxConfigError> {
        let Some(dvr) = self.dvrs.get_mut(&dvr_id) else {
            return Err(DemuxConfigError::NotFound);
        };
        if dvr.direction != summary.direction {
            return Err(DemuxConfigError::InvalidKind);
        }
        dvr.configured = true;
        dvr.config = Some(summary);
        Ok(())
    }

    pub fn configure_dvr_with_summary(&mut self, dvr_id: i32, summary: DvrConfig) -> bool {
        self.configure_dvr_with_summary_result(dvr_id, summary)
            .is_ok()
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
            return Err(DemuxConfigError::InvalidKind);
        }
        if !dvr.configured || dvr.config.is_none() {
            return Err(DemuxConfigError::InvalidState);
        }
        if !filter.configured || filter.config.is_none() {
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

    pub fn attach_filter_to_dvr(&mut self, dvr_id: i32, filter_id: i32) -> bool {
        self.attach_filter_to_dvr_result(dvr_id, filter_id).is_ok()
    }

    pub fn detach_filter_from_dvr(&mut self, dvr_id: i32, filter_id: i32) -> bool {
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.attached_filter_ids.retain(|id| *id != filter_id);
            return true;
        }
        false
    }

    fn record_dvr_has_attached_record_filter(&self, dvr: &DemuxDvrRecord) -> bool {
        dvr.attached_filter_ids.iter().any(|filter_id| {
            self.filters.get(filter_id).map_or(false, |filter| {
                filter.configured
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
        if !dvr.configured || dvr.config.is_none() {
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
        dvr.started = true;
        Ok(())
    }

    pub fn start_dvr(&mut self, dvr_id: i32) -> bool {
        self.start_dvr_result(dvr_id).is_ok()
    }

    pub fn stop_dvr(&mut self, dvr_id: i32) -> bool {
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.started = false;
            return true;
        }
        false
    }

    pub fn flush_dvr(&mut self, dvr_id: i32) -> bool {
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.queued_bytes = 0;
            dvr.pending_overflow = false;
            dvr.overflow_events = 0;
            dvr.drop_bytes = 0;
            if dvr.direction == DemuxPathDirection::Playback {
                dvr.playback_injected_packets = 0;
                dvr.playback_injected_bytes = 0;
                dvr.playback_malformed_bytes = 0;
            }
            self.dvr_queues.insert(dvr_id, VecDeque::new());
            return true;
        }
        false
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
                dvr.queued_bytes = dvr.queued_bytes.saturating_sub(bytes.len());
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
        self.current_filter_fill_bytes(filter_id)
            .map_or(false, |n| n > 0)
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
        Some(FilterQueueModel {
            queue_kind: QueueKind::FilterOutput,
            discipline: match filter.config.as_ref().map(|cfg| &cfg.kind) {
                Some(FilterConfigKind::Section { .. }) => FilterQueueDiscipline::SectionReassembled,
                _ => FilterQueueDiscipline::PacketPassthrough,
            },
            policy: QueuePolicy::bounded(filter.buffer_size.max(0) as usize),
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
                    QueuePolicy::bounded(dvr.buffer_size.max(0) as usize)
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
        let has_time_delay = filter.delay_hints.time_delay_ms.unwrap_or(0) > 0;
        let has_data_size_delay = filter.delay_hints.data_size_delay_bytes.unwrap_or(0) > 0;
        if !has_time_delay && !has_data_size_delay {
            return FilterDeliveryReadiness::Ready;
        }

        let time_ready = has_time_delay
            && filter
                .delivery_not_before
                .map(|deadline| Instant::now() >= deadline)
                .unwrap_or(true);
        let data_ready = has_data_size_delay
            && filter.queued_bytes >= filter.delay_hints.data_size_delay_bytes.unwrap_or(0);

        if time_ready || data_ready {
            return FilterDeliveryReadiness::Ready;
        }
        if has_time_delay {
            FilterDeliveryReadiness::WaitingForTime
        } else {
            FilterDeliveryReadiness::WaitingForDataSize
        }
    }

    pub fn drain_filter_payloads_for_delivery(&mut self, filter_id: i32) -> Vec<FilterPayload> {
        if self
            .filters
            .get(&filter_id)
            .map_or(true, |filter| !filter.started)
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
        Some((self.demux_id << 16) | (filter_id & 0xffff))
    }

    pub fn source_time_now(&self) -> Option<i64> {
        let base = self.latest_pcr_90khz?;
        let instant = self.latest_pcr_instant?;
        let elapsed_ns = instant.elapsed().as_nanos();
        let elapsed_90khz_u128 = elapsed_ns.saturating_mul(90_000) / 1_000_000_000;
        let elapsed_90khz = elapsed_90khz_u128.min(i64::MAX as u128) as i64;
        Some(base.saturating_add(elapsed_90khz))
    }

    pub fn av_sync_time_now(&self, av_sync_hw_id: i32) -> Option<i64> {
        if av_sync_hw_id < 0 {
            return None;
        }
        if (av_sync_hw_id >> 16) != self.demux_id {
            return None;
        }
        let filter_id = av_sync_hw_id & 0xffff;
        if self.av_sync_hw_id_for(filter_id)? != av_sync_hw_id {
            return None;
        }
        self.source_time_now()
    }

    pub fn close(&mut self) {
        self.drop_all_pes_assemblers();
        self.closed = true;
        self.av_sync_states.clear();
        self.filters.clear();
        self.filter_queues.clear();
        self.section_filter_runtime.clear();
        self.dvrs.clear();
        self.dvr_queues.clear();
        self.section_assemblers.clear();
        self.pes_assemblers.clear();
        self.section_assembler_generations.clear();
        self.pes_assembler_generations.clear();
        self.filter_section_flush_generations.clear();
        self.filter_pes_flush_generations.clear();
        self.continuity_trackers.clear();
        self.latest_pcr = None;
        self.latest_pcr_instant = None;
        self.pcr_extender.reset();
        self.latest_pcr_90khz = None;
        self.resync = TsPacketResyncBuffer::default();
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
    ) {
        let Some(filter) = self.filters.get(&filter_id).cloned() else {
            return;
        };
        if !filter.started
            || !self.filter_accepts_pes(&filter, pid, pes)
            || !self.pes_generation_allows_delivery(origin, filter_id, pid, generation)
        {
            return;
        }
        let _ = pes.pts_90khz;
        let kind = filter.config.as_ref().map(|c| c.kind.clone());
        let payload = match kind {
            Some(FilterConfigKind::PesData { raw: true, .. }) => FilterPayload::PesData {
                bytes: pes.raw_bytes.clone(),
                stream_id: pes.stream_id as i32,
                raw: true,
            },
            Some(FilterConfigKind::PesData { raw: false, .. }) => FilterPayload::PesData {
                bytes: pes.payload.clone(),
                stream_id: pes.stream_id as i32,
                raw: false,
            },
            Some(FilterConfigKind::Av {
                passthrough: false, ..
            }) => FilterPayload::AvEs {
                bytes: pes.payload.clone(),
                metadata: AvPayloadMetadata {
                    pts_90khz: pes.pts_90khz,
                    dts_90khz: pes.dts_90khz,
                    stream_id: pes.stream_id as i32,
                },
            },
            Some(FilterConfigKind::Av { .. }) => FilterPayload::Bytes(pes.raw_bytes.clone()),
            _ => FilterPayload::Bytes(pes.raw_bytes.clone()),
        };
        self.push_filter_payload(filter_id, payload.clone());
        self.propagate_filter_output_with_origin_generation(
            filter_id,
            &payload,
            origin,
            Some(AssemblyGeneration::Pes { pid, generation }),
        );
        if let Some(filter_mut) = self.filters.get_mut(&filter_id) {
            filter_mut.events_emitted += 1;
        }
    }

    fn mirror_filter_payload_to_record_dvrs(&mut self, filter_id: i32, payload: &FilterPayload) {
        let FilterPayload::TsPacket(bytes) = payload else {
            return;
        };
        if bytes.len() != maleicacid_tuner_hal_common::TS_PACKET_SIZE {
            return;
        }
        let attached: Vec<i32> = self
            .dvrs
            .iter()
            .filter(|(_, dvr)| {
                dvr.started
                    && dvr.direction == DemuxPathDirection::Record
                    && dvr.attached_filter_ids.contains(&filter_id)
            })
            .map(|(id, _)| *id)
            .collect();
        for dvr_id in attached {
            self.push_dvr_payload(dvr_id, bytes);
        }
    }

    fn propagate_filter_output(&mut self, source_filter_id: i32, payload: &FilterPayload) {
        self.propagate_filter_output_with_origin(
            source_filter_id,
            payload,
            TsInputOrigin::Frontend,
        );
    }

    fn propagate_filter_output_with_origin(
        &mut self,
        source_filter_id: i32,
        payload: &FilterPayload,
        origin: TsInputOrigin,
    ) {
        self.propagate_filter_output_with_origin_generation(
            source_filter_id,
            payload,
            origin,
            None,
        );
    }

    fn generation_allows_downstream_delivery(
        &self,
        origin: TsInputOrigin,
        downstream_id: i32,
        generation: Option<AssemblyGeneration>,
    ) -> bool {
        match generation {
            Some(AssemblyGeneration::Section { pid, generation }) => {
                self.section_generation_allows_delivery(origin, downstream_id, pid, generation)
            }
            Some(AssemblyGeneration::Pes { pid, generation }) => {
                self.pes_generation_allows_delivery(origin, downstream_id, pid, generation)
            }
            None => true,
        }
    }

    fn propagate_filter_output_with_origin_generation(
        &mut self,
        source_filter_id: i32,
        payload: &FilterPayload,
        origin: TsInputOrigin,
        generation: Option<AssemblyGeneration>,
    ) {
        let mut stack = vec![source_filter_id];
        let mut visited = BTreeSet::new();
        while let Some(current_source) = stack.pop() {
            if !visited.insert(current_source) {
                continue;
            }
            let downstreams: Vec<i32> = self
                .filters
                .iter()
                .filter(|(_, filter)| {
                    filter.started && filter.data_source_filter_id == Some(current_source)
                })
                .map(|(id, _)| *id)
                .collect();
            for downstream_id in downstreams {
                if !self.generation_allows_downstream_delivery(origin, downstream_id, generation) {
                    continue;
                }
                self.push_filter_payload(downstream_id, payload.clone());
                if origin.allows_record_mirror() {
                    self.mirror_filter_payload_to_record_dvrs(downstream_id, payload);
                }
                if let Some(filter_mut) = self.filters.get_mut(&downstream_id) {
                    filter_mut.events_emitted += 1;
                }
                stack.push(downstream_id);
            }
        }
    }

    fn payload_matches_filter(&self, filter: &DemuxFilterRecord, payload: &[u8]) -> bool {
        match filter.config.as_ref().map(|c| &c.kind) {
            None
            | Some(FilterConfigKind::Noinit)
            | Some(FilterConfigKind::Other)
            | Some(FilterConfigKind::Av { .. })
            | Some(FilterConfigKind::PesData { .. })
            | Some(FilterConfigKind::Record { .. }) => true,
            Some(FilterConfigKind::Section {
                condition,
                check_crc,
                length_field_bits,
                ..
            }) => {
                if *check_crc && !section_crc_valid(payload, *length_field_bits) {
                    return false;
                }
                condition.matches(payload)
            }
        }
    }

    fn pid_has_started_section_filter(&self, pid: i32) -> bool {
        self.filters.values().any(|filter| {
            filter.started
                && filter.config.as_ref().map_or(false, |config| {
                    config.tpid == pid && matches!(&config.kind, FilterConfigKind::Section { .. })
                })
        })
    }

    fn pid_has_started_pes_or_av_filter(&self, pid: i32) -> bool {
        self.filters.values().any(|filter| {
            filter.started
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
            let section_filter_ids_for_pid: BTreeSet<i32> = self
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
            self.section_assemblers
                .retain(|(_, filter_id), _| !section_filter_ids_for_pid.contains(filter_id));
            self.section_assembler_generations
                .retain(|(_, stored_pid), _| *stored_pid != pid);
            self.filter_section_flush_generations
                .retain(|(_, _, stored_pid), _| *stored_pid != pid);
        }
        if !self.pid_has_started_pes_or_av_filter(pid) {
            let pes_filter_ids_for_pid: BTreeSet<i32> = self
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
            self.pes_assemblers
                .retain(|(_, filter_id), _| !pes_filter_ids_for_pid.contains(filter_id));
            self.pes_assembler_generations
                .retain(|(_, stored_pid), _| *stored_pid != pid);
            self.filter_pes_flush_generations
                .retain(|(_, _, stored_pid), _| *stored_pid != pid);
        }
    }

    fn remove_section_assemblers_for_pid(&mut self, pid: i32) {
        let filter_ids_for_pid: BTreeSet<i32> = self
            .filters
            .iter()
            .filter_map(|(filter_id, filter)| {
                let matches_pid = filter.config.as_ref().map_or(false, |config| {
                    config.tpid == pid && matches!(config.kind, FilterConfigKind::Section { .. })
                });
                matches_pid.then_some(*filter_id)
            })
            .collect();
        self.section_assemblers
            .retain(|(_, filter_id), _| !filter_ids_for_pid.contains(filter_id));
    }

    fn remove_section_assemblers_for_origin_pid(&mut self, origin: TsInputOrigin, pid: i32) {
        let filter_ids_for_pid: BTreeSet<i32> = self
            .filters
            .iter()
            .filter_map(|(filter_id, filter)| {
                let matches_pid = filter.config.as_ref().map_or(false, |config| {
                    config.tpid == pid && matches!(config.kind, FilterConfigKind::Section { .. })
                });
                matches_pid.then_some(*filter_id)
            })
            .collect();
        self.section_assemblers
            .retain(|(stored_origin, filter_id), _| {
                *stored_origin != origin || !filter_ids_for_pid.contains(filter_id)
            });
    }

    fn remove_pes_assemblers_for_pid(&mut self, pid: i32) {
        let filter_ids_for_pid: BTreeSet<i32> = self
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
        self.pes_assemblers
            .retain(|(_, filter_id), _| !filter_ids_for_pid.contains(filter_id));
    }

    fn remove_pes_assemblers_for_origin_pid(&mut self, origin: TsInputOrigin, pid: i32) {
        let filter_ids_for_pid: BTreeSet<i32> = self
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
        self.pes_assemblers.retain(|(stored_origin, filter_id), _| {
            *stored_origin != origin || !filter_ids_for_pid.contains(filter_id)
        });
    }

    fn current_section_generation(&self, origin: TsInputOrigin, pid: i32) -> u64 {
        self.section_assembler_generations
            .get(&(origin, pid))
            .copied()
            .unwrap_or(0)
    }

    fn current_pes_generation(&self, origin: TsInputOrigin, pid: i32) -> u64 {
        self.pes_assembler_generations
            .get(&(origin, pid))
            .copied()
            .unwrap_or(0)
    }

    fn bump_section_generation(&mut self, origin: TsInputOrigin, pid: i32) -> u64 {
        let generation = self
            .section_assembler_generations
            .entry((origin, pid))
            .or_insert(0);
        *generation = generation.saturating_add(1);
        *generation
    }

    fn bump_pes_generation(&mut self, origin: TsInputOrigin, pid: i32) -> u64 {
        let generation = self
            .pes_assembler_generations
            .entry((origin, pid))
            .or_insert(0);
        *generation = generation.saturating_add(1);
        *generation
    }

    fn section_generation_allows_delivery(
        &self,
        origin: TsInputOrigin,
        filter_id: i32,
        pid: i32,
        generation: u64,
    ) -> bool {
        self.filter_section_flush_generations
            .get(&(origin, filter_id, pid))
            .map_or(true, |flushed_generation| generation > *flushed_generation)
    }

    fn pes_generation_allows_delivery(
        &self,
        origin: TsInputOrigin,
        filter_id: i32,
        pid: i32,
        generation: u64,
    ) -> bool {
        self.filter_pes_flush_generations
            .get(&(origin, filter_id, pid))
            .map_or(true, |flushed_generation| generation > *flushed_generation)
    }

    fn mark_filter_flush_generation(&mut self, filter_id: i32) {
        let Some(pid) = self
            .filters
            .get(&filter_id)
            .and_then(|filter| filter.config.as_ref().map(|config| config.tpid))
        else {
            return;
        };
        for origin in [TsInputOrigin::Frontend, TsInputOrigin::Playback] {
            self.filter_section_flush_generations.insert(
                (origin, filter_id, pid),
                self.current_section_generation(origin, pid),
            );
            self.filter_pes_flush_generations.insert(
                (origin, filter_id, pid),
                self.current_pes_generation(origin, pid),
            );
        }
        self.section_assemblers
            .retain(|(_, stored_filter_id), _| *stored_filter_id != filter_id);
        self.pes_assemblers
            .retain(|(_, stored_filter_id), _| *stored_filter_id != filter_id);
    }

    fn filter_accepts_packet(&self, filter: &DemuxFilterRecord, pid: i32) -> bool {
        let Some(config) = filter.config.as_ref() else {
            return false;
        };
        if config.tpid != pid {
            return false;
        }
        matches!(
            config.kind,
            FilterConfigKind::Other | FilterConfigKind::Record { .. }
        )
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
            FilterConfigKind::Av { .. } => true,
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
            length_field_bits,
            condition_kind,
            condition,
            ..
        } = &config.kind
        else {
            return false;
        };
        let Some(header) = parse_section_header(payload, *length_field_bits) else {
            return false;
        };
        if header.total_length > MAX_SECTION_PAYLOAD_BYTES {
            return false;
        }
        let payload = &payload[..header.total_length];
        if *check_crc && !section_crc_valid(payload, *length_field_bits) {
            return false;
        }
        if !condition.matches(payload) {
            return false;
        }
        if *repeat {
            return true;
        }
        let runtime = self.section_filter_runtime.entry(filter_id).or_default();
        match condition_kind {
            SectionConditionKind::SectionBits => {
                if runtime.finished {
                    return false;
                }
                runtime.finished = true;
                true
            }
            SectionConditionKind::TableInfo => {
                if runtime.finished {
                    return false;
                }
                let table_extension = header.table_id_extension.unwrap_or(0);
                let version = header.version.unwrap_or(0);
                let section_number = header.section_number.unwrap_or(0);
                let last_section_number = header.last_section_number.unwrap_or(section_number);
                let section_key = (header.table_id, table_extension, version);
                if !runtime.seen_bytes.insert(payload.to_vec()) {
                    if runtime
                        .table_progress
                        .get(&section_key)
                        .map(|t| t.is_complete())
                        .unwrap_or(false)
                    {
                        runtime.finished = true;
                    }
                    return false;
                }
                let progress = runtime.table_progress.entry(section_key).or_default();
                progress.observe(section_number, last_section_number);
                if progress.is_complete() {
                    runtime.finished = true;
                }
                true
            }
        }
    }

    fn drop_all_pes_assemblers(&mut self) {
        self.pes_assemblers.clear();
        self.pes_assembler_generations.clear();
        self.filter_pes_flush_generations.clear();
    }

    fn is_aligned_ts_stream(payload: &[u8]) -> bool {
        !payload.is_empty()
            && payload.len() % TS_PACKET_SIZE == 0
            && payload
                .chunks_exact(TS_PACKET_SIZE)
                .all(|packet| packet[0] == 0x47)
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
        if max_bytes > 0 && !drop_old_policy {
            let queued = self
                .filters
                .get(&filter_id)
                .map(|f| f.queued_bytes)
                .unwrap_or(0);
            if queued.saturating_add(payload_len) > max_bytes {
                if let Some(filter) = self.filters.get_mut(&filter_id) {
                    filter.pending_overflow = true;
                    filter.overflow_events = filter.overflow_events.saturating_add(1);
                    filter.drop_bytes = filter.drop_bytes.saturating_add(payload_len as u64);
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
        let queue = self.filter_queues.entry(filter_id).or_default();
        queue.push_back(payload);
        outcome.accepted_bytes = payload_len;
        if let Some(filter) = self.filters.get_mut(&filter_id) {
            if queue_was_empty && filter.started {
                filter.delivery_not_before = filter
                    .delay_hints
                    .time_delay_ms
                    .filter(|ms| *ms > 0)
                    .map(|ms| Instant::now() + Duration::from_millis(ms));
            }
            filter.queued_bytes = filter.queued_bytes.saturating_add(payload_len);
            if max_bytes > 0 && drop_old_policy {
                while filter.queued_bytes > max_bytes {
                    if let Some(removed) = queue.pop_front() {
                        let removed_len = removed.len();
                        filter.queued_bytes = filter.queued_bytes.saturating_sub(removed_len);
                        outcome.dropped_bytes = outcome.dropped_bytes.saturating_add(removed_len);
                        outcome.dropped_entries = outcome.dropped_entries.saturating_add(1);
                        outcome.dropped_old = true;
                    } else {
                        break;
                    }
                }
            }
            if outcome.dropped_entries > 0 {
                outcome.overflowed = true;
                filter.pending_overflow = true;
                filter.overflow_events = filter.overflow_events.saturating_add(1);
                filter.drop_bytes = filter
                    .drop_bytes
                    .saturating_add(outcome.dropped_bytes as u64);
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
        if max_bytes > 0
            && self
                .dvrs
                .get(&dvr_id)
                .map(|d| d.queued_bytes.saturating_add(payload.len()) > max_bytes)
                .unwrap_or(false)
        {
            if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
                dvr.pending_overflow = true;
                dvr.overflow_events = dvr.overflow_events.saturating_add(1);
                dvr.drop_bytes = dvr.drop_bytes.saturating_add(payload.len() as u64);
            }
            outcome.dropped_bytes = payload.len();
            outcome.dropped_entries = 1;
            outcome.dropped_new = true;
            outcome.overflowed = true;
            return outcome;
        }
        let queue = self.dvr_queues.entry(dvr_id).or_default();
        queue.push_back(payload.to_vec());
        if let Some(dvr) = self.dvrs.get_mut(&dvr_id) {
            dvr.queued_bytes = dvr.queued_bytes.saturating_add(payload.len());
            outcome.accepted_bytes = payload.len();
        }
        outcome
    }
}

#[derive(Clone, Copy, Debug)]
struct TsPacketView<'a> {
    pid: i32,
    transport_error_indicator: bool,
    payload_unit_start: bool,
    continuity_counter: u8,
    payload: Option<&'a [u8]>,
    pcr_90khz: Option<u64>,
}

impl<'a> TsPacketView<'a> {
    fn parse(packet: &'a [u8]) -> Option<Self> {
        if packet.len() != TS_PACKET_SIZE || packet[0] != 0x47 {
            return None;
        }
        let transport_error_indicator = (packet[1] & 0x80) != 0;
        let payload_unit_start = (packet[1] & 0x40) != 0;
        let pid = (((packet[1] & 0x1f) as i32) << 8) | packet[2] as i32;
        let adaptation_control = (packet[3] >> 4) & 0x03;
        let continuity_counter = packet[3] & 0x0f;
        let mut offset = 4usize;
        let mut pcr_90khz = None;
        if adaptation_control == 0 {
            return None;
        }
        if adaptation_control == 2 || adaptation_control == 3 {
            if offset >= packet.len() {
                return None;
            }
            let adaptation_len = packet[offset] as usize;
            if adaptation_len > 0 && offset + 1 + adaptation_len <= packet.len() {
                let flags = packet[offset + 1];
                if (flags & 0x10) != 0 && adaptation_len >= 7 {
                    let p = &packet[offset + 2..offset + 8];
                    let base = ((p[0] as u64) << 25)
                        | ((p[1] as u64) << 17)
                        | ((p[2] as u64) << 9)
                        | ((p[3] as u64) << 1)
                        | ((p[4] as u64) >> 7);
                    pcr_90khz = Some(base);
                }
            }
            offset = offset.saturating_add(1 + adaptation_len);
            if offset > packet.len() {
                return None;
            }
            if adaptation_control == 2 {
                return Some(Self {
                    pid,
                    transport_error_indicator,
                    payload_unit_start,
                    continuity_counter,
                    payload: None,
                    pcr_90khz,
                });
            }
        }
        Some(Self {
            pid,
            transport_error_indicator,
            payload_unit_start,
            continuity_counter,
            payload: packet.get(offset..),
            pcr_90khz,
        })
    }
}

#[cfg(test)]
mod pes_metadata_tests {
    use super::FilterPayload;

    #[test]
    fn pes_payload_preserves_stream_id_even_when_payload_is_es_only() {
        let payload = FilterPayload::PesData {
            bytes: vec![0x00, 0x11, 0x22, 0x33],
            stream_id: 0xbd,
            raw: false,
        };
        assert_eq!(payload.pes_stream_id(), Some(0xbd));
        assert_eq!(payload.bytes(), &[0x00, 0x11, 0x22, 0x33]);
    }
}

#[cfg(test)]
mod record_dvr_tests {
    use super::FilterPayload;
    use maleicacid_tuner_hal_common::TS_PACKET_SIZE;

    #[test]
    fn record_dvr_filters_only_ts_packets_by_type() {
        let ts = FilterPayload::TsPacket(vec![0x47; TS_PACKET_SIZE]);
        let pes = FilterPayload::PesData {
            bytes: vec![1, 2, 3],
            stream_id: 0xbd,
            raw: false,
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
        let emitted = demux
            .pes_assemblers
            .entry((TsInputOrigin::Frontend, pid as i32))
            .or_default()
            .push(pid, true, &pending);
        assert!(emitted.is_empty());
        assert!(!demux.pes_assemblers.is_empty());
        demux.drop_all_pes_assemblers();
        assert!(demux.pes_assemblers.is_empty());
    }

    #[test]
    fn same_stream_next_pusi_emits_previous_length_zero_pes() {
        let mut demux = DemuxHandle::new(0);
        let pid = 0x0100u16;
        let first = length_zero_video_pes_payload(0xaa);
        let second = length_zero_video_pes_payload(0xbb);
        let emitted_first = demux
            .pes_assemblers
            .entry((TsInputOrigin::Frontend, pid as i32))
            .or_default()
            .push(pid, true, &first);
        assert!(emitted_first.is_empty());
        let emitted_second = demux
            .pes_assemblers
            .entry((TsInputOrigin::Frontend, pid as i32))
            .or_default()
            .push(pid, true, &second);
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
        let filter = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        demux
            .configure_record_pid_filter(filter.filter_id, 0x0100)
            .unwrap();
        assert!(demux.configure_dvr_with_summary(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 0,
                high_threshold: 0,
                data_format: 0,
                packet_size: 188,
            }
        ));
        assert!(demux.attach_filter_to_dvr(dvr.dvr_id, filter.filter_id));
        assert!(demux.start_dvr(dvr.dvr_id));
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
            let filter = demux.register_filter(1, FilterOpenType::TsAudio, 4096);
            assert!(demux.configure_filter_with_summary(filter.filter_id, av_config()));
            assert!(demux.set_filter_av_stream_type_hint(
                filter.filter_id,
                2,
                AvFilterStreamKind::Audio
            ));
            audio_ids.push(filter.filter_id);
        }
        let extra_audio = demux.register_filter(1, FilterOpenType::TsAudio, 4096);
        assert!(demux.configure_filter_with_summary(extra_audio.filter_id, av_config()));
        assert!(!demux.set_filter_av_stream_type_hint(
            extra_audio.filter_id,
            2,
            AvFilterStreamKind::Audio
        ));

        for _ in 0..DEMUX_MAX_VIDEO_FILTERS {
            let filter = demux.register_filter(1, FilterOpenType::TsVideo, 4096);
            assert!(demux.configure_filter_with_summary(filter.filter_id, av_config()));
            assert!(demux.set_filter_av_stream_type_hint(
                filter.filter_id,
                2,
                AvFilterStreamKind::Video
            ));
        }
        let extra_video = demux.register_filter(1, FilterOpenType::TsVideo, 4096);
        assert!(demux.configure_filter_with_summary(extra_video.filter_id, av_config()));
        assert!(!demux.set_filter_av_stream_type_hint(
            extra_video.filter_id,
            2,
            AvFilterStreamKind::Video
        ));
        assert_eq!(audio_ids.len(), DEMUX_MAX_AUDIO_FILTERS as usize);
    }

    #[test]
    fn av_stream_type_must_match_open_subtype() {
        let mut demux = DemuxHandle::new(0);
        let audio = demux.register_filter(1, FilterOpenType::TsAudio, 4096);
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

        let video = demux.register_filter(1, FilterOpenType::TsVideo, 4096);
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
    fn av_filter_cannot_start_without_configure_av_stream_type() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsVideo, 4096);
        assert!(demux.configure_filter_with_summary(filter.filter_id, av_config()));
        assert_eq!(
            demux.start_filter_result(filter.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert!(demux.set_filter_av_stream_type_hint(
            filter.filter_id,
            2,
            AvFilterStreamKind::Video
        ));
        assert_eq!(demux.start_filter_result(filter.filter_id), Ok(()));
    }

    #[test]
    fn av_reconfigure_requires_configure_av_stream_type_again() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsVideo, 4096);
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
        assert_eq!(
            demux.start_filter_result(filter.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
    }

    #[test]
    fn unregister_releases_section_capacity() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config()));
        for _ in 1..DEMUX_MAX_SECTION_FILTERS {
            let f = demux.register_filter(1, FilterOpenType::TsSection, 4096);
            assert!(demux.configure_filter_with_summary(f.filter_id, section_config()));
        }
        let extra = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
    fn duplicate_ts_packet_is_kept_for_raw_record_filter_but_not_parser_assembly() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
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
        let queued = demux.drain_filter_payloads(filter.filter_id);
        assert_eq!(queued.len(), 2);
    }

    #[test]
    fn duplicate_ts_packet_reaches_record_dvr_queue() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
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
        assert!(demux.configure_dvr_with_summary(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 0,
                high_threshold: 0,
                data_format: 0,
                packet_size: 188,
            }
        ));
        assert!(demux.attach_filter_to_dvr(dvr.dvr_id, filter.filter_id));
        assert!(demux.start_dvr(dvr.dvr_id));

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
        let section = vec![0x80, 0x00, 0x03, 0xaa, 0xbb, 0xcc];
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
            filter_bytes: vec![0x42, 0x00, 0x00, 0xff],
            mask_bytes: vec![0xff, 0xff, 0xff, 0xff],
            mode_bytes: Vec::new(),
            table_id: None,
            version: None,
        };
        let valid_short_section = [0x42, 0x00, 0x00];
        assert!(!condition.matches(&valid_short_section));

        let exact_condition = SectionCondition {
            filter_bytes: vec![0x42, 0x00, 0x00],
            mask_bytes: vec![0xff, 0xff, 0xff],
            mode_bytes: Vec::new(),
            table_id: None,
            version: None,
        };
        assert!(exact_condition.matches(&valid_short_section));
    }

    #[test]
    fn section_condition_width_limit_tracks_capability() {
        let mut c = SectionCondition::default();
        c.filter_bytes = vec![0; MAX_SECTION_FILTER_BYTES as usize];
        assert!(c.validates_section_filter_width());
        c.mask_bytes = vec![0; MAX_SECTION_FILTER_BYTES as usize + 1];
        assert!(!c.validates_section_filter_width());
    }
}

#[cfg(test)]
mod record_pid_set_tests {
    use super::*;

    #[test]
    fn can_configure_record_pid_set() {
        let mut demux = DemuxHandle::new(0);
        let f1 = demux
            .register_filter(1, FilterOpenType::TsRecord, 4096)
            .filter_id;
        let f2 = demux
            .register_filter(1, FilterOpenType::TsRecord, 4096)
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
        let parsed = TsPacketView::parse(&tei_packet).expect("packet parses");
        assert!(parsed.transport_error_indicator);

        let clean_packet = packet(0x0100, 0, false);
        let parsed = TsPacketView::parse(&clean_packet).expect("packet parses");
        assert!(!parsed.transport_error_indicator);
    }

    #[test]
    fn tei_packet_is_kept_for_record_filter_and_dvr() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        assert!(demux.configure_filter_with_summary(filter.filter_id, record_config(pid)));
        demux.start_filter(filter.filter_id);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert!(demux.configure_dvr_with_summary(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 0,
                high_threshold: 0,
                data_format: 0,
                packet_size: 188,
            }
        ));
        assert!(demux.attach_filter_to_dvr(dvr.dvr_id, filter.filter_id));
        assert!(demux.start_dvr(dvr.dvr_id));

        let tei = packet(pid, 0, true);
        assert!(demux.push_ts_packet(&tei));
        let queued = demux.drain_filter_payloads(filter.filter_id);
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].bytes(), &tei);
        assert_eq!(demux.pop_dvr_payload(dvr.dvr_id).as_deref(), Some(&tei[..]));
    }

    #[test]
    fn tei_packet_does_not_advance_continuity_state() {
        let pid = 0x0100;
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        assert!(demux.configure_filter_with_summary(filter.filter_id, record_config(pid)));
        demux.start_filter(filter.filter_id);

        assert!(demux.push_ts_packet(&packet(pid, 0, false)));
        assert!(demux.push_ts_packet(&packet(pid, 1, true)));
        assert!(demux.push_ts_packet(&packet(pid, 1, false)));
        let queued = demux.drain_filter_payloads(filter.filter_id);
        assert_eq!(queued.len(), 3);
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
        let section = vec![0x80, 0x00, 0x03, 0xaa, 0xbb, 0xcc];
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
        let pes = demux.register_filter(1, FilterOpenType::TsPes, 4096);
        assert!(demux.configure_filter_with_summary(pes.filter_id, pes_config(pid)));
        assert!(demux.start_filter(pes.filter_id));

        let av = demux.register_filter(1, FilterOpenType::TsVideo, 4096);
        assert!(demux.configure_filter_with_summary(av.filter_id, av_config(pid)));
        assert!(demux.set_filter_av_stream_type_hint(av.filter_id, 2, AvFilterStreamKind::Video));
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
        let wildcard_filter = wildcard.register_filter(1, FilterOpenType::TsPes, 4096);
        assert!(wildcard.configure_filter_with_summary(
            wildcard_filter.filter_id,
            FilterConfig {
                kind: FilterConfigKind::PesData { stream_id: -1, raw: true },
                ..pes_config(pid)
            },
        ));
        assert!(wildcard.filter_accepts_pes(
            wildcard.filter_record(wildcard_filter.filter_id).unwrap(),
            pid as i32,
            &pes,
        ));

        let mut zero_exact = DemuxHandle::new(0);
        let zero_filter = zero_exact.register_filter(1, FilterOpenType::TsPes, 4096);
        assert!(zero_exact.configure_filter_with_summary(
            zero_filter.filter_id,
            FilterConfig {
                kind: FilterConfigKind::PesData { stream_id: 0, raw: true },
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
        let filter = demux.register_filter(1, open_type, 4096);
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

        let section = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
        demux.route_pes_packet(0x0100, &pes);
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

    #[test]
    fn demux_exposes_oversized_section_drop_counter() {
        let mut demux = DemuxHandle::new(9);
        let pid = 0x0123;
        let assembler = demux
            .section_assemblers
            .entry((TsInputOrigin::Frontend, pid))
            .or_default();
        assert!(!assembler.set_expected_len_or_drop(MAX_SECTION_PAYLOAD_BYTES + 1));
        assert_eq!(demux.oversized_section_drop_count(), 1);
    }

    #[test]
    fn demux_exposes_stale_partial_section_discard_counter() {
        let mut demux = DemuxHandle::new(9);
        let pid = 0x0123;
        let assembler = demux
            .section_assemblers
            .entry((TsInputOrigin::Frontend, pid))
            .or_default();
        let stale = [0x00, 0xb0, 0x0d, 0x00, 0x01, 0xc1];
        let replacement = [
            0x42, 0xf0, 0x05, 0x00, 0x01, 0xc1, 0x00, 0x00,
        ];
        let mut first = vec![0x00];
        first.extend_from_slice(&stale);
        assert!(assembler.push_payload(true, &first).is_empty());
        let mut second = vec![0x00];
        second.extend_from_slice(&replacement);
        assert_eq!(assembler.push_payload(true, &second), vec![replacement.to_vec()]);
        assert_eq!(demux.stale_partial_section_discard_count(), 1);
    }
}

#[cfg(test)]
mod stream_boundary_reset_tests {
    use super::*;

    #[test]
    fn reset_for_stream_boundary_keeps_configuration_but_drops_runtime_state() {
        let mut demux = DemuxHandle::new(7);
        let filter = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        demux
            .configure_record_pid_filter(filter.filter_id, 0x0100)
            .unwrap();
        assert!(demux.start_filter(filter.filter_id));
        assert!(demux.configure_dvr_with_summary(
            dvr.dvr_id,
            DvrConfig {
                direction: DemuxPathDirection::Record,
                status_mask: 0,
                low_threshold: 0,
                high_threshold: 0,
                data_format: 0,
                packet_size: 188,
            }
        ));
        assert!(demux.attach_filter_to_dvr(dvr.dvr_id, filter.filter_id));
        assert!(demux.start_dvr(dvr.dvr_id));
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
        demux.section_assemblers.insert(
            (TsInputOrigin::Frontend, 0x100),
            crate::sections::SectionAssembler::default(),
        );
        demux.pes_assemblers.insert(
            (TsInputOrigin::Frontend, 0x100),
            crate::ts_core::PesAssembler::default(),
        );
        demux.latest_pcr = Some(123);
        demux.latest_pcr_instant = Some(Instant::now());
        demux.latest_pcr_90khz = Some(123);

        demux.reset_for_stream_boundary();

        assert!(demux.has_filter(filter.filter_id));
        assert!(demux.filter_record(filter.filter_id).unwrap().started);
        assert!(demux.has_dvr(dvr.dvr_id));
        assert!(demux.dvr_record(dvr.dvr_id).unwrap().started);
        assert_eq!(demux.current_filter_fill_bytes(filter.filter_id), Some(0));
        assert_eq!(demux.current_fill_bytes(dvr.dvr_id), Some(0));
        assert!(demux
            .filter_queues
            .get(&filter.filter_id)
            .unwrap()
            .is_empty());
        assert!(demux.dvr_queues.get(&dvr.dvr_id).unwrap().is_empty());
        assert!(demux.section_assemblers.is_empty());
        assert!(demux.pes_assemblers.is_empty());
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
            DemuxConfigError::CapacityExceeded
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
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
    fn time_delay_holds_payload_until_deadline() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
    fn time_delay_rearms_for_each_queue_burst() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
        assert_eq!(demux.drain_filter_payloads_for_delivery(filter.filter_id).len(), 1);

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
        let audio = demux.register_filter(1, FilterOpenType::TsAudio, 4096);
        let video = demux.register_filter(1, FilterOpenType::TsVideo, 4096);
        let section = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let pes = demux.register_filter(1, FilterOpenType::TsPes, 4096);
        let record = demux.register_filter(1, FilterOpenType::TsRecord, 4096);

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
        let audio_as_section = mismatch.register_filter(1, FilterOpenType::TsAudio, 4096);
        let section_as_av = mismatch.register_filter(1, FilterOpenType::TsSection, 4096);
        let record_as_pes = mismatch.register_filter(1, FilterOpenType::TsRecord, 4096);
        let pes_as_record = mismatch.register_filter(1, FilterOpenType::TsPes, 4096);
        assert!(
            !mismatch.configure_filter_with_summary(audio_as_section.filter_id, section_config())
        );
        assert!(!mismatch.configure_filter_with_summary(section_as_av.filter_id, av_config()));
        assert!(!mismatch.configure_filter_with_summary(record_as_pes.filter_id, pes_config()));
        assert!(!mismatch.configure_filter_with_summary(pes_as_record.filter_id, record_config()));
    }

    #[test]
    fn time_and_data_size_delay_are_kept_independently_and_or_ready() {
        let mut demux = DemuxHandle::new(0);
        let by_time = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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

        let by_size = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
    fn pending_start_event_is_cleared_by_stop_flush_and_stream_boundary_reset() {
        let mut demux = DemuxHandle::new(0);
        let stopped = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        assert!(demux.configure_filter_with_summary(stopped.filter_id, section_config()));
        assert!(demux.start_filter(stopped.filter_id));
        assert!(demux.set_filter_start_event_pending(stopped.filter_id, true));
        assert!(demux.stop_filter(stopped.filter_id));
        assert_eq!(
            demux.filter_start_event_pending(stopped.filter_id),
            Some(false)
        );

        let flushed = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        assert!(demux.configure_filter_with_summary(flushed.filter_id, section_config()));
        assert!(demux.start_filter(flushed.filter_id));
        assert!(demux.set_filter_start_event_pending(flushed.filter_id, true));
        assert!(demux.flush_filter(flushed.filter_id));
        assert_eq!(
            demux.filter_start_event_pending(flushed.filter_id),
            Some(false)
        );

        let reset = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        assert!(demux.configure_filter_with_summary(reset.filter_id, section_config()));
        assert!(demux.start_filter(reset.filter_id));
        assert!(demux.set_filter_start_event_pending(reset.filter_id, true));
        demux.reset_for_stream_boundary();
        assert_eq!(
            demux.filter_start_event_pending(reset.filter_id),
            Some(false)
        );
    }

    #[test]
    fn stop_filter_clears_queued_payload_and_blocks_delivery_drain() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
                record.started,
                record.queued_bytes,
                record.pending_overflow
            )),
            Some((false, 0, false))
        );
        assert!(demux
            .drain_filter_payloads_for_delivery(filter.filter_id)
            .is_empty());
        assert!(demux.drain_filter_payloads(filter.filter_id).is_empty());
    }

    #[test]
    fn stop_filter_clears_queue_for_all_claimed_filter_types() {
        let mut cases = vec![
            (FilterOpenType::TsSection, section_config(), None),
            (FilterOpenType::TsPes, pes_config(), None),
            (FilterOpenType::TsRecord, record_config(), None),
            (
                FilterOpenType::TsAudio,
                av_config(),
                Some((2, AvFilterStreamKind::Audio)),
            ),
            (
                FilterOpenType::TsVideo,
                av_config(),
                Some((2, AvFilterStreamKind::Video)),
            ),
        ];
        for (open_type, config, av_hint) in cases.drain(..) {
            let mut demux = DemuxHandle::new(open_type as i32);
            let filter = demux.register_filter(1, open_type, 4096);
            assert!(demux.configure_filter_with_summary(filter.filter_id, config));
            if let Some((stream_type, stream_kind)) = av_hint {
                assert!(demux.set_filter_av_stream_type_hint(
                    filter.filter_id,
                    stream_type,
                    stream_kind
                ));
            }
            assert!(demux.start_filter(filter.filter_id));
            demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![9, 8, 7]));
            assert!(demux.stop_filter(filter.filter_id));
            assert_eq!(
                demux
                    .filter_record(filter.filter_id)
                    .map(|record| (record.started, record.queued_bytes, record.pending_overflow)),
                Some((false, 0, false))
            );
            assert!(demux.drain_filter_payloads(filter.filter_id).is_empty());
        }
    }

    #[test]
    fn reconfigure_clears_old_linkage_and_queued_payload() {
        let mut demux = DemuxHandle::new(0);
        let source = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let downstream = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        assert!(demux.configure_filter_with_summary(source.filter_id, section_config()));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, section_config()));
        assert!(demux.set_filter_data_source(downstream.filter_id, source.filter_id));
        demux.push_filter_payload(downstream.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert_eq!(
            demux
                .filter_record(downstream.filter_id)
                .map(|record| (record.data_source_filter_id, record.queued_bytes)),
            Some((Some(source.filter_id), 3))
        );

        assert!(demux.configure_filter_with_summary(downstream.filter_id, section_config()));

        assert_eq!(
            demux
                .filter_record(downstream.filter_id)
                .map(|record| (record.data_source_filter_id, record.queued_bytes)),
            Some((None, 0))
        );
        assert!(demux.drain_filter_payloads(downstream.filter_id).is_empty());
    }

    #[test]
    fn upstream_unregister_stops_downstream_and_clears_queue() {
        let mut demux = DemuxHandle::new(0);
        let source = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        let downstream = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        assert!(demux.configure_filter_with_summary(source.filter_id, section_config()));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, section_config()));
        assert!(demux.set_filter_data_source(downstream.filter_id, source.filter_id));
        assert!(demux.start_filter(downstream.filter_id));
        demux.push_filter_payload(downstream.filter_id, FilterPayload::Bytes(vec![4, 5, 6]));

        assert!(demux.unregister_filter(source.filter_id).is_some());

        assert_eq!(
            demux
                .filter_record(downstream.filter_id)
                .map(|record| (
                    record.data_source_filter_id,
                    record.started,
                    record.queued_bytes,
                    record.pending_overflow
                )),
            Some((None, false, 0, false))
        );
        assert!(demux
            .drain_filter_payloads_for_delivery(downstream.filter_id)
            .is_empty());
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
        let private_section = vec![0x80, 0x00, 0x03, 0xaa, 0xbb, 0xcc];
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
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
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        assert_eq!(
            demux.start_filter_result(filter.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(0x0000)));
        assert_eq!(demux.start_filter_result(filter.filter_id), Ok(()));
        assert!(demux.filter_record(filter.filter_id).unwrap().started);
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

        let record_before_dvr_config = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        assert!(demux.configure_filter_with_summary(
            record_before_dvr_config.filter_id,
            record_config(0x0102)
        ));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record_before_dvr_config.filter_id),
            Err(DemuxConfigError::InvalidState)
        );

        assert!(
            demux.configure_dvr_with_summary(dvr.dvr_id, dvr_config(DemuxPathDirection::Record))
        );

        let section = demux.register_filter(1, FilterOpenType::TsSection, 4096);
        assert!(demux.configure_filter_with_summary(section.filter_id, section_config(0x0000)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, section.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );

        let audio = demux.register_filter(1, FilterOpenType::TsAudio, 4096);
        assert!(demux.configure_filter_with_summary(audio.filter_id, av_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, audio.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );

        let pes = demux.register_filter(1, FilterOpenType::TsPes, 4096);
        assert!(demux.configure_filter_with_summary(pes.filter_id, pes_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, pes.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );

        let unconfigured_record = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, unconfigured_record.filter_id),
            Err(DemuxConfigError::InvalidState)
        );

        let record = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
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
        let record = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record.filter_id),
            Err(DemuxConfigError::InvalidKind)
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
        assert!(demux
            .configure_dvr_with_summary(record_dvr.dvr_id, dvr_config(DemuxPathDirection::Record)));
        assert_eq!(
            demux.start_dvr_result(record_dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert!(!demux.dvr_record(record_dvr.dvr_id).unwrap().started);

        let record = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(record_dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert_eq!(demux.start_dvr_result(record_dvr.dvr_id), Ok(()));
        assert!(demux.dvr_record(record_dvr.dvr_id).unwrap().started);

        let playback_dvr = demux
            .register_dvr(DemuxPathDirection::Playback, 4096)
            .unwrap();
        assert_eq!(
            demux.start_dvr_result(playback_dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert!(demux.configure_dvr_with_summary(
            playback_dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ));
        assert_eq!(demux.start_dvr_result(playback_dvr.dvr_id), Ok(()));
        assert!(demux.dvr_record(playback_dvr.dvr_id).unwrap().started);
    }

    #[test]
    fn record_dvr_start_rejects_after_last_filter_detached() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, 4096)
            .unwrap();
        assert!(
            demux.configure_dvr_with_summary(dvr.dvr_id, dvr_config(DemuxPathDirection::Record))
        );
        let record = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert!(demux.stop_dvr(dvr.dvr_id));
        assert!(demux.detach_filter_from_dvr(dvr.dvr_id, record.filter_id));
        assert_eq!(
            demux.start_dvr_result(dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );
    }

    #[test]
    fn record_dvr_detach_stops_delivery_for_detached_pid() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(
            demux.configure_dvr_with_summary(dvr.dvr_id, dvr_config(DemuxPathDirection::Record))
        );
        let record = demux.register_filter(1, FilterOpenType::TsRecord, TS_PACKET_SIZE * 4);
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

        assert!(demux.detach_filter_from_dvr(dvr.dvr_id, record.filter_id));
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
            .register_dvr(DemuxPathDirection::Record, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(
            demux.configure_dvr_with_summary(dvr.dvr_id, dvr_config(DemuxPathDirection::Record))
        );
        let record = demux.register_filter(1, FilterOpenType::TsRecord, TS_PACKET_SIZE * 4);
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

        let record2 = demux.register_filter(1, FilterOpenType::TsRecord, TS_PACKET_SIZE * 4);
        assert!(demux.configure_filter_with_summary(record2.filter_id, record_config(0x0101)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(dvr.dvr_id, record2.filter_id),
            Ok(())
        );
        demux
            .filter_record_mut_for_test(record2.filter_id)
            .configured = false;
        assert_eq!(
            demux.start_dvr_result(dvr.dvr_id),
            Err(DemuxConfigError::InvalidState)
        );

        demux
            .filter_record_mut_for_test(record2.filter_id)
            .configured = true;
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
    fn record_filter_capacity_supports_one_service_pid_set() {
        let mut demux = DemuxHandle::new(0);
        let mut filters = Vec::new();
        for index in 0..maleicacid_tuner_hal_common::DEMUX_MAX_RECORD_FILTERS {
            let filter = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
            assert!(demux.configure_filter_with_summary(
                filter.filter_id,
                record_config(0x0100 + index as u16)
            ));
            filters.push(filter.filter_id);
        }
        let extra = demux.register_filter(1, FilterOpenType::TsRecord, 4096);
        assert_eq!(
            demux.configure_record_pid_filter(extra.filter_id, 0x1fff),
            Err(DemuxConfigError::CapacityExceeded)
        );
        assert_eq!(filters.len(), 32);
    }

    #[test]
    fn internal_filter_overflow_sets_pending_overflow() {
        let mut demux = DemuxHandle::new(0);
        let filter = demux.register_filter(1, FilterOpenType::TsSection, 3);
        assert!(demux.configure_filter_with_summary(filter.filter_id, section_config(0x0000)));
        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![1, 2, 3]));
        assert!(!demux.take_filter_pending_overflow(filter.filter_id));
        demux.push_filter_payload(filter.filter_id, FilterPayload::Bytes(vec![4, 5]));
        assert!(demux.take_filter_pending_overflow(filter.filter_id));
        assert!(!demux.take_filter_pending_overflow(filter.filter_id));
        assert!(demux.filter_record(filter.filter_id).unwrap().drop_bytes > 0);
    }

    #[test]
    fn record_dvr_overflow_drops_new_packet_and_reports_pending_overflow() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Record, TS_PACKET_SIZE)
            .unwrap();
        assert!(
            demux.configure_dvr_with_summary(dvr.dvr_id, dvr_config(DemuxPathDirection::Record))
        );
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
            .register_dvr(DemuxPathDirection::Record, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(demux
            .configure_dvr_with_summary(record_dvr.dvr_id, dvr_config(DemuxPathDirection::Record)));
        let record = demux.register_filter(1, FilterOpenType::TsRecord, TS_PACKET_SIZE * 4);
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
            .register_dvr(DemuxPathDirection::Playback, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(
            demux.configure_dvr_with_summary(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback))
        );
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
            .register_dvr(DemuxPathDirection::Record, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(demux
            .configure_dvr_with_summary(record_dvr.dvr_id, dvr_config(DemuxPathDirection::Record)));
        let record = demux.register_filter(1, FilterOpenType::TsRecord, TS_PACKET_SIZE * 4);
        assert!(demux.configure_filter_with_summary(record.filter_id, record_config(0x0100)));
        assert_eq!(
            demux.attach_filter_to_dvr_result(record_dvr.dvr_id, record.filter_id),
            Ok(())
        );
        assert_eq!(demux.start_filter_result(record.filter_id), Ok(()));
        assert_eq!(demux.start_dvr_result(record_dvr.dvr_id), Ok(()));

        let playback_dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(demux.configure_dvr_with_summary(
            playback_dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ));
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
            .register_dvr(DemuxPathDirection::Playback, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(
            demux.configure_dvr_with_summary(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback))
        );
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
        let section = vec![0x80, 0x00, 0x03, 0x10, 0x20, 0x30];
        let mut demux = DemuxHandle::new(0);
        let section_filter =
            demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
        let playback_dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(demux.configure_filter_with_summary(section_filter.filter_id, section_config(pid)));
        assert!(demux.configure_dvr_with_summary(
            playback_dvr.dvr_id,
            dvr_config(DemuxPathDirection::Playback)
        ));
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
        let section = vec![0x80, 0x00, 0x03, 0x44, 0x55, 0x66];
        let mut demux = DemuxHandle::new(0);
        let section_filter =
            demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
        let record_filter = demux.register_filter(1, FilterOpenType::TsRecord, TS_PACKET_SIZE * 4);
        let record_dvr = demux
            .register_dvr(DemuxPathDirection::Record, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(demux.configure_filter_with_summary(section_filter.filter_id, section_config(pid)));
        assert!(demux.configure_filter_with_summary(record_filter.filter_id, record_config(pid)));
        assert!(demux
            .configure_dvr_with_summary(record_dvr.dvr_id, dvr_config(DemuxPathDirection::Record)));
        assert!(demux.attach_filter_to_dvr(record_dvr.dvr_id, record_filter.filter_id));
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
        assert_eq!(
            demux.drain_filter_payloads(record_filter.filter_id).len(),
            1
        );
        assert_eq!(demux.drain_dvr_payloads(record_dvr.dvr_id).len(), 1);
    }

    #[test]
    fn filter_flush_generation_suppresses_stale_partial_section_without_breaking_peer_filter() {
        let pid = 0x0120;
        let section = vec![0x80, 0x00, 0x03, 0xaa, 0xbb, 0xcc];
        let mut demux = DemuxHandle::new(0);
        let flushed = demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
        let peer = demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
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
        for source in TS_LINKABLE_OPEN_TYPES {
            for destination in TS_LINKABLE_OPEN_TYPES {
                assert!(can_link_filter_open_types(*source, *destination));
            }
        }
        assert!(!can_link_filter_open_types(
            FilterOpenType::NonTs,
            FilterOpenType::TsSection
        ));
        assert!(!can_link_filter_open_types(
            FilterOpenType::TsSection,
            FilterOpenType::NonTs
        ));
    }

    #[test]
    fn set_filter_data_source_rejects_unadvertised_non_ts_linkage_without_mutating_graph() {
        let mut demux = DemuxHandle::new(0);
        let source = demux.register_filter(0, FilterOpenType::NonTs, TS_PACKET_SIZE * 4);
        let destination = demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
        assert_eq!(
            demux.set_filter_data_source_result(destination.filter_id, source.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );
        assert_eq!(
            demux
                .filter_record(destination.filter_id)
                .unwrap()
                .data_source_filter_id,
            None
        );
    }

    #[test]
    fn downstream_flush_generation_suppresses_stale_linkage_section() {
        let pid = 0x0121;
        let section = vec![0x80, 0x00, 0x03, 0x11, 0x22, 0x33];
        let mut demux = DemuxHandle::new(0);
        let source = demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
        let downstream = demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
        assert!(demux.configure_filter_with_summary(source.filter_id, section_config(pid)));
        assert!(demux.configure_filter_with_summary(downstream.filter_id, section_config(pid)));
        assert!(demux.set_filter_data_source(downstream.filter_id, source.filter_id));
        assert!(demux.start_filter(source.filter_id));
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
        let pes = demux.register_filter(1, FilterOpenType::TsPes, TS_PACKET_SIZE * 4);
        let av = demux.register_filter(1, FilterOpenType::TsVideo, TS_PACKET_SIZE * 4);
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
            .register_dvr(DemuxPathDirection::Playback, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(
            demux.configure_dvr_with_summary(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback))
        );
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
        assert!(demux.flush_dvr(dvr.dvr_id));
        assert_eq!(demux.playback_diagnostics(dvr.dvr_id), Some((0, 0, 0)));
    }

    #[test]
    fn playback_residual_drops_malformed_full_packet_with_diagnostic() {
        let mut demux = DemuxHandle::new(0);
        let dvr = demux
            .register_dvr(DemuxPathDirection::Playback, TS_PACKET_SIZE * 4)
            .unwrap();
        assert!(
            demux.configure_dvr_with_summary(dvr.dvr_id, dvr_config(DemuxPathDirection::Playback))
        );
        assert_eq!(demux.start_dvr_result(dvr.dvr_id), Ok(()));
        let malformed = [0u8; TS_PACKET_SIZE];
        assert!(demux.inject_playback_payload(dvr.dvr_id, &malformed));
        assert_eq!(
            demux.playback_diagnostics(dvr.dvr_id),
            Some((0, 0, TS_PACKET_SIZE as u64))
        );
    }

    #[test]
    fn set_filter_data_source_rejects_self_cycle_cycle_and_started_rewire_without_mutating_graph() {
        let mut demux = DemuxHandle::new(0);
        let a = demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
        let b = demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
        let c = demux.register_filter(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4);
        assert_eq!(
            demux.set_filter_data_source_result(a.filter_id, a.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );
        assert_eq!(
            demux.set_filter_data_source_result(b.filter_id, a.filter_id),
            Ok(())
        );
        assert_eq!(
            demux.set_filter_data_source_result(c.filter_id, b.filter_id),
            Ok(())
        );
        assert_eq!(
            demux.set_filter_data_source_result(a.filter_id, c.filter_id),
            Err(DemuxConfigError::InvalidKind)
        );
        assert_eq!(
            demux
                .filter_record(a.filter_id)
                .unwrap()
                .data_source_filter_id,
            None
        );

        assert!(demux.configure_filter_with_summary(c.filter_id, section_config(0x0123)));
        assert!(demux.start_filter(c.filter_id));
        assert_eq!(
            demux.set_filter_data_source_result(c.filter_id, a.filter_id),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(
            demux
                .filter_record(c.filter_id)
                .unwrap()
                .data_source_filter_id,
            Some(b.filter_id)
        );
    }

    #[test]
    fn r50ap4_closed_demux_rejects_new_filter_and_dvr_registration() {
        let mut demux = DemuxHandle::new(0);
        demux.close();
        assert_eq!(
            demux.register_filter_result(1, FilterOpenType::TsSection, TS_PACKET_SIZE * 4),
            Err(DemuxConfigError::InvalidState)
        );
        assert_eq!(
            demux.register_dvr(DemuxPathDirection::Record, TS_PACKET_SIZE * 4),
            Err(DemuxConfigError::InvalidState)
        );
    }
}
