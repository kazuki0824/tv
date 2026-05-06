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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrQueueDiscipline {
    PacketPassthrough,
    PlaybackReinject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuePolicy {
    pub bounded_bytes: usize,
    pub oldest_drop: bool,
}

impl QueuePolicy {
    pub const fn bounded(bounded_bytes: usize) -> Self {
        Self {
            bounded_bytes,
            oldest_drop: true,
        }
    }

    pub const fn bounded_drop_new(bounded_bytes: usize) -> Self {
        Self {
            bounded_bytes,
            oldest_drop: false,
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
    pub output_queue: QueuePolicy,
}

impl FilterContractSkeleton {
    pub fn new(filter_id: i32, buffer_size: usize) -> Self {
        Self {
            filter_id,
            buffer_size,
            lifecycle: FilterLifecycleState::Allocated,
            output_queue: QueuePolicy::bounded(buffer_size),
        }
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

    pub fn queue_model(&self, section_reassembled: bool) -> FilterQueueModel {
        FilterQueueModel {
            queue_kind: QueueKind::FilterOutput,
            discipline: if section_reassembled {
                FilterQueueDiscipline::SectionReassembled
            } else {
                FilterQueueDiscipline::PacketPassthrough
            },
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
            queue: QueuePolicy::bounded(buffer_size),
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
                DvrDirection::Playback => self.queue,
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

    use super::{DvrContractSkeleton, DvrDirection, DvrLifecycleState, DvrQueueDiscipline, FilterContractSkeleton, FilterLifecycleState, FilterQueueDiscipline, QueueKind};

    #[test]
    fn filter_contract_switches_between_section_and_packet_disciplines() {
        let mut filter = FilterContractSkeleton::new(1, 4096);
        assert_eq!(filter.lifecycle, FilterLifecycleState::Allocated);
        filter.configure();
        filter.start();
        let section = filter.queue_model(true);
        let packet = filter.queue_model(false);
        assert_eq!(section.queue_kind, QueueKind::FilterOutput);
        assert_eq!(section.discipline, FilterQueueDiscipline::SectionReassembled);
        assert_eq!(packet.discipline, FilterQueueDiscipline::PacketPassthrough);
        filter.stop();
        filter.close();
        assert_eq!(filter.lifecycle, FilterLifecycleState::Closed);
    }

    #[test]
    fn dvr_contract_exposes_record_and_playback_queue_models() {
        let mut record = DvrContractSkeleton::new(10, DvrDirection::Record, 32768);
        record.configure();
        record.start();
        let record_model = record.queue_model();
        assert_eq!(record_model.queue_kind, QueueKind::DvrRecord);
        assert_eq!(record_model.discipline, DvrQueueDiscipline::PacketPassthrough);

        let mut playback = DvrContractSkeleton::new(11, DvrDirection::Playback, 32768);
        playback.configure();
        playback.start();
        let playback_model = playback.queue_model();
        assert_eq!(playback_model.queue_kind, QueueKind::DvrPlayback);
        assert_eq!(playback_model.discipline, DvrQueueDiscipline::PlaybackReinject);

        playback.stop();
        playback.close();
        assert_eq!(playback.lifecycle, DvrLifecycleState::Closed);
    }
}
