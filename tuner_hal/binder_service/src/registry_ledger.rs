//! HAL 内部 registry と live ID 台帳を一元化する部品。
//!
//! live ID、registry record、reserve/commit/rollback、close 再試行状態を
//! ledger が所有する。呼び出し側は ID と record map を別々に更新しない。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use crate::hal_sync::{HalLockError, HalMutex};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct LedgerId(pub i32);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum DemuxCleanupStep {
    UnbindFrontend,
    CloseHandle,
    InvalidateDescramblers,
    CommitClose,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LedgerState { Reserved, Live, Closing, CleanupFailed, Quarantined }

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LedgerError { AlreadyExists, NotFound, IdExhausted, InvalidState }

#[derive(Debug, Clone)]
struct LedgerRecord<R> {
    state: LedgerState,
    generation: u64,
    record: Option<R>,
    ref_count: usize,
    bound_frontend_id: Option<i32>,
    bound_frontend_generation: Option<u64>,
    next_cleanup_step: DemuxCleanupStep,
}

#[derive(Debug)]
pub struct DemuxLedger<R = ()> { records: BTreeMap<LedgerId, LedgerRecord<R>>, next_generation: u64 }

impl<R> Default for DemuxLedger<R> {
    fn default() -> Self {
        Self { records: BTreeMap::new(), next_generation: 0 }
    }
}

#[derive(Debug, Clone)]
pub enum DemuxCloseAction<R> {
    StillReferenced,
    Final {
        record: R,
        generation: u64,
        bound_frontend_id: Option<i32>,
        next_step: DemuxCleanupStep,
    },
}

