//! descrambler の PID 寿命と key token 寿命を分離して所有する。
//!
//! r50dz28 では TunerDescramblerState 側の PID / key / source / close 状態を
//! この session に移し、runtime registry も session snapshot を読む。

use std::collections::{BTreeMap, BTreeSet};

use maleicacid_tuner_hal_descrambler::DescramblerKeySlot;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct PidBinding {
    pub pid: i32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SourceFilterBinding {
    pub filter_id: i32,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct PendingDemuxBinding {
    pub demux_id: i32,
    pub demux_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescramblerPidRegistration {
    pub upstream_filter_id: i32,
    pub upstream_filter_generation: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DescramblerCloseState {
    Open,
    Closing,
    Closed,
}

impl Default for DescramblerCloseState {
    fn default() -> Self {
        Self::Open
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum DescramblerCleanupItem {
    RuntimeRegistry,
    KeyRelease,
    DemuxLedgerClose,
}

impl DescramblerCleanupItem {
    #[cfg(test)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeRegistry => "runtime_registry",
            Self::KeyRelease => "key_release",
            Self::DemuxLedgerClose => "demux_ledger_close",
        }
    }
}

pub struct DescramblerSession {
    pub demux_id: Option<i32>,
    pub demux_generation: Option<u64>,
    pub pending_demux_binding: Option<PendingDemuxBinding>,
    pub key_token: Option<Vec<u8>>,
    pub key_slot: Option<DescramblerKeySlot>,
    pub pid_registrations: BTreeMap<u16, DescramblerPidRegistration>,
    pub close_state: DescramblerCloseState,
    pub pending_cleanup: BTreeSet<DescramblerCleanupItem>,
}

impl Default for DescramblerSession {
    fn default() -> Self {
        Self {
            demux_id: None,
            demux_generation: None,
            pending_demux_binding: None,
            key_token: None,
            key_slot: None,
            pid_registrations: BTreeMap::new(),
            close_state: DescramblerCloseState::Open,
            pending_cleanup: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DescramblerCloseSnapshot {
    pub demux_id: Option<i32>,
    pub demux_generation: Option<u64>,
    pub key_token: Option<Vec<u8>>,
    pub pids: Vec<PidBinding>,
    pub upstream_filters: Vec<(PidBinding, i32, u64)>,
}

impl DescramblerSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_closed(&self) -> bool {
        self.close_state == DescramblerCloseState::Closed
    }

    pub fn begin_pending_demux_binding(&mut self, demux_id: i32, demux_generation: u64) -> bool {
        if self.demux_id.is_some() || self.pending_demux_binding.is_some() {
            return false;
        }
        self.pending_demux_binding = Some(PendingDemuxBinding {
            demux_id,
            demux_generation,
        });
        true
    }

    pub fn commit_pending_demux_binding(&mut self, demux_id: i32, demux_generation: u64) -> bool {
        if self.pending_demux_binding
            != Some(PendingDemuxBinding {
                demux_id,
                demux_generation,
            })
            || self.demux_id.is_some()
        {
            return false;
        }
        self.pending_demux_binding = None;
        self.demux_id = Some(demux_id);
        self.demux_generation = Some(demux_generation);
        true
    }

    pub fn rollback_pending_demux_binding(&mut self, demux_id: i32, demux_generation: u64) {
        if self.pending_demux_binding
            == Some(PendingDemuxBinding {
                demux_id,
                demux_generation,
            })
        {
            self.pending_demux_binding = None;
        }
    }

    pub fn clear_demux(&mut self) {
        self.demux_id = None;
        self.demux_generation = None;
        self.pending_demux_binding = None;
        self.pid_registrations.clear();
    }

    pub fn set_resolved_key(
        &mut self,
        token: Vec<u8>,
        slot: DescramblerKeySlot,
    ) -> Option<Vec<u8>> {
        let old = self.key_token.replace(token.clone());
        self.key_slot = Some(slot);
        old
    }

    pub fn clear_key(&mut self) -> Option<Vec<u8>> {
        let old = self.key_token.take();
        self.key_slot = None;
        old
    }

    pub fn add_pid(&mut self, pid: PidBinding, upstream_filter: SourceFilterBinding) {
        self.pid_registrations.insert(
            pid.pid as u16,
            DescramblerPidRegistration {
                upstream_filter_id: upstream_filter.filter_id,
                upstream_filter_generation: upstream_filter.generation,
            },
        );
    }

    pub fn remove_pid(&mut self, pid: PidBinding) {
        self.pid_registrations.remove(&(pid.pid as u16));
    }

    pub fn pid_bindings(&self) -> impl Iterator<Item = PidBinding> + '_ {
        self.pid_registrations
            .keys()
            .copied()
            .map(|pid| PidBinding { pid: pid as i32 })
    }

    pub fn upstream_filter_bindings(&self) -> impl Iterator<Item = (PidBinding, i32, u64)> + '_ {
        self.pid_registrations.iter().map(|(pid, registration)| {
            (
                PidBinding { pid: *pid as i32 },
                registration.upstream_filter_id,
                registration.upstream_filter_generation,
            )
        })
    }

    #[cfg(test)]
    pub fn has_pid(&self, pid: PidBinding) -> bool {
        self.pid_registrations.contains_key(&(pid.pid as u16))
    }

    pub fn begin_close(&mut self) {
        self.close_state = DescramblerCloseState::Closing;
    }

    pub fn mark_closed(&mut self) {
        self.close_state = DescramblerCloseState::Closed;
        self.pending_cleanup.clear();
    }

    pub fn close_snapshot(&self) -> DescramblerCloseSnapshot {
        DescramblerCloseSnapshot {
            demux_id: self.demux_id,
            demux_generation: self.demux_generation,
            key_token: self.key_token.clone(),
            pids: self.pid_bindings().collect(),
            upstream_filters: self.upstream_filter_bindings().collect(),
        }
    }

    pub fn begin_close_with_snapshot(&mut self) -> DescramblerCloseSnapshot {
        self.begin_close();
        self.close_snapshot()
    }

    pub fn mark_cleanup_failed(&mut self, item: DescramblerCleanupItem) {
        self.pending_cleanup.insert(item);
        if self.close_state == DescramblerCloseState::Closed {
            self.close_state = DescramblerCloseState::Closing;
        }
    }

    pub fn complete_close_after_cleanup(&mut self) {
        self.key_token = None;
        self.key_slot = None;
        self.demux_id = None;
        self.demux_generation = None;
        self.pending_demux_binding = None;
        self.pid_registrations.clear();
        if self.can_complete_close() {
            self.mark_closed();
        } else {
            self.close_state = DescramblerCloseState::Closing;
        }
    }

    #[cfg(test)]

    pub fn pending_cleanup_items(&self) -> impl Iterator<Item = DescramblerCleanupItem> + '_ {
        self.pending_cleanup.iter().copied()
    }

    #[cfg(test)]
    pub fn pending_cleanup_names(&self) -> Vec<&'static str> {
        self.pending_cleanup
            .iter()
            .copied()
            .map(DescramblerCleanupItem::as_str)
            .collect()
    }

    #[cfg(test)]
    pub fn has_pending_cleanup(&self, item: DescramblerCleanupItem) -> bool {
        self.pending_cleanup.contains(&item)
    }

    pub fn mark_cleanup_complete(&mut self, item: DescramblerCleanupItem) {
        self.pending_cleanup.remove(&item);
    }

    pub fn can_complete_close(&self) -> bool {
        self.pending_cleanup.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descrambler_session_allows_pid_without_key() {
        let mut session = DescramblerSession::new();
        session.add_pid(
            PidBinding { pid: 100 },
            SourceFilterBinding {
                filter_id: -1,
                generation: 0,
            },
        );
        assert!(session.has_pid(PidBinding { pid: 100 }));
    }

    #[test]
    fn pending_cleanup_blocks_close_completion_until_retried() {
        let mut session = DescramblerSession::new();
        session.mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
        assert!(session.has_pending_cleanup(DescramblerCleanupItem::KeyRelease));
        assert!(!session.can_complete_close());
        session.mark_cleanup_complete(DescramblerCleanupItem::KeyRelease);
        assert!(session.can_complete_close());
    }
    #[test]
    fn pending_demux_binding_blocks_second_commit_until_resolved() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(3, 7));
        assert!(!session.begin_pending_demux_binding(4, 8));
        assert!(!session.commit_pending_demux_binding(4, 8));
        assert!(session.commit_pending_demux_binding(3, 7));
        assert_eq!(session.demux_id, Some(3));
        assert_eq!(session.demux_generation, Some(7));
    }

    #[test]
    fn pending_demux_binding_can_rollback_without_publishing_bound_state() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(3, 7));
        session.rollback_pending_demux_binding(3, 7);
        assert!(session.pending_demux_binding.is_none());
        assert!(session.demux_id.is_none());
        assert!(session.demux_generation.is_none());
    }

    #[test]
    fn pending_cleanup_uses_typed_items_and_exposes_stable_names() {
        let mut session = DescramblerSession::new();
        session.mark_cleanup_failed(DescramblerCleanupItem::RuntimeRegistry);
        session.mark_cleanup_failed(DescramblerCleanupItem::DemuxLedgerClose);
        assert_eq!(
            session.pending_cleanup_names(),
            vec!["runtime_registry", "demux_ledger_close"]
        );
        session.mark_cleanup_complete(DescramblerCleanupItem::RuntimeRegistry);
        assert_eq!(session.pending_cleanup_names(), vec!["demux_ledger_close"]);
    }

    #[test]
    fn complete_close_does_not_clear_pending_cleanup_items() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(5, 9));
        assert!(session.commit_pending_demux_binding(5, 9));
        session.mark_cleanup_failed(DescramblerCleanupItem::DemuxLedgerClose);
        session.complete_close_after_cleanup();
        assert!(!session.is_closed());
        assert!(session.has_pending_cleanup(DescramblerCleanupItem::DemuxLedgerClose));
        session.mark_cleanup_complete(DescramblerCleanupItem::DemuxLedgerClose);
        session.complete_close_after_cleanup();
        assert!(session.is_closed());
    }
}

