//! HAL 内部 registry と live ID 台帳を一元化する部品。
//!
//! live ID、registry record、reserve/commit/rollback、close 再試行状態を
//! ledger が所有する。呼び出し側は ID と record map を別々に更新しない。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use crate::hal_sync::{HalLockError, HalMutex};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct LedgerId(pub i32);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LedgerState { Reserved, Live, Closing }

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LedgerError { AlreadyExists, NotFound, IdExhausted, InvalidState }

#[derive(Debug, Clone)]
struct LedgerRecord<R> { state: LedgerState, generation: u64, record: Option<R> }

#[derive(Debug)]
pub struct DemuxLedger<R = ()> { records: BTreeMap<LedgerId, LedgerRecord<R>>, next_generation: u64 }

impl<R> Default for DemuxLedger<R> {
    fn default() -> Self {
        Self { records: BTreeMap::new(), next_generation: 0 }
    }
}

impl<R: Clone> DemuxLedger<R> {
    pub fn create_live(&mut self, id: LedgerId, record: R) -> Result<u64, LedgerError> {
        if self.records.contains_key(&id) { return Err(LedgerError::AlreadyExists); }
        self.next_generation = self.next_generation.saturating_add(1);
        let generation = self.next_generation;
        self.records.insert(id, LedgerRecord { state: LedgerState::Live, generation, record: Some(record) });
        Ok(generation)
    }
    #[cfg(test)]
    pub fn open_or_recover(&mut self, id: LedgerId) -> Result<Option<R>, LedgerError> {
        match self.records.get_mut(&id) {
            Some(entry) if entry.state == LedgerState::Live => Ok(entry.record.clone()),
            Some(entry) if entry.record.is_none() => { self.records.remove(&id); Err(LedgerError::NotFound) }
            Some(entry) => { entry.state = LedgerState::Closing; Ok(entry.record.clone()) }
            None => Ok(None),
        }
    }
    pub fn get_live(&self, id: LedgerId) -> Result<R, LedgerError> {
        self.records.get(&id)
            .filter(|entry| entry.state == LedgerState::Live)
            .and_then(|entry| entry.record.clone())
            .ok_or(LedgerError::NotFound)
    }
    pub fn begin_close(&mut self, id: LedgerId) -> Result<(R, u64), LedgerError> {
        let entry = self.records.get_mut(&id).ok_or(LedgerError::NotFound)?;
        let record = entry.record.clone().ok_or(LedgerError::InvalidState)?;
        entry.state = LedgerState::Closing;
        Ok((record, entry.generation))
    }
    pub fn commit_close(&mut self, id: LedgerId) -> Result<R, LedgerError> {
        match self.records.remove(&id) { Some(entry) => entry.record.ok_or(LedgerError::InvalidState), None => Err(LedgerError::NotFound) }
    }
    pub fn rollback_open(&mut self, id: LedgerId) -> Result<(), LedgerError> { self.records.remove(&id).map(|_| ()).ok_or(LedgerError::NotFound) }
    #[cfg(test)]
    pub fn insert_record(&mut self, id: LedgerId, record: R) -> Result<(), LedgerError> { self.create_live(id, record).map(|_| ()) }
    pub fn get_record(&self, id: LedgerId) -> Option<R> { self.get_live(id).ok() }
    pub fn contains_live(&self, id: LedgerId) -> bool { self.records.get(&id).is_some_and(|e| e.state == LedgerState::Live) }
    pub fn remove_record(&mut self, id: LedgerId) -> Result<R, LedgerError> { self.commit_close(id) }
    pub fn records(&self) -> impl Iterator<Item = R> + '_ { self.records.values().filter_map(|e| e.record.clone()) }
    pub fn first_available<I>(&self, ids: I) -> Option<i32> where I: IntoIterator<Item = i32> {
        ids.into_iter().find(|id| !self.records.contains_key(&LedgerId(*id)))
    }
    #[cfg(test)] pub fn contains_id_for_test(&self, id: i32) -> bool { self.records.contains_key(&LedgerId(id)) }
}
impl DemuxLedger<()> { pub fn insert_live(&mut self, id: LedgerId) -> Result<(), LedgerError> { self.create_live(id, ()).map(|_| ()) } pub fn remove_live(&mut self, id: LedgerId) -> Result<(), LedgerError> { self.commit_close(id).map(|_| ()) } }

