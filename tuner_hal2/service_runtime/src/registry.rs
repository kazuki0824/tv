use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::descrambler_key_table::{DescramblerKeyLookupError, DescramblerKeyTable};
use crate::descrambler_session::{
    DescramblerCleanupReport, DescramblerCleanupTxnError, DescramblerClearKeyOutcome,
    DescramblerClearKeyTxnError, DescramblerReplaceKeyOutcome, DescramblerReplaceKeyTxnError,
    DescramblerRuntime, DescramblerSessionFailure, DescramblerSessionFailureKind,
    DescramblerSessionTxnStep, DescramblerSourceCallFailure,
};
use crate::diagnostics::{DescramblerDiagnosticKind, DescramblerDiagnosticRecord};
use maleicacid_tuner_hal2_common::TS_PACKET_SIZE;
use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, FrontendSystem, HalError, HalInternalKind, HalInvalidArgumentKind,
    HalInvalidStateKind,
};
use maleicacid_tuner_hal2_demux::{
    AvDataIdAllocator, AvRuntimeBudget, DemuxRuntime, DvrKind, FilterOpenType, FilterRuntimeState,
    DEFAULT_AV_MAX_EVENT_BYTES, DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER,
    DEFAULT_AV_PER_FILTER_LIVE_BYTES,
};
use maleicacid_tuner_hal2_demux::{
    PacketDescramblePolicyFailure, PacketPid, PipelineDiagnostic, PipelineReport,
};
use maleicacid_tuner_hal2_descrambler::{
    descramble_validated_ts_packet_in_place, packet_policy_for_descramble_failure,
    DescrambleFailure, DescrambleOutcome, DescramblerKeySlot, DescramblerKeyToken, DescramblerPid,
    DescramblerPidClaim, PacketPolicyAction,
};
use maleicacid_tuner_hal2_device::FrontendRuntime;
use maleicacid_tuner_hal2_lnb::{
    finish_lnb_close, finish_lnb_state_apply, prepare_lnb_close, prepare_lnb_state_apply,
    LnbBackendApplyOutcome, LnbElectricalState, LnbFailureKind, LnbFailureRecord, LnbFailureStep,
    LnbRuntime, PreparedLnbClose, PreparedLnbStateApply,
};

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
    /// 起動時probeで検証し、公開後は変更しないfrontend capability。
    pub capability: FrontendCapabilitySnapshot,
    /// frontend exportと同じprobe sourceから導出した固定LNB profile。
    /// Noneの場合、frontendはLNB voltage statusやLNB bindingをadvertiseしてはならない。
    pub lnb_profile: Option<LnbRegistryProfile>,
    /// Product-wiring evidence for satellite power. This is independent of
    /// whether a caller-controllable ILnb endpoint is exported.
    pub satellite_power_topology: SatellitePowerTopology,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendScalarCapability {
    pub min_frequency_hz: i64,
    pub max_frequency_hz: i64,
    pub min_symbol_rate: i32,
    pub max_symbol_rate: i32,
    pub acquire_range_hz: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IsdbtSegmentCapability {
    pub is_segment_auto: bool,
    pub is_full_segment: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendCapabilitySnapshot {
    pub scalar: FrontendScalarCapability,
    pub exclusive_group_id: i32,
    pub isdbt_segment: Option<IsdbtSegmentCapability>,
}

impl FrontendRegistryEntry {
    pub fn hardware_info(&self) -> String {
        let backend = match self.backend {
            FrontendBackendKind::Px4CharDevice => "px4",
            FrontendBackendKind::LinuxDvb => "linux-dvb",
        };
        format!(
            "maleicacid/{backend}/{}/{}",
            self.system.as_hint(),
            self.device_path.display()
        )
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SatellitePowerTopology {
    InternalFixed15V,
    ExternalOrShared,
    UnknownOrDisabled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnbRegistryEntry {
    pub id: LnbRuntimeId,
    pub name: Option<String>,
    pub owner_frontend_id: FrontendRuntimeId,
    pub profile: LnbRegistryProfile,
}

#[derive(Clone, Debug)]
pub(crate) struct LnbPhysicalIoAuthority {
    gate: Arc<Mutex<()>>,
}

pub(crate) struct LnbPhysicalIoPermit<'a> {
    _guard: MutexGuard<'a, ()>,
}

impl LnbPhysicalIoAuthority {
    fn new() -> Self {
        Self {
            gate: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn execute<T>(
        &self,
        execute: impl FnOnce(LnbPhysicalIoPermit<'_>) -> Result<T, HalError>,
    ) -> Result<T, HalError> {
        let guard = self.gate.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "physical LNB I/O authority lock poisoned",
            )
        })?;
        execute(LnbPhysicalIoPermit { _guard: guard })
    }
}

/// 永続LNB stateと物理LNBごとのI/O直列化権限を所有する正規owner。
#[derive(Debug, Default)]
pub struct LnbRegistry {
    entries: BTreeMap<LnbRuntimeId, LnbRegistryEntry>,
    runtimes: BTreeMap<LnbRuntimeId, LnbRuntime>,
    physical_keys: BTreeMap<LnbRuntimeId, String>,
    physical_io: BTreeMap<String, LnbPhysicalIoAuthority>,
    assignment_leases: BTreeMap<FrontendRuntimeId, LnbAssignmentLease>,
    prepared_assignment_leases: BTreeMap<u64, (FrontendRuntimeId, LnbRuntimeId)>,
    pending_assignment_cleanup: BTreeMap<u64, (FrontendRuntimeId, LnbRuntimeId)>,
    fixed_power_leases: BTreeMap<FrontendRuntimeId, LnbRuntimeId>,
    rail_reference_counts: BTreeMap<String, usize>,
    next_assignment_lease_token: u64,
}

impl LnbRegistry {
    fn clear(&mut self) {
        self.entries.clear();
        self.runtimes.clear();
        self.physical_keys.clear();
        self.physical_io.clear();
        self.clear_assignment_state();
        self.rail_reference_counts.clear();
    }

    fn clear_assignment_state(&mut self) {
        self.assignment_leases.clear();
        self.prepared_assignment_leases.clear();
        self.pending_assignment_cleanup.clear();
        self.fixed_power_leases.clear();
        self.next_assignment_lease_token = 0;
        for count in self.rail_reference_counts.values_mut() {
            *count = 0;
        }
    }

    pub(crate) fn physical_io_authority(&self, id: LnbRuntimeId) -> Option<LnbPhysicalIoAuthority> {
        let key = self.physical_keys.get(&id)?;
        self.physical_io.get(key).cloned()
    }

    pub fn rail_reference_count(&self, id: LnbRuntimeId) -> Option<usize> {
        let key = self.physical_keys.get(&id)?;
        self.rail_reference_counts.get(key).copied()
    }

    fn missing_runtime_failure(id: LnbRuntimeId) -> LnbFailureRecord {
        LnbFailureRecord {
            lnb_id: id.0,
            kind: LnbFailureKind::InvalidState,
            step: LnbFailureStep::ValidateState,
        }
    }

    fn prepare_state_apply(
        &mut self,
        id: LnbRuntimeId,
        target: LnbElectricalState,
    ) -> Result<PreparedLnbStateApply, LnbFailureRecord> {
        if target.voltage != maleicacid_tuner_hal2_lnb::LnbVoltage::Voltage15V
            && self
                .fixed_power_leases
                .values()
                .any(|fixed_power_lnb_id| *fixed_power_lnb_id == id)
        {
            return Err(LnbFailureRecord {
                lnb_id: id.0,
                kind: LnbFailureKind::InvalidState,
                step: LnbFailureStep::ValidateState,
            });
        }
        let runtime = self
            .runtimes
            .get_mut(&id)
            .ok_or_else(|| Self::missing_runtime_failure(id))?;
        prepare_lnb_state_apply(runtime, target)
    }

    fn finish_state_apply(
        &mut self,
        id: LnbRuntimeId,
        prepared: PreparedLnbStateApply,
        outcome: LnbBackendApplyOutcome,
    ) -> Result<LnbElectricalState, LnbFailureRecord> {
        let runtime = self
            .runtimes
            .get_mut(&id)
            .ok_or_else(|| Self::missing_runtime_failure(id))?;
        finish_lnb_state_apply(runtime, prepared, outcome)
    }

    fn prepare_close(&mut self, id: LnbRuntimeId) -> Result<PreparedLnbClose, LnbFailureRecord> {
        if self
            .fixed_power_leases
            .values()
            .any(|fixed_power_lnb_id| *fixed_power_lnb_id == id)
        {
            return Err(LnbFailureRecord {
                lnb_id: id.0,
                kind: LnbFailureKind::InvalidState,
                step: LnbFailureStep::ValidateState,
            });
        }
        let runtime = self
            .runtimes
            .get_mut(&id)
            .ok_or_else(|| Self::missing_runtime_failure(id))?;
        prepare_lnb_close(runtime)
    }

    fn finish_close(
        &mut self,
        id: LnbRuntimeId,
        prepared: PreparedLnbClose,
        outcome: LnbBackendApplyOutcome,
    ) -> Result<(), LnbFailureRecord> {
        let runtime = self
            .runtimes
            .get_mut(&id)
            .ok_or_else(|| Self::missing_runtime_failure(id))?;
        finish_lnb_close(runtime, prepared, outcome)
    }

    fn reopen(&mut self, id: LnbRuntimeId) -> Result<(), LnbFailureRecord> {
        let runtime = self
            .runtimes
            .get_mut(&id)
            .ok_or_else(|| Self::missing_runtime_failure(id))?;
        runtime.reopen_after_public_open()
    }

    fn set_callback_registered(
        &mut self,
        id: LnbRuntimeId,
        registered: bool,
    ) -> Result<(), LnbFailureRecord> {
        let runtime = self
            .runtimes
            .get_mut(&id)
            .ok_or_else(|| Self::missing_runtime_failure(id))?;
        if runtime.state() != maleicacid_tuner_hal2_lnb::LnbRuntimeState::Open {
            return Err(LnbFailureRecord {
                lnb_id: id.0,
                kind: LnbFailureKind::InvalidState,
                step: LnbFailureStep::ValidateState,
            });
        }
        runtime.set_callback_registered(registered);
        Ok(())
    }

    fn record_drop_leak(&mut self, id: LnbRuntimeId) -> Result<(), LnbFailureRecord> {
        let runtime = self
            .runtimes
            .get_mut(&id)
            .ok_or_else(|| Self::missing_runtime_failure(id))?;
        runtime.record_unclosed_drop();
        Ok(())
    }

    fn finish_diseqc(
        &mut self,
        id: LnbRuntimeId,
        expected_generation: u64,
        outcome: LnbBackendApplyOutcome,
    ) -> Result<(), LnbFailureRecord> {
        let runtime = self
            .runtimes
            .get_mut(&id)
            .ok_or_else(|| Self::missing_runtime_failure(id))?;
        if runtime.generation() != expected_generation
            || runtime.state() != maleicacid_tuner_hal2_lnb::LnbRuntimeState::Open
        {
            return Err(LnbFailureRecord {
                lnb_id: id.0,
                kind: LnbFailureKind::InvalidState,
                step: LnbFailureStep::ValidateState,
            });
        }
        match outcome {
            LnbBackendApplyOutcome::Applied => Ok(()),
            LnbBackendApplyOutcome::Rejected(kind) => Err(LnbFailureRecord {
                lnb_id: id.0,
                kind,
                step: LnbFailureStep::SendDiseqc,
            }),
            LnbBackendApplyOutcome::Indeterminate(kind) => {
                Err(runtime.quarantine_indeterminate_backend(kind, LnbFailureStep::SendDiseqc))
            }
        }
    }

    fn physical_key_for_entry(entry: &LnbRegistryEntry) -> String {
        entry
            .name
            .clone()
            .unwrap_or_else(|| format!("lnb-endpoint-{}", entry.id.0))
    }

    fn retain_rail_reference(&mut self, id: LnbRuntimeId) -> Result<(), HalError> {
        let key = self.physical_keys.get(&id).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "LNB physical rail key is missing while retaining a lease",
            )
        })?;
        let count = self.rail_reference_counts.get_mut(key).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "LNB physical rail reference count is missing",
            )
        })?;
        *count = count.checked_add(1).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "LNB physical rail reference count overflow",
            )
        })?;
        Ok(())
    }

    fn release_rail_reference(&mut self, id: LnbRuntimeId) -> Result<(), HalError> {
        let key = self.physical_keys.get(&id).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "LNB physical rail key is missing while releasing a lease",
            )
        })?;
        let count = self.rail_reference_counts.get_mut(key).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "LNB physical rail reference count is missing",
            )
        })?;
        *count = count.checked_sub(1).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "LNB physical rail reference count underflow",
            )
        })?;
        Ok(())
    }

    fn retain_fixed_power_lease(
        &mut self,
        frontend_id: FrontendRuntimeId,
        lnb_id: LnbRuntimeId,
    ) -> Result<bool, HalError> {
        if let Some(current) = self.fixed_power_leases.get(&frontend_id).copied() {
            if current == lnb_id {
                return Ok(false);
            }
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend fixed-power lease points at another LNB rail",
            ));
        }
        self.retain_rail_reference(lnb_id)?;
        self.fixed_power_leases.insert(frontend_id, lnb_id);
        Ok(true)
    }

    fn release_fixed_power_lease(
        &mut self,
        frontend_id: FrontendRuntimeId,
    ) -> Result<Option<(LnbRuntimeId, usize)>, HalError> {
        let Some(lnb_id) = self.fixed_power_leases.get(&frontend_id).copied() else {
            return Ok(None);
        };
        let key = self.physical_keys.get(&lnb_id).cloned().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "fixed-power lease lost its physical LNB rail key",
            )
        })?;
        let count = self.rail_reference_counts.get_mut(&key).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "fixed-power lease lost its rail reference count",
            )
        })?;
        let remaining = count.checked_sub(1).ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "fixed-power rail reference count underflow",
            )
        })?;
        self.fixed_power_leases.remove(&frontend_id);
        *count = remaining;
        Ok(Some((lnb_id, remaining)))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LnbAssignmentLease {
    lnb_id: LnbRuntimeId,
    token: u64,
}

