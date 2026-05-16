#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrDirection {
    Playback,
    Record,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterLifecycleState {
    Allocated,
    Configured,
    Started,
    Stopped,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrLifecycleState {
    Allocated,
    Configured,
    Started,
    Stopped,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueKind {
    FilterOutput,
    DvrRecord,
    DvrPlayback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterQueueDiscipline {
    PacketPassthrough,
    SectionReassembled,
    AvMediaEvent,
    RecordEventMetadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrQueueDiscipline {
    PacketPassthrough,
    PlaybackReinject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueOverflowPolicy {
    DropNew,
    DropOld,
    MetadataEntryDropNew,
    ProducerBackpressure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuePolicy {
    pub bounded_bytes: usize,
    pub bounded_entries: Option<usize>,
    pub overflow_policy: QueueOverflowPolicy,
}

impl QueuePolicy {
    pub const fn bounded_drop_old(bounded_bytes: usize) -> Self {
        Self {
            bounded_bytes,
            bounded_entries: None,
            overflow_policy: QueueOverflowPolicy::DropOld,
        }
    }

    pub const fn bounded_drop_new(bounded_bytes: usize) -> Self {
        Self {
            bounded_bytes,
            bounded_entries: None,
            overflow_policy: QueueOverflowPolicy::DropNew,
        }
    }

    pub const fn bounded_metadata_entries(bounded_entries: usize) -> Self {
        Self {
            bounded_bytes: 0,
            bounded_entries: Some(bounded_entries),
            overflow_policy: QueueOverflowPolicy::MetadataEntryDropNew,
        }
    }

    pub const fn producer_backpressure(bounded_bytes: usize) -> Self {
        Self {
            bounded_bytes,
            bounded_entries: None,
            overflow_policy: QueueOverflowPolicy::ProducerBackpressure,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrSessionState {
    pub demux_id: i32,
    pub direction: DvrDirection,
    pub attached_filter_ids: Vec<i32>,
    pub started: bool,
}

impl DvrSessionState {
    pub fn new(demux_id: i32, direction: DvrDirection) -> Self {
        Self {
            demux_id,
            direction,
            attached_filter_ids: Vec::new(),
            started: false,
        }
    }

    pub fn attach_filter(&mut self, filter_id: i32) {
        if !self.attached_filter_ids.contains(&filter_id) {
            self.attached_filter_ids.push(filter_id);
        }
    }

    pub fn detach_filter(&mut self, filter_id: i32) {
        self.attached_filter_ids.retain(|id| *id != filter_id);
    }

    pub fn start(&mut self) {
        self.started = true;
    }

    pub fn stop(&mut self) {
        self.started = false;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterContractSkeleton {
    pub filter_id: i32,
    pub buffer_size: usize,
    pub lifecycle: FilterLifecycleState,
    pub discipline: FilterQueueDiscipline,
    pub output_queue: QueuePolicy,
}

impl FilterContractSkeleton {
    pub fn new(
        filter_id: i32,
        buffer_size: usize,
        discipline: FilterQueueDiscipline,
        output_queue: QueuePolicy,
    ) -> Self {
        Self {
            filter_id,
            buffer_size,
            lifecycle: FilterLifecycleState::Allocated,
            discipline,
            output_queue,
        }
    }

    pub fn new_packet_passthrough_drop_new(filter_id: i32, buffer_size: usize) -> Self {
        Self::new(
            filter_id,
            buffer_size,
            FilterQueueDiscipline::PacketPassthrough,
            QueuePolicy::bounded_drop_new(buffer_size),
        )
    }

    pub fn new_section_reassembled(filter_id: i32, buffer_size: usize) -> Self {
        Self::new(
            filter_id,
            buffer_size,
            FilterQueueDiscipline::SectionReassembled,
            QueuePolicy::bounded_drop_new(buffer_size),
        )
    }

    pub fn new_av_media(filter_id: i32, buffer_size: usize) -> Self {
        Self::new(
            filter_id,
            buffer_size,
            FilterQueueDiscipline::AvMediaEvent,
            QueuePolicy::bounded_drop_old(buffer_size),
        )
    }

    pub fn new_record_metadata(filter_id: i32, bounded_entries: usize) -> Self {
        Self::new(
            filter_id,
            0,
            FilterQueueDiscipline::RecordEventMetadata,
            QueuePolicy::bounded_metadata_entries(bounded_entries),
        )
    }

    pub fn configure(&mut self) {
        self.lifecycle = FilterLifecycleState::Configured;
    }

    pub fn start(&mut self) {
        self.lifecycle = FilterLifecycleState::Started;
    }

    pub fn stop(&mut self) {
        self.lifecycle = FilterLifecycleState::Stopped;
    }

    pub fn close(&mut self) {
        self.lifecycle = FilterLifecycleState::Closed;
    }

    pub fn queue_model(&self) -> FilterQueueModel {
        FilterQueueModel {
            queue_kind: QueueKind::FilterOutput,
            discipline: self.discipline,
            policy: self.output_queue,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrContractSkeleton {
    pub dvr_id: i32,
    pub direction: DvrDirection,
    pub buffer_size: usize,
    pub lifecycle: DvrLifecycleState,
    pub queue: QueuePolicy,
}

impl DvrContractSkeleton {
    pub fn new(dvr_id: i32, direction: DvrDirection, buffer_size: usize) -> Self {
        Self {
            dvr_id,
            direction,
            buffer_size,
            lifecycle: DvrLifecycleState::Allocated,
            queue: match direction {
                DvrDirection::Record => QueuePolicy::bounded_drop_new(buffer_size),
                DvrDirection::Playback => QueuePolicy::producer_backpressure(buffer_size),
            },
        }
    }

    pub fn configure(&mut self) {
        self.lifecycle = DvrLifecycleState::Configured;
    }

    pub fn start(&mut self) {
        self.lifecycle = DvrLifecycleState::Started;
    }

    pub fn stop(&mut self) {
        self.lifecycle = DvrLifecycleState::Stopped;
    }

    pub fn close(&mut self) {
        self.lifecycle = DvrLifecycleState::Closed;
    }

    pub fn queue_model(&self) -> DvrQueueModel {
        DvrQueueModel {
            queue_kind: match self.direction {
                DvrDirection::Record => QueueKind::DvrRecord,
                DvrDirection::Playback => QueueKind::DvrPlayback,
            },
            discipline: match self.direction {
                DvrDirection::Record => DvrQueueDiscipline::PacketPassthrough,
                DvrDirection::Playback => DvrQueueDiscipline::PlaybackReinject,
            },
            policy: match self.direction {
                DvrDirection::Record => QueuePolicy::bounded_drop_new(self.buffer_size),
                DvrDirection::Playback => QueuePolicy::producer_backpressure(self.buffer_size),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterQueueModel {
    pub queue_kind: QueueKind,
    pub discipline: FilterQueueDiscipline,
    pub policy: QueuePolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrQueueModel {
    pub queue_kind: QueueKind,
    pub discipline: DvrQueueDiscipline,
    pub policy: QueuePolicy,
}

#[cfg(test)]
mod tests {
    use super::{
        DvrContractSkeleton, DvrDirection, DvrLifecycleState, DvrQueueDiscipline,
        FilterContractSkeleton, FilterLifecycleState, FilterQueueDiscipline, QueueKind,
        QueueOverflowPolicy,
    };

    #[test]
    fn filter_contract_exposes_explicit_queue_disciplines_and_policies() {
        let mut packet = FilterContractSkeleton::new_packet_passthrough_drop_new(1, 4096);
        assert_eq!(packet.lifecycle, FilterLifecycleState::Allocated);
        packet.configure();
        packet.start();
        let packet_model = packet.queue_model();
        assert_eq!(packet_model.queue_kind, QueueKind::FilterOutput);
        assert_eq!(packet_model.discipline, FilterQueueDiscipline::PacketPassthrough);
        assert_eq!(packet_model.policy.bounded_entries, None);
        assert_eq!(packet_model.policy.overflow_policy, QueueOverflowPolicy::DropNew);
        packet.stop();
        packet.close();
        assert_eq!(packet.lifecycle, FilterLifecycleState::Closed);

        let section = FilterContractSkeleton::new_section_reassembled(2, 4096).queue_model();
        assert_eq!(section.discipline, FilterQueueDiscipline::SectionReassembled);
        assert_eq!(section.policy.overflow_policy, QueueOverflowPolicy::DropNew);

        let av = FilterContractSkeleton::new_av_media(3, 4096).queue_model();
        assert_eq!(av.discipline, FilterQueueDiscipline::AvMediaEvent);
        assert_eq!(av.policy.overflow_policy, QueueOverflowPolicy::DropOld);

        let record = FilterContractSkeleton::new_record_metadata(4, 8).queue_model();
        assert_eq!(record.discipline, FilterQueueDiscipline::RecordEventMetadata);
        assert_eq!(record.policy.bounded_bytes, 0);
        assert_eq!(record.policy.bounded_entries, Some(8));
        assert_eq!(
            record.policy.overflow_policy,
            QueueOverflowPolicy::MetadataEntryDropNew
        );
    }

    #[test]
    fn dvr_contract_exposes_record_and_playback_queue_models() {
        let mut record = DvrContractSkeleton::new(10, DvrDirection::Record, 32768);
        record.configure();
        record.start();
        let record_model = record.queue_model();
        assert_eq!(record_model.queue_kind, QueueKind::DvrRecord);
        assert_eq!(record_model.discipline, DvrQueueDiscipline::PacketPassthrough);
        assert_eq!(record.queue.overflow_policy, QueueOverflowPolicy::DropNew);
        assert_eq!(record_model.policy.overflow_policy, QueueOverflowPolicy::DropNew);

        let mut playback = DvrContractSkeleton::new(11, DvrDirection::Playback, 32768);
        playback.configure();
        playback.start();
        let playback_model = playback.queue_model();
        assert_eq!(playback_model.queue_kind, QueueKind::DvrPlayback);
        assert_eq!(playback_model.discipline, DvrQueueDiscipline::PlaybackReinject);
        assert_eq!(
            playback.queue.overflow_policy,
            QueueOverflowPolicy::ProducerBackpressure
        );
        assert_eq!(
            playback_model.policy.overflow_policy,
            QueueOverflowPolicy::ProducerBackpressure
        );

        playback.stop();
        playback.close();
        assert_eq!(playback.lifecycle, DvrLifecycleState::Closed);
    }
}
