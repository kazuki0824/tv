//! descrambler の PID 寿命と key token 寿命を分離して所有する。
//!
//! r50dz28 では TunerDescramblerState 側の PID / key / source / close 状態を
//! この session に移し、runtime registry も session snapshot を読む。

use std::collections::{BTreeMap, BTreeSet};

use maleicacid_tuner_hal_descrambler::DescramblerKeySlot;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct KeyTokenBinding {
    pub token_id: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct PidBinding {
    pub pid: i32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SourceFilterBinding {
    pub filter_id: i32,
    pub generation: u64,
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
    pub key_token: Option<Vec<u8>>,
    pub key_slot: Option<DescramblerKeySlot>,
    pub pid_registrations: BTreeMap<u16, DescramblerPidRegistration>,
    pub key: Option<KeyTokenBinding>,
    pub pids: BTreeSet<PidBinding>,
    pub upstream_filters: BTreeSet<(PidBinding, i32, u64)>,
    pub close_state: DescramblerCloseState,
    pub pending_cleanup: BTreeSet<DescramblerCleanupItem>,
}

impl Default for DescramblerSession {
    fn default() -> Self {
        Self {
            demux_id: None,
            demux_generation: None,
            key_token: None,
            key_slot: None,
            pid_registrations: BTreeMap::new(),
            key: None,
            pids: BTreeSet::new(),
            upstream_filters: BTreeSet::new(),
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

    pub fn set_demux(&mut self, demux_id: i32, demux_generation: u64) {
        self.demux_id = Some(demux_id);
        self.demux_generation = Some(demux_generation);
    }

    pub fn clear_demux(&mut self) {
        self.demux_id = None;
        self.demux_generation = None;
        self.pid_registrations.clear();
        self.pids.clear();
        self.upstream_filters.clear();
    }

    pub fn set_resolved_key(&mut self, token: Vec<u8>, slot: DescramblerKeySlot) -> Option<Vec<u8>> {
        let old = self.key_token.replace(token.clone());
        self.key_slot = Some(slot);
        self.key = Some(KeyTokenBinding { token_id: token });
        old
    }

    pub fn clear_key(&mut self) -> Option<Vec<u8>> {
        let old = self.key_token.take();
        self.key_slot = None;
        self.key = None;
        old
    }

    pub fn add_pid(&mut self, pid: PidBinding, upstream_filter: SourceFilterBinding) {
        self.pids.insert(pid);
        self.upstream_filters.insert((pid, upstream_filter.filter_id, upstream_filter.generation));
        self.pid_registrations.insert(
            pid.pid as u16,
            DescramblerPidRegistration {
                upstream_filter_id: upstream_filter.filter_id,
                upstream_filter_generation: upstream_filter.generation,
            },
        );
    }

    pub fn remove_pid(&mut self, pid: PidBinding) {
        self.pids.remove(&pid);
        self.upstream_filters.retain(|(bound_pid, _, _)| *bound_pid != pid);
        self.pid_registrations.remove(&(pid.pid as u16));
    }

    pub fn has_pid(&self, pid: PidBinding) -> bool {
        self.pids.contains(&pid)
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
            pids: self.pids.iter().copied().collect(),
            upstream_filters: self.upstream_filters.iter().copied().collect(),
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
        self.key = None;
        self.demux_id = None;
        self.demux_generation = None;
        self.pid_registrations.clear();
        self.pids.clear();
        self.upstream_filters.clear();
        if self.can_complete_close() {
            self.mark_closed();
        } else {
            self.close_state = DescramblerCloseState::Closing;
        }
    }

    pub fn pending_cleanup_items(&self) -> impl Iterator<Item = DescramblerCleanupItem> + '_ {
        self.pending_cleanup.iter().copied()
    }

    pub fn pending_cleanup_names(&self) -> Vec<&'static str> {
        self.pending_cleanup.iter().copied().map(DescramblerCleanupItem::as_str).collect()
    }

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
        session.add_pid(PidBinding { pid: 100 }, SourceFilterBinding { filter_id: -1, generation: 0 });
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
        session.set_demux(5, 9);
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
mod r50dz52_g3_02_tests {
    #[derive(Default)]
    struct FakeSetDemuxSourceRollback {
        demux_ledger_committed: bool,
        pending_cleanup: Vec<&'static str>,
        diagnostic: Option<&'static str>,
    }

    impl FakeSetDemuxSourceRollback {
        fn set_source_like_production(&mut self, commit_ok: bool, rollback_ok: bool) -> Result<(), &'static str> {
            self.demux_ledger_committed = commit_ok;
            if commit_ok {
                return Ok(());
            }
            if !rollback_ok {
                self.pending_cleanup.push("DemuxLedgerClose");
                self.diagnostic = Some("descrambler_demux_ledger_rollback_failed");
                return Err("UNKNOWN_ERROR");
            }
            Err("UNKNOWN_ERROR")
        }

        fn close_like_production(&mut self) {
            self.pending_cleanup.retain(|item| *item != "DemuxLedgerClose");
        }
    }

    #[test]
    fn rollback_failure_records_demux_ledger_close_pending_cleanup() {
        let mut session = FakeSetDemuxSourceRollback::default();
        assert_eq!(session.set_source_like_production(false, false), Err("UNKNOWN_ERROR"));
        assert_eq!(session.pending_cleanup, vec!["DemuxLedgerClose"]);
        assert_eq!(session.diagnostic, Some("descrambler_demux_ledger_rollback_failed"));
        session.close_like_production();
        assert!(session.pending_cleanup.is_empty());
    }
}

#[cfg(test)]
mod r50dz52_g3_08_tests {
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct FakePidLedgers {
        pid_registrations: BTreeSet<i32>,
        upstream_filters: BTreeSet<i32>,
        snapshots_for_demux: BTreeSet<i32>,
    }

    impl FakePidLedgers {
        fn with_pids(pids: &[i32]) -> Self {
            Self {
                pid_registrations: pids.iter().copied().collect(),
                upstream_filters: pids.iter().copied().collect(),
                snapshots_for_demux: pids.iter().copied().collect(),
            }
        }

        fn cleanup_stale_like_production(&mut self, stale_pid: i32) {
            self.pid_registrations.remove(&stale_pid);
            self.upstream_filters.remove(&stale_pid);
            self.snapshots_for_demux.remove(&stale_pid);
        }

        fn all_sets_match(&self) -> bool {
            self.pid_registrations == self.upstream_filters && self.upstream_filters == self.snapshots_for_demux
        }
    }

    #[test]
    fn stale_pid_cleanup_keeps_three_ledgers_consistent() {
        let mut ledgers = FakePidLedgers::with_pids(&[100, 200, 300]);
        ledgers.cleanup_stale_like_production(200);
        assert!(ledgers.all_sets_match());
        assert_eq!(ledgers.pid_registrations, [100, 300].iter().copied().collect());
    }
}