impl<R: Clone> DemuxLedger<R> {
    fn create_live(&mut self, id: LedgerId, record: R) -> Result<u64, LedgerError> {
        if self.records.contains_key(&id) { return Err(LedgerError::AlreadyExists); }
        self.next_generation = self.next_generation.checked_add(1).ok_or(LedgerError::IdExhausted)?;
        let generation = self.next_generation;
        self.records.insert(id, LedgerRecord {
            state: LedgerState::Live,
            generation,
            record: Some(record),
            ref_count: 1,
            bound_frontend_id: None,
            bound_frontend_generation: None,
            next_cleanup_step: DemuxCleanupStep::UnbindFrontend,
        });
        Ok(generation)
    }
    #[cfg(test)]
    fn open_or_recover(&mut self, id: LedgerId) -> Result<Option<R>, LedgerError> {
        match self.records.get_mut(&id) {
            Some(entry) if entry.state == LedgerState::Live && entry.ref_count > 0 => Ok(entry.record.clone()),
            Some(entry) if entry.record.is_none() => { self.records.remove(&id); Err(LedgerError::NotFound) }
            Some(entry) => { entry.state = LedgerState::Closing; Ok(entry.record.clone()) }
            None => Ok(None),
        }
    }
    fn get_live(&self, id: LedgerId) -> Result<R, LedgerError> {
        self.records.get(&id)
            .filter(|entry| entry.state == LedgerState::Live && entry.ref_count > 0)
            .and_then(|entry| entry.record.clone())
            .ok_or(LedgerError::NotFound)
    }
    fn generation(&self, id: LedgerId) -> Option<u64> {
        self.records.get(&id).map(|entry| entry.generation)
    }
    fn acquire_ref(&mut self, id: LedgerId) -> Result<R, LedgerError> {
        let entry = self.records.get_mut(&id).ok_or(LedgerError::NotFound)?;
        if entry.state != LedgerState::Live || entry.ref_count == 0 { return Err(LedgerError::InvalidState); }
        entry.ref_count = entry.ref_count.checked_add(1).ok_or(LedgerError::IdExhausted)?;
        entry.record.clone().ok_or(LedgerError::InvalidState)
    }
    fn begin_close_ref(&mut self, id: LedgerId) -> Result<DemuxCloseAction<R>, LedgerError> {
        let entry = self.records.get_mut(&id).ok_or(LedgerError::NotFound)?;
        let record = entry.record.clone().ok_or(LedgerError::InvalidState)?;
        match entry.state {
            LedgerState::Live => {
                if entry.ref_count == 0 { return Err(LedgerError::InvalidState); }
                if entry.ref_count > 1 {
                    entry.ref_count -= 1;
                    return Ok(DemuxCloseAction::StillReferenced);
                }
                entry.ref_count = 0;
                entry.state = LedgerState::Closing;
                entry.next_cleanup_step = DemuxCleanupStep::UnbindFrontend;
            }
            LedgerState::Closing | LedgerState::CleanupFailed | LedgerState::Quarantined => {}
            LedgerState::Reserved => return Err(LedgerError::InvalidState),
        }
        Ok(DemuxCloseAction::Final {
            record,
            generation: entry.generation,
            bound_frontend_id: entry.bound_frontend_id,
            next_step: entry.next_cleanup_step,
        })
    }
    fn mark_cleanup_failed(&mut self, id: LedgerId, next_step: DemuxCleanupStep) -> Result<(), LedgerError> {
        let entry = self.records.get_mut(&id).ok_or(LedgerError::NotFound)?;
        entry.state = LedgerState::CleanupFailed;
        entry.next_cleanup_step = next_step;
        Ok(())
    }
    fn mark_cleanup_progress(&mut self, id: LedgerId, next_step: DemuxCleanupStep) -> Result<(), LedgerError> {
        let entry = self.records.get_mut(&id).ok_or(LedgerError::NotFound)?;
        if !matches!(entry.state, LedgerState::Closing | LedgerState::CleanupFailed | LedgerState::Quarantined) {
            return Err(LedgerError::InvalidState);
        }
        entry.next_cleanup_step = next_step;
        Ok(())
    }
    fn quarantine(&mut self, id: LedgerId) -> Result<(), LedgerError> {
        let entry = self.records.get_mut(&id).ok_or(LedgerError::NotFound)?;
        entry.state = LedgerState::Quarantined;
        entry.ref_count = 0;
        entry.next_cleanup_step = DemuxCleanupStep::InvalidateDescramblers;
        Ok(())
    }
    fn commit_close(&mut self, id: LedgerId) -> Result<R, LedgerError> {
        match self.records.get(&id) {
            Some(entry) if matches!(entry.state, LedgerState::Closing | LedgerState::CleanupFailed | LedgerState::Quarantined) => {}
            Some(_) => return Err(LedgerError::InvalidState),
            None => return Err(LedgerError::NotFound),
        }
        match self.records.remove(&id) { Some(entry) => entry.record.ok_or(LedgerError::InvalidState), None => Err(LedgerError::NotFound) }
    }
    fn rollback_open(&mut self, id: LedgerId) -> Result<(), LedgerError> { self.records.remove(&id).map(|_| ()).ok_or(LedgerError::NotFound) }
    #[cfg(test)]
    fn insert_record(&mut self, id: LedgerId, record: R) -> Result<(), LedgerError> { self.create_live(id, record).map(|_| ()) }
    fn get_record(&self, id: LedgerId) -> Option<R> { self.get_live(id).ok() }
    fn get_record_any_state(&self, id: LedgerId) -> Option<R> { self.records.get(&id).and_then(|e| e.record.clone()) }
    fn contains_live(&self, id: LedgerId) -> bool { self.records.get(&id).is_some_and(|e| e.state == LedgerState::Live && e.ref_count > 0) }
    fn remove_record(&mut self, id: LedgerId) -> Result<R, LedgerError> { self.commit_close(id) }
    fn records(&self) -> impl Iterator<Item = R> + '_ { self.records.values().filter_map(|e| e.record.clone()) }
    fn first_available<I>(&self, ids: I) -> Option<i32> where I: IntoIterator<Item = i32> {
        ids.into_iter().find(|id| !self.records.contains_key(&LedgerId(*id)))
    }
    fn current_binding(&self, id: LedgerId) -> Result<(Option<i32>, Option<u64>), LedgerError> {
        let entry = self.records.get(&id).ok_or(LedgerError::NotFound)?;
        Ok((entry.bound_frontend_id, entry.bound_frontend_generation))
    }
    fn commit_binding(&mut self, id: LedgerId, frontend_id: Option<i32>, generation: Option<u64>) -> Result<(), LedgerError> {
        let entry = self.records.get_mut(&id).ok_or(LedgerError::NotFound)?;
        if entry.state != LedgerState::Live { return Err(LedgerError::InvalidState); }
        entry.bound_frontend_id = frontend_id;
        entry.bound_frontend_generation = generation;
        Ok(())
    }
    fn clear_binding_if_matches(&mut self, id: LedgerId, frontend_id: i32, generation: u64) -> Result<bool, LedgerError> {
        let entry = self.records.get_mut(&id).ok_or(LedgerError::NotFound)?;
        if entry.bound_frontend_id == Some(frontend_id) && entry.bound_frontend_generation == Some(generation) {
            entry.bound_frontend_id = None;
            entry.bound_frontend_generation = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    #[cfg(test)] fn contains_id_for_test(&self, id: i32) -> bool { self.records.contains_key(&LedgerId(id)) }
}

/// DemuxLifecycleTxn is the owner-side façade for the ref-counted demux ledger.
///
/// Demux has different semantics from child resources because multiple binder
/// handles can reference the same live demux. Public demux open/close/binding
/// paths should use this façade instead of hand-calling DemuxLedger primitives.
pub struct DemuxLifecycleTxn;

impl DemuxLifecycleTxn {
    pub fn create_live<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId, record: R) -> Result<u64, LedgerError> { ledger.create_live(id, record) }
    pub fn get_record<R: Clone>(ledger: &DemuxLedger<R>, id: LedgerId) -> Option<R> { ledger.get_record(id) }
    pub fn get_live<R: Clone>(ledger: &DemuxLedger<R>, id: LedgerId) -> Result<R, LedgerError> { ledger.get_live(id) }
    pub fn acquire_ref<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId) -> Result<R, LedgerError> { ledger.acquire_ref(id) }
    pub fn begin_close<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId) -> Result<DemuxCloseAction<R>, LedgerError> { ledger.begin_close_ref(id) }
    pub fn rollback_open<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId) -> Result<(), LedgerError> { ledger.rollback_open(id) }
    pub fn quarantine<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId) -> Result<(), LedgerError> { ledger.quarantine(id) }
    pub fn commit_close<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId) -> Result<R, LedgerError> { ledger.commit_close(id) }
    pub fn mark_cleanup_failed<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId, step: DemuxCleanupStep) -> Result<(), LedgerError> { ledger.mark_cleanup_failed(id, step) }
    pub fn mark_cleanup_progress<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId, step: DemuxCleanupStep) -> Result<(), LedgerError> { ledger.mark_cleanup_progress(id, step) }
    pub fn generation<R: Clone>(ledger: &DemuxLedger<R>, id: LedgerId) -> Option<u64> { ledger.generation(id) }
    pub fn current_binding<R: Clone>(ledger: &DemuxLedger<R>, id: LedgerId) -> Result<(Option<i32>, Option<u64>), LedgerError> { ledger.current_binding(id) }
    pub fn commit_binding<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId, frontend_id: Option<i32>, generation: Option<u64>) -> Result<(), LedgerError> { ledger.commit_binding(id, frontend_id, generation) }
    pub fn clear_binding_if_matches<R: Clone>(ledger: &mut DemuxLedger<R>, id: LedgerId, frontend_id: i32, generation: u64) -> Result<bool, LedgerError> { ledger.clear_binding_if_matches(id, frontend_id, generation) }
    pub fn first_available<R: Clone, I>(ledger: &DemuxLedger<R>, ids: I) -> Option<i32> where I: IntoIterator<Item = i32> { ledger.first_available(ids) }
    pub fn records<R: Clone>(ledger: &DemuxLedger<R>) -> impl Iterator<Item = R> + '_ { ledger.records() }
}

