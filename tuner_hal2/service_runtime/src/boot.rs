use std::collections::BTreeSet;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, FrontendDevicePath, FrontendSystem, FrontendTuneRequest, HalError,
    HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind, TS_PACKET_SIZE,
};
use maleicacid_tuner_hal2_demux::config::{
    AvStreamKind, AvStreamTypeConfig, FilterConfig, FilterDelayHint, FilterOpenType,
};
use maleicacid_tuner_hal2_demux::packet_pipeline::{
    PipelineAssemblySuppressionReason, PipelineBoundaryReason, PipelineDiagnosticKind,
    PipelineReport, PipelineResetReport,
};
use maleicacid_tuner_hal2_demux::runtime::{
    DemuxRuntimeError, DemuxRuntimeErrorKind, DemuxRuntimeState, DvrKind,
};
use maleicacid_tuner_hal2_demux::runtime::{
    DemuxRuntimeSnapshot, DemuxStreamGeneration, GenerationBoundaryReport, GenerationBoundaryTxn,
};
use maleicacid_tuner_hal2_demux::OpenFilterRequest;
use maleicacid_tuner_hal2_demux::{
    DvrConfigureTxn, DvrRuntime, DvrRuntimeState, FilterConfigureTxn, FilterRuntime,
    FilterRuntimeState, TsInputOrigin,
};
use maleicacid_tuner_hal2_descrambler::{
    descramble_ts_packet_in_place, packet_policy_for_descramble_failure, parse_ts_packet_header,
    DescrambleFailure, DescrambleOutcome, DescramblerKeyLookupError,
    DescramblerKeyRegistrationError, DescramblerKeySlot, DescramblerKeyToken,
    DescramblerKeyTokenError, DescramblerPidClaim, DescramblerPidClaimError,
    DescramblerSessionFailureKind, DescramblerSessionTxn, PacketPolicyAction,
};
use maleicacid_tuner_hal2_device::{
    FrontendLivePacketSink, FrontendLivePumpOwner, FrontendLivePumpReport,
    FrontendLiveReaderDescriptor, FrontendRuntimeSnapshot, FrontendRuntimeState,
    FrontendSignalState, FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind,
    FrontendWorkerRegistry, FrontendWorkerStartError, FrontendWorkerStopOutcome,
};
use maleicacid_tuner_hal2_domain_request::{
    AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan, DvrConfigureKind,
    DvrConfigureRequest, DvrOpenKind, FilterAvStreamKind, FilterAvStreamTypeRequest,
    FilterDelayHintKind, FilterDelayHintRequest, OpenDvrRequest, RuntimeExecutableRequest,
    RuntimeTransactionName,
};

use crate::callback_registry::RuntimeCallbackRegistry;
use crate::command_dispatch::{
    RuntimeCommandDispatchError, RuntimeCommandDispatchPlan, RuntimeCommandDispatcher,
};
use crate::diagnostics::{
    CapabilitySuppressionReason, ChildOpenRollbackDiagnosticRecord, DescramblerDiagnosticKind,
    DescramblerDiagnosticPhase, DescramblerDiagnosticRecord, StartupDiagnosticRecord,
};
use crate::dispatch::{
    adapter_transactions_are_covered, dispatch_target_for, ServiceRuntimeDispatchTarget,
};
use crate::object_table::{
    RuntimeObjectEntry, RuntimeObjectLifecycle, RuntimeObjectTable, RuntimeObjectTableError,
    RuntimeOwnerRelation,
};
use crate::registry::{
    DemuxRuntimeId, DescramblerRuntimeId, DvrRuntimeId, FilterRuntimeId, FrontendRegistryEntry,
    FrontendRuntimeId, LnbRegistryEntry, LnbRegistryProfile, LnbRuntimeId, RegistryCommitError,
    RuntimeRegistry,
};
use crate::ServiceState;
use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};

