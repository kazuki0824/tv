use std::collections::BTreeSet;

#[cfg(test)]
use maleicacid_tuner_hal2_common::TsPacketBufferDrain;
use maleicacid_tuner_hal2_common::TsPacketCompletionBuffer;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrRuntimeSnapshot {
    pub kind: DvrKind,
    pub state: DvrRuntimeState,
    pub generation: u64,
    pub buffer_size: i32,
    pub callback_present: bool,
    pub status_check_interval_ms: u64,
    pub queue_present: bool,
    pub playback_assembler_present: bool,
    pub playback_completion: TsPacketCompletionBuffer,
    pub attached_record_filters: BTreeSet<i32>,
    pub pending_overflow: bool,
    pub status_mask: i32,
    pub low_threshold_bytes: usize,
    pub high_threshold_bytes: usize,
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
    playback_assembler_present: bool,
    playback_completion: TsPacketCompletionBuffer,
    attached_record_filters: BTreeSet<i32>,
    pending_overflow: bool,
    status_mask: i32,
    low_threshold_bytes: usize,
    high_threshold_bytes: usize,
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
            queue_present: false,
            playback_assembler_present: matches!(kind, DvrKind::Playback),
            playback_completion: TsPacketCompletionBuffer::default(),
            attached_record_filters: BTreeSet::new(),
            pending_overflow: false,
            status_mask: 0,
            low_threshold_bytes: 0,
            high_threshold_bytes: 0,
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
    pub fn buffer_size(&self) -> i32 {
        self.buffer_size
    }
    pub fn allows_queue_desc(&self) -> bool {
        matches!(
            self.state,
            DvrRuntimeState::Configured | DvrRuntimeState::Started | DvrRuntimeState::Stopped
        ) && self.queue_present
    }
    pub fn attached_record_filters(&self) -> &BTreeSet<i32> {
        &self.attached_record_filters
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
            playback_assembler_present: self.playback_assembler_present,
            playback_completion: self.playback_completion.clone(),
            attached_record_filters: self.attached_record_filters.clone(),
            pending_overflow: self.pending_overflow,
            status_mask: self.status_mask,
            low_threshold_bytes: self.low_threshold_bytes,
            high_threshold_bytes: self.high_threshold_bytes,
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
        self.playback_assembler_present = snapshot.playback_assembler_present;
        self.playback_completion = snapshot.playback_completion;
        self.attached_record_filters = snapshot.attached_record_filters;
        self.pending_overflow = snapshot.pending_overflow;
        self.status_mask = snapshot.status_mask;
        self.low_threshold_bytes = snapshot.low_threshold_bytes;
        self.high_threshold_bytes = snapshot.high_threshold_bytes;
        self.callback_unhealthy = snapshot.callback_unhealthy;
    }

    pub fn configure_with_generation(&mut self, generation: u64) {
        self.generation = generation;
        self.queue_present = true;
        self.playback_assembler_present = matches!(self.kind, DvrKind::Playback);
        self.playback_completion = TsPacketCompletionBuffer::default();
        self.pending_overflow = false;
        self.callback_unhealthy = false;
        self.state = DvrRuntimeState::Configured;
    }

    pub fn clear_queue_marker(&mut self) -> bool {
        let had_queue = self.queue_present;
        self.queue_present = false;
        had_queue
    }

    pub fn reset_playback_assembler_marker(&mut self) -> bool {
        let had = self.playback_assembler_present;
        self.playback_assembler_present = false;
        self.playback_completion = TsPacketCompletionBuffer::default();
        had
    }
    #[cfg(test)]
    pub fn push_playback_bytes(&mut self, data: &[u8]) -> TsPacketBufferDrain {
        self.playback_completion.push(data)
    }
    pub fn clear_playback_completion(&mut self) {
        self.playback_completion = TsPacketCompletionBuffer::default();
    }
    pub fn attach_record_filter(&mut self, filter_id: i32) {
        self.attached_record_filters.insert(filter_id);
    }
    pub fn detach_record_filter(&mut self, filter_id: i32) {
        self.attached_record_filters.remove(&filter_id);
    }
    pub fn clear_pending_overflow(&mut self) {
        self.pending_overflow = false;
    }
    pub fn mark_pending_overflow(&mut self) {
        self.pending_overflow = true;
    }
    pub fn configure_status_reporting(
        &mut self,
        status_mask: i32,
        low_threshold_bytes: usize,
        high_threshold_bytes: usize,
    ) {
        self.status_mask = status_mask;
        self.low_threshold_bytes = low_threshold_bytes;
        self.high_threshold_bytes = high_threshold_bytes;
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

    pub fn status_event_for_fill(&self, fill_bytes: usize) -> Option<DvrStatusEvent> {
        match self.kind {
            DvrKind::Record => {
                if self.pending_overflow && self.status_enabled(DVR_STATUS_BIT_3) {
                    return Some(DvrStatusEvent::RecordOverflow);
                }
                if fill_bytes >= self.high_threshold_bytes && self.status_enabled(DVR_STATUS_BIT_2)
                {
                    return Some(DvrStatusEvent::RecordHighWater);
                }
                if fill_bytes <= self.low_threshold_bytes && self.status_enabled(DVR_STATUS_BIT_1) {
                    return Some(DvrStatusEvent::RecordLowWater);
                }
                if fill_bytes > 0 && self.status_enabled(DVR_STATUS_BIT_0) {
                    return Some(DvrStatusEvent::RecordDataReady);
                }
                None
            }
            DvrKind::Playback => {
                let capacity = usize::try_from(self.buffer_size).ok()?;
                let available_space = capacity.saturating_sub(fill_bytes);
                if available_space == 0 && self.status_enabled(DVR_STATUS_BIT_0) {
                    Some(DvrStatusEvent::PlaybackSpaceEmpty)
                } else if available_space >= capacity && self.status_enabled(DVR_STATUS_BIT_3) {
                    Some(DvrStatusEvent::PlaybackSpaceFull)
                } else if available_space <= self.low_threshold_bytes
                    && self.status_enabled(DVR_STATUS_BIT_1)
                {
                    Some(DvrStatusEvent::PlaybackSpaceAlmostEmpty)
                } else if available_space >= self.high_threshold_bytes
                    && self.status_enabled(DVR_STATUS_BIT_2)
                {
                    Some(DvrStatusEvent::PlaybackSpaceAlmostFull)
                } else {
                    None
                }
            }
        }
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
