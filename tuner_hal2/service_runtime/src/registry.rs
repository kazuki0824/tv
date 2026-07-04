use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::descrambler_key_table::{DescramblerKeyLookupError, DescramblerKeyTable};
use crate::descrambler_session::{
    DescramblerCleanupReport, DescramblerCleanupTxnError, DescramblerClearKeyTxnError,
    DescramblerReplaceKeyOutcome, DescramblerReplaceKeyTxnError, DescramblerRuntime,
    DescramblerSessionFailure, DescramblerSessionFailureKind, DescramblerSessionTxnStep,
};
use crate::diagnostics::{DescramblerDiagnosticKind, DescramblerDiagnosticRecord};
use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;
use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, FrontendSystem, HalError, HalInvalidArgumentKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_demux::{DemuxRuntime, FilterOpenType, FilterRuntimeState};
use maleicacid_tuner_hal2_demux::{PacketPid, PipelineDiagnostic};
use maleicacid_tuner_hal2_descrambler::{
    descramble_ts_packet_in_place, DescrambleFailure, DescrambleOutcome, DescramblerKeySlot,
    DescramblerKeyToken, DescramblerPid, DescramblerPidClaim,
};
use maleicacid_tuner_hal2_device::FrontendRuntime;
use maleicacid_tuner_hal2_lnb::LnbRuntime;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FrontendRuntimeId(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DemuxRuntimeId(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct LnbRuntimeId(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FilterRuntimeId(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DvrRuntimeId(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerRuntimeId(pub i32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendRegistryEntry {
    pub id: FrontendRuntimeId,
    pub backend: FrontendBackendKind,
    pub system: FrontendSystem,
    pub device_path: PathBuf,
    /// frontend exportと同じprobe sourceから導出した固定LNB profile。
    /// Noneの場合、frontendはLNB voltage statusやLNB bindingをadvertiseしてはならない。
    pub lnb_profile: Option<LnbRegistryProfile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemuxRegistryEntry {
    pub id: DemuxRuntimeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LnbRegistryProfile {
    Px4Device15VOnly,
    EarthPt1FixedLnb,
    NoPower,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnbRegistryEntry {
    pub id: LnbRuntimeId,
    pub name: Option<String>,
    pub owner_frontend_id: FrontendRuntimeId,
    pub profile: LnbRegistryProfile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterRegistryEntry {
    pub id: FilterRuntimeId,
    pub owner_demux_id: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrRegistryEntry {
    pub id: DvrRuntimeId,
    pub owner_demux_id: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescramblerRegistryEntry {
    pub id: DescramblerRuntimeId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedDescramblerClaimSet {
    claims: Vec<DescramblerPidClaim>,
    key_slot: Option<DescramblerKeySlot>,
}

impl ResolvedDescramblerClaimSet {
    pub(crate) fn into_parts(self) -> (Vec<DescramblerPidClaim>, Option<DescramblerKeySlot>) {
        (self.claims, self.key_slot)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedDescramblerPacketSnapshot {
    descrambler_pids: BTreeSet<DescramblerPid>,
    packet_pids: BTreeSet<PacketPid>,
    key_slot: Option<DescramblerKeySlot>,
    source_filter_ids_by_pid: BTreeMap<PacketPid, BTreeSet<i32>>,
}

impl ResolvedDescramblerPacketSnapshot {
    pub(crate) fn targets_packet_pid(&self, pid: PacketPid) -> bool {
        self.packet_pids.contains(&pid)
    }

    pub(crate) fn descramble_packet(
        &self,
        packet: &[u8; TS_PACKET_SIZE],
    ) -> Option<Result<([u8; TS_PACKET_SIZE], DescrambleOutcome), DescrambleFailure>> {
        let key_slot = self.key_slot.as_ref()?;
        let mut candidate = *packet;
        Some(
            descramble_ts_packet_in_place(&mut candidate, &self.descrambler_pids, key_slot)
                .map(|outcome| (candidate, outcome)),
        )
    }

    pub(crate) fn source_filter_ids_for_packet_pid(
        &self,
        pid: PacketPid,
    ) -> Option<&BTreeSet<i32>> {
        self.source_filter_ids_by_pid.get(&pid)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedDescramblerPacketMaterial {
    snapshots: Vec<ResolvedDescramblerPacketSnapshot>,
    diagnostics: Vec<PipelineDiagnostic>,
    diagnostic_records: Vec<DescramblerDiagnosticRecord>,
}

impl ResolvedDescramblerPacketMaterial {
    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<ResolvedDescramblerPacketSnapshot>,
        Vec<PipelineDiagnostic>,
        Vec<DescramblerDiagnosticRecord>,
    ) {
        (self.snapshots, self.diagnostics, self.diagnostic_records)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistryCommitError {
    DuplicateFrontendId {
        id: FrontendRuntimeId,
    },
    DuplicateDemuxId {
        id: DemuxRuntimeId,
    },
    DuplicateLnbId {
        id: LnbRuntimeId,
    },
    MissingFrontendId {
        id: FrontendRuntimeId,
    },
    MissingLnbId {
        id: LnbRuntimeId,
    },
    LnbFrontendMismatch {
        frontend_id: FrontendRuntimeId,
        lnb_id: LnbRuntimeId,
    },
    DuplicateFilterId {
        id: FilterRuntimeId,
    },
    DuplicateDvrId {
        id: DvrRuntimeId,
    },
    DuplicateDescramblerId {
        id: DescramblerRuntimeId,
    },
    RuntimeIdExhausted {
        kind: RuntimeRegistryKind,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeRegistryKind {
    Demux,
    Filter,
    Dvr,
    Descrambler,
}

#[derive(Debug)]
pub struct RuntimeRegistry {
    frontends: BTreeMap<FrontendRuntimeId, FrontendRegistryEntry>,
    frontend_runtimes: BTreeMap<FrontendRuntimeId, FrontendRuntime>,
    demuxes: BTreeMap<DemuxRuntimeId, DemuxRegistryEntry>,
    demux_runtimes: BTreeMap<DemuxRuntimeId, DemuxRuntime>,
    demux_frontend_bindings: BTreeMap<DemuxRuntimeId, FrontendRuntimeId>,
    lnbs: BTreeMap<LnbRuntimeId, LnbRegistryEntry>,
    lnb_runtimes: BTreeMap<LnbRuntimeId, LnbRuntime>,
    frontend_lnb_bindings: BTreeMap<FrontendRuntimeId, LnbRuntimeId>,
    filters: BTreeMap<FilterRuntimeId, FilterRegistryEntry>,
    dvrs: BTreeMap<DvrRuntimeId, DvrRegistryEntry>,
    descramblers: BTreeMap<DescramblerRuntimeId, DescramblerRegistryEntry>,
    descrambler_runtimes: BTreeMap<DescramblerRuntimeId, DescramblerRuntime>,
    descrambler_key_table: DescramblerKeyTable,
    next_demux_id: i32,
    next_lnb_id: i32,
    next_filter_id: i32,
    next_dvr_id: i32,
    next_descrambler_id: i32,
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        Self {
            frontends: BTreeMap::new(),
            frontend_runtimes: BTreeMap::new(),
            demuxes: BTreeMap::new(),
            demux_runtimes: BTreeMap::new(),
            demux_frontend_bindings: BTreeMap::new(),
            lnbs: BTreeMap::new(),
            lnb_runtimes: BTreeMap::new(),
            frontend_lnb_bindings: BTreeMap::new(),
            filters: BTreeMap::new(),
            dvrs: BTreeMap::new(),
            descramblers: BTreeMap::new(),
            descrambler_runtimes: BTreeMap::new(),
            descrambler_key_table: DescramblerKeyTable::default(),
            next_demux_id: 1,
            next_lnb_id: 1,
            next_filter_id: 1,
            next_dvr_id: 1,
            next_descrambler_id: 1,
        }
    }
}

impl RuntimeRegistry {
    pub fn register_frontend(
        &mut self,
        entry: FrontendRegistryEntry,
    ) -> Result<(), RegistryCommitError> {
        if self.frontends.contains_key(&entry.id) {
            return Err(RegistryCommitError::DuplicateFrontendId { id: entry.id });
        }
        let runtime = FrontendRuntime::new(entry.id.0, entry.backend);
        self.frontend_runtimes.insert(entry.id, runtime);
        self.frontends.insert(entry.id, entry);
        Ok(())
    }

    pub fn clear_frontends(&mut self) {
        self.frontends.clear();
        self.frontend_runtimes.clear();
        self.frontend_lnb_bindings.clear();
    }

    pub fn clear_lnbs(&mut self) {
        self.lnbs.clear();
        self.lnb_runtimes.clear();
        self.frontend_lnb_bindings.clear();
        self.next_lnb_id = 1;
    }

    pub fn clear_transient_objects(&mut self) {
        self.demuxes.clear();
        self.demux_runtimes.clear();
        self.demux_frontend_bindings.clear();
        self.filters.clear();
        self.dvrs.clear();
        self.descramblers.clear();
        self.descrambler_runtimes.clear();
        self.descrambler_key_table = DescramblerKeyTable::default();
        self.next_demux_id = 1;
        self.next_filter_id = 1;
        self.next_dvr_id = 1;
        self.next_descrambler_id = 1;
    }

    pub fn frontend_count(&self) -> usize {
        self.frontends.len()
    }

    pub fn allocate_demux(&mut self) -> Result<DemuxRegistryEntry, RegistryCommitError> {
        let id = DemuxRuntimeId(self.next_demux_id);
        let next = self
            .next_demux_id
            .checked_add(1)
            .filter(|value| *value > 0)
            .ok_or(RegistryCommitError::RuntimeIdExhausted {
                kind: RuntimeRegistryKind::Demux,
            })?;
        self.next_demux_id = next;
        let entry = DemuxRegistryEntry { id };
        self.register_demux(entry.clone())?;
        Ok(entry)
    }

    pub fn register_demux(&mut self, entry: DemuxRegistryEntry) -> Result<(), RegistryCommitError> {
        if self.demuxes.contains_key(&entry.id) {
            return Err(RegistryCommitError::DuplicateDemuxId { id: entry.id });
        }
        self.demux_runtimes
            .insert(entry.id, DemuxRuntime::new(entry.id.0, 1));
        self.demuxes.insert(entry.id, entry);
        Ok(())
    }

    pub fn unregister_demux(&mut self, id: DemuxRuntimeId) -> Option<DemuxRegistryEntry> {
        self.demux_frontend_bindings.remove(&id);
        self.demux_runtimes.remove(&id);
        self.demuxes.remove(&id)
    }

    pub fn demux_runtime(&self, id: DemuxRuntimeId) -> Option<&DemuxRuntime> {
        self.demux_runtimes.get(&id)
    }

    pub fn demux_runtime_mut(&mut self, id: DemuxRuntimeId) -> Option<&mut DemuxRuntime> {
        self.demux_runtimes.get_mut(&id)
    }

    pub fn bind_demux_frontend(
        &mut self,
        demux_id: DemuxRuntimeId,
        frontend_id: FrontendRuntimeId,
    ) {
        self.demux_frontend_bindings.insert(demux_id, frontend_id);
    }

    pub fn unbind_frontend_demuxes(
        &mut self,
        frontend_id: FrontendRuntimeId,
    ) -> Vec<DemuxRuntimeId> {
        let demux_ids = self.frontend_bound_demux_ids(frontend_id);
        for demux_id in &demux_ids {
            self.demux_frontend_bindings.remove(demux_id);
        }
        demux_ids
    }

    pub fn frontend_bound_demux_ids(&self, frontend_id: FrontendRuntimeId) -> Vec<DemuxRuntimeId> {
        self.demux_frontend_bindings
            .iter()
            .filter_map(|(demux_id, bound_frontend)| {
                (*bound_frontend == frontend_id).then_some(*demux_id)
            })
            .collect()
    }

    pub fn quarantine_bound_demuxes_for_frontend(
        &mut self,
        frontend_id: FrontendRuntimeId,
    ) -> Vec<DemuxRuntimeId> {
        let demux_ids = self.frontend_bound_demux_ids(frontend_id);
        for demux_id in &demux_ids {
            if let Some(runtime) = self.demux_runtimes.get_mut(demux_id) {
                runtime.quarantine();
            }
        }
        demux_ids
    }

    pub fn demux_ids(&self) -> Vec<DemuxRuntimeId> {
        self.demuxes.keys().copied().collect()
    }

    pub fn demux(&self, id: DemuxRuntimeId) -> Option<&DemuxRegistryEntry> {
        self.demuxes.get(&id)
    }

    pub fn frontend_ids(&self) -> Vec<FrontendRuntimeId> {
        self.frontends.keys().copied().collect()
    }

    pub fn frontend(&self, id: FrontendRuntimeId) -> Option<&FrontendRegistryEntry> {
        self.frontends.get(&id)
    }

    pub fn frontend_runtime(&self, id: FrontendRuntimeId) -> Option<&FrontendRuntime> {
        self.frontend_runtimes.get(&id)
    }

    pub fn frontend_runtime_mut(&mut self, id: FrontendRuntimeId) -> Option<&mut FrontendRuntime> {
        self.frontend_runtimes.get_mut(&id)
    }

    pub fn lnb_ids(&self) -> Vec<LnbRuntimeId> {
        self.lnbs.keys().copied().collect()
    }

    pub fn lnb(&self, id: LnbRuntimeId) -> Option<&LnbRegistryEntry> {
        self.lnbs.get(&id)
    }

    pub fn lnb_for_frontend(&self, frontend_id: FrontendRuntimeId) -> Option<&LnbRegistryEntry> {
        self.lnbs
            .values()
            .find(|entry| entry.owner_frontend_id == frontend_id)
    }

    pub fn lnb_by_name(&self, name: &str) -> Option<&LnbRegistryEntry> {
        self.lnbs
            .values()
            .find(|entry| entry.name.as_deref() == Some(name))
    }

    pub fn lnb_runtime(&self, id: LnbRuntimeId) -> Option<&LnbRuntime> {
        self.lnb_runtimes.get(&id)
    }

    pub fn lnb_runtime_mut(&mut self, id: LnbRuntimeId) -> Option<&mut LnbRuntime> {
        self.lnb_runtimes.get_mut(&id)
    }

    pub fn selected_lnb_for_frontend(
        &self,
        frontend_id: FrontendRuntimeId,
    ) -> Option<LnbRuntimeId> {
        self.frontend_lnb_bindings.get(&frontend_id).copied()
    }

    pub fn selected_frontends_for_lnb(&self, lnb_id: LnbRuntimeId) -> Vec<FrontendRuntimeId> {
        self.frontend_lnb_bindings
            .iter()
            .filter_map(|(frontend_id, selected_lnb)| {
                (*selected_lnb == lnb_id).then_some(*frontend_id)
            })
            .collect()
    }

    pub fn bind_lnb_to_frontend(
        &mut self,
        frontend_id: FrontendRuntimeId,
        lnb_id: LnbRuntimeId,
    ) -> Result<(), RegistryCommitError> {
        if !self.frontends.contains_key(&frontend_id) {
            return Err(RegistryCommitError::MissingFrontendId { id: frontend_id });
        }
        let Some(entry) = self.lnbs.get(&lnb_id) else {
            return Err(RegistryCommitError::MissingLnbId { id: lnb_id });
        };
        if entry.owner_frontend_id != frontend_id {
            return Err(RegistryCommitError::LnbFrontendMismatch {
                frontend_id,
                lnb_id,
            });
        }
        self.frontend_lnb_bindings.insert(frontend_id, lnb_id);
        Ok(())
    }

    pub fn register_lnb(&mut self, entry: LnbRegistryEntry) -> Result<(), RegistryCommitError> {
        if self.lnbs.contains_key(&entry.id) || self.lnb_runtimes.contains_key(&entry.id) {
            return Err(RegistryCommitError::DuplicateLnbId { id: entry.id });
        }
        self.lnb_runtimes
            .insert(entry.id, LnbRuntime::new(entry.id.0));
        self.lnbs.insert(entry.id, entry);
        Ok(())
    }

    pub fn allocate_filter(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<FilterRegistryEntry, RegistryCommitError> {
        let id = FilterRuntimeId(self.next_filter_id);
        let next = self
            .next_filter_id
            .checked_add(1)
            .filter(|value| *value > 0)
            .ok_or(RegistryCommitError::RuntimeIdExhausted {
                kind: RuntimeRegistryKind::Filter,
            })?;
        self.next_filter_id = next;
        let entry = FilterRegistryEntry { id, owner_demux_id };
        self.register_filter(entry.clone())?;
        Ok(entry)
    }

    pub fn register_filter(
        &mut self,
        entry: FilterRegistryEntry,
    ) -> Result<(), RegistryCommitError> {
        if self.filters.contains_key(&entry.id) {
            return Err(RegistryCommitError::DuplicateFilterId { id: entry.id });
        }
        self.filters.insert(entry.id, entry);
        Ok(())
    }

    pub fn filter(&self, id: FilterRuntimeId) -> Option<&FilterRegistryEntry> {
        self.filters.get(&id)
    }

    pub fn filters_for_demux(&self, owner_demux_id: i32) -> Vec<FilterRegistryEntry> {
        self.filters
            .values()
            .filter(|entry| entry.owner_demux_id == owner_demux_id)
            .cloned()
            .collect()
    }

    pub fn unregister_filter(&mut self, id: FilterRuntimeId) -> Option<FilterRegistryEntry> {
        self.filters.remove(&id)
    }

    pub fn allocate_dvr(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<DvrRegistryEntry, RegistryCommitError> {
        let id = DvrRuntimeId(self.next_dvr_id);
        let next = self
            .next_dvr_id
            .checked_add(1)
            .filter(|value| *value > 0)
            .ok_or(RegistryCommitError::RuntimeIdExhausted {
                kind: RuntimeRegistryKind::Dvr,
            })?;
        self.next_dvr_id = next;
        let entry = DvrRegistryEntry { id, owner_demux_id };
        self.register_dvr(entry.clone())?;
        Ok(entry)
    }

    pub fn register_dvr(&mut self, entry: DvrRegistryEntry) -> Result<(), RegistryCommitError> {
        if self.dvrs.contains_key(&entry.id) {
            return Err(RegistryCommitError::DuplicateDvrId { id: entry.id });
        }
        self.dvrs.insert(entry.id, entry);
        Ok(())
    }

    pub fn dvr(&self, id: DvrRuntimeId) -> Option<&DvrRegistryEntry> {
        self.dvrs.get(&id)
    }

    pub fn unregister_dvr(&mut self, id: DvrRuntimeId) -> Option<DvrRegistryEntry> {
        self.dvrs.remove(&id)
    }

    pub(crate) fn allocate_descrambler(
        &mut self,
    ) -> Result<DescramblerRegistryEntry, RegistryCommitError> {
        let id = DescramblerRuntimeId(self.next_descrambler_id);
        let next = self
            .next_descrambler_id
            .checked_add(1)
            .filter(|value| *value > 0)
            .ok_or(RegistryCommitError::RuntimeIdExhausted {
                kind: RuntimeRegistryKind::Descrambler,
            })?;
        self.next_descrambler_id = next;
        let entry = DescramblerRegistryEntry { id };
        self.register_descrambler(entry.clone())?;
        Ok(entry)
    }

    pub(crate) fn register_descrambler(
        &mut self,
        entry: DescramblerRegistryEntry,
    ) -> Result<(), RegistryCommitError> {
        if self.descramblers.contains_key(&entry.id) {
            return Err(RegistryCommitError::DuplicateDescramblerId { id: entry.id });
        }
        self.descrambler_runtimes
            .insert(entry.id, DescramblerRuntime::new());
        self.descramblers.insert(entry.id, entry);
        Ok(())
    }

    pub(crate) fn unregister_descrambler(
        &mut self,
        id: DescramblerRuntimeId,
    ) -> Option<DescramblerRegistryEntry> {
        self.descrambler_runtimes.remove(&id);
        self.descramblers.remove(&id)
    }

    pub(crate) fn descrambler(
        &self,
        id: DescramblerRuntimeId,
    ) -> Option<&DescramblerRegistryEntry> {
        self.descramblers.get(&id)
    }

    #[cfg(test)]
    pub(crate) fn descrambler_runtime(
        &self,
        id: DescramblerRuntimeId,
    ) -> Option<&DescramblerRuntime> {
        self.descrambler_runtimes.get(&id)
    }

    pub(crate) fn descrambler_runtime_exists(&self, id: DescramblerRuntimeId) -> bool {
        self.descrambler_runtimes.contains_key(&id)
    }

    pub(crate) fn descrambler_bound_demux(&self, id: DescramblerRuntimeId) -> Option<(i32, u64)> {
        self.descrambler_runtimes.get(&id)?.demux_binding()
    }

    pub(crate) fn bind_descrambler_demux_use_case(
        &mut self,
        id: DescramblerRuntimeId,
        demux_id: i32,
        generation: u64,
    ) -> Result<(), DescramblerSessionFailure> {
        self.descrambler_runtimes
            .get_mut(&id)
            .ok_or(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            })?
            .bind_demux_use_case(demux_id, generation)
    }

    pub(crate) fn add_descrambler_pid_claim_use_case(
        &mut self,
        id: DescramblerRuntimeId,
        claim: DescramblerPidClaim,
    ) -> Result<(), DescramblerSessionFailure> {
        self.descrambler_runtimes
            .get_mut(&id)
            .ok_or(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            })?
            .add_pid_claim_use_case(claim)
    }

    pub(crate) fn remove_descrambler_pid_claim_use_case(
        &mut self,
        id: DescramblerRuntimeId,
        claim: DescramblerPidClaim,
    ) -> Result<(), DescramblerSessionFailure> {
        self.descrambler_runtimes
            .get_mut(&id)
            .ok_or(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            })?
            .remove_pid_claim_use_case(claim)
    }

    pub(crate) fn descrambler_pid_claimed_by_other(
        &self,
        current_id: DescramblerRuntimeId,
        demux_id: i32,
        demux_generation: u64,
        pid: DescramblerPid,
    ) -> bool {
        self.descrambler_runtimes
            .iter()
            .filter(|(id, _)| **id != current_id)
            .any(|(_, runtime)| {
                runtime.is_bound_to_demux(demux_id, demux_generation) && runtime.has_pid_claim(pid)
            })
    }

    pub(crate) fn descrambler_ids_bound_to_demux(
        &self,
        demux_id: i32,
    ) -> Vec<DescramblerRuntimeId> {
        self.descrambler_runtimes
            .iter()
            .filter_map(|(id, runtime)| runtime.is_bound_to_demux_id(demux_id).then_some(*id))
            .collect()
    }

    pub(crate) fn descrambler_token_resolution_available(&self) -> bool {
        self.descrambler_key_table.has_token_resolution_state()
    }

    pub(crate) fn descrambler_has_stale_source_generation(
        &self,
        descrambler_id: DescramblerRuntimeId,
        pid: DescramblerPid,
        source_filter_id: i32,
        source_generation: u64,
    ) -> bool {
        self.descrambler_runtimes
            .get(&descrambler_id)
            .is_some_and(|runtime| {
                runtime.has_stale_source_generation(pid, source_filter_id, source_generation)
            })
    }

    pub(crate) fn validate_descrambler_source_filter(
        &self,
        expected_demux_id: i32,
        expected_demux_generation: u64,
        source_filter_id: i32,
        pid: DescramblerPid,
    ) -> Result<u64, HalError> {
        let filter_entry = self
            .filter(FilterRuntimeId(source_filter_id))
            .ok_or_else(|| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "source filter registry entry is missing",
                )
            })?;
        if filter_entry.owner_demux_id != expected_demux_id {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "source filter belongs to another demux",
            ));
        }
        let Some(demux_runtime) = self.demux_runtime(DemuxRuntimeId(filter_entry.owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        if demux_runtime.generation() != expected_demux_generation {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler demux generation is stale",
            ));
        }
        let source_snapshot = demux_runtime
            .filter_snapshot(source_filter_id)
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "source filter runtime is not available",
                )
            })?;
        if source_snapshot.state == FilterRuntimeState::Open
            || source_snapshot.state.is_closed_or_failed()
            || source_snapshot.tpid.is_none()
        {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "source filter is not configured",
            ));
        }
        if !PacketPid::from_descrambler_pid_for_service_runtime_boundary(pid)
            .matches_config_tpid_for_service_runtime_boundary(source_snapshot.tpid)
        {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "source filter PID does not match descrambler PID",
            ));
        }
        if !matches!(
            source_snapshot.open_type,
            FilterOpenType::TsAudio
                | FilterOpenType::TsVideo
                | FilterOpenType::TsPes
                | FilterOpenType::TsRecord
        ) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "source filter subtype is not valid for descrambler PID source",
            ));
        }
        Ok(source_snapshot.generation)
    }

    pub(crate) fn resolved_descrambler_claims_for_demux(
        &self,
        demux_id: i32,
        demux_generation: u64,
    ) -> Vec<ResolvedDescramblerClaimSet> {
        self.descrambler_runtimes
            .values()
            .filter_map(|runtime| {
                let claim_set = runtime.resolved_claim_set_for_demux(
                    demux_id,
                    demux_generation,
                    &self.descrambler_key_table,
                )?;
                let (claims, key_slot) = claim_set.into_parts();
                Some(ResolvedDescramblerClaimSet { claims, key_slot })
            })
            .collect()
    }

    pub(crate) fn resolved_descrambler_packet_material_for_demux(
        &self,
        demux_id: i32,
        demux_generation: u64,
        packet_pid: PacketPid,
    ) -> ResolvedDescramblerPacketMaterial {
        let claim_sets = self.resolved_descrambler_claims_for_demux(demux_id, demux_generation);
        let mut snapshots = Vec::with_capacity(claim_sets.len());
        let mut diagnostics = Vec::new();
        let mut diagnostic_records = Vec::new();
        for claim_set in claim_sets {
            let (claims, key_slot) = claim_set.into_parts();
            let mut descrambler_pids = BTreeSet::new();
            let mut packet_pids = BTreeSet::new();
            let mut source_filter_ids_by_pid: BTreeMap<PacketPid, BTreeSet<i32>> = BTreeMap::new();
            for claim in claims {
                let descrambler_pid = claim.pid();
                let pid =
                    PacketPid::from_descrambler_pid_for_service_runtime_boundary(descrambler_pid);
                if pid != packet_pid {
                    continue;
                }
                let Some(source) = claim.source_filter_ref() else {
                    descrambler_pids.insert(descrambler_pid);
                    packet_pids.insert(pid);
                    continue;
                };
                match self.validate_descrambler_source_filter(
                    demux_id,
                    demux_generation,
                    source.filter_id(),
                    descrambler_pid,
                ) {
                    Ok(generation) if generation == source.generation() => {
                        descrambler_pids.insert(descrambler_pid);
                        packet_pids.insert(pid);
                        source_filter_ids_by_pid
                            .entry(pid)
                            .or_default()
                            .insert(source.filter_id());
                    }
                    Ok(_) => {
                        let error = HalError::invalid_state(
                            HalInvalidStateKind::InvalidLifecycle,
                            "source filter generation changed before packet descramble",
                        );
                        diagnostics.push(PipelineDiagnostic::source_filter_validation_failure(
                            packet_pid,
                            source.filter_id(),
                            error.clone(),
                        ));
                        diagnostic_records.push(
                            DescramblerDiagnosticRecord::packet_source_filter_validation(
                                demux_id,
                                pid,
                                source.filter_id(),
                                DescramblerDiagnosticKind::PacketSourceFilterGenerationMismatch,
                                error,
                            ),
                        );
                    }
                    Err(error) => {
                        diagnostics.push(PipelineDiagnostic::source_filter_validation_failure(
                            packet_pid,
                            source.filter_id(),
                            error.clone(),
                        ));
                        diagnostic_records.push(
                            DescramblerDiagnosticRecord::packet_source_filter_validation(
                                demux_id,
                                pid,
                                source.filter_id(),
                                DescramblerDiagnosticKind::PacketSourceFilterInvalid,
                                error,
                            ),
                        );
                    }
                }
            }
            if packet_pids.is_empty() {
                continue;
            }
            snapshots.push(ResolvedDescramblerPacketSnapshot {
                descrambler_pids,
                packet_pids,
                key_slot,
                source_filter_ids_by_pid,
            });
        }
        ResolvedDescramblerPacketMaterial {
            snapshots,
            diagnostics,
            diagnostic_records,
        }
    }

    pub(crate) fn descrambler_keyless_claim_exists_for_demux_packet_pid(
        &self,
        demux_id: i32,
        demux_generation: u64,
        packet_pid: PacketPid,
    ) -> bool {
        self.descrambler_runtimes.values().any(|runtime| {
            runtime.has_keyless_claim_for_demux_packet_pid(demux_id, demux_generation, packet_pid)
        })
    }

    pub(crate) fn clear_descrambler_key_use_case(
        &mut self,
        descrambler_id: DescramblerRuntimeId,
    ) -> Result<(), DescramblerClearKeyTxnError<DescramblerKeyLookupError>> {
        let runtime = self.descrambler_runtimes.get_mut(&descrambler_id).ok_or(
            DescramblerClearKeyTxnError::Session(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            }),
        )?;
        runtime.clear_key_use_case(&mut self.descrambler_key_table)
    }

    pub(crate) fn replace_descrambler_key_use_case(
        &mut self,
        descrambler_id: DescramblerRuntimeId,
        token: DescramblerKeyToken,
    ) -> Result<
        DescramblerReplaceKeyOutcome,
        DescramblerReplaceKeyTxnError<DescramblerKeyLookupError, DescramblerKeyLookupError>,
    > {
        let runtime = self.descrambler_runtimes.get_mut(&descrambler_id).ok_or(
            DescramblerReplaceKeyTxnError::Session(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            }),
        )?;
        runtime.replace_key_use_case(&mut self.descrambler_key_table, token)
    }

    pub(crate) fn cleanup_descrambler_use_case(
        &mut self,
        descrambler_id: DescramblerRuntimeId,
    ) -> Result<DescramblerCleanupReport, DescramblerCleanupTxnError<DescramblerKeyLookupError>>
    {
        let runtime = self.descrambler_runtimes.get_mut(&descrambler_id).ok_or(
            DescramblerCleanupTxnError::Session(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            }),
        )?;
        runtime.cleanup_all_use_case(&mut self.descrambler_key_table)
    }

    #[cfg(test)]
    pub(crate) fn descrambler_key_table_mut(&mut self) -> &mut DescramblerKeyTable {
        &mut self.descrambler_key_table
    }
}