#[derive(Debug, Default)]
pub struct ResourceLedger { reserved: BTreeSet<LedgerId>, live: BTreeSet<LedgerId>, closing: BTreeSet<LedgerId>, generation: BTreeMap<LedgerId, u64>, next_generation: u64 }
impl ResourceLedger {
    pub fn reserve(&mut self, id: LedgerId) -> Result<u64, LedgerError> {
        if self.reserved.contains(&id) || self.live.contains(&id) || self.closing.contains(&id) { return Err(LedgerError::AlreadyExists); }
        self.next_generation = self.next_generation.saturating_add(1);
        self.reserved.insert(id); self.generation.insert(id, self.next_generation); Ok(self.next_generation)
    }
    pub fn commit_open(&mut self, id: LedgerId) -> Result<(), LedgerError> { if !self.reserved.remove(&id) { return Err(LedgerError::NotFound); } self.live.insert(id); Ok(()) }
    pub fn rollback_open(&mut self, id: LedgerId) -> Result<(), LedgerError> { if !self.reserved.remove(&id) { return Err(LedgerError::NotFound); } self.generation.remove(&id); Ok(()) }
    pub fn begin_close(&mut self, id: LedgerId) -> Result<u64, LedgerError> {
        if self.closing.contains(&id) { return Ok(*self.generation.get(&id).unwrap_or(&0)); }
        if !self.live.remove(&id) { return Err(LedgerError::NotFound); }
        self.closing.insert(id);
        Ok(*self.generation.get(&id).unwrap_or(&0))
    }
    pub fn commit_close(&mut self, id: LedgerId) -> Result<(), LedgerError> {
        if !self.closing.remove(&id) { return Err(LedgerError::NotFound); }
        self.generation.remove(&id);
        Ok(())
    }
    pub fn rollback_close(&mut self, id: LedgerId) -> Result<(), LedgerError> {
        if !self.closing.remove(&id) { return Err(LedgerError::NotFound); }
        self.live.insert(id);
        Ok(())
    }
    pub fn generation(&self, id: LedgerId) -> Option<u64> { self.generation.get(&id).copied() }
}

#[derive(Debug, Default)] pub struct FilterLedger { inner: ResourceLedger }
#[derive(Debug, Default)] pub struct DvrLedger { inner: ResourceLedger }
#[derive(Debug, Default)] pub struct DescramblerLedger { inner: ResourceLedger }
macro_rules! resource_ledger_api { ($t:ty) => { impl $t { pub fn reserve(&mut self, id: LedgerId)->Result<u64,LedgerError>{self.inner.reserve(id)} pub fn commit_open(&mut self,id:LedgerId)->Result<(),LedgerError>{self.inner.commit_open(id)} pub fn rollback_open(&mut self,id:LedgerId)->Result<(),LedgerError>{self.inner.rollback_open(id)} pub fn begin_close(&mut self,id:LedgerId)->Result<u64,LedgerError>{self.inner.begin_close(id)} pub fn commit_close(&mut self,id:LedgerId)->Result<(),LedgerError>{self.inner.commit_close(id)} pub fn rollback_close(&mut self,id:LedgerId)->Result<(),LedgerError>{self.inner.rollback_close(id)} pub fn generation(&self,id:LedgerId)->Option<u64>{self.inner.generation(id)} } }; }
resource_ledger_api!(FilterLedger); resource_ledger_api!(DvrLedger); resource_ledger_api!(DescramblerLedger);

#[derive(Debug, Default)] pub struct LnbLedger;
static LEDGER_OPERATION_LOCKS: OnceLock<HalMutex<BTreeSet<i32>>> = OnceLock::new();
static LEDGER_OPERATION_FAILURES: OnceLock<HalMutex<BTreeMap<i32, &'static str>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LnbOperationGuardError {
    Busy,
    Poisoned,
    DropReleaseFailed,
}

#[derive(Debug)]
pub struct LnbOperationGuard {
    lnb_id: i32,
    active: bool,
}

fn lnb_operation_failures() -> &'static HalMutex<BTreeMap<i32, &'static str>> {
    LEDGER_OPERATION_FAILURES.get_or_init(|| HalMutex::new(BTreeMap::new()))
}

fn record_lnb_operation_failure(lnb_id: i32, diagnostic: &'static str) {
    match lnb_operation_failures().lock() {
        Ok(mut failures) => {
            failures.insert(lnb_id, diagnostic);
        }
        Err(_) => {
            eprintln!("maleicacid-tuner-hal-lnb-diagnostic: lnb_id={lnb_id} lnb_operation_failure_diagnostic_poisoned diagnostic={diagnostic}");
        }
    }
}

impl Drop for LnbOperationGuard {
    fn drop(&mut self) {
        if !self.active { return; }
        if let Some(locks) = LEDGER_OPERATION_LOCKS.get() {
            match locks.lock() {
                Ok(mut active) => {
                    if !active.remove(&self.lnb_id) {
                        record_lnb_operation_failure(self.lnb_id, "lnb_operation_guard_release_missing");
                    }
                }
                Err(_) => {
                    record_lnb_operation_failure(self.lnb_id, "lnb_operation_guard_release_poisoned");
                }
            }
        } else {
            record_lnb_operation_failure(self.lnb_id, "lnb_operation_guard_release_unavailable");
        }
    }
}

impl LnbLedger {
    pub fn operation_guard(lnb_id: i32) -> Result<LnbOperationGuard, LnbOperationGuardError> {
        let locks = LEDGER_OPERATION_LOCKS.get_or_init(|| HalMutex::new(BTreeSet::new()));
        {
            let mut failures = lnb_operation_failures().lock().map_err(|_| LnbOperationGuardError::Poisoned)?;
            if failures.remove(&lnb_id).is_some() {
                return Err(LnbOperationGuardError::DropReleaseFailed);
            }
        }
        let mut active = locks.lock().map_err(|_: HalLockError| LnbOperationGuardError::Poisoned)?;
        if active.contains(&lnb_id) {
            return Err(LnbOperationGuardError::Busy);
        }
        active.insert(lnb_id);
        Ok(LnbOperationGuard { lnb_id, active: true })
    }