// Operation implementations are boot child modules so they can use
// TunerServiceRuntime private state without widening field visibility.
mod query_api;
pub use query_api::{RuntimeObjectPublicEntry, RuntimeObjectQueryError};
mod demux_filter_dvr_txn;
mod descrambler_txn;
mod frontend_txn;
mod lnb_txn;
mod packet_txn;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendProbeOutcome {
    Available {
        id: FrontendRuntimeId,
        backend: FrontendBackendKind,
        system: FrontendSystem,
        path: PathBuf,
        lnb_profile: Option<LnbRegistryProfile>,
    },
    DeviceMissing {
        backend: FrontendBackendKind,
        path: PathBuf,
    },
    DeviceOpenFailed {
        backend: FrontendBackendKind,
        path: PathBuf,
        error: HalError,
    },
    CapabilitySuppressed {
        backend: FrontendBackendKind,
        path: PathBuf,
        reason: CapabilitySuppressionReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceBootOutcome {
    Ready,
    Degraded,
}

fn dvb_dvr_path_for_frontend_path(path: &std::path::Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    Some(parent.join("dvr0"))
}

fn live_reader_descriptor_for_frontend_entry(
    entry: &FrontendRegistryEntry,
) -> Result<FrontendLiveReaderDescriptor, HalError> {
    match entry.backend {
        FrontendBackendKind::Px4CharDevice => {
            Ok(FrontendLiveReaderDescriptor::px4_from_control_fd(
                entry.id.0,
                FrontendDevicePath::new(entry.device_path.clone()),
            ))
        }
        FrontendBackendKind::LinuxDvb => {
            let dvr_path = dvb_dvr_path_for_frontend_path(&entry.device_path).ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    format!(
                        "DVB frontend path has no parent directory: {}",
                        entry.device_path.display()
                    ),
                )
            })?;
            Ok(FrontendLiveReaderDescriptor::dvb_dvr_device(
                entry.id.0,
                FrontendDevicePath::new(dvr_path),
            ))
        }
    }
}

fn default_lnb_entry_for_frontend(entry: &FrontendRegistryEntry) -> Option<LnbRegistryEntry> {
    let profile = entry.lnb_profile?;
    let id = LnbRuntimeId(entry.id.0.checked_add(10_000)?);
    let name = match entry.backend {
        FrontendBackendKind::Px4CharDevice => {
            let dev = entry
                .device_path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("unknown");
            let rel = entry.id.0.saturating_sub(1_000_000);
            let unit = rel.rem_euclid(10_000).div_euclid(10);
            Some(format!("maleicacid-lnb-px4-{dev}-unit-{unit}"))
        }
        FrontendBackendKind::LinuxDvb => {
            let path = entry.device_path.display().to_string();
            Some(format!("maleicacid-lnb-{path}"))
        }
    };
    Some(LnbRegistryEntry {
        id,
        name,
        owner_frontend_id: entry.id,
        profile,
    })
}

#[derive(Clone)]
pub struct FrontendDemuxPacketSink {
    runtime: Arc<Mutex<TunerServiceRuntime>>,
    frontend_id: i32,
}

impl FrontendDemuxPacketSink {
    pub fn new(runtime: Arc<Mutex<TunerServiceRuntime>>, frontend_id: i32) -> Self {
        Self {
            runtime,
            frontend_id,
        }
    }

    pub fn frontend_id(&self) -> i32 {
        self.frontend_id
    }
}

impl FrontendLivePacketSink for FrontendDemuxPacketSink {
    fn deliver_ts_packet(&mut self, packet: &[u8; TS_PACKET_SIZE]) -> Result<(), HalError> {
        self.runtime
            .lock()
            .map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while delivering frontend TS packet",
                )
            })?
            .push_frontend_ts_packet_to_bound_demuxes(self.frontend_id, packet)
            .map(|_| ())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveDescramblerSnapshot {
    pids: BTreeSet<u16>,
    key_slot: Option<DescramblerKeySlot>,
}

impl ActiveDescramblerSnapshot {
    fn targets_pid(&self, pid: u16) -> bool {
        self.pids.contains(&pid)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DescramblePacketFlow {
    Clear,
    Descrambled,
    RecordPassThroughAndDropAssembly,
    Drop,
    DiagnoseOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescramblePacketDecision {
    packet: [u8; TS_PACKET_SIZE],
    flow: DescramblePacketFlow,
}

fn demux_runtime_error_to_hal(
    error: maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeError,
) -> HalError {
    match error.kind {
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::GenerationExhausted => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtime generation exhausted",
            )
        }
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::FilterMissing
        | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::DvrMissing
        | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::QueueMissing => {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtime object is missing",
            )
        }
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidState
        | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidDvrFilter
        | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::SourceLifecycle
        | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::SinkLifecycle => {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtime lifecycle is invalid",
            )
        }
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidSourceSubtype
        | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidSinkSubtype => {
            HalError::Unsupported("demux source/sink subtype is unsupported")
        }
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::UnsupportedDvrOperation => {
            HalError::Unsupported("DVR operation is unavailable for this DVR kind")
        }
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::PidMismatch => {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "demux source/sink PID mismatch",
            )
        }
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::PipelineFailed => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtime pipeline operation failed",
            )
        }
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::QueueRuntimeFailure => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtime queue operation failed",
            )
        }
    }
}