#[cfg(test)]
mod r50ea7_descrambler_session_completion_tests {
    use super::*;

    #[test]
    fn descrambler_set_demux_source_session_commit_before_ledger_live() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(10, 33));
        assert_eq!(session.demux_id, None);
        assert_eq!(session.demux_generation, None);
        assert_eq!(
            session.pending_demux_binding,
            Some(PendingDemuxBinding {
                demux_id: 10,
                demux_generation: 33
            })
        );
        assert!(session.commit_pending_demux_binding(10, 33));
        assert_eq!(session.demux_id, Some(10));
        assert_eq!(session.demux_generation, Some(33));
    }

    #[test]
    fn descrambler_set_demux_source_failure_no_live_entry() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(10, 33));
        session.rollback_pending_demux_binding(10, 33);
        assert_eq!(session.demux_id, None);
        assert_eq!(session.demux_generation, None);
        assert_eq!(session.pending_demux_binding, None);
        assert!(!session.has_pending_cleanup(DescramblerCleanupItem::DemuxLedgerClose));
    }

    #[test]
    fn descrambler_set_demux_source_parallel_rejects_second_without_ledger_leak() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(1, 101));
        assert!(!session.begin_pending_demux_binding(2, 202));
        assert_eq!(session.demux_id, None);
        assert_eq!(session.demux_generation, None);
        assert_eq!(
            session.pending_demux_binding,
            Some(PendingDemuxBinding {
                demux_id: 1,
                demux_generation: 101
            })
        );
    }

    #[test]
    fn descrambler_snapshot_expire_failure_keeps_token_for_retry() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(3, 303));
        assert!(session.commit_pending_demux_binding(3, 303));
        session.key_token = Some(vec![1, 2, 3, 4]);
        session.mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
        assert_eq!(session.key_token, Some(vec![1, 2, 3, 4]));
        assert_eq!(session.demux_id, Some(3));
        assert!(session.has_pending_cleanup(DescramblerCleanupItem::KeyRelease));
    }

    #[test]
    fn descrambler_invalidate_release_failure_keeps_binding_for_retry() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(4, 404));
        assert!(session.commit_pending_demux_binding(4, 404));
        session.key_token = Some(vec![5, 6, 7, 8]);
        session.mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
        assert_eq!(session.demux_id, Some(4));
        assert_eq!(session.demux_generation, Some(404));
        assert_eq!(session.key_token, Some(vec![5, 6, 7, 8]));
        assert!(session.has_pending_cleanup(DescramblerCleanupItem::KeyRelease));
    }

    #[test]
    fn descrambler_set_key_token_parallel_returns_invalid_state_if_not_committed() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(5, 505));
        assert!(session.commit_pending_demux_binding(5, 505));
        session.key_token = Some(vec![9]);
        session.mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
        assert_eq!(session.key_token, Some(vec![9]));
        assert!(session.has_pending_cleanup(DescramblerCleanupItem::KeyRelease));
    }

    #[test]
    fn descrambler_set_demux_source_ledger_live_and_session_bound_are_not_split() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(7, 707));
        assert_eq!(session.demux_id, None);
        assert_eq!(session.demux_generation, None);
        assert!(session.commit_pending_demux_binding(7, 707));
        assert_eq!(session.pending_demux_binding, None);
        assert_eq!(session.demux_id, Some(7));
        assert_eq!(session.demux_generation, Some(707));
    }

    #[test]
    fn descrambler_set_demux_source_live_after_bound_failure_quarantines_ledger() {
        let mut session = DescramblerSession::new();
        assert!(session.begin_pending_demux_binding(8, 808));
        session.mark_cleanup_failed(DescramblerCleanupItem::DemuxLedgerClose);
        assert_eq!(session.demux_id, None);
        assert_eq!(session.demux_generation, None);
        assert!(session.has_pending_cleanup(DescramblerCleanupItem::DemuxLedgerClose));
    }

    #[test]
    fn descrambler_old_token_release_failure_keeps_old_token() {
        let mut session = DescramblerSession::new();
        session.key_token = Some(vec![0x11, 0x22]);
        session.mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
        assert_eq!(session.key_token, Some(vec![0x11, 0x22]));
        assert!(session.has_pending_cleanup(DescramblerCleanupItem::KeyRelease));
    }

    #[test]
    fn descrambler_old_token_release_failure_does_not_commit_new_token() {
        let mut session = DescramblerSession::new();
        session.key_token = Some(vec![0xaa]);
        session.mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
        let attempted_new_token = vec![0xbb];
        assert_ne!(session.key_token, Some(attempted_new_token));
        assert_eq!(session.key_token, Some(vec![0xaa]));
    }

    #[test]
    fn descrambler_key_retry_pending_rejects_new_key() {
        let mut session = DescramblerSession::new();
        session.key_token = Some(vec![0xcc]);
        session.mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
        assert!(session.has_pending_cleanup(DescramblerCleanupItem::KeyRelease));
        assert_eq!(session.key_token, Some(vec![0xcc]));
    }

    #[test]
    fn descrambler_old_token_release_failure_is_retry_pending() {
        let mut session = DescramblerSession::new();
        session.key_token = Some(vec![0xaa]);
        session.mark_cleanup_failed(DescramblerCleanupItem::KeyRelease);
        assert_eq!(session.key_token, Some(vec![0xaa]));
        assert!(session.has_pending_cleanup(DescramblerCleanupItem::KeyRelease));
        session.mark_cleanup_complete(DescramblerCleanupItem::KeyRelease);
        assert!(!session.has_pending_cleanup(DescramblerCleanupItem::KeyRelease));
    }
}