impl DemuxLedger<()> {
    fn insert_live(&mut self, id: LedgerId) -> Result<(), LedgerError> { self.create_live(id, ()).map(|_| ()) }
    fn remove_live(&mut self, id: LedgerId) -> Result<(), LedgerError> {
        match self.begin_close_ref(id)? {
            DemuxCloseAction::StillReferenced => Err(LedgerError::InvalidState),
            DemuxCloseAction::Final { .. } => self.commit_close(id).map(|_| ()),
        }
    }
}

#[derive(Debug, Default)]
struct ResourceLedger {
    reserved: BTreeSet<LedgerId>,
    live: BTreeSet<LedgerId>,
    closing: BTreeSet<LedgerId>,
    quarantined: BTreeSet<LedgerId>,
    generation: BTreeMap<LedgerId, u64>,
    cleanup_step: BTreeMap<LedgerId, &'static str>,
    next_generation: u64,
}
impl ResourceLedger {
    fn reserve(&mut self, id: LedgerId) -> Result<u64, LedgerError> {
        if self.reserved.contains(&id) || self.live.contains(&id) || self.closing.contains(&id) || self.quarantined.contains(&id) { return Err(LedgerError::AlreadyExists); }
        self.next_generation = self.next_generation.checked_add(1).ok_or(LedgerError::IdExhausted)?;
        self.reserved.insert(id); self.generation.insert(id, self.next_generation); self.cleanup_step.remove(&id); Ok(self.next_generation)
    }
    fn commit_open(&mut self, id: LedgerId) -> Result<(), LedgerError> { if !self.reserved.remove(&id) { return Err(LedgerError::NotFound); } self.live.insert(id); Ok(()) }
    fn rollback_open(&mut self, id: LedgerId) -> Result<(), LedgerError> { if !self.reserved.remove(&id) { return Err(LedgerError::NotFound); } self.generation.remove(&id); self.cleanup_step.remove(&id); Ok(()) }
    fn begin_close(&mut self, id: LedgerId) -> Result<u64, LedgerError> {
        if self.closing.contains(&id) || self.quarantined.contains(&id) { return Ok(*self.generation.get(&id).unwrap_or(&0)); }
        if !self.live.remove(&id) { return Err(LedgerError::NotFound); }
        self.closing.insert(id);
        self.cleanup_step.entry(id).or_insert("begin_close");
        Ok(*self.generation.get(&id).unwrap_or(&0))
    }
    fn quarantine(&mut self, id: LedgerId) -> Result<(), LedgerError> {
        if self.reserved.remove(&id) || self.live.remove(&id) || self.closing.remove(&id) || self.generation.contains_key(&id) {
            self.quarantined.insert(id);
            self.cleanup_step.insert(id, "quarantined");
            return Ok(());
        }
        Err(LedgerError::NotFound)
    }
    fn commit_close(&mut self, id: LedgerId) -> Result<(), LedgerError> {
        if !self.closing.remove(&id) && !self.quarantined.remove(&id) { return Err(LedgerError::NotFound); }
        self.generation.remove(&id);
        self.cleanup_step.remove(&id);
        Ok(())
    }
    fn rollback_close(&mut self, id: LedgerId) -> Result<(), LedgerError> {
        if !self.closing.remove(&id) { return Err(LedgerError::NotFound); }
        self.live.insert(id);
        self.cleanup_step.remove(&id);
        Ok(())
    }
    fn generation(&self, id: LedgerId) -> Option<u64> { self.generation.get(&id).copied() }
    fn cleanup_step(&self, id: LedgerId) -> Option<&'static str> { self.cleanup_step.get(&id).copied() }
    fn mark_cleanup_step(&mut self, id: LedgerId, step: &'static str) -> Result<(), LedgerError> {
        if !self.closing.contains(&id) && !self.quarantined.contains(&id) {
            return Err(LedgerError::InvalidState);
        }
        self.cleanup_step.insert(id, step);
        Ok(())
    }
}