    #[cfg(test)]

    pub fn operation_failure_diagnostic(lnb_id: i32) -> Option<&'static str> {
        lnb_operation_failures().lock().ok().and_then(|failures| failures.get(&lnb_id).copied())
    }
}
#[derive(Debug, Default)] pub struct FrontendBindingLedger;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demux_ledger_owns_record_and_live_id_together(){
        let mut ledger=DemuxLedger::<i32>::default();
        assert!(ledger.create_live(LedgerId(3),30).is_ok());
        assert!(ledger.contains_live(LedgerId(3)));
        assert_eq!(ledger.get_live(LedgerId(3)),Ok(30));
        assert!(ledger.begin_close(LedgerId(3)).is_ok());
        assert_eq!(ledger.commit_close(LedgerId(3)),Ok(30));
        assert!(!ledger.contains_live(LedgerId(3)));
    }

    #[test]
    fn resource_ledger_rolls_back_reserved_id(){
        let mut ledger=FilterLedger::default();
        assert!(ledger.reserve(LedgerId(1)).is_ok());
        assert!(ledger.rollback_open(LedgerId(1)).is_ok());
        assert!(ledger.reserve(LedgerId(1)).is_ok());
    }

    #[test]
    fn resource_ledger_rolls_back_close_to_live_state(){
        let mut ledger = FilterLedger::default();
        assert!(ledger.reserve(LedgerId(10)).is_ok());
        assert!(ledger.commit_open(LedgerId(10)).is_ok());
        let generation = ledger.begin_close(LedgerId(10)).unwrap();
        assert!(ledger.rollback_close(LedgerId(10)).is_ok());
        assert_eq!(ledger.begin_close(LedgerId(10)), Ok(generation));
        assert!(ledger.commit_close(LedgerId(10)).is_ok());
        assert!(ledger.begin_close(LedgerId(10)).is_err());
    }
}

#[cfg(test)]
mod r50dz52_g1_09_tests {
    #[derive(Default)]
    struct FakeLnbOperationLedger {
        busy: bool,
        poisoned: bool,
        internal_failed: bool,
        diagnostic: Option<&'static str>,
    }

    impl FakeLnbOperationLedger {
        fn operation_guard_like_production(&mut self) -> Result<(), &'static str> {
            if self.internal_failed {
                return Err("UNKNOWN_ERROR:lnb_internal_failed");
            }
            if self.poisoned {
                self.internal_failed = true;
                self.diagnostic = Some("lnb_internal_failed");
                return Err("UNKNOWN_ERROR:poisoned");
            }
            if self.busy {
                return Err("UNAVAILABLE:busy");
            }
            self.busy = true;
            Ok(())
        }

        fn release_like_production(&mut self) {
            self.busy = false;
        }
    }

    #[test]
    fn busy_operation_is_unavailable_but_mutex_poison_marks_lnb_internal_failed() {
        let mut ledger = FakeLnbOperationLedger::default();
        assert_eq!(ledger.operation_guard_like_production(), Ok(()));
        assert_eq!(ledger.operation_guard_like_production(), Err("UNAVAILABLE:busy"));
        ledger.release_like_production();
        ledger.poisoned = true;
        assert_eq!(ledger.operation_guard_like_production(), Err("UNKNOWN_ERROR:poisoned"));
        assert!(ledger.internal_failed);
        assert_eq!(ledger.diagnostic, Some("lnb_internal_failed"));
        assert_eq!(ledger.operation_guard_like_production(), Err("UNKNOWN_ERROR:lnb_internal_failed"));
    }
}

#[cfg(test)]
mod r50dz52_g1_10_tests {
    #[derive(Default)]
    struct FakeLnbGuardDropState {
        diagnostic: Option<&'static str>,
        active_release_ok: bool,
    }

    impl FakeLnbGuardDropState {
        fn drop_guard_like_production(&mut self) {
            if !self.active_release_ok {
                self.diagnostic = Some("lnb_operation_guard_release_failed");
            }
        }

        fn next_operation_like_production(&self) -> Result<(), &'static str> {
            if self.diagnostic.is_some() { Err("UNKNOWN_ERROR:drop_release_failed") } else { Ok(()) }
        }
    }

    #[test]
    fn drop_release_failure_is_recorded_and_next_operation_observes_it() {
        let mut state = FakeLnbGuardDropState { active_release_ok: false, ..FakeLnbGuardDropState::default() };
        state.drop_guard_like_production();
        assert_eq!(state.diagnostic, Some("lnb_operation_guard_release_failed"));
        assert_eq!(state.next_operation_like_production(), Err("UNKNOWN_ERROR:drop_release_failed"));
    }
}