fn descrambler_session_failure_to_hal(kind: DescramblerSessionFailureKind) -> HalError {
    match kind {
        DescramblerSessionFailureKind::SessionClosed => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "descrambler session is closed",
        ),
        DescramblerSessionFailureKind::DemuxNotBound => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "descrambler demux source is not bound",
        ),
        DescramblerSessionFailureKind::UnknownToken => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "descrambler key token is unknown",
        ),
        DescramblerSessionFailureKind::ExpiredToken => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "descrambler key token is expired",
        ),
    }
}

fn descrambler_key_token_error_to_hal(error: DescramblerKeyTokenError) -> HalError {
    match error {
        DescramblerKeyTokenError::Empty => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "descrambler key token must not be empty",
        ),
        DescramblerKeyTokenError::InvalidLength { .. } => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "descrambler key token length is invalid",
        ),
    }
}

fn descrambler_key_lookup_error_to_hal(error: DescramblerKeyLookupError) -> HalError {
    match error {
        DescramblerKeyLookupError::UnknownToken => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "descrambler key token is unknown",
        ),
        DescramblerKeyLookupError::ExpiredToken => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "descrambler key token is expired",
        ),
    }
}

fn descrambler_key_release_error_to_hal(_error: DescramblerKeyLookupError) -> HalError {
    HalError::internal(
        HalInternalKind::InvariantViolation,
        "descrambler key token release failed",
    )
}

fn descrambler_pid_claim_error_to_hal(error: DescramblerPidClaimError) -> HalError {
    match error {
        DescramblerPidClaimError::NullSourceFilterUnsupported => HalError::Unsupported(
            "nullable upstream filter is outside the current Rust AIDL boundary scope",
        ),
        DescramblerPidClaimError::InvalidPid => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "descrambler PID is invalid",
        ),
    }
}

fn diagnostic_kind_for_descramble_failure(failure: DescrambleFailure) -> DescramblerDiagnosticKind {
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

pub fn start_frontend_demux_live_pump_from_reader(
    runtime: Arc<Mutex<TunerServiceRuntime>>,
    frontend_id: i32,
    reader: Box<dyn Read + Send>,
) -> Result<FrontendLivePumpOwner, HalError> {
    {
        let guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while preparing frontend demux live pump",
            )
        })?;
        guard
            .query()
            .ensure_frontend_demux_sink_ready(frontend_id)?;
    }
    let sink: Box<dyn FrontendLivePacketSink> = Box::new(FrontendDemuxPacketSink::new(
        Arc::clone(&runtime),
        frontend_id,
    ));
    FrontendLivePumpOwner::start(reader, sink)
}

#[derive(Debug)]
pub struct TunerServiceRuntime {
    state: ServiceState,
    registry: RuntimeRegistry,
    object_table: RuntimeObjectTable,
    diagnostics: Vec<StartupDiagnosticRecord>,
    descrambler_diagnostics: Vec<DescramblerDiagnosticRecord>,
    child_open_rollback_diagnostics: Vec<ChildOpenRollbackDiagnosticRecord>,
    callback_registry: RuntimeCallbackRegistry,
    frontend_workers: FrontendWorkerRegistry,
    next_aidl_generation: u64,
    next_aidl_object_id: i64,
}

impl Default for TunerServiceRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl TunerServiceRuntime {
    pub fn new() -> Self {
        Self {
            state: ServiceState::Booting,
            registry: RuntimeRegistry::default(),
            object_table: RuntimeObjectTable::default(),
            diagnostics: Vec::new(),
            descrambler_diagnostics: Vec::new(),
            child_open_rollback_diagnostics: Vec::new(),
            callback_registry: RuntimeCallbackRegistry::default(),
            frontend_workers: FrontendWorkerRegistry::default(),
            next_aidl_generation: 0,
            next_aidl_object_id: 0,
        }
    }

    pub fn state(&self) -> ServiceState {
        self.state
    }

    pub fn registry(&self) -> &RuntimeRegistry {
        &self.registry
    }

    fn registry_mut(&mut self) -> &mut RuntimeRegistry {
        &mut self.registry
    }

