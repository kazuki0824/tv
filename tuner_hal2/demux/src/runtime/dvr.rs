#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrKind { Record, Playback }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DvrRuntimeState { Open, Configured, Started, Stopped, Closing, CleanupFailed, Closed, Failed }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DvrRuntimeSnapshot { pub state: DvrRuntimeState, pub generation: u64, pub queue_present: bool, pub playback_assembler_present: bool }

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DvrRuntime { dvr_id: i32, kind: DvrKind, state: DvrRuntimeState, generation: u64, queue_present: bool, playback_assembler_present: bool }

impl DvrRuntime {
    pub fn new(dvr_id: i32, kind: DvrKind, generation: u64) -> Self { Self { dvr_id, kind, state: DvrRuntimeState::Open, generation, queue_present: false, playback_assembler_present: matches!(kind, DvrKind::Playback) } }
    pub fn dvr_id(&self) -> i32 { self.dvr_id }
    pub fn kind(&self) -> DvrKind { self.kind }
    pub fn state(&self) -> DvrRuntimeState { self.state }
    pub fn generation(&self) -> u64 { self.generation }
    pub fn queue_present(&self) -> bool { self.queue_present }
    pub fn playback_assembler_present(&self) -> bool { self.playback_assembler_present }

    pub fn snapshot(&self) -> DvrRuntimeSnapshot {
        DvrRuntimeSnapshot { state: self.state, generation: self.generation, queue_present: self.queue_present, playback_assembler_present: self.playback_assembler_present }
    }

    pub fn restore(&mut self, snapshot: DvrRuntimeSnapshot) {
        self.state = snapshot.state;
        self.generation = snapshot.generation;
        self.queue_present = snapshot.queue_present;
        self.playback_assembler_present = snapshot.playback_assembler_present;
    }

    pub fn configure_with_generation(&mut self, generation: u64) {
        self.generation = generation;
        self.queue_present = true;
        self.playback_assembler_present = matches!(self.kind, DvrKind::Playback);
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
        had
    }

    pub fn mark_started(&mut self) { self.state = DvrRuntimeState::Started; }
    pub fn mark_stopped(&mut self) { self.state = DvrRuntimeState::Stopped; }
    pub fn mark_failed(&mut self) { self.state = DvrRuntimeState::Failed; }
}