#[derive(Debug, Default)] pub struct FilterLedger { inner: ResourceLedger }
#[derive(Debug, Default)] pub struct DvrLedger { inner: ResourceLedger }
#[derive(Debug, Default)] pub struct DescramblerLedger { inner: ResourceLedger }
macro_rules! resource_ledger_api { ($t:ty) => { impl $t { fn reserve(&mut self, id: LedgerId)->Result<u64,LedgerError>{self.inner.reserve(id)} fn commit_open(&mut self,id:LedgerId)->Result<(),LedgerError>{self.inner.commit_open(id)} fn rollback_open(&mut self,id:LedgerId)->Result<(),LedgerError>{self.inner.rollback_open(id)} fn begin_close(&mut self,id:LedgerId)->Result<u64,LedgerError>{self.inner.begin_close(id)} fn quarantine(&mut self,id:LedgerId)->Result<(),LedgerError>{self.inner.quarantine(id)} fn commit_close(&mut self,id:LedgerId)->Result<(),LedgerError>{self.inner.commit_close(id)} fn generation(&self,id:LedgerId)->Option<u64>{self.inner.generation(id)} fn cleanup_step(&self,id:LedgerId)->Option<&'static str>{self.inner.cleanup_step(id)} fn mark_cleanup_step(&mut self,id:LedgerId,step:&'static str)->Result<(),LedgerError>{self.inner.mark_cleanup_step(id,step)} } }; }
resource_ledger_api!(FilterLedger); resource_ledger_api!(DvrLedger); resource_ledger_api!(DescramblerLedger);