    #[cfg(test)]
    pub(crate) fn registry_mut_for_test(&mut self) -> &mut RuntimeRegistry {
        &mut self.registry
    }

    pub fn diagnostics(&self) -> &[StartupDiagnosticRecord] {
        &self.diagnostics
    }

    pub fn descrambler_diagnostics(&self) -> &[DescramblerDiagnosticRecord] {
        &self.descrambler_diagnostics
    }

    pub fn child_open_rollback_diagnostics(&self) -> &[ChildOpenRollbackDiagnosticRecord] {
        &self.child_open_rollback_diagnostics
    }

    pub fn register_descrambler_key_slot(
        &mut self,
        token: DescramblerKeyToken,
        key_slot: DescramblerKeySlot,
    ) -> Result<(), DescramblerKeyRegistrationError> {
        self.registry
            .descrambler_key_table_mut()
            .register_key_slot(token, key_slot)
            .map(|_| ())
    }

    pub(crate) fn record_child_open_rollback_diagnostic(
        &mut self,
        record: ChildOpenRollbackDiagnosticRecord,
    ) {
        self.child_open_rollback_diagnostics.push(record);
    }

    fn record_descrambler_diagnostic(&mut self, record: DescramblerDiagnosticRecord) {
        self.descrambler_diagnostics.push(record);
    }

    pub fn object_table(&self) -> &RuntimeObjectTable {
        &self.object_table
    }

    pub fn object_table_mut(&mut self) -> &mut RuntimeObjectTable {
        &mut self.object_table
    }

    pub fn callback_registry(&self) -> &RuntimeCallbackRegistry {
        &self.callback_registry
    }

    pub fn callback_registry_mut(&mut self) -> &mut RuntimeCallbackRegistry {
        &mut self.callback_registry
    }

    pub fn boot_from_probe_results<I>(&mut self, results: I) -> ServiceBootOutcome
    where
        I: IntoIterator<Item = FrontendProbeOutcome>,
    {
        self.state = ServiceState::Booting;
        self.registry.clear_frontends();
        self.registry.clear_lnbs();
        self.registry.clear_transient_objects();
        self.object_table.clear();
        self.diagnostics.clear();
        self.callback_registry = RuntimeCallbackRegistry::default();
        self.frontend_workers = FrontendWorkerRegistry::default();
        self.next_aidl_generation = 0;
        self.next_aidl_object_id = 0;

        if !adapter_transactions_are_covered() {
            self.diagnostics
                .push(StartupDiagnosticRecord::runtime_dispatch_missing());
        }

        for result in results {
            match result {
                FrontendProbeOutcome::Available {
                    id,
                    backend,
                    system,
                    path,
                    lnb_profile,
                } => {
                    let entry = FrontendRegistryEntry {
                        id,
                        backend,
                        system,
                        device_path: path.clone(),
                        lnb_profile,
                    };
                    match self.registry.register_frontend(entry.clone()) {
                        Ok(()) => {
                            if let Some(lnb_entry) = default_lnb_entry_for_frontend(&entry) {
                                if let Err(RegistryCommitError::DuplicateLnbId { .. }) =
                                    self.registry.register_lnb(lnb_entry)
                                {
                                    self.diagnostics.push(
                                        StartupDiagnosticRecord::duplicate_lnb_id(
                                            backend,
                                            path.clone(),
                                        ),
                                    );
                                }
                            }
                        }
                        Err(RegistryCommitError::DuplicateFrontendId { .. }) => {
                            self.diagnostics
                                .push(StartupDiagnosticRecord::duplicate_frontend_id(
                                    backend, path,
                                ));
                        }
                        Err(_) => {
                            self.diagnostics
                                .push(StartupDiagnosticRecord::duplicate_frontend_id(
                                    backend, path,
                                ));
                        }
                    }
                }
                FrontendProbeOutcome::DeviceMissing { backend, path } => {
                    self.diagnostics
                        .push(StartupDiagnosticRecord::device_missing(backend, path));
                }
                FrontendProbeOutcome::DeviceOpenFailed {
                    backend,
                    path,
                    error,
                } => {
                    self.diagnostics
                        .push(StartupDiagnosticRecord::device_open_failed(
                            backend, path, error,
                        ));
                }
                FrontendProbeOutcome::CapabilitySuppressed {
                    backend,
                    path,
                    reason,
                } => {
                    self.diagnostics
                        .push(StartupDiagnosticRecord::capability_suppressed(
                            backend, path, reason,
                        ));
                }
            }
        }

        if self.registry.frontend_count() > 0 && self.diagnostics.is_empty() {
            self.state = ServiceState::Ready;
            ServiceBootOutcome::Ready
        } else {
            self.state = ServiceState::Degraded;
            ServiceBootOutcome::Degraded
        }
    }

