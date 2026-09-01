use std::collections::BTreeMap;
use std::marker::PhantomData;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct LedgerId(pub i64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct LedgerGeneration(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedgerState {
    Reserved,
    Live,
    Closing,
    CleanupFailed,
    Closed,
    Quarantined,
}

impl LedgerState {
    pub fn is_terminal(self) -> bool {
        matches!(self, LedgerState::Closed | LedgerState::Quarantined)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupStep {
    StopWorker,
    ClearQueue,
    UnregisterRuntime,
    ReleaseBackend,
    ReleaseLedger,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceKind {
    Frontend,
    Demux,
    Filter,
    Dvr,
    Descrambler,
    Lnb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedgerOperation {
    Reserve,
    CommitLive,
    BeginClose,
    CommitClose,
    RollbackOpen,
    MarkCleanupFailed,
    Quarantine,
    AdvanceCleanupStep,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedgerErrorKind {
    AlreadyExists {
        state: LedgerState,
    },
    NotFound,
    GenerationMismatch {
        expected: LedgerGeneration,
        actual: LedgerGeneration,
    },
    InvalidTransition {
        from: LedgerState,
        op: LedgerOperation,
    },
    TerminalState {
        state: LedgerState,
        op: LedgerOperation,
    },
    GenerationOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LedgerError {
    pub id: LedgerId,
    pub kind: LedgerErrorKind,
}

impl LedgerError {
    fn already_exists(id: LedgerId, state: LedgerState) -> Self {
        Self {
            id,
            kind: LedgerErrorKind::AlreadyExists { state },
        }
    }

    fn not_found(id: LedgerId) -> Self {
        Self {
            id,
            kind: LedgerErrorKind::NotFound,
        }
    }

    fn generation_mismatch(
        id: LedgerId,
        expected: LedgerGeneration,
        actual: LedgerGeneration,
    ) -> Self {
        Self {
            id,
            kind: LedgerErrorKind::GenerationMismatch { expected, actual },
        }
    }

    fn invalid_transition(id: LedgerId, from: LedgerState, op: LedgerOperation) -> Self {
        Self {
            id,
            kind: LedgerErrorKind::InvalidTransition { from, op },
        }
    }

    fn terminal_state(id: LedgerId, state: LedgerState, op: LedgerOperation) -> Self {
        Self {
            id,
            kind: LedgerErrorKind::TerminalState { state, op },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LedgerEntry {
    pub id: LedgerId,
    pub generation: LedgerGeneration,
    pub state: LedgerState,
    pub cleanup_step: Option<CleanupStep>,
}

#[derive(Debug, Default)]
pub struct ResourceLedger {
    entries: BTreeMap<LedgerId, LedgerEntry>,
    next_generation: u64,
}

impl ResourceLedger {
    pub fn reserve(&mut self, id: LedgerId) -> Result<LedgerEntry, LedgerError> {
        if let Some(existing) = self.entries.get(&id) {
            return Err(LedgerError::already_exists(id, existing.state));
        }
        let next_generation = self.next_generation.checked_add(1).ok_or(LedgerError {
            id,
            kind: LedgerErrorKind::GenerationOverflow,
        })?;
        self.next_generation = next_generation;
        let entry = LedgerEntry {
            id,
            generation: LedgerGeneration(next_generation),
            state: LedgerState::Reserved,
            cleanup_step: None,
        };
        self.entries.insert(id, entry);
        Ok(entry)
    }

    pub fn commit_live(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
    ) -> Result<LedgerEntry, LedgerError> {
        let entry = self.entry_mut_checked(id, generation, LedgerOperation::CommitLive)?;
        match entry.state {
            LedgerState::Reserved => {
                entry.state = LedgerState::Live;
                entry.cleanup_step = None;
                Ok(*entry)
            }
            state if state.is_terminal() => Err(LedgerError::terminal_state(
                id,
                state,
                LedgerOperation::CommitLive,
            )),
            state => Err(LedgerError::invalid_transition(
                id,
                state,
                LedgerOperation::CommitLive,
            )),
        }
    }

    pub fn begin_close(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
        first_step: CleanupStep,
    ) -> Result<LedgerEntry, LedgerError> {
        let entry = self.entry_mut_checked(id, generation, LedgerOperation::BeginClose)?;
        match entry.state {
            LedgerState::Live | LedgerState::CleanupFailed => {
                entry.state = LedgerState::Closing;
                entry.cleanup_step = Some(first_step);
                Ok(*entry)
            }
            state if state.is_terminal() => Err(LedgerError::terminal_state(
                id,
                state,
                LedgerOperation::BeginClose,
            )),
            state => Err(LedgerError::invalid_transition(
                id,
                state,
                LedgerOperation::BeginClose,
            )),
        }
    }

    pub fn advance_cleanup_step(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
        step: CleanupStep,
    ) -> Result<LedgerEntry, LedgerError> {
        let entry = self.entry_mut_checked(id, generation, LedgerOperation::AdvanceCleanupStep)?;
        match entry.state {
            LedgerState::Closing | LedgerState::CleanupFailed => {
                entry.cleanup_step = Some(step);
                Ok(*entry)
            }
            state if state.is_terminal() => Err(LedgerError::terminal_state(
                id,
                state,
                LedgerOperation::AdvanceCleanupStep,
            )),
            state => Err(LedgerError::invalid_transition(
                id,
                state,
                LedgerOperation::AdvanceCleanupStep,
            )),
        }
    }

    pub fn mark_cleanup_failed(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
        failed_step: CleanupStep,
    ) -> Result<LedgerEntry, LedgerError> {
        let entry = self.entry_mut_checked(id, generation, LedgerOperation::MarkCleanupFailed)?;
        match entry.state {
            LedgerState::Closing | LedgerState::CleanupFailed => {
                entry.state = LedgerState::CleanupFailed;
                entry.cleanup_step = Some(failed_step);
                Ok(*entry)
            }
            state if state.is_terminal() => Err(LedgerError::terminal_state(
                id,
                state,
                LedgerOperation::MarkCleanupFailed,
            )),
            state => Err(LedgerError::invalid_transition(
                id,
                state,
                LedgerOperation::MarkCleanupFailed,
            )),
        }
    }

    pub fn commit_close(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
    ) -> Result<LedgerEntry, LedgerError> {
        let entry = self.entry_mut_checked(id, generation, LedgerOperation::CommitClose)?;
        match entry.state {
            LedgerState::Closing | LedgerState::CleanupFailed => {
                entry.state = LedgerState::Closed;
                entry.cleanup_step = None;
                Ok(*entry)
            }
            state if state.is_terminal() => Err(LedgerError::terminal_state(
                id,
                state,
                LedgerOperation::CommitClose,
            )),
            state => Err(LedgerError::invalid_transition(
                id,
                state,
                LedgerOperation::CommitClose,
            )),
        }
    }

    pub fn rollback_open(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
    ) -> Result<LedgerEntry, LedgerError> {
        let entry = *self.entry_checked(id, generation, LedgerOperation::RollbackOpen)?;
        match entry.state {
            LedgerState::Reserved => {
                self.entries.remove(&id);
                Ok(entry)
            }
            state if state.is_terminal() => Err(LedgerError::terminal_state(
                id,
                state,
                LedgerOperation::RollbackOpen,
            )),
            state => Err(LedgerError::invalid_transition(
                id,
                state,
                LedgerOperation::RollbackOpen,
            )),
        }
    }

    pub fn quarantine(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
        step: Option<CleanupStep>,
    ) -> Result<LedgerEntry, LedgerError> {
        let entry = self.entry_mut_checked(id, generation, LedgerOperation::Quarantine)?;
        match entry.state {
            LedgerState::Closed | LedgerState::Quarantined => Err(LedgerError::terminal_state(
                id,
                entry.state,
                LedgerOperation::Quarantine,
            )),
            _ => {
                entry.state = LedgerState::Quarantined;
                entry.cleanup_step = step;
                Ok(*entry)
            }
        }
    }

    pub fn entry(&self, id: LedgerId) -> Option<&LedgerEntry> {
        self.entries.get(&id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn entry_checked(
        &self,
        id: LedgerId,
        generation: LedgerGeneration,
        _op: LedgerOperation,
    ) -> Result<&LedgerEntry, LedgerError> {
        let entry = self
            .entries
            .get(&id)
            .ok_or_else(|| LedgerError::not_found(id))?;
        if entry.generation != generation {
            return Err(LedgerError::generation_mismatch(
                id,
                entry.generation,
                generation,
            ));
        }
        Ok(entry)
    }

    fn entry_mut_checked(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
        op: LedgerOperation,
    ) -> Result<&mut LedgerEntry, LedgerError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or_else(|| LedgerError::not_found(id))?;
        if entry.generation != generation {
            return Err(LedgerError::generation_mismatch(
                id,
                entry.generation,
                generation,
            ));
        }
        if entry.state.is_terminal() && !matches!(op, LedgerOperation::Quarantine) {
            return Err(LedgerError::terminal_state(id, entry.state, op));
        }
        Ok(entry)
    }
}

pub trait LedgerResourceKind {
    const KIND: ResourceKind;
}

#[derive(Debug)]
pub struct TypedResourceLedger<K: LedgerResourceKind> {
    inner: ResourceLedger,
    _kind: PhantomData<K>,
}

impl<K: LedgerResourceKind> Default for TypedResourceLedger<K> {
    fn default() -> Self {
        Self {
            inner: ResourceLedger::default(),
            _kind: PhantomData,
        }
    }
}

impl<K: LedgerResourceKind> TypedResourceLedger<K> {
    pub fn kind(&self) -> ResourceKind {
        K::KIND
    }

    pub fn reserve(&mut self, id: LedgerId) -> Result<LedgerEntry, LedgerError> {
        self.inner.reserve(id)
    }

    pub fn commit_live(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
    ) -> Result<LedgerEntry, LedgerError> {
        self.inner.commit_live(id, generation)
    }

    pub fn begin_close(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
        first_step: CleanupStep,
    ) -> Result<LedgerEntry, LedgerError> {
        self.inner.begin_close(id, generation, first_step)
    }

    pub fn advance_cleanup_step(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
        step: CleanupStep,
    ) -> Result<LedgerEntry, LedgerError> {
        self.inner.advance_cleanup_step(id, generation, step)
    }

    pub fn mark_cleanup_failed(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
        failed_step: CleanupStep,
    ) -> Result<LedgerEntry, LedgerError> {
        self.inner.mark_cleanup_failed(id, generation, failed_step)
    }

    pub fn commit_close(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
    ) -> Result<LedgerEntry, LedgerError> {
        self.inner.commit_close(id, generation)
    }

    pub fn rollback_open(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
    ) -> Result<LedgerEntry, LedgerError> {
        self.inner.rollback_open(id, generation)
    }

    pub fn quarantine(
        &mut self,
        id: LedgerId,
        generation: LedgerGeneration,
        step: Option<CleanupStep>,
    ) -> Result<LedgerEntry, LedgerError> {
        self.inner.quarantine(id, generation, step)
    }

    pub fn entry(&self, id: LedgerId) -> Option<&LedgerEntry> {
        self.inner.entry(id)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

macro_rules! define_ledger_wrapper {
    ($wrapper:ident, $marker:ident, $kind:expr) => {
        #[derive(Debug, Default)]
        pub struct $marker;

        impl LedgerResourceKind for $marker {
            const KIND: ResourceKind = $kind;
        }

        #[derive(Debug, Default)]
        pub struct $wrapper {
            inner: TypedResourceLedger<$marker>,
        }

        impl $wrapper {
            pub fn kind(&self) -> ResourceKind {
                self.inner.kind()
            }
            pub fn reserve(&mut self, id: LedgerId) -> Result<LedgerEntry, LedgerError> {
                self.inner.reserve(id)
            }
            pub fn commit_live(
                &mut self,
                id: LedgerId,
                generation: LedgerGeneration,
            ) -> Result<LedgerEntry, LedgerError> {
                self.inner.commit_live(id, generation)
            }
            pub fn begin_close(
                &mut self,
                id: LedgerId,
                generation: LedgerGeneration,
                first_step: CleanupStep,
            ) -> Result<LedgerEntry, LedgerError> {
                self.inner.begin_close(id, generation, first_step)
            }
            pub fn advance_cleanup_step(
                &mut self,
                id: LedgerId,
                generation: LedgerGeneration,
                step: CleanupStep,
            ) -> Result<LedgerEntry, LedgerError> {
                self.inner.advance_cleanup_step(id, generation, step)
            }
            pub fn mark_cleanup_failed(
                &mut self,
                id: LedgerId,
                generation: LedgerGeneration,
                failed_step: CleanupStep,
            ) -> Result<LedgerEntry, LedgerError> {
                self.inner.mark_cleanup_failed(id, generation, failed_step)
            }
            pub fn commit_close(
                &mut self,
                id: LedgerId,
                generation: LedgerGeneration,
            ) -> Result<LedgerEntry, LedgerError> {
                self.inner.commit_close(id, generation)
            }
            pub fn rollback_open(
                &mut self,
                id: LedgerId,
                generation: LedgerGeneration,
            ) -> Result<LedgerEntry, LedgerError> {
                self.inner.rollback_open(id, generation)
            }
            pub fn quarantine(
                &mut self,
                id: LedgerId,
                generation: LedgerGeneration,
                step: Option<CleanupStep>,
            ) -> Result<LedgerEntry, LedgerError> {
                self.inner.quarantine(id, generation, step)
            }
            pub fn entry(&self, id: LedgerId) -> Option<&LedgerEntry> {
                self.inner.entry(id)
            }
            pub fn len(&self) -> usize {
                self.inner.len()
            }
            pub fn is_empty(&self) -> bool {
                self.inner.is_empty()
            }
        }
    };
}

define_ledger_wrapper!(FrontendLedger, FrontendLedgerKind, ResourceKind::Frontend);
define_ledger_wrapper!(DemuxLedger, DemuxLedgerKind, ResourceKind::Demux);
define_ledger_wrapper!(FilterLedger, FilterLedgerKind, ResourceKind::Filter);
define_ledger_wrapper!(DvrLedger, DvrLedgerKind, ResourceKind::Dvr);
define_ledger_wrapper!(
    DescramblerLedger,
    DescramblerLedgerKind,
    ResourceKind::Descrambler
);
define_ledger_wrapper!(LnbLedger, LnbLedgerKind, ResourceKind::Lnb);

#[cfg(test)]
mod tests {
    use super::*;

    fn live_entry(ledger: &mut ResourceLedger, id: LedgerId) -> LedgerEntry {
        let reserved = ledger.reserve(id).expect("reserve succeeds");
        ledger
            .commit_live(id, reserved.generation)
            .expect("commit live succeeds")
    }

    #[test]
    fn single_entry_holds_one_state_for_id() {
        let id = LedgerId(10);
        let mut ledger = ResourceLedger::default();
        let live = live_entry(&mut ledger, id);
        assert_eq!(ledger.len(), 1);
        assert_eq!(
            ledger.entry(id).map(|entry| entry.state),
            Some(LedgerState::Live)
        );
        let closing = ledger
            .begin_close(id, live.generation, CleanupStep::StopWorker)
            .expect("begin close succeeds");
        assert_eq!(closing.state, LedgerState::Closing);
        assert_eq!(closing.cleanup_step, Some(CleanupStep::StopWorker));
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn cleanup_step_is_typed_enum() {
        let id = LedgerId(11);
        let mut ledger = ResourceLedger::default();
        let live = live_entry(&mut ledger, id);
        ledger
            .begin_close(id, live.generation, CleanupStep::StopWorker)
            .expect("begin close succeeds");
        let failed = ledger
            .mark_cleanup_failed(id, live.generation, CleanupStep::ClearQueue)
            .expect("mark cleanup failed succeeds");
        assert_eq!(failed.state, LedgerState::CleanupFailed);
        assert_eq!(failed.cleanup_step, Some(CleanupStep::ClearQueue));
    }

    #[test]
    fn commit_before_begin_is_rejected() {
        let id = LedgerId(12);
        let mut ledger = ResourceLedger::default();
        let reserved = ledger.reserve(id).expect("reserve succeeds");
        let err = ledger
            .commit_close(id, reserved.generation)
            .expect_err("reserved cannot close commit");
        assert_eq!(
            err.kind,
            LedgerErrorKind::InvalidTransition {
                from: LedgerState::Reserved,
                op: LedgerOperation::CommitClose
            }
        );
    }

    #[test]
    fn begin_close_before_live_commit_is_rejected() {
        let id = LedgerId(13);
        let mut ledger = ResourceLedger::default();
        let reserved = ledger.reserve(id).expect("reserve succeeds");
        let err = ledger
            .begin_close(id, reserved.generation, CleanupStep::StopWorker)
            .expect_err("reserved cannot begin close");
        assert_eq!(
            err.kind,
            LedgerErrorKind::InvalidTransition {
                from: LedgerState::Reserved,
                op: LedgerOperation::BeginClose
            }
        );
    }

    #[test]
    fn terminal_after_commit_rejects_further_state_change() {
        let id = LedgerId(14);
        let mut ledger = ResourceLedger::default();
        let live = live_entry(&mut ledger, id);
        ledger
            .begin_close(id, live.generation, CleanupStep::StopWorker)
            .expect("begin close succeeds");
        ledger
            .commit_close(id, live.generation)
            .expect("commit close succeeds");
        let err = ledger
            .commit_live(id, live.generation)
            .expect_err("closed entry cannot become live");
        assert_eq!(
            err.kind,
            LedgerErrorKind::TerminalState {
                state: LedgerState::Closed,
                op: LedgerOperation::CommitLive
            }
        );
    }

    #[test]
    fn double_rollback_open_is_rejected() {
        let id = LedgerId(15);
        let mut ledger = ResourceLedger::default();
        let reserved = ledger.reserve(id).expect("reserve succeeds");
        ledger
            .rollback_open(id, reserved.generation)
            .expect("first rollback succeeds");
        let err = ledger
            .rollback_open(id, reserved.generation)
            .expect_err("second rollback is not found");
        assert_eq!(err.kind, LedgerErrorKind::NotFound);
    }

    #[test]
    fn typed_wrappers_hold_resource_kind() {
        let mut frontend = FrontendLedger::default();
        let mut demux = DemuxLedger::default();
        assert_eq!(frontend.kind(), ResourceKind::Frontend);
        assert_eq!(demux.kind(), ResourceKind::Demux);
        let f_entry = frontend
            .reserve(LedgerId(1))
            .expect("frontend reserve succeeds");
        let d_entry = demux.reserve(LedgerId(1)).expect("demux reserve succeeds");
        assert_eq!(f_entry.generation, LedgerGeneration(1));
        assert_eq!(d_entry.generation, LedgerGeneration(1));
    }
}