/// ResourceLifecycleTxn is the owner-side façade for per-resource ledgers.
///
/// Public API paths must use this façade for reserve / commit / rollback /
/// begin-close / quarantine / cleanup-step / commit-close so that Filter, DVR,
/// and Descrambler resource lifetimes do not drift into hand-written variants.
pub struct ResourceLifecycleTxn;

impl ResourceLifecycleTxn {
    pub fn reserve_filter(ledger: &mut FilterLedger, id: LedgerId) -> Result<u64, LedgerError> { ledger.reserve(id) }
    pub fn commit_filter_open(ledger: &mut FilterLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.commit_open(id) }
    pub fn rollback_filter_open(ledger: &mut FilterLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.rollback_open(id) }
    pub fn begin_filter_close(ledger: &mut FilterLedger, id: LedgerId) -> Result<u64, LedgerError> { ledger.begin_close(id) }
    pub fn quarantine_filter(ledger: &mut FilterLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.quarantine(id) }
    pub fn commit_filter_close(ledger: &mut FilterLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.commit_close(id) }
    pub fn filter_cleanup_step(ledger: &FilterLedger, id: LedgerId) -> Option<&'static str> { ledger.cleanup_step(id) }
    pub fn filter_generation(ledger: &FilterLedger, id: LedgerId) -> Option<u64> { ledger.generation(id) }
    pub fn mark_filter_cleanup_step(ledger: &mut FilterLedger, id: LedgerId, step: &'static str) -> Result<(), LedgerError> { ledger.mark_cleanup_step(id, step) }

    pub fn reserve_dvr(ledger: &mut DvrLedger, id: LedgerId) -> Result<u64, LedgerError> { ledger.reserve(id) }
    pub fn commit_dvr_open(ledger: &mut DvrLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.commit_open(id) }
    pub fn rollback_dvr_open(ledger: &mut DvrLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.rollback_open(id) }
    pub fn begin_dvr_close(ledger: &mut DvrLedger, id: LedgerId) -> Result<u64, LedgerError> { ledger.begin_close(id) }
    pub fn quarantine_dvr(ledger: &mut DvrLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.quarantine(id) }
    pub fn commit_dvr_close(ledger: &mut DvrLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.commit_close(id) }
    pub fn dvr_cleanup_step(ledger: &DvrLedger, id: LedgerId) -> Option<&'static str> { ledger.cleanup_step(id) }
    pub fn dvr_generation(ledger: &DvrLedger, id: LedgerId) -> Option<u64> { ledger.generation(id) }
    pub fn mark_dvr_cleanup_step(ledger: &mut DvrLedger, id: LedgerId, step: &'static str) -> Result<(), LedgerError> { ledger.mark_cleanup_step(id, step) }

    pub fn reserve_descrambler(ledger: &mut DescramblerLedger, id: LedgerId) -> Result<u64, LedgerError> { ledger.reserve(id) }
    pub fn commit_descrambler_open(ledger: &mut DescramblerLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.commit_open(id) }
    pub fn rollback_descrambler_open(ledger: &mut DescramblerLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.rollback_open(id) }
    pub fn begin_descrambler_close(ledger: &mut DescramblerLedger, id: LedgerId) -> Result<u64, LedgerError> { ledger.begin_close(id) }
    pub fn quarantine_descrambler(ledger: &mut DescramblerLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.quarantine(id) }
    pub fn commit_descrambler_close(ledger: &mut DescramblerLedger, id: LedgerId) -> Result<(), LedgerError> { ledger.commit_close(id) }
    pub fn descrambler_generation(ledger: &DescramblerLedger, id: LedgerId) -> Option<u64> { ledger.generation(id) }
}


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

    #[cfg(test)]
    pub fn inject_operation_failure_for_test(lnb_id: i32, diagnostic: &'static str) {
        lnb_operation_failures()
            .lock()
            .expect("lnb operation failure table should be lockable in test")
            .insert(lnb_id, diagnostic);
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
        assert!(matches!(ledger.begin_close_ref(LedgerId(3)), Ok(DemuxCloseAction::Final { next_step: DemuxCleanupStep::UnbindFrontend, .. })));
        assert_eq!(ledger.commit_close(LedgerId(3)),Ok(30));
        assert!(!ledger.contains_live(LedgerId(3)));
    }

    #[test]
    fn demux_ledger_cleanup_failure_retries_from_recorded_step(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(4), 40), Ok(1));
        assert!(matches!(ledger.begin_close_ref(LedgerId(4)), Ok(DemuxCloseAction::Final { next_step: DemuxCleanupStep::UnbindFrontend, .. })));
        assert_eq!(ledger.mark_cleanup_failed(LedgerId(4), DemuxCleanupStep::CloseHandle), Ok(()));
        assert!(matches!(ledger.begin_close_ref(LedgerId(4)), Ok(DemuxCloseAction::Final { next_step: DemuxCleanupStep::CloseHandle, .. })));
    }

    #[test]
    fn demux_ledger_quarantine_blocks_id_reuse_until_close_commit(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(5), 50), Ok(1));
        assert!(matches!(ledger.begin_close_ref(LedgerId(5)), Ok(DemuxCloseAction::Final { .. })));
        assert_eq!(ledger.quarantine(LedgerId(5)), Ok(()));
        assert_eq!(ledger.create_live(LedgerId(5), 51), Err(LedgerError::AlreadyExists));
        assert!(matches!(ledger.begin_close_ref(LedgerId(5)), Ok(DemuxCloseAction::Final { next_step: DemuxCleanupStep::InvalidateDescramblers, .. })));
        assert_eq!(ledger.commit_close(LedgerId(5)), Ok(50));
        assert_eq!(ledger.create_live(LedgerId(5), 51), Ok(2));
    }

    #[test]
    fn demux_generation_exhaustion_rejects_open(){
        let mut ledger = DemuxLedger::<i32>::default();
        ledger.next_generation = u64::MAX;
        assert_eq!(
            ledger.create_live(LedgerId(901), 9010),
            Err(LedgerError::IdExhausted)
        );
        assert_eq!(ledger.generation(LedgerId(901)), None);
        assert!(!ledger.contains_live(LedgerId(901)));
    }

    #[test]
    fn demux_close_failure_retries_from_failed_step(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(902), 9020), Ok(1));
        assert!(matches!(
            ledger.begin_close_ref(LedgerId(902)),
            Ok(DemuxCloseAction::Final {
                next_step: DemuxCleanupStep::UnbindFrontend,
                ..
            })
        ));
        assert_eq!(
            ledger.mark_cleanup_failed(LedgerId(902), DemuxCleanupStep::InvalidateDescramblers),
            Ok(())
        );
        assert!(matches!(
            ledger.begin_close_ref(LedgerId(902)),
            Ok(DemuxCloseAction::Final {
                next_step: DemuxCleanupStep::InvalidateDescramblers,
                ..
            })
        ));
    }

    #[test]
    fn demux_ledger_removed_only_after_cleanup_success(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(903), 9030), Ok(1));
        assert!(matches!(ledger.begin_close_ref(LedgerId(903)), Ok(DemuxCloseAction::Final { .. })));
        assert_eq!(
            ledger.mark_cleanup_failed(LedgerId(903), DemuxCleanupStep::CloseHandle),
            Ok(())
        );
        assert!(ledger.get_record_any_state(LedgerId(903)).is_some());
        assert_eq!(ledger.commit_close(LedgerId(903)), Ok(9030));
        assert!(ledger.get_record_any_state(LedgerId(903)).is_none());
    }

    #[test]
    fn demux_descrambler_invalidate_failure_quarantines_demux(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(904), 9040), Ok(1));
        assert!(matches!(ledger.begin_close_ref(LedgerId(904)), Ok(DemuxCloseAction::Final { .. })));
        assert_eq!(ledger.quarantine(LedgerId(904)), Ok(()));
        assert_eq!(
            ledger.create_live(LedgerId(904), 9041),
            Err(LedgerError::AlreadyExists)
        );
        assert!(matches!(
            ledger.begin_close_ref(LedgerId(904)),
            Ok(DemuxCloseAction::Final {
                next_step: DemuxCleanupStep::InvalidateDescramblers,
                ..
            })
        ));
    }

    #[test]
    fn demux_quarantine_blocks_id_reuse(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(905), 9050), Ok(1));
        assert_eq!(ledger.quarantine(LedgerId(905)), Ok(()));
        assert_eq!(
            ledger.create_live(LedgerId(905), 9051),
            Err(LedgerError::AlreadyExists)
        );
        assert_eq!(ledger.commit_close(LedgerId(905)), Ok(9050));
        assert_eq!(ledger.create_live(LedgerId(905), 9051), Ok(2));
    }



    #[test]
    fn demux_quarantine_excluded_from_open_demux_by_id(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(908), 9080), Ok(1));
        assert_eq!(ledger.quarantine(LedgerId(908)), Ok(()));
        assert_eq!(ledger.get_live(LedgerId(908)), Err(LedgerError::NotFound));
        assert_eq!(ledger.acquire_ref(LedgerId(908)), Err(LedgerError::InvalidState));
        assert_eq!(ledger.create_live(LedgerId(908), 9081), Err(LedgerError::AlreadyExists));
    }

    #[test]
    fn demux_quarantine_released_only_after_close_retry_success(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(909), 9090), Ok(1));
        assert_eq!(ledger.quarantine(LedgerId(909)), Ok(()));
        assert!(ledger.get_record_any_state(LedgerId(909)).is_some());
        assert!(matches!(
            ledger.begin_close_ref(LedgerId(909)),
            Ok(DemuxCloseAction::Final {
                next_step: DemuxCleanupStep::InvalidateDescramblers,
                ..
            })
        ));
        assert_eq!(ledger.commit_close(LedgerId(909)), Ok(9090));
        assert!(ledger.get_record_any_state(LedgerId(909)).is_none());
        assert_eq!(ledger.create_live(LedgerId(909), 9091), Ok(2));
    }

    #[test]
    fn frontend_unbind_boundary_failure_keeps_binding(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(906), 9060), Ok(1));
        assert_eq!(ledger.commit_binding(LedgerId(906), Some(10), Some(77)), Ok(()));

        // Simulate the production ordering: when stream-boundary reset fails,
        // DemuxLedger::commit_binding() is not called, so the old binding is
        // still the published binding.
        assert_eq!(ledger.current_binding(LedgerId(906)), Ok((Some(10), Some(77))));
    }

    #[test]
    fn frontend_unbind_boundary_success_clears_binding(){
        let mut ledger = DemuxLedger::<i32>::default();
        assert_eq!(ledger.create_live(LedgerId(907), 9070), Ok(1));
        assert_eq!(ledger.commit_binding(LedgerId(907), Some(10), Some(77)), Ok(()));

        // After stream-boundary reset succeeds, binding removal/update may be
        // committed.  This is the only point where the old binding disappears.
        assert_eq!(ledger.commit_binding(LedgerId(907), None, None), Ok(()));
        assert_eq!(ledger.current_binding(LedgerId(907)), Ok((None, None)));
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
mod r50ea3_filter_dvr_cleanup_step_tests {
    use super::*;

    #[test]
    fn filter_ledger_records_cleanup_step_for_retry(){
        let mut ledger = FilterLedger::default();
        assert!(ledger.reserve(LedgerId(20)).is_ok());
        assert!(ledger.commit_open(LedgerId(20)).is_ok());
        assert!(ledger.begin_close(LedgerId(20)).is_ok());
        assert_eq!(ledger.cleanup_step(LedgerId(20)), Some("begin_close"));
        assert_eq!(ledger.mark_cleanup_step(LedgerId(20), "runtime_unregister_filter"), Ok(()));
        assert_eq!(ledger.cleanup_step(LedgerId(20)), Some("runtime_unregister_filter"));
        assert!(ledger.begin_close(LedgerId(20)).is_ok());
        assert_eq!(ledger.cleanup_step(LedgerId(20)), Some("runtime_unregister_filter"));
    }

    #[test]
    fn dvr_ledger_records_cleanup_step_for_retry(){
        let mut ledger = DvrLedger::default();
        assert!(ledger.reserve(LedgerId(30)).is_ok());
        assert!(ledger.commit_open(LedgerId(30)).is_ok());
        assert!(ledger.begin_close(LedgerId(30)).is_ok());
        assert_eq!(ledger.mark_cleanup_step(LedgerId(30), "demux_unregister_dvr"), Ok(()));
        assert_eq!(ledger.cleanup_step(LedgerId(30)), Some("demux_unregister_dvr"));
        assert!(ledger.commit_close(LedgerId(30)).is_ok());
        assert_eq!(ledger.cleanup_step(LedgerId(30)), None);
    }
}


#[cfg(test)]
mod r50ea3_generation_exhaustion_tests {
    use super::*;

    #[test]
    fn filter_ledger_generation_exhaustion_rejects_open() {
        let mut ledger = FilterLedger::default();
        ledger.inner.next_generation = u64::MAX;
        assert_eq!(ledger.reserve(LedgerId(901)), Err(LedgerError::IdExhausted));
        assert_eq!(ledger.generation(LedgerId(901)), None);
        assert_eq!(ledger.cleanup_step(LedgerId(901)), None);
    }

    #[test]
    fn dvr_ledger_generation_exhaustion_rejects_open() {
        let mut ledger = DvrLedger::default();
        ledger.inner.next_generation = u64::MAX;
        assert_eq!(ledger.reserve(LedgerId(902)), Err(LedgerError::IdExhausted));
        assert_eq!(ledger.generation(LedgerId(902)), None);
        assert_eq!(ledger.cleanup_step(LedgerId(902)), None);
    }

    #[test]
    fn descrambler_ledger_generation_exhaustion_rejects_open() {
        let mut ledger = DescramblerLedger::default();
        ledger.inner.next_generation = u64::MAX;
        assert_eq!(ledger.reserve(LedgerId(903)), Err(LedgerError::IdExhausted));
        assert_eq!(ledger.generation(LedgerId(903)), None);
        assert_eq!(ledger.cleanup_step(LedgerId(903)), None);
    }
}