    pub fn dispatch_target(
        &mut self,
        transaction: RuntimeTransactionName,
    ) -> Option<ServiceRuntimeDispatchTarget> {
        let target = dispatch_target_for(transaction);
        if target.is_none() {
            self.diagnostics
                .push(StartupDiagnosticRecord::runtime_dispatch_missing());
        }
        target
    }

    fn allocate_aidl_generation(
        &mut self,
    ) -> Result<AidlObjectGeneration, RuntimeObjectTableError> {
        let next = self
            .next_aidl_generation
            .checked_add(1)
            .ok_or(RuntimeObjectTableError::GenerationOverflow)?;
        self.next_aidl_generation = next;
        Ok(AidlObjectGeneration(next))
    }

    fn allocate_aidl_object_id(&mut self) -> Result<AidlObjectId, RuntimeObjectTableError> {
        let next = self
            .next_aidl_object_id
            .checked_add(1)
            .filter(|value| *value > 0)
            .ok_or(RuntimeObjectTableError::GenerationOverflow)?;
        self.next_aidl_object_id = next;
        Ok(AidlObjectId(next))
    }

    pub fn register_aidl_object_for_runtime(
        &mut self,
        object_kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        public_runtime_id: i64,
        owner: RuntimeOwnerRelation,
    ) -> Result<crate::object_table::RuntimeObjectEntry, RuntimeObjectTableError> {
        let entry = RuntimeObjectEntry {
            object_kind,
            object_id,
            generation,
            ledger_id: LedgerId(public_runtime_id),
            ledger_generation: LedgerGeneration(generation.0 as u64),
            owner,
            lifecycle: RuntimeObjectLifecycle::Live,
        };
        self.object_table.insert(entry.clone())?;
        Ok(entry)
    }

    pub fn register_aidl_object_for_runtime_auto_generation(
        &mut self,
        object_kind: AidlObjectKind,
        public_runtime_id: i64,
        owner: RuntimeOwnerRelation,
    ) -> Result<crate::object_table::RuntimeObjectEntry, RuntimeObjectTableError> {
        let object_id = self.allocate_aidl_object_id()?;
        let generation = self.allocate_aidl_generation()?;
        self.register_aidl_object_for_runtime(
            object_kind,
            object_id,
            generation,
            public_runtime_id,
            owner,
        )
    }

    pub fn unregister_aidl_object_after_registration_failure(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        self.object_table.remove(object_id, generation)
    }

    pub fn unregister_public_runtime_for_closed_aidl_entry(
        &mut self,
        entry: &RuntimeObjectEntry,
    ) -> Result<(), HalError> {
        let id = i32::try_from(entry.ledger_id.0).map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!(
                    "public runtime id is outside i32 range during close cleanup: kind={:?}",
                    entry.object_kind
                ),
            )
        })?;
        let removed = match entry.object_kind {
            AidlObjectKind::Demux => match self.unregister_demux_runtime(id)? {
                Some(_) => true,
                None => false,
            },
            AidlObjectKind::Filter => match self.unregister_filter_runtime(id)? {
                Some(_) => true,
                None => false,
            },
            AidlObjectKind::Dvr => match self.unregister_dvr_runtime(id)? {
                Some(_) => true,
                None => false,
            },
            AidlObjectKind::Descrambler => match self.unregister_descrambler_runtime(id)? {
                Some(_) => true,
                None => false,
            },
            _ => return Ok(()),
        };
        if removed {
            Ok(())
        } else {
            Err(HalError::cleanup_failed(
                "public runtime unregister after AIDL object close",
                format!(
                    "runtime entry missing during close cleanup: kind={:?} id={id}",
                    entry.object_kind
                ),
            ))
        }
    }
    pub fn plan_command_dispatch(
        &mut self,
        command_plan: CommandPlan,
        executable_request: Option<RuntimeExecutableRequest>,
    ) -> Result<RuntimeCommandDispatchPlan, RuntimeCommandDispatchError> {
        let plan = RuntimeCommandDispatcher::plan(command_plan, executable_request);
        if plan.is_err() {
            self.diagnostics
                .push(StartupDiagnosticRecord::runtime_dispatch_missing());
        }
        plan
    }
}