#[derive(Debug, Eq, PartialEq)]
#[must_use = "準備済みLNB割当leaseはcommitまたはabortで消費する必要があります"]
pub(crate) struct PreparedLnbAssignmentLease {
    frontend_id: FrontendRuntimeId,
    lnb_id: LnbRuntimeId,
    token: u64,
    expected_relation: Option<LnbRuntimeId>,
    expected_lease: Option<LnbAssignmentLease>,
}

#[derive(Debug)]
pub(crate) struct PreparedLnbAssignmentCommitError {
    error: HalError,
    prepared: PreparedLnbAssignmentLease,
}

impl PreparedLnbAssignmentCommitError {
    fn new(error: HalError, prepared: PreparedLnbAssignmentLease) -> Self {
        Self { error, prepared }
    }

    pub(crate) fn into_parts(self) -> (HalError, PreparedLnbAssignmentLease) {
        (self.error, self.prepared)
    }
}

#[derive(Debug, Eq, PartialEq)]
#[must_use = "LNB割当cleanup権限は値として完了する必要があります"]
pub(crate) struct LnbAssignmentCleanupRecord {
    frontend_id: FrontendRuntimeId,
    lnb_id: LnbRuntimeId,
    token: u64,
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

fn packet_descramble_policy_failure_for_registry_boundary(
    failure: DescrambleFailure,
) -> PacketDescramblePolicyFailure {
    match failure {
        DescrambleFailure::NoKey => PacketDescramblePolicyFailure::NoKey,
        DescrambleFailure::ScrambledPidNotRegistered => {
            PacketDescramblePolicyFailure::ScrambledPidNotRegistered
        }
        DescrambleFailure::TransportErrorRecord => {
            PacketDescramblePolicyFailure::TransportErrorRecord
        }
        DescrambleFailure::InvalidTsc => PacketDescramblePolicyFailure::InvalidTsc,
        DescrambleFailure::ScrambledNullPid => PacketDescramblePolicyFailure::ScrambledNullPid,
        DescrambleFailure::ScrambledWithoutPayload => {
            PacketDescramblePolicyFailure::ScrambledWithoutPayload
        }
        DescrambleFailure::BadToken => PacketDescramblePolicyFailure::BadToken,
        DescrambleFailure::Multi2Fail => PacketDescramblePolicyFailure::Multi2Fail,
        DescrambleFailure::InvalidPacketSize
        | DescrambleFailure::BadSyncByte
        | DescrambleFailure::InvalidAfc
        | DescrambleFailure::InvalidAdaptationField => PacketDescramblePolicyFailure::InvalidPacket,
    }
}

fn descrambler_diagnostic_kind_for_registry_boundary(
    failure: DescrambleFailure,
) -> DescramblerDiagnosticKind {
    match failure {
        DescrambleFailure::InvalidPacketSize => DescramblerDiagnosticKind::InvalidPacketSize,
        DescrambleFailure::BadSyncByte => DescramblerDiagnosticKind::BadSyncByte,
        DescrambleFailure::InvalidAfc => DescramblerDiagnosticKind::InvalidAfc,
        DescrambleFailure::InvalidAdaptationField => {
            DescramblerDiagnosticKind::InvalidAdaptationField
        }
        DescrambleFailure::InvalidTsc => DescramblerDiagnosticKind::InvalidTsc,
        DescrambleFailure::TransportErrorRecord => DescramblerDiagnosticKind::TransportErrorRecord,
        DescrambleFailure::ScrambledNullPid => DescramblerDiagnosticKind::ScrambledNullPid,
        DescrambleFailure::ScrambledWithoutPayload => {
            DescramblerDiagnosticKind::ScrambledWithoutPayload
        }
        DescrambleFailure::NoKey => DescramblerDiagnosticKind::PacketScrambledWithoutKey,
        DescrambleFailure::BadToken => DescramblerDiagnosticKind::BadToken,
        DescrambleFailure::Multi2Fail => DescramblerDiagnosticKind::Multi2Fail,
        DescrambleFailure::ScrambledPidNotRegistered => {
            DescramblerDiagnosticKind::ScrambledWithoutDescrambler
        }
    }
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
            descramble_validated_ts_packet_in_place(
                &mut candidate,
                &self.descrambler_pids,
                key_slot,
            )
            .map(|outcome| (candidate, outcome)),
        )
    }

    pub(crate) fn source_filter_descramble_policy_diagnostics(
        &self,
        pid: PacketPid,
        failure: DescrambleFailure,
    ) -> Vec<PipelineDiagnostic> {
        self.source_filter_ids_by_pid
            .get(&pid)
            .into_iter()
            .flat_map(|ids| ids.iter().copied())
            .map(|filter_id| {
                PipelineDiagnostic::source_filter_descramble_policy_failure(
                    pid,
                    filter_id,
                    packet_descramble_policy_failure_for_registry_boundary(failure),
                )
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResolvedDescramblerPacketFlow {
    Clear,
    Descrambled,
    RecordPassThroughAndDropAssembly,
    Drop,
    DiagnoseOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedDescramblerPacketDecision {
    pub(crate) packet: [u8; TS_PACKET_SIZE],
    pub(crate) flow: ResolvedDescramblerPacketFlow,
    pub(crate) diagnostics: Vec<PipelineDiagnostic>,
    pub(crate) diagnostic_records: Vec<DescramblerDiagnosticRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedDescramblerPacketFailureDecision {
    pub(crate) flow: ResolvedDescramblerPacketFlow,
    pub(crate) diagnostic_records: Vec<DescramblerDiagnosticRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedDescramblerPacketMaterial {
    snapshots: Vec<ResolvedDescramblerPacketSnapshot>,
    diagnostics: Vec<PipelineDiagnostic>,
    diagnostic_records: Vec<DescramblerDiagnosticRecord>,
}

impl ResolvedDescramblerPacketMaterial {
    fn flow_for_descramble_failure(failure: DescrambleFailure) -> ResolvedDescramblerPacketFlow {
        match packet_policy_for_descramble_failure(failure) {
            PacketPolicyAction::RecordPassThroughAndDropAssembly => {
                ResolvedDescramblerPacketFlow::RecordPassThroughAndDropAssembly
            }
            PacketPolicyAction::DropAndDiagnose => ResolvedDescramblerPacketFlow::Drop,
            PacketPolicyAction::DiagnoseOnly => ResolvedDescramblerPacketFlow::DiagnoseOnly,
        }
    }

    fn failure_records(
        demux_id: i32,
        packet_pid: PacketPid,
        failure: DescrambleFailure,
    ) -> Vec<DescramblerDiagnosticRecord> {
        let mut records = Vec::new();
        if matches!(
            packet_policy_for_descramble_failure(failure),
            PacketPolicyAction::RecordPassThroughAndDropAssembly
        ) {
            records.push(DescramblerDiagnosticRecord::packet_policy(
                demux_id,
                packet_pid,
                DescramblerDiagnosticKind::PacketAssemblySuppressed,
            ));
        }
        records.push(DescramblerDiagnosticRecord::packet_policy(
            demux_id,
            packet_pid,
            descrambler_diagnostic_kind_for_registry_boundary(failure),
        ));
        records
    }

    fn failure_diagnostics(
        &self,
        packet_pid: PacketPid,
        failure: DescrambleFailure,
    ) -> Vec<PipelineDiagnostic> {
        self.snapshots
            .iter()
            .flat_map(|snapshot| {
                snapshot.source_filter_descramble_policy_diagnostics(packet_pid, failure)
            })
            .collect()
    }

    pub(crate) fn decide_descrambled_packet(
        self,
        demux_id: i32,
        packet_pid: PacketPid,
        packet: &[u8; TS_PACKET_SIZE],
    ) -> ResolvedDescramblerPacketDecision {
        let mut diagnostic_records = self.diagnostic_records.clone();
        let mut saw_target_descrambler = false;
        for snapshot in self
            .snapshots
            .iter()
            .filter(|snapshot| snapshot.targets_packet_pid(packet_pid))
        {
            saw_target_descrambler = true;
            let Some(descramble_result) = snapshot.descramble_packet(packet) else {
                continue;
            };
            match descramble_result {
                Ok((candidate, DescrambleOutcome::Descrambled { .. })) => {
                    diagnostic_records.push(DescramblerDiagnosticRecord::packet_policy(
                        demux_id,
                        packet_pid,
                        DescramblerDiagnosticKind::PacketDescrambled,
                    ));
                    return ResolvedDescramblerPacketDecision {
                        packet: candidate,
                        flow: ResolvedDescramblerPacketFlow::Descrambled,
                        diagnostics: self.diagnostics,
                        diagnostic_records,
                    };
                }
                Ok((_, DescrambleOutcome::PassedThrough { .. })) => {
                    return ResolvedDescramblerPacketDecision {
                        packet: *packet,
                        flow: ResolvedDescramblerPacketFlow::Clear,
                        diagnostics: self.diagnostics,
                        diagnostic_records,
                    };
                }
                Err(failure) => {
                    diagnostic_records.extend(Self::failure_records(demux_id, packet_pid, failure));
                    let failure_diagnostics = self.failure_diagnostics(packet_pid, failure);
                    let mut diagnostics = self.diagnostics;
                    diagnostics.extend(failure_diagnostics);
                    return ResolvedDescramblerPacketDecision {
                        packet: *packet,
                        flow: Self::flow_for_descramble_failure(failure),
                        diagnostics,
                        diagnostic_records,
                    };
                }
            }
        }

        let failure = if saw_target_descrambler {
            DescrambleFailure::NoKey
        } else {
            DescrambleFailure::ScrambledPidNotRegistered
        };
        diagnostic_records.extend(Self::failure_records(demux_id, packet_pid, failure));
        let failure_diagnostics = self.failure_diagnostics(packet_pid, failure);
        let mut diagnostics = self.diagnostics;
        diagnostics.extend(failure_diagnostics);
        ResolvedDescramblerPacketDecision {
            packet: *packet,
            flow: Self::flow_for_descramble_failure(failure),
            diagnostics,
            diagnostic_records,
        }
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
    #[cfg(test)]
    MissingFrontendId {
        id: FrontendRuntimeId,
    },
    #[cfg(test)]
    MissingLnbId {
        id: LnbRuntimeId,
    },
    #[cfg(test)]
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
    #[cfg(test)]
    Demux,
    #[cfg(test)]
    Lnb,
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
    lnb_registry: LnbRegistry,
    frontend_lnb_bindings: BTreeMap<FrontendRuntimeId, LnbRuntimeId>,
    filters: BTreeMap<FilterRuntimeId, FilterRegistryEntry>,
    dvrs: BTreeMap<DvrRuntimeId, DvrRegistryEntry>,
    descramblers: BTreeMap<DescramblerRuntimeId, DescramblerRegistryEntry>,
    descrambler_runtimes: BTreeMap<DescramblerRuntimeId, DescramblerRuntime>,
    descrambler_key_table: DescramblerKeyTable,
    av_data_id_allocator: Arc<AvDataIdAllocator>,
    av_runtime_budget: Arc<AvRuntimeBudget>,
    av_max_event_bytes: usize,
    av_max_outstanding_events_per_filter: usize,
    av_per_filter_live_bytes: usize,
    #[cfg(test)]
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
            lnb_registry: LnbRegistry::default(),
            frontend_lnb_bindings: BTreeMap::new(),
            filters: BTreeMap::new(),
            dvrs: BTreeMap::new(),
            descramblers: BTreeMap::new(),
            descrambler_runtimes: BTreeMap::new(),
            descrambler_key_table: DescramblerKeyTable::default(),
            av_data_id_allocator: Arc::new(AvDataIdAllocator::default()),
            av_runtime_budget: Arc::new(AvRuntimeBudget::unlimited()),
            av_max_event_bytes: DEFAULT_AV_MAX_EVENT_BYTES,
            av_max_outstanding_events_per_filter: DEFAULT_AV_MAX_OUTSTANDING_EVENTS_PER_FILTER,
            av_per_filter_live_bytes: DEFAULT_AV_PER_FILTER_LIVE_BYTES,
            #[cfg(test)]
            next_demux_id: 1,
            next_lnb_id: 1,
            next_filter_id: 1,
            next_dvr_id: 1,
            next_descrambler_id: 1,
        }
    }
}

impl RuntimeRegistry {
    pub(crate) fn with_av_runtime_limits(
        av_max_event_bytes: usize,
        av_max_outstanding_events_per_filter: usize,
        av_per_filter_live_bytes: usize,
        av_runtime_budget_bytes: usize,
    ) -> Self {
        Self {
            av_runtime_budget: Arc::new(AvRuntimeBudget::new(av_runtime_budget_bytes)),
            av_max_event_bytes,
            av_max_outstanding_events_per_filter,
            av_per_filter_live_bytes,
            ..Self::default()
        }
    }

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
        self.lnb_registry.clear_assignment_state();
    }

    pub fn clear_lnbs(&mut self) {
        self.lnb_registry.clear();
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
        #[cfg(test)]
        {
            self.next_demux_id = 1;
        }
        self.next_filter_id = 1;
        self.next_dvr_id = 1;
        self.next_descrambler_id = 1;
    }

    pub fn frontend_count(&self) -> usize {
        self.frontends.len()
    }

    #[cfg(test)]
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
        self.demux_runtimes.insert(
            entry.id,
            DemuxRuntime::new_with_av_runtime_limits(
                entry.id.0,
                1,
                Arc::clone(&self.av_data_id_allocator),
                Arc::clone(&self.av_runtime_budget),
                self.av_max_event_bytes,
                self.av_max_outstanding_events_per_filter,
                self.av_per_filter_live_bytes,
            ),
        );
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

    pub fn frontend_bound_to_demux(&self, demux_id: DemuxRuntimeId) -> Option<FrontendRuntimeId> {
        self.demux_frontend_bindings.get(&demux_id).copied()
    }

    pub fn unbind_demux_frontend(&mut self, demux_id: DemuxRuntimeId) {
        self.demux_frontend_bindings.remove(&demux_id);
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
                runtime.quarantine_runtime_from_typed_request(
                    maleicacid_tuner_hal2_demux::DemuxRuntimeQuarantineRequest::new(),
                );
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
        self.lnb_registry.entries.keys().copied().collect()
    }

    #[cfg(test)]
    pub fn lnb_registry(&self) -> &LnbRegistry {
        &self.lnb_registry
    }

    pub fn lnb(&self, id: LnbRuntimeId) -> Option<&LnbRegistryEntry> {
        self.lnb_registry.entries.get(&id)
    }

    pub fn lnb_for_frontend(&self, frontend_id: FrontendRuntimeId) -> Option<&LnbRegistryEntry> {
        self.lnb_registry
            .entries
            .values()
            .find(|entry| entry.owner_frontend_id == frontend_id)
    }

    #[cfg(test)]
    pub fn lnb_by_name(&self, name: &str) -> Option<&LnbRegistryEntry> {
        self.lnb_registry
            .entries
            .values()
            .find(|entry| entry.name.as_deref() == Some(name))
    }

    pub fn lnb_runtime(&self, id: LnbRuntimeId) -> Option<&LnbRuntime> {
        self.lnb_registry.runtimes.get(&id)
    }

    #[cfg(test)]
    fn lnb_runtime_mut(&mut self, id: LnbRuntimeId) -> Option<&mut LnbRuntime> {
        self.lnb_registry.runtimes.get_mut(&id)
    }

    pub(crate) fn prepare_lnb_state_apply(
        &mut self,
        id: LnbRuntimeId,
        target: LnbElectricalState,
    ) -> Result<PreparedLnbStateApply, LnbFailureRecord> {
        self.lnb_registry.prepare_state_apply(id, target)
    }

    pub(crate) fn finish_lnb_state_apply(
        &mut self,
        id: LnbRuntimeId,
        prepared: PreparedLnbStateApply,
        outcome: LnbBackendApplyOutcome,
    ) -> Result<LnbElectricalState, LnbFailureRecord> {
        self.lnb_registry.finish_state_apply(id, prepared, outcome)
    }

    pub(crate) fn prepare_lnb_close(
        &mut self,
        id: LnbRuntimeId,
    ) -> Result<PreparedLnbClose, LnbFailureRecord> {
        self.lnb_registry.prepare_close(id)
    }

    pub(crate) fn finish_lnb_close(
        &mut self,
        id: LnbRuntimeId,
        prepared: PreparedLnbClose,
        outcome: LnbBackendApplyOutcome,
    ) -> Result<(), LnbFailureRecord> {
        self.lnb_registry.finish_close(id, prepared, outcome)
    }

    pub(crate) fn reopen_lnb(&mut self, id: LnbRuntimeId) -> Result<(), LnbFailureRecord> {
        self.lnb_registry.reopen(id)
    }

    pub(crate) fn set_lnb_callback_registered(
        &mut self,
        id: LnbRuntimeId,
        registered: bool,
    ) -> Result<(), LnbFailureRecord> {
        self.lnb_registry.set_callback_registered(id, registered)
    }

    pub(crate) fn record_lnb_drop_leak(
        &mut self,
        id: LnbRuntimeId,
    ) -> Result<(), LnbFailureRecord> {
        self.lnb_registry.record_drop_leak(id)
    }

    pub(crate) fn finish_lnb_diseqc(
        &mut self,
        id: LnbRuntimeId,
        expected_generation: u64,
        outcome: LnbBackendApplyOutcome,
    ) -> Result<(), LnbFailureRecord> {
        self.lnb_registry
            .finish_diseqc(id, expected_generation, outcome)
    }

    pub(crate) fn lnb_physical_io_authority(
        &self,
        id: LnbRuntimeId,
    ) -> Option<LnbPhysicalIoAuthority> {
        self.lnb_registry.physical_io_authority(id)
    }

    pub(crate) fn retain_frontend_fixed_power_lease(
        &mut self,
        frontend_id: FrontendRuntimeId,
        lnb_id: LnbRuntimeId,
    ) -> Result<bool, HalError> {
        let frontend = self.frontends.get(&frontend_id).ok_or_else(|| {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "frontend is missing while retaining fixed LNB power",
            )
        })?;
        if frontend.satellite_power_topology != SatellitePowerTopology::InternalFixed15V {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend does not own an internal fixed-15V rail",
            ));
        }
        let lnb = self.lnb_registry.entries.get(&lnb_id).ok_or_else(|| {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "LNB is missing while retaining fixed frontend power",
            )
        })?;
        if lnb.owner_frontend_id != frontend_id {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "LNB does not belong to the fixed-power frontend",
            ));
        }
        self.lnb_registry
            .retain_fixed_power_lease(frontend_id, lnb_id)
    }

    pub(crate) fn release_frontend_fixed_power_lease(
        &mut self,
        frontend_id: FrontendRuntimeId,
    ) -> Result<Option<(LnbRuntimeId, usize)>, HalError> {
        self.lnb_registry.release_fixed_power_lease(frontend_id)
    }

    pub(crate) fn frontend_fixed_power_lnb(
        &self,
        frontend_id: FrontendRuntimeId,
    ) -> Option<LnbRuntimeId> {
        self.lnb_registry
            .fixed_power_leases
            .get(&frontend_id)
            .copied()
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

    fn validate_lnb_assignment_target(
        &self,
        frontend_id: FrontendRuntimeId,
        lnb_id: LnbRuntimeId,
    ) -> Result<(), HalError> {
        if !self.frontends.contains_key(&frontend_id) {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "frontend id is missing for LNB assignment",
            ));
        }
        let Some(entry) = self.lnb_registry.entries.get(&lnb_id) else {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "LNB id is missing for frontend assignment",
            ));
        };
        if entry.owner_frontend_id != frontend_id {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "LNB does not belong to the assignment frontend",
            ));
        }
        Ok(())
    }

    pub(crate) fn prepare_lnb_assignment_lease(
        &mut self,
        frontend_id: FrontendRuntimeId,
        lnb_id: LnbRuntimeId,
    ) -> Result<Option<PreparedLnbAssignmentLease>, HalError> {
        self.validate_lnb_assignment_target(frontend_id, lnb_id)?;
        let current_relation = self.frontend_lnb_bindings.get(&frontend_id).copied();
        let current_lease = self
            .lnb_registry
            .assignment_leases
            .get(&frontend_id)
            .copied();
        if current_relation != current_lease.map(|current| current.lnb_id) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend/LNB relation and assignment lease are inconsistent",
            ));
        }
        if current_relation == Some(lnb_id) {
            return Ok(None);
        }
        if self
            .lnb_registry
            .prepared_assignment_leases
            .values()
            .any(|(prepared_frontend, _)| *prepared_frontend == frontend_id)
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend already has a prepared LNB assignment lease",
            ));
        }
        let token = self
            .lnb_registry
            .next_assignment_lease_token
            .checked_add(1)
            .filter(|token| *token != 0)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "LNB assignment lease token exhausted",
                )
            })?;
        self.lnb_registry.next_assignment_lease_token = token;
        self.lnb_registry
            .prepared_assignment_leases
            .insert(token, (frontend_id, lnb_id));
        Ok(Some(PreparedLnbAssignmentLease {
            frontend_id,
            lnb_id,
            token,
            expected_relation: current_relation,
            expected_lease: current_lease,
        }))
    }

    pub(crate) fn abort_prepared_lnb_assignment_lease(
        &mut self,
        prepared: PreparedLnbAssignmentLease,
    ) -> bool {
        if self
            .lnb_registry
            .prepared_assignment_leases
            .get(&prepared.token)
            .is_some_and(|entry| *entry == (prepared.frontend_id, prepared.lnb_id))
        {
            self.lnb_registry
                .prepared_assignment_leases
                .remove(&prepared.token);
            true
        } else {
            false
        }
    }

    pub(crate) fn commit_prepared_lnb_assignment(
        &mut self,
        prepared: PreparedLnbAssignmentLease,
    ) -> Result<Option<LnbAssignmentCleanupRecord>, PreparedLnbAssignmentCommitError> {
        if self
            .lnb_registry
            .prepared_assignment_leases
            .get(&prepared.token)
            != Some(&(prepared.frontend_id, prepared.lnb_id))
            || self
                .frontend_lnb_bindings
                .get(&prepared.frontend_id)
                .copied()
                != prepared.expected_relation
            || self
                .lnb_registry
                .assignment_leases
                .get(&prepared.frontend_id)
                .copied()
                != prepared.expected_lease
            || !self.lnb_registry.runtimes.contains_key(&prepared.lnb_id)
        {
            return Err(PreparedLnbAssignmentCommitError::new(
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "prepared frontend/LNB assignment no longer matches its commit snapshot",
                ),
                prepared,
            ));
        }

        if let Err(error) = self.lnb_registry.retain_rail_reference(prepared.lnb_id) {
            return Err(PreparedLnbAssignmentCommitError::new(error, prepared));
        }
        self.lnb_registry
            .prepared_assignment_leases
            .remove(&prepared.token);
        self.frontend_lnb_bindings
            .insert(prepared.frontend_id, prepared.lnb_id);
        let old_lease = self.lnb_registry.assignment_leases.insert(
            prepared.frontend_id,
            LnbAssignmentLease {
                lnb_id: prepared.lnb_id,
                token: prepared.token,
            },
        );
        Ok(old_lease.map(|lease| {
            self.lnb_registry
                .pending_assignment_cleanup
                .insert(lease.token, (prepared.frontend_id, lease.lnb_id));
            LnbAssignmentCleanupRecord {
                frontend_id: prepared.frontend_id,
                lnb_id: lease.lnb_id,
                token: lease.token,
            }
        }))
    }

    pub(crate) fn complete_lnb_assignment_cleanup(
        &mut self,
        cleanup: LnbAssignmentCleanupRecord,
    ) -> Result<(), HalError> {
        if self
            .lnb_registry
            .pending_assignment_cleanup
            .get(&cleanup.token)
            != Some(&(cleanup.frontend_id, cleanup.lnb_id))
        {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "old frontend/LNB assignment cleanup record is missing",
            ));
        }
        self.lnb_registry.release_rail_reference(cleanup.lnb_id)?;
        self.lnb_registry
            .pending_assignment_cleanup
            .remove(&cleanup.token);
        Ok(())
    }

    pub(crate) fn release_lnb_assignment(
        &mut self,
        frontend_id: FrontendRuntimeId,
    ) -> Result<Option<LnbRuntimeId>, HalError> {
        let relation = self.frontend_lnb_bindings.get(&frontend_id).copied();
        let lease = self
            .lnb_registry
            .assignment_leases
            .get(&frontend_id)
            .copied();
        if relation != lease.map(|lease| lease.lnb_id) {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend/LNB release found inconsistent relation and lease state",
            ));
        }
        if let Some(lease) = lease {
            self.lnb_registry.release_rail_reference(lease.lnb_id)?;
        }
        self.frontend_lnb_bindings.remove(&frontend_id);
        self.lnb_registry.assignment_leases.remove(&frontend_id);
        Ok(relation)
    }

    #[cfg(test)]
    pub(crate) fn bind_lnb_to_frontend(
        &mut self,
        frontend_id: FrontendRuntimeId,
        lnb_id: LnbRuntimeId,
    ) -> Result<(), RegistryCommitError> {
        if !self.frontends.contains_key(&frontend_id) {
            return Err(RegistryCommitError::MissingFrontendId { id: frontend_id });
        }
        let Some(entry) = self.lnb_registry.entries.get(&lnb_id) else {
            return Err(RegistryCommitError::MissingLnbId { id: lnb_id });
        };
        if entry.owner_frontend_id != frontend_id {
            return Err(RegistryCommitError::LnbFrontendMismatch {
                frontend_id,
                lnb_id,
            });
        }
        let token = self
            .lnb_registry
            .next_assignment_lease_token
            .checked_add(1)
            .filter(|token| *token != 0)
            .ok_or(RegistryCommitError::RuntimeIdExhausted {
                kind: RuntimeRegistryKind::Lnb,
            })?;
        self.lnb_registry.next_assignment_lease_token = token;
        self.lnb_registry
            .retain_rail_reference(lnb_id)
            .map_err(|_| RegistryCommitError::RuntimeIdExhausted {
                kind: RuntimeRegistryKind::Lnb,
            })?;
        self.frontend_lnb_bindings.insert(frontend_id, lnb_id);
        self.lnb_registry
            .assignment_leases
            .insert(frontend_id, LnbAssignmentLease { lnb_id, token });
        Ok(())
    }

    #[cfg(test)]
    fn unbind_lnb_from_frontend(&mut self, frontend_id: FrontendRuntimeId) -> Option<LnbRuntimeId> {
        if let Some(lease) = self.lnb_registry.assignment_leases.remove(&frontend_id) {
            let _ = self.lnb_registry.release_rail_reference(lease.lnb_id);
        }
        self.frontend_lnb_bindings.remove(&frontend_id)
    }

    pub fn register_lnb(&mut self, entry: LnbRegistryEntry) -> Result<(), RegistryCommitError> {
        if self.lnb_registry.entries.contains_key(&entry.id)
            || self.lnb_registry.runtimes.contains_key(&entry.id)
        {
            return Err(RegistryCommitError::DuplicateLnbId { id: entry.id });
        }
        let physical_key = LnbRegistry::physical_key_for_entry(&entry);
        if let Some(existing_id) = self
            .lnb_registry
            .physical_keys
            .iter()
            .find_map(|(id, key)| (key == &physical_key).then_some(*id))
        {
            let compatible = self
                .lnb_registry
                .entries
                .get(&existing_id)
                .is_some_and(|existing| existing.profile == entry.profile);
            if !compatible {
                return Err(RegistryCommitError::DuplicateLnbId { id: entry.id });
            }
        }
        self.lnb_registry
            .physical_io
            .entry(physical_key.clone())
            .or_insert_with(LnbPhysicalIoAuthority::new);
        self.lnb_registry
            .rail_reference_counts
            .entry(physical_key.clone())
            .or_insert(0);
        self.lnb_registry
            .physical_keys
            .insert(entry.id, physical_key);
        self.lnb_registry
            .runtimes
            .insert(entry.id, LnbRuntime::new(entry.id.0));
        self.lnb_registry.entries.insert(entry.id, entry);
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

    fn filter_open_type(&self, entry: &FilterRegistryEntry) -> Result<FilterOpenType, HalError> {
        let demux = self
            .demux_runtimes
            .get(&DemuxRuntimeId(entry.owner_demux_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter registry owner demux runtime is missing",
                )
            })?;
        demux
            .filter_snapshot(entry.id.0)
            .map(|snapshot| snapshot.open_type)
            .map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "filter registry entry is missing from its owner demux runtime",
                )
            })
    }

    pub fn filter_open_type_count(&self, open_type: FilterOpenType) -> Result<usize, HalError> {
        let mut count = 0;
        for entry in self.filters.values() {
            if self.filter_open_type(entry)? == open_type {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn demux_has_filter_open_type(
        &self,
        owner_demux_id: i32,
        open_type: FilterOpenType,
    ) -> Result<bool, HalError> {
        for entry in self
            .filters
            .values()
            .filter(|entry| entry.owner_demux_id == owner_demux_id)
        {
            if self.filter_open_type(entry)? == open_type {
                return Ok(true);
            }
        }
        Ok(false)
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

    fn dvr_kind(&self, entry: &DvrRegistryEntry) -> Result<DvrKind, HalError> {
        let demux = self
            .demux_runtimes
            .get(&DemuxRuntimeId(entry.owner_demux_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "DVR registry owner demux runtime is missing",
                )
            })?;
        demux
            .dvr_snapshot(entry.id.0)
            .map(|snapshot| snapshot.kind)
            .map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "DVR registry entry is missing from its owner demux runtime",
                )
            })
    }

    pub fn dvr_kind_count(&self, kind: DvrKind) -> Result<usize, HalError> {
        let mut count = 0;
        for entry in self.dvrs.values() {
            if self.dvr_kind(entry)? == kind {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn demux_has_dvr_kind(&self, owner_demux_id: i32, kind: DvrKind) -> Result<bool, HalError> {
        for entry in self
            .dvrs
            .values()
            .filter(|entry| entry.owner_demux_id == owner_demux_id)
        {
            if self.dvr_kind(entry)? == kind {
                return Ok(true);
            }
        }
        Ok(false)
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
            .commit_demux_binding_use_case(demux_id, generation)
    }

    pub(crate) fn begin_descrambler_demux_source_call_use_case(
        &mut self,
        id: DescramblerRuntimeId,
    ) -> Result<(), DescramblerSessionFailure> {
        self.descrambler_runtimes
            .get_mut(&id)
            .ok_or(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            })?
            .begin_demux_source_call_use_case()
    }

    pub(crate) fn record_descrambler_demux_source_call_failure_use_case(
        &mut self,
        id: DescramblerRuntimeId,
        failure: DescramblerSourceCallFailure,
    ) -> Result<(), DescramblerSessionFailure> {
        let runtime = self
            .descrambler_runtimes
            .get_mut(&id)
            .ok_or(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateOpen,
                kind: DescramblerSessionFailureKind::SessionClosed,
            })?;
        if runtime.record_demux_source_call_failure_use_case(failure) {
            Ok(())
        } else {
            Err(DescramblerSessionFailure {
                step: DescramblerSessionTxnStep::ValidateDemux,
                kind: DescramblerSessionFailureKind::DemuxAlreadyBound,
            })
        }
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
                runtime.holds_binding_to_demux(demux_id, demux_generation)
                    && runtime.has_pid_claim(pid)
            })
    }

    pub(crate) fn descrambler_ids_bound_to_demux(
        &self,
        demux_id: i32,
    ) -> Vec<DescramblerRuntimeId> {
        self.descrambler_runtimes
            .iter()
            .filter_map(|(id, runtime)| runtime.holds_binding_to_demux_id(demux_id).then_some(*id))
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

    pub(crate) fn keyless_assembly_suppression_records_for_demux_packet_pid(
        &self,
        demux_id: i32,
        demux_generation: u64,
        packet_pid: PacketPid,
    ) -> Vec<DescramblerDiagnosticRecord> {
        let mut records = Vec::new();
        let keyless_claim = self.descrambler_runtimes.values().any(|runtime| {
            runtime.has_keyless_claim_for_demux_packet_pid(demux_id, demux_generation, packet_pid)
        });
        if keyless_claim {
            records.push(DescramblerDiagnosticRecord::packet_policy(
                demux_id,
                packet_pid,
                DescramblerDiagnosticKind::PacketScrambledWithoutKey,
            ));
        }
        records.push(DescramblerDiagnosticRecord::packet_policy(
            demux_id,
            packet_pid,
            DescramblerDiagnosticKind::PacketAssemblySuppressed,
        ));
        records
    }

    pub(crate) fn packet_pipeline_diagnostic_records_for_demux_report(
        &self,
        demux_id: i32,
        demux_generation: u64,
        report: &PipelineReport,
    ) -> Vec<DescramblerDiagnosticRecord> {
        if !report
            .assembly_suppression_reasons
            .contains(&maleicacid_tuner_hal2_demux::PipelineAssemblySuppressionReason::KeylessScrambledWithoutDescrambler)
        {
            return Vec::new();
        }
        report
            .diagnostics
            .iter()
            .filter_map(|diagnostic| match diagnostic {
                PipelineDiagnostic::KeylessScrambledAssemblySuppressed { pid } => Some(*pid),
                _ => None,
            })
            .flat_map(|pid| {
                self.keyless_assembly_suppression_records_for_demux_packet_pid(
                    demux_id,
                    demux_generation,
                    pid,
                )
            })
            .collect()
    }

    pub(crate) fn descrambler_validation_failure_without_pid_decision(
        &self,
        demux_id: i32,
        failure: DescrambleFailure,
    ) -> ResolvedDescramblerPacketFailureDecision {
        let mut diagnostic_records = Vec::new();
        if matches!(
            packet_policy_for_descramble_failure(failure),
            PacketPolicyAction::RecordPassThroughAndDropAssembly
        ) {
            diagnostic_records.push(DescramblerDiagnosticRecord::packet_policy_without_pid(
                demux_id,
                DescramblerDiagnosticKind::PacketAssemblySuppressed,
            ));
        }
        diagnostic_records.push(DescramblerDiagnosticRecord::packet_policy_without_pid(
            demux_id,
            descrambler_diagnostic_kind_for_registry_boundary(failure),
        ));
        ResolvedDescramblerPacketFailureDecision {
            flow: ResolvedDescramblerPacketMaterial::flow_for_descramble_failure(failure),
            diagnostic_records,
        }
    }

    pub(crate) fn clear_descrambler_key_use_case(
        &mut self,
        descrambler_id: DescramblerRuntimeId,
    ) -> Result<DescramblerClearKeyOutcome<DescramblerKeyLookupError>, DescramblerClearKeyTxnError>
    {
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
        DescramblerReplaceKeyOutcome<DescramblerKeyLookupError>,
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
