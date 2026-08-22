use std::cell::Cell;
use std::collections::BTreeSet;

use super::watermark_classifier::DvrWatermarkClassifier;

#[cfg(test)]
use maleicacid_tuner_hal2_common::{
    TsPacketBufferDrain, TsPacketCompletionBuffer, TS_PACKET_SIZE,
};
#[cfg(not(test))]
use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;

const DVR_STATUS_BIT_0: i32 = 1 << 0;
const DVR_STATUS_BIT_1: i32 = 1 << 1;
const DVR_STATUS_BIT_2: i32 = 1 << 2;
const DVR_STATUS_BIT_3: i32 = 1 << 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrKind {
    Record,
    Playback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrDataFormat {
    Ts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrRuntimeState {
    Open,
    Configured,
    Started,
    Stopped,
    Closing,
    CleanupFailed,
    Closed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordDvrFilterRelationState {
    Healthy,
    Quarantined,
}

impl DvrRuntimeState {
    pub const fn is_closed_or_failed(self) -> bool {
        matches!(
            self,
            Self::Closing | Self::CleanupFailed | Self::Closed | Self::Failed
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrStatusEvent {
    RecordDataReady,
    RecordLowWater,
    RecordHighWater,
    RecordOverflow,
    PlaybackSpaceEmpty,
    PlaybackSpaceAlmostEmpty,
    PlaybackSpaceAlmostFull,
    PlaybackSpaceFull,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlaybackStats {
    pub injected_bytes: u64,
    pub injected_packets: u64,
    pub malformed_packets: u64,
    pub dropped_bytes: u64,
    pub counter_saturated: bool,
}

impl PlaybackStats {
    fn add_counter(value: &mut u64, amount: u64, saturated: &mut bool) {
        match value.checked_add(amount) {
            Some(next) => {
                *value = next;
                if amount > 0 && next == u64::MAX {
                    *saturated = true;
                }
            }
            None => {
                *value = u64::MAX;
                *saturated = true;
            }
        }
    }

    fn add(&mut self, injected_packets: usize, malformed_packets: usize, malformed_bytes: usize) {
        let injected_packets = u64::try_from(injected_packets).unwrap_or(u64::MAX);
        let malformed_packets = u64::try_from(malformed_packets).unwrap_or(u64::MAX);
        let malformed_bytes = u64::try_from(malformed_bytes).unwrap_or(u64::MAX);
        let packet_size = u64::try_from(TS_PACKET_SIZE).unwrap_or(u64::MAX);
        let injected_bytes = injected_packets.saturating_mul(packet_size);
        let malformed_packet_bytes = malformed_packets.saturating_mul(packet_size);

        Self::add_counter(
            &mut self.injected_bytes,
            injected_bytes,
            &mut self.counter_saturated,
        );
        Self::add_counter(
            &mut self.injected_packets,
            injected_packets,
            &mut self.counter_saturated,
        );
        Self::add_counter(
            &mut self.malformed_packets,
            malformed_packets,
            &mut self.counter_saturated,
        );
        Self::add_counter(
            &mut self.dropped_bytes,
            malformed_bytes,
            &mut self.counter_saturated,
        );
        Self::add_counter(
            &mut self.dropped_bytes,
            malformed_packet_bytes,
            &mut self.counter_saturated,
        );
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlaybackFlushDiagnostic {
    pub flush_count: u64,
    pub total_dropped_bytes: u64,
    pub last_dropped_bytes: u64,
    pub counter_saturated: bool,
}

impl PlaybackFlushDiagnostic {
    fn record(&mut self, dropped_bytes: usize) {
        let dropped_bytes = u64::try_from(dropped_bytes).unwrap_or(u64::MAX);
        PlaybackStats::add_counter(
            &mut self.flush_count,
            1,
            &mut self.counter_saturated,
        );
        PlaybackStats::add_counter(
            &mut self.total_dropped_bytes,
            dropped_bytes,
            &mut self.counter_saturated,
        );
        self.last_dropped_bytes = dropped_bytes;
    }

    fn augment_last(&mut self, dropped_bytes: usize) {
        let dropped_bytes = u64::try_from(dropped_bytes).unwrap_or(u64::MAX);
        PlaybackStats::add_counter(
            &mut self.total_dropped_bytes,
            dropped_bytes,
            &mut self.counter_saturated,
        );
        PlaybackStats::add_counter(
            &mut self.last_dropped_bytes,
            dropped_bytes,
            &mut self.counter_saturated,
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrRuntimeSnapshot {
    pub kind: DvrKind,
    pub state: DvrRuntimeState,
    pub generation: u64,
    pub buffer_size: i32,
    pub callback_present: bool,
    pub status_check_interval_ms: u64,
    pub queue_present: bool,
    #[cfg(test)]
    pub playback_assembler_present: bool,
    #[cfg(test)]
    pub playback_completion: TsPacketCompletionBuffer,
    #[cfg(test)]
    pub playback_processing_buffer: Vec<u8>,
    pub playback_stats: PlaybackStats,
    pub playback_flush_diagnostic: PlaybackFlushDiagnostic,
    pub attached_record_filters: BTreeSet<i32>,
    pub record_filter_relation_generation: u64,
    pub record_filter_relation_state: RecordDvrFilterRelationState,
    pub pending_overflow: bool,
    pub pending_data_ready: bool,
    pub last_watermark_status: Option<DvrStatusEvent>,
    pub status_mask: i32,
    pub low_threshold_bytes: usize,
    pub high_threshold_bytes: usize,
    pub data_format: Option<DvrDataFormat>,
    pub packet_size: Option<i64>,
    pub callback_unhealthy: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DvrRuntime {
    dvr_id: i32,
    kind: DvrKind,
    state: DvrRuntimeState,
    generation: u64,
    buffer_size: i32,
    callback_present: bool,
    status_check_interval_ms: u64,
    queue_present: bool,
    #[cfg(test)]
    playback_assembler_present: bool,
    #[cfg(test)]
    playback_completion: TsPacketCompletionBuffer,
    #[cfg(test)]
    playback_processing_buffer: Vec<u8>,
    playback_stats: PlaybackStats,
    playback_flush_diagnostic: PlaybackFlushDiagnostic,
    attached_record_filters: BTreeSet<i32>,
    record_filter_relation_generation: u64,
    record_filter_relation_state: RecordDvrFilterRelationState,
    pending_overflow: Cell<bool>,
    pending_data_ready: Cell<bool>,
    last_watermark_status: Cell<Option<DvrStatusEvent>>,
    status_mask: i32,
    low_threshold_bytes: usize,
    high_threshold_bytes: usize,
    data_format: Option<DvrDataFormat>,
    packet_size: Option<i64>,
    callback_unhealthy: bool,
}

impl DvrRuntime {
    #[cfg(test)]
    pub(crate) fn new(dvr_id: i32, kind: DvrKind, generation: u64) -> Self {
        Self::new_open_request(dvr_id, kind, generation, 0, false)
    }
    pub(crate) fn new_open_request(
        dvr_id: i32,
        kind: DvrKind,
        generation: u64,
        buffer_size: i32,
        callback_present: bool,
    ) -> Self {
        Self {
            dvr_id,
            kind,
            state: DvrRuntimeState::Open,
            generation,
            buffer_size,
            callback_present,
            status_check_interval_ms: 0,
            queue_present: buffer_size > 0,
            #[cfg(test)]
            playback_assembler_present: matches!(kind, DvrKind::Playback),
            #[cfg(test)]
            playback_completion: TsPacketCompletionBuffer::default(),
            #[cfg(test)]
            playback_processing_buffer: Vec::new(),
            playback_stats: PlaybackStats::default(),
            playback_flush_diagnostic: PlaybackFlushDiagnostic::default(),
            attached_record_filters: BTreeSet::new(),
            record_filter_relation_generation: 0,
            record_filter_relation_state: RecordDvrFilterRelationState::Healthy,
            pending_overflow: Cell::new(false),
            pending_data_ready: Cell::new(false),
            last_watermark_status: Cell::new(None),
            status_mask: 0,
            low_threshold_bytes: 0,
            high_threshold_bytes: 0,
            data_format: None,
            packet_size: None,
            callback_unhealthy: false,
        }
    }
    pub fn dvr_id(&self) -> i32 {
        self.dvr_id
    }
    pub fn kind(&self) -> DvrKind {
        self.kind
    }
    pub fn state(&self) -> DvrRuntimeState {
        self.state
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
    }
    pub fn buffer_size(&self) -> i32 {
        self.buffer_size
    }
    pub fn queue_present(&self) -> bool {
        self.queue_present
    }
    pub fn allows_queue_desc(&self) -> bool {
        matches!(
            self.state,
            DvrRuntimeState::Open
                | DvrRuntimeState::Configured
                | DvrRuntimeState::Started
                | DvrRuntimeState::Stopped
        ) && self.queue_present
    }
    pub fn attached_record_filters(&self) -> &BTreeSet<i32> {
        &self.attached_record_filters
    }
    pub const fn record_filter_relation_generation(&self) -> u64 {
        self.record_filter_relation_generation
    }
    pub const fn record_filter_relation_state(&self) -> RecordDvrFilterRelationState {
        self.record_filter_relation_state
    }
    pub fn snapshot(&self) -> DvrRuntimeSnapshot {
        DvrRuntimeSnapshot {
            kind: self.kind,
            state: self.state,
            generation: self.generation,
            buffer_size: self.buffer_size,
            callback_present: self.callback_present,
            status_check_interval_ms: self.status_check_interval_ms,
            queue_present: self.queue_present,
            #[cfg(test)]
            playback_assembler_present: self.playback_assembler_present,
            #[cfg(test)]
            playback_completion: self.playback_completion.clone(),
            #[cfg(test)]
            playback_processing_buffer: self.playback_processing_buffer.clone(),
            playback_stats: self.playback_stats,
            playback_flush_diagnostic: self.playback_flush_diagnostic,
            attached_record_filters: self.attached_record_filters.clone(),
            record_filter_relation_generation: self.record_filter_relation_generation,
            record_filter_relation_state: self.record_filter_relation_state,
            pending_overflow: self.pending_overflow.get(),
            pending_data_ready: self.pending_data_ready.get(),
            last_watermark_status: self.last_watermark_status.get(),
            status_mask: self.status_mask,
            low_threshold_bytes: self.low_threshold_bytes,
            high_threshold_bytes: self.high_threshold_bytes,
            data_format: self.data_format,
            packet_size: self.packet_size,
            callback_unhealthy: self.callback_unhealthy,
        }
    }

    pub fn restore(&mut self, snapshot: DvrRuntimeSnapshot) {
        self.kind = snapshot.kind;
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.buffer_size = snapshot.buffer_size;
        self.callback_present = snapshot.callback_present;
        self.status_check_interval_ms = snapshot.status_check_interval_ms;
        self.queue_present = snapshot.queue_present;
        #[cfg(test)]
        {
            self.playback_assembler_present = snapshot.playback_assembler_present;
            self.playback_completion = snapshot.playback_completion;
            self.playback_processing_buffer = snapshot.playback_processing_buffer;
        }
        self.playback_stats = snapshot.playback_stats;
        self.playback_flush_diagnostic = snapshot.playback_flush_diagnostic;
        self.attached_record_filters = snapshot.attached_record_filters;
        self.record_filter_relation_generation = snapshot.record_filter_relation_generation;
        self.record_filter_relation_state = snapshot.record_filter_relation_state;
        self.pending_overflow.set(snapshot.pending_overflow);
        self.pending_data_ready.set(snapshot.pending_data_ready);
        self.last_watermark_status
            .set(snapshot.last_watermark_status);
        self.status_mask = snapshot.status_mask;
        self.low_threshold_bytes = snapshot.low_threshold_bytes;
        self.high_threshold_bytes = snapshot.high_threshold_bytes;
        self.data_format = snapshot.data_format;
        self.packet_size = snapshot.packet_size;
        self.callback_unhealthy = snapshot.callback_unhealthy;
    }

    pub fn configure_with_generation(&mut self, generation: u64) {
        self.generation = generation;
        self.queue_present = true;
        self.state = DvrRuntimeState::Configured;
    }

    #[cfg(test)]
    pub(crate) fn install_test_playback_processing_buffer(&mut self, buffer: Vec<u8>) {
        self.playback_assembler_present = matches!(self.kind, DvrKind::Playback);
        self.playback_processing_buffer = buffer;
    }

    pub fn clear_queue_marker(&mut self) -> bool {
        let had_queue = self.queue_present;
        self.queue_present = false;
        had_queue
    }

    #[cfg(test)]
    pub fn push_playback_bytes(&mut self, data: &[u8]) -> TsPacketBufferDrain {
        self.playback_completion.push(data)
    }
    #[cfg(test)]
    pub fn take_playback_processing_buffer(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.playback_processing_buffer)
    }
    #[cfg(test)]
    pub fn restore_playback_processing_buffer(&mut self, buffer: Vec<u8>) {
        self.playback_processing_buffer = buffer;
    }
    #[cfg(test)]
    pub fn drain_playback_completion_for_boundary(&mut self) -> usize {
        let drain = self.playback_completion.drain_for_boundary();
        drain
            .packets
            .len()
            .saturating_mul(TS_PACKET_SIZE)
            .saturating_add(drain.malformed_bytes)
    }
    pub fn playback_stats(&self) -> PlaybackStats {
        self.playback_stats
    }
    pub fn playback_flush_diagnostic(&self) -> PlaybackFlushDiagnostic {
        self.playback_flush_diagnostic
    }
    pub fn note_playback_consume(
        &mut self,
        injected_packets: usize,
        malformed_packets: usize,
        malformed_bytes: usize,
    ) {
        self.playback_stats
            .add(injected_packets, malformed_packets, malformed_bytes);
    }
    pub fn reset_playback_stats_after_flush(&mut self, dropped_bytes: usize) {
        self.playback_flush_diagnostic.record(dropped_bytes);
        if dropped_bytes > 0 {
            eprintln!(
                "maleicacid-tuner-hal2-dvr-playback-diagnostic: dvr_id={} boundary=flush dropped_bytes={} total_dropped_bytes={}",
                self.dvr_id,
                dropped_bytes,
                self.playback_flush_diagnostic.total_dropped_bytes,
            );
        }
        self.playback_stats = PlaybackStats::default();
    }
    pub fn augment_playback_flush_diagnostic(&mut self, dropped_bytes: usize) {
        if dropped_bytes == 0 {
            return;
        }
        self.playback_flush_diagnostic.augment_last(dropped_bytes);
        eprintln!(
            "maleicacid-tuner-hal2-dvr-playback-diagnostic: dvr_id={} boundary=flush retained_dropped_bytes={} total_dropped_bytes={}",
            self.dvr_id,
            dropped_bytes,
            self.playback_flush_diagnostic.total_dropped_bytes,
        );
    }
    pub(crate) fn commit_record_filter_relation(
        &mut self,
        generation: u64,
        attached_record_filters: BTreeSet<i32>,
    ) {
        self.attached_record_filters = attached_record_filters;
        self.record_filter_relation_generation = generation;
    }
    pub(crate) fn quarantine_record_filter_relation(&mut self) {
        self.record_filter_relation_state = RecordDvrFilterRelationState::Quarantined;
    }
    pub fn clear_pending_overflow(&mut self) {
        self.pending_overflow.set(false);
    }
    pub fn mark_pending_overflow(&mut self) {
        self.pending_overflow.set(true);
    }
    pub fn mark_pending_data_ready(&mut self) {
        self.pending_data_ready.set(true);
    }
    pub fn configure_settings(
        &mut self,
        status_mask: i32,
        low_threshold_bytes: usize,
        high_threshold_bytes: usize,
        data_format: DvrDataFormat,
        packet_size: i64,
    ) {
        self.status_mask = status_mask;
        self.low_threshold_bytes = low_threshold_bytes;
        self.high_threshold_bytes = high_threshold_bytes;
        self.data_format = Some(data_format);
        self.packet_size = Some(packet_size);
        self.last_watermark_status.set(None);
    }
    pub fn set_status_check_interval_ms(&mut self, interval_ms: u64) {
        self.status_check_interval_ms = interval_ms;
    }
    pub fn mark_callback_unhealthy(&mut self) {
        self.callback_unhealthy = true;
    }

    fn status_enabled(&self, bit: i32) -> bool {
        (self.status_mask & bit) != 0
    }

    pub fn status_event_for_snapshot(
        &self,
        readable_bytes: usize,
        writable_bytes: usize,
    ) -> Option<DvrStatusEvent> {
        if self.kind == DvrKind::Record && self.pending_overflow.replace(false) {
            return self
                .status_enabled(DVR_STATUS_BIT_3)
                .then_some(DvrStatusEvent::RecordOverflow);
        }
        if self.kind == DvrKind::Record && self.pending_data_ready.replace(false) {
            return self
                .status_enabled(DVR_STATUS_BIT_0)
                .then_some(DvrStatusEvent::RecordDataReady);
        }

        let semantic_status = DvrWatermarkClassifier::classify(
            self.kind,
            readable_bytes,
            writable_bytes,
            self.low_threshold_bytes,
            self.high_threshold_bytes,
        );
        let Some(semantic_status) = semantic_status else {
            return None;
        };
        if self.last_watermark_status.replace(Some(semantic_status)) == Some(semantic_status) {
            return None;
        }
        let enabled_bit = match semantic_status {
            DvrStatusEvent::RecordLowWater | DvrStatusEvent::PlaybackSpaceAlmostEmpty => {
                DVR_STATUS_BIT_1
            }
            DvrStatusEvent::RecordHighWater | DvrStatusEvent::PlaybackSpaceAlmostFull => {
                DVR_STATUS_BIT_2
            }
            DvrStatusEvent::PlaybackSpaceFull => DVR_STATUS_BIT_3,
            DvrStatusEvent::PlaybackSpaceEmpty => DVR_STATUS_BIT_0,
            DvrStatusEvent::RecordDataReady | DvrStatusEvent::RecordOverflow => return None,
        };
        self.status_enabled(enabled_bit).then_some(semantic_status)
    }

    pub fn mark_started(&mut self) {
        self.state = DvrRuntimeState::Started;
    }
    pub fn mark_stopped(&mut self) {
        self.state = DvrRuntimeState::Stopped;
    }
    pub fn mark_failed(&mut self) {
        self.state = DvrRuntimeState::Failed;
    }
}
