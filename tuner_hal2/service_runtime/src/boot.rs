use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FirstErrorCollector, FrontendBackendKind, FrontendDevicePath,
    FrontendSystem, FrontendTuneRequest, HalError, HalInternalKind, HalInvalidArgumentKind,
    HalInvalidStateKind, TS_PACKET_SIZE,
};
use maleicacid_tuner_hal2_demux::av::AvMediaEventDescriptor;
use maleicacid_tuner_hal2_demux::config::{
    AvStreamKind, AvStreamTypeConfig, FilterConfig, FilterDelayHint, FilterOpenType,
};
use maleicacid_tuner_hal2_demux::packet_pipeline::{
    PacketDescramblePolicyFailure, PacketPid, PipelineAssemblySuppressionReason,
    PipelineBoundaryReason, PipelineDiagnostic, PipelineReport, PipelineResetReport,
    TsPacketValidationError, ValidatedTsPacket,
};
use maleicacid_tuner_hal2_demux::runtime::{
    DemuxRuntimeError, DemuxRuntimeErrorKind, DemuxRuntimeState, DvrKind,
};
use maleicacid_tuner_hal2_demux::runtime::{
    DemuxRuntimeSnapshot, DemuxStreamGeneration, GenerationBoundaryReport, GenerationBoundaryTxn,
};
use maleicacid_tuner_hal2_demux::OpenFilterRequest;
use maleicacid_tuner_hal2_demux::{
    DvrConfigureOutcome, DvrConfigureTxn, DvrRuntime, DvrRuntimeState, FilterConfigureOutcome,
    FilterConfigureTxn, FilterRuntime, FilterRuntimeState, TsInputOrigin,
};
use crate::descrambler_key_table::DescramblerKeyLookupError;
#[cfg(test)]
use crate::descrambler_key_table::DescramblerKeySlotId;
use maleicacid_tuner_hal2_descrambler::{
    descramble_ts_packet_in_place, packet_policy_for_descramble_failure, DescrambleFailure,
    DescrambleOutcome, DescramblerKeySlot, DescramblerKeyToken, DescramblerKeyTokenError,
    DescramblerPid, DescramblerPidClaim, DescramblerPidClaimError, PacketPolicyAction,
};
use maleicacid_tuner_hal2_device::{
    FrontendLivePacketSink, FrontendLivePumpOwner, FrontendLivePumpReport,
    FrontendLiveReaderDescriptor, FrontendRuntimeSnapshot, FrontendRuntimeState,
    FrontendSignalState, FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind,
    FrontendWorkerRegistry, FrontendWorkerStartError, FrontendWorkerStopOutcome,
};
use maleicacid_tuner_hal2_domain_request::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan, DvrConfigureKind,
    DvrConfigureRequest, DvrOpenKind, FilterAvStreamKind, FilterAvStreamTypeRequest,
    FilterDelayHintKind, FilterDelayHintRequest, OpenDvrRequest, RuntimeExecutableRequest,
    RuntimeTransactionName,
};

use crate::callback_registry::{
    CallbackHealthState, CallbackRegistryUpdate, RuntimeCallbackRegistry,
};
use crate::command_dispatch::{
    RuntimeCommandDispatchError, RuntimeCommandDispatchPlan, RuntimeCommandDispatcher,
};
use crate::descrambler_session::{
    DescramblerCleanupTxnError, DescramblerClearKeyTxnError,
    DescramblerReplaceKeyOutcome, DescramblerReplaceKeyTxnError,
    DescramblerSessionFailureKind,
};
use crate::diagnostics::{
    BoundedDiagnosticStore, CallbackArtifactRuntimeSplitDiagnosticRecord,
    CallbackArtifactRuntimeSplitOutcome, CallbackArtifactRuntimeSplitPhase, CapabilitySuppressionReason,
    ChildOpenRollbackDiagnosticRecord, DescramblerDiagnosticKind, DescramblerDiagnosticPhase,
    DescramblerDiagnosticRecord, DvrPostCommitNotificationDiagnosticRecord,
    DvrPostCommitNotificationPhase, FilterCallbackDeliveryDiagnosticPhase,
    FilterCallbackDeliveryDiagnosticRecord, StartupDiagnosticRecord,
};
use crate::dispatch::{
    adapter_transactions_are_covered, dispatch_target_for, ServiceRuntimeDispatchTarget,
};
use crate::object_lifecycle::aidl_object_live;
use crate::object_method_txn::ObjectMethodExecutionToken;
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
pub use query_api::DvrStatusPollSnapshot;
pub(crate) use query_api::RuntimeQuery;
mod demux_filter_dvr_txn;
mod descrambler_txn;
mod frontend_txn;
mod lnb_txn;
mod packet_txn;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterChildRuntimeOpen {
    pub runtime_entry: RuntimeObjectEntry,
    pub filter_id: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DvrChildRuntimeOpen {
    pub runtime_entry: RuntimeObjectEntry,
    pub dvr_id: i32,
}

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
    dispatcher: Arc<dyn FilterEventDispatcher>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterEventDeliverySnapshot {
    pub object_id: AidlObjectId,
    pub generation: AidlObjectGeneration,
    pub event: FilterEventDelivery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterEventDelivery {
    Media(AvMediaEventDescriptor),
    Section { data_length: usize },
    Pes { stream_id: i32, data_length: usize },
}

pub trait FilterEventDispatcher: Send + Sync {
    fn dispatch(
        &self,
        runtime: &Arc<Mutex<TunerServiceRuntime>>,
        events: Vec<FilterEventDeliverySnapshot>,
    ) -> Result<(), HalError>;
}

#[derive(Clone)]
struct FilterEventDispatcherHandle {
    dispatcher: Arc<dyn FilterEventDispatcher>,
}

impl FilterEventDispatcherHandle {
    fn new(dispatcher: Arc<dyn FilterEventDispatcher>) -> Self {
        Self { dispatcher }
    }

    fn dispatcher(&self) -> Arc<dyn FilterEventDispatcher> {
        Arc::clone(&self.dispatcher)
    }
}

impl fmt::Debug for FilterEventDispatcherHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FilterEventDispatcherHandle")
            .field("dispatcher", &"<filter event dispatcher>")
            .finish()
    }
}

impl FrontendDemuxPacketSink {
    pub fn new(
        runtime: Arc<Mutex<TunerServiceRuntime>>,
        frontend_id: i32,
        dispatcher: Arc<dyn FilterEventDispatcher>,
    ) -> Self {
        Self {
            runtime,
            frontend_id,
            dispatcher,
        }
    }

    pub fn frontend_id(&self) -> i32 {
        self.frontend_id
    }
}

impl FrontendLivePacketSink for FrontendDemuxPacketSink {
    fn deliver_ts_packet(&mut self, packet: &[u8; TS_PACKET_SIZE]) -> Result<(), HalError> {
        let events = {
            let mut runtime = self.runtime.lock().map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while delivering frontend TS packet",
                )
            })?;
            let reports =
                runtime.push_frontend_ts_packet_to_bound_demuxes(self.frontend_id, packet)?;
            runtime.filter_event_delivery_snapshots(&reports)
        };
        if events.is_empty() {
            return Ok(());
        }
        self.dispatcher.dispatch(&self.runtime, events)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveDescramblerSnapshot {
    descrambler_pids: BTreeSet<DescramblerPid>,
    packet_pids: BTreeSet<PacketPid>,
    key_slot: Option<DescramblerKeySlot>,
    source_filter_ids_by_pid: BTreeMap<PacketPid, BTreeSet<i32>>,
}

impl ActiveDescramblerSnapshot {
    fn targets_packet_pid(&self, pid: PacketPid) -> bool {
        self.packet_pids.contains(&pid)
    }

    fn source_filter_ids_for_packet_pid(&self, pid: PacketPid) -> Option<&BTreeSet<i32>> {
        self.source_filter_ids_by_pid.get(&pid)
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
    diagnostics: Vec<PipelineDiagnostic>,
}

pub(super) fn demux_runtime_error_to_hal(
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
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed => {
            HalError::cleanup_failed(
                "demux source boundary rollback",
                "demux runtime was quarantined after source boundary rollback failure",
            )
        }
        maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::QueueRuntimeFailure
        | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::AvBackingFailure => {
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
        DescramblerSessionFailureKind::ClearKeyPlanMismatch => HalError::internal(
            HalInternalKind::InvariantViolation,
            "descrambler clear-key plan no longer matches session state",
        ),
        DescramblerSessionFailureKind::ReplaceKeyPlanMismatch => HalError::internal(
            HalInternalKind::InvariantViolation,
            "descrambler replace-key plan no longer matches session state",
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
    let dispatcher = {
        let guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while preparing frontend demux live pump",
            )
        })?;
        guard
            .query()
            .ensure_frontend_demux_sink_ready(frontend_id)?;
        guard.filter_event_dispatcher()?
    };
    let sink: Box<dyn FrontendLivePacketSink> = Box::new(FrontendDemuxPacketSink::new(
        Arc::clone(&runtime),
        frontend_id,
        dispatcher,
    ));
    FrontendLivePumpOwner::start(reader, sink)
}

#[derive(Debug)]
pub struct TunerServiceRuntime {
    state: ServiceState,
    registry: RuntimeRegistry,
    object_table: RuntimeObjectTable,
    diagnostics: BoundedDiagnosticStore<StartupDiagnosticRecord>,
    descrambler_diagnostics: BoundedDiagnosticStore<DescramblerDiagnosticRecord>,
    child_open_rollback_diagnostics: BoundedDiagnosticStore<ChildOpenRollbackDiagnosticRecord>,
    dvr_post_commit_notification_diagnostics:
        BoundedDiagnosticStore<DvrPostCommitNotificationDiagnosticRecord>,
    filter_callback_delivery_diagnostics:
        BoundedDiagnosticStore<FilterCallbackDeliveryDiagnosticRecord>,
    callback_artifact_runtime_split_diagnostics:
        BoundedDiagnosticStore<CallbackArtifactRuntimeSplitDiagnosticRecord>,
    filter_event_dispatcher: Option<FilterEventDispatcherHandle>,
    callback_registry: RuntimeCallbackRegistry,
    frontend_workers: FrontendWorkerRegistry,
    next_aidl_generation: u64,
    next_aidl_object_id: i64,
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerCallbackCleanupArtifactCommand {
    owner_kind: AidlObjectKind,
    owner_id: AidlObjectId,
    owner_generation: AidlObjectGeneration,
    registration_api: Option<AidlApi>,
    cleanup_failure_message: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackArtifactResetCommand {
    failure_message: &'static str,
}


#[derive(Debug)]
pub struct OwnerCallbackCleanupUseCaseOutcome<T> {
    command: OwnerCallbackCleanupArtifactCommand,
    primary_result: Result<T, HalError>,
}

impl<T> OwnerCallbackCleanupUseCaseOutcome<T> {
    fn new(command: OwnerCallbackCleanupArtifactCommand, primary_result: Result<T, HalError>) -> Self {
        Self { command, primary_result }
    }

    pub fn command(&self) -> &OwnerCallbackCleanupArtifactCommand {
        &self.command
    }

    fn into_parts(self) -> (OwnerCallbackCleanupArtifactCommand, Result<T, HalError>) {
        (self.command, self.primary_result)
    }
}

#[derive(Debug)]
pub struct CallbackRegistrationArtifactOutcome {
    rollback_command: Option<OwnerCallbackCleanupArtifactCommand>,
    primary_result: Result<(), HalError>,
}

impl CallbackRegistrationArtifactOutcome {
    fn new(
        rollback_command: Option<OwnerCallbackCleanupArtifactCommand>,
        primary_result: Result<(), HalError>,
    ) -> Self {
        Self { rollback_command, primary_result }
    }

    pub fn rollback_command(&self) -> Option<&OwnerCallbackCleanupArtifactCommand> {
        self.rollback_command.as_ref()
    }

    fn into_parts(self) -> (Option<OwnerCallbackCleanupArtifactCommand>, Result<(), HalError>) {
        (self.rollback_command, self.primary_result)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackDeliveryOwnerKind {
    Frontend,
    Filter,
    Dvr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackDeliveryFailurePhase {
    CallbackArtifactLookup,
    EventConversion,
    BinderDelivery,
    ScanEndDelivery,
    PostCommitNotification,
    NotifierTerminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallbackDeliveryFailureReport {
    owner_kind: CallbackDeliveryOwnerKind,
    owner_id: AidlObjectId,
    owner_generation: AidlObjectGeneration,
    phase: CallbackDeliveryFailurePhase,
    primary: HalError,
    frontend_scan_context: Option<(i32, u64)>,
    dvr_post_commit_phase: Option<DvrPostCommitNotificationPhase>,
}

impl CallbackDeliveryFailureReport {
    pub fn filter(
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        phase: CallbackDeliveryFailurePhase,
        primary: HalError,
    ) -> Self {
        Self {
            owner_kind: CallbackDeliveryOwnerKind::Filter,
            owner_id,
            owner_generation,
            phase,
            primary,
            frontend_scan_context: None,
            dvr_post_commit_phase: None,
        }
    }

    pub fn dvr(
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        phase: CallbackDeliveryFailurePhase,
        dvr_post_commit_phase: DvrPostCommitNotificationPhase,
        primary: HalError,
    ) -> Self {
        Self {
            owner_kind: CallbackDeliveryOwnerKind::Dvr,
            owner_id,
            owner_generation,
            phase,
            primary,
            frontend_scan_context: None,
            dvr_post_commit_phase: Some(dvr_post_commit_phase),
        }
    }

    pub fn frontend_scan_end(
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        frontend_id: i32,
        scan_generation: u64,
        phase: CallbackDeliveryFailurePhase,
        primary: HalError,
    ) -> Self {
        Self {
            owner_kind: CallbackDeliveryOwnerKind::Frontend,
            owner_id,
            owner_generation,
            phase,
            primary,
            frontend_scan_context: Some((frontend_id, scan_generation)),
            dvr_post_commit_phase: None,
        }
    }

    pub fn phase(&self) -> CallbackDeliveryFailurePhase {
        self.phase
    }

    pub fn dvr_post_commit_phase(&self) -> Option<DvrPostCommitNotificationPhase> {
        self.dvr_post_commit_phase
    }

    fn filter_diagnostic_phase(&self) -> FilterCallbackDeliveryDiagnosticPhase {
        match self.phase {
            CallbackDeliveryFailurePhase::CallbackArtifactLookup => {
                FilterCallbackDeliveryDiagnosticPhase::CallbackRegistryAccounting
            }
            CallbackDeliveryFailurePhase::EventConversion
            | CallbackDeliveryFailurePhase::BinderDelivery
            | CallbackDeliveryFailurePhase::ScanEndDelivery
            | CallbackDeliveryFailurePhase::PostCommitNotification
            | CallbackDeliveryFailurePhase::NotifierTerminal => {
                FilterCallbackDeliveryDiagnosticPhase::EventDelivery
            }
        }
    }
}

impl OwnerCallbackCleanupArtifactCommand {
    pub(crate) fn new(
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: Option<AidlApi>,
        cleanup_failure_message: &'static str,
    ) -> Self {
        Self {
            owner_kind,
            owner_id,
            owner_generation,
            registration_api,
            cleanup_failure_message,
        }
    }

    pub fn owner_kind(&self) -> AidlObjectKind {
        self.owner_kind
    }

    pub fn owner_id(&self) -> AidlObjectId {
        self.owner_id
    }

    pub fn owner_generation(&self) -> AidlObjectGeneration {
        self.owner_generation
    }

    pub fn registration_api(&self) -> Option<AidlApi> {
        self.registration_api
    }

    pub fn cleanup_failure_message(&self) -> &'static str {
        self.cleanup_failure_message
    }
}

impl CallbackArtifactResetCommand {
    pub(crate) fn new(failure_message: &'static str) -> Self {
        Self { failure_message }
    }

    pub fn failure_message(&self) -> &'static str {
        self.failure_message
    }
}

impl TunerServiceRuntime {
    pub fn plan_callback_artifact_reset_before_boot_use_case(&self) -> CallbackArtifactResetCommand {
        CallbackArtifactResetCommand::new("callback artifact reset failed before runtime boot")
    }

    fn filter_event_delivery_snapshots(
        &self,
        reports: &[PipelineReport],
    ) -> Vec<FilterEventDeliverySnapshot> {
        reports
            .iter()
            .flat_map(|report| report.generated_events.iter())
            .filter_map(|event| {
                use maleicacid_tuner_hal2_demux::packet_pipeline::PipelineGeneratedEvent;
                let (filter_id, event) = match event {
                    PipelineGeneratedEvent::AvMedia {
                        filter_id,
                        descriptor,
                    } => (*filter_id, FilterEventDelivery::Media(*descriptor)),
                    PipelineGeneratedEvent::SectionPayloadReady {
                        filter_id, bytes, ..
                    } => (
                        *filter_id,
                        FilterEventDelivery::Section {
                            data_length: bytes.len(),
                        },
                    ),
                    PipelineGeneratedEvent::PesPacketReady {
                        filter_id,
                        pid,
                        packet,
                        ..
                    } => (
                        *filter_id,
                        FilterEventDelivery::Pes {
                            stream_id: pid.to_i32_for_aidl_boundary(),
                            data_length: packet.raw_bytes.len(),
                        },
                    ),
                    _ => return None,
                };
                let entry = self.object_table.live_entry_for_runtime(
                    AidlObjectKind::Filter,
                    LedgerId(i64::from(filter_id)),
                )?;
                Some(FilterEventDeliverySnapshot {
                    object_id: entry.object_id,
                    generation: entry.generation,
                    event,
                })
            })
            .collect()
    }
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
            diagnostics: BoundedDiagnosticStore::default(),
            descrambler_diagnostics: BoundedDiagnosticStore::default(),
            child_open_rollback_diagnostics: BoundedDiagnosticStore::default(),
            dvr_post_commit_notification_diagnostics: BoundedDiagnosticStore::default(),
            filter_callback_delivery_diagnostics: BoundedDiagnosticStore::default(),
            callback_artifact_runtime_split_diagnostics: BoundedDiagnosticStore::default(),
            filter_event_dispatcher: None,
            callback_registry: RuntimeCallbackRegistry::default(),
            frontend_workers: FrontendWorkerRegistry::default(),
            next_aidl_generation: 0,
            next_aidl_object_id: 0,
        }
    }

    pub fn state(&self) -> ServiceState {
        self.state
    }

    pub(crate) fn registry(&self) -> &RuntimeRegistry {
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
        self.diagnostics.as_slice()
    }

    pub fn diagnostics_dropped_count(&self) -> u64 {
        self.diagnostics.dropped_count()
    }

    pub fn descrambler_diagnostics(&self) -> &[DescramblerDiagnosticRecord] {
        self.descrambler_diagnostics.as_slice()
    }

    pub fn descrambler_diagnostics_dropped_count(&self) -> u64 {
        self.descrambler_diagnostics.dropped_count()
    }

    pub fn child_open_rollback_diagnostics(&self) -> &[ChildOpenRollbackDiagnosticRecord] {
        self.child_open_rollback_diagnostics.as_slice()
    }

    pub fn child_open_rollback_diagnostics_dropped_count(&self) -> u64 {
        self.child_open_rollback_diagnostics.dropped_count()
    }

    pub fn dvr_post_commit_notification_diagnostics(
        &self,
    ) -> &[DvrPostCommitNotificationDiagnosticRecord] {
        self.dvr_post_commit_notification_diagnostics.as_slice()
    }

    pub fn dvr_post_commit_notification_diagnostics_dropped_count(&self) -> u64 {
        self.dvr_post_commit_notification_diagnostics
            .dropped_count()
    }

    pub fn filter_callback_delivery_diagnostics(
        &self,
    ) -> &[FilterCallbackDeliveryDiagnosticRecord] {
        self.filter_callback_delivery_diagnostics.as_slice()
    }

    pub fn filter_callback_delivery_diagnostics_dropped_count(&self) -> u64 {
        self.filter_callback_delivery_diagnostics.dropped_count()
    }

    pub fn callback_artifact_runtime_split_diagnostics(
        &self,
    ) -> &[CallbackArtifactRuntimeSplitDiagnosticRecord] {
        self.callback_artifact_runtime_split_diagnostics.as_slice()
    }

    pub fn callback_artifact_runtime_split_diagnostics_dropped_count(&self) -> u64 {
        self.callback_artifact_runtime_split_diagnostics.dropped_count()
    }

    #[cfg(test)]
    pub(crate) fn register_descrambler_key_slot(
        &mut self,
        token: DescramblerKeyToken,
        key_slot: DescramblerKeySlot,
    ) -> Result<(), HalError> {
        self.registry
            .descrambler_key_table_mut()
            .insert_test_key_slot(token, DescramblerKeySlotId(1), key_slot);
        Ok(())
    }

    pub(crate) fn record_child_open_rollback_diagnostic(
        &mut self,
        record: ChildOpenRollbackDiagnosticRecord,
    ) {
        self.child_open_rollback_diagnostics.push(record);
    }

    pub fn record_dvr_post_commit_notification_diagnostic(
        &mut self,
        record: DvrPostCommitNotificationDiagnosticRecord,
    ) {
        self.dvr_post_commit_notification_diagnostics.push(record);
    }

    pub fn record_filter_callback_delivery_diagnostic(
        &mut self,
        record: FilterCallbackDeliveryDiagnosticRecord,
    ) {
        self.filter_callback_delivery_diagnostics.push(record);
    }

    pub fn record_callback_artifact_runtime_split_diagnostic(
        &mut self,
        record: CallbackArtifactRuntimeSplitDiagnosticRecord,
    ) {
        self.callback_artifact_runtime_split_diagnostics.push(record);
    }

    pub fn install_filter_event_dispatcher(
        &mut self,
        dispatcher: Arc<dyn FilterEventDispatcher>,
    ) -> Result<(), HalError> {
        if self.filter_event_dispatcher.is_some() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter event dispatcher is already installed for this runtime",
            ));
        }
        self.filter_event_dispatcher = Some(FilterEventDispatcherHandle::new(dispatcher));
        Ok(())
    }

    fn filter_event_dispatcher(&self) -> Result<Arc<dyn FilterEventDispatcher>, HalError> {
        self.filter_event_dispatcher
            .as_ref()
            .map(FilterEventDispatcherHandle::dispatcher)
            .ok_or_else(|| {
                HalError::callback_failed(
                    "IFilterCallback.onFilterEvent",
                    "filter event dispatcher is not installed for this runtime",
                )
            })
    }

    fn record_descrambler_diagnostic(&mut self, record: DescramblerDiagnosticRecord) {
        self.descrambler_diagnostics.push(record);
    }


    pub fn finish_filter_child_open_artifact_retain_failure_use_case(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        filter_id: i32,
        primary_error: HalError,
    ) -> Result<(), HalError> {
        match self.rollback_filter_child_open_after_aidl_failure(object_id, generation, filter_id) {
            Ok(()) => Err(primary_error),
            Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                "filter child callback retain failure rollback failed",
                primary_error,
                cleanup_error,
            )),
        }
    }

    pub fn finish_dvr_child_open_artifact_retain_failure_use_case(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        dvr_id: i32,
        primary_error: HalError,
    ) -> Result<(), HalError> {
        match self.rollback_dvr_child_open_after_aidl_failure(object_id, generation, dvr_id) {
            Ok(()) => Err(primary_error),
            Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                "DVR child callback retain failure rollback failed",
                primary_error,
                cleanup_error,
            )),
        }
    }

    pub fn finish_filter_child_open_object_construction_failure_use_case(
        &mut self,
        primary_error: HalError,
        cleanup_result: Result<(), HalError>,
    ) -> Result<(), HalError> {
        match cleanup_result {
            Ok(()) => Err(primary_error),
            Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                "filter object construction failure cleanup failed",
                primary_error,
                cleanup_error,
            )),
        }
    }

    pub fn finish_dvr_child_open_object_construction_failure_use_case(
        &mut self,
        primary_error: HalError,
        cleanup_result: Result<(), HalError>,
    ) -> Result<(), HalError> {
        match cleanup_result {
            Ok(()) => Err(primary_error),
            Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                "DVR object construction failure cleanup failed",
                primary_error,
                cleanup_error,
            )),
        }
    }

    pub fn begin_filter_child_open_object_failure_cleanup_use_case(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        filter_id: i32,
    ) -> OwnerCallbackCleanupUseCaseOutcome<()> {
        let primary_result = self.rollback_filter_child_open_after_aidl_failure(
            owner_id,
            owner_generation,
            filter_id,
        );
        let command = OwnerCallbackCleanupArtifactCommand::new(
            AidlObjectKind::Filter,
            owner_id,
            owner_generation,
            Some(AidlApi::DemuxOpenFilter),
            "filter child callback rollback failed",
        );
        OwnerCallbackCleanupUseCaseOutcome::new(command, primary_result)
    }

    pub fn begin_dvr_child_open_object_failure_cleanup_use_case(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        dvr_id: i32,
    ) -> OwnerCallbackCleanupUseCaseOutcome<()> {
        let primary_result = self.rollback_dvr_child_open_after_aidl_failure(
            owner_id,
            owner_generation,
            dvr_id,
        );
        let command = OwnerCallbackCleanupArtifactCommand::new(
            AidlObjectKind::Dvr,
            owner_id,
            owner_generation,
            Some(AidlApi::DemuxOpenDvr),
            "DVR child callback rollback failed",
        );
        OwnerCallbackCleanupUseCaseOutcome::new(command, primary_result)
    }

    pub fn execute_callback_unregistration_for_object_use_case(
        &mut self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
        dispatch: ObjectMethodExecutionToken,
    ) -> OwnerCallbackCleanupUseCaseOutcome<()> {
        let primary_result = match (owner_kind, registration_api) {
            (AidlObjectKind::Frontend, AidlApi::FrontendSetCallback) => self
                .clear_frontend_callback_registration_for_object(
                    owner_id,
                    owner_generation,
                    dispatch,
                ),
            (AidlObjectKind::Lnb, AidlApi::LnbSetCallback) => self
                .clear_lnb_callback_registration_for_object(
                    owner_id,
                    owner_generation,
                    dispatch,
                ),
            _ => Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                format!(
                    "unsupported callback unregistration target: {:?}/{:?}",
                    owner_kind, registration_api
                ),
            )),
        };
        let command = self.plan_owner_callback_cleanup_artifact_command(
            owner_kind,
            owner_id,
            owner_generation,
            Some(registration_api),
            "callback artifact unregister failed after domain unregister",
        );
        OwnerCallbackCleanupUseCaseOutcome::new(command, primary_result)
    }

    pub fn execute_callback_registration_after_artifact_result_for_object_use_case(
        &mut self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
        artifact_retain_result: Result<(), HalError>,
        dispatch: ObjectMethodExecutionToken,
    ) -> CallbackRegistrationArtifactOutcome {
        let mut rollback_command = None;
        let primary_result = match artifact_retain_result {
            Err(error) => Err(error),
            Ok(()) => {
                self.record_callback_registration_for_object(
                    owner_kind,
                    owner_id,
                    owner_generation,
                    registration_api,
                );
                let commit_result = match (owner_kind, registration_api) {
                    (AidlObjectKind::Frontend, AidlApi::FrontendSetCallback) => self
                        .commit_frontend_callback_registration_for_object(
                            owner_id,
                            owner_generation,
                            dispatch,
                        ),
                    (AidlObjectKind::Lnb, AidlApi::LnbSetCallback) => self
                        .commit_lnb_callback_registration_for_object(
                            owner_id,
                            owner_generation,
                            dispatch,
                        ),
                    _ => Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        format!(
                            "unsupported callback registration target: {:?}/{:?}",
                            owner_kind, registration_api
                        ),
                    )),
                };
                if commit_result.is_err() {
                    rollback_command = Some(self.plan_owner_callback_cleanup_artifact_command(
                        owner_kind,
                        owner_id,
                        owner_generation,
                        Some(registration_api),
                        "callback artifact rollback failed after domain registration failure",
                    ));
                }
                commit_result
            }
        };
        CallbackRegistrationArtifactOutcome::new(rollback_command, primary_result)
    }

    pub fn record_callback_artifact_after_owner_ready_use_case(
        &mut self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
        artifact_retain_result: Result<(), HalError>,
    ) -> CallbackRegistrationArtifactOutcome {
        let mut rollback_command = None;
        let primary_result = match artifact_retain_result {
            Err(error) => Err(error),
            Ok(()) => {
                let record_result = aidl_object_live(self, owner_id, owner_generation, owner_kind)
                    .map(|_| {
                        self.record_callback_registration_for_object(
                            owner_kind,
                            owner_id,
                            owner_generation,
                            registration_api,
                        );
                    });
                if record_result.is_err() {
                    rollback_command = Some(self.plan_owner_callback_cleanup_artifact_command(
                        owner_kind,
                        owner_id,
                        owner_generation,
                        Some(registration_api),
                        "callback artifact rollback failed before runtime registration",
                    ));
                }
                record_result
            }
        };
        CallbackRegistrationArtifactOutcome::new(rollback_command, primary_result)
    }

    pub fn finish_owner_callback_cleanup_outcome<T>(
        &mut self,
        outcome: OwnerCallbackCleanupUseCaseOutcome<T>,
        artifact_cleanup_result: Result<(), HalError>,
    ) -> Result<T, HalError> {
        let (command, primary_result) = outcome.into_parts();
        self.finish_owner_callback_cleanup_use_case(command, primary_result, artifact_cleanup_result)
    }

    pub fn finish_object_close_callback_cleanup_outcome(
        &mut self,
        command: OwnerCallbackCleanupArtifactCommand,
        artifact_cleanup_result: Result<(), HalError>,
    ) -> Result<(), HalError> {
        self.finish_owner_callback_cleanup_use_case_with_phase(
            CallbackArtifactRuntimeSplitPhase::ObjectCloseCleanupFinish,
            command,
            Ok(()),
            artifact_cleanup_result,
        )
    }

    pub fn finish_callback_registration_after_artifact_result_use_case(
        &mut self,
        outcome: CallbackRegistrationArtifactOutcome,
        rollback_result: Option<Result<(), HalError>>,
    ) -> Result<(), HalError> {
        let (rollback_command, primary_result) = outcome.into_parts();
        match rollback_command {
            Some(command) => {
                let cleanup_result = rollback_result.unwrap_or_else(|| {
                    Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "callback registration rollback command was not executed by AIDL artifact bridge",
                    ))
                });
                self.finish_owner_callback_cleanup_use_case_with_phase(
                    CallbackArtifactRuntimeSplitPhase::RegistrationRollbackFinish,
                    command,
                    primary_result,
                    cleanup_result,
                )
            }
            None => primary_result,
        }
    }

    fn record_callback_registration_for_object(
        &mut self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) {
        self.callback_registry.record_registration(
            owner_kind,
            owner_id,
            owner_generation,
            registration_api,
        );
    }

    fn mark_callback_registration_unhealthy(
        &mut self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) -> CallbackRegistryUpdate {
        self.callback_registry.mark_unhealthy(
            owner_kind,
            owner_id,
            owner_generation,
            registration_api,
        )
    }

    pub(crate) fn mark_frontend_callback_delivery_failed_use_case(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
    ) -> Result<(), HalError> {
        match self.mark_callback_registration_unhealthy(
            AidlObjectKind::Frontend,
            owner_id,
            owner_generation,
            AidlApi::FrontendSetCallback,
        ) {
            CallbackRegistryUpdate::Updated => Ok(()),
            CallbackRegistryUpdate::Missing => Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend callback registry entry missing while marking unhealthy",
            )),
        }
    }

    pub(crate) fn mark_filter_callback_delivery_failed_use_case(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
    ) -> Result<(), HalError> {
        let mut first_error = None;
        if self.mark_callback_registration_unhealthy(
            AidlObjectKind::Filter,
            owner_id,
            owner_generation,
            AidlApi::DemuxOpenFilter,
        ) == CallbackRegistryUpdate::Missing
        {
            let error = HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter callback registry entry missing while marking unhealthy",
            );
            self.record_filter_callback_delivery_diagnostic(
                FilterCallbackDeliveryDiagnosticRecord::new(
                    FilterCallbackDeliveryDiagnosticPhase::CallbackRegistryAccounting,
                    owner_id,
                    owner_generation,
                    error.clone(),
                ),
            );
            first_error = Some(error);
        }
        if let Err(error) = self
            .mark_filter_callback_unhealthy_for_object(owner_id, owner_generation)
        {
            self.record_filter_callback_delivery_diagnostic(
                FilterCallbackDeliveryDiagnosticRecord::new(
                    FilterCallbackDeliveryDiagnosticPhase::RuntimeCallbackAccounting,
                    owner_id,
                    owner_generation,
                    error.clone(),
                ),
            );
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) fn mark_dvr_callback_delivery_failed_use_case(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
    ) -> Result<(), HalError> {
        let mut first_error = None;
        if self.mark_callback_registration_unhealthy(
            AidlObjectKind::Dvr,
            owner_id,
            owner_generation,
            AidlApi::DemuxOpenDvr,
        ) == CallbackRegistryUpdate::Missing
        {
            first_error = Some(HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR callback registry entry missing while marking unhealthy",
            ));
        }
        if let Err(error) = self
            .mark_dvr_callback_unhealthy_for_object(owner_id, owner_generation)
        {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn finish_callback_delivery_failure_use_case(
        &mut self,
        report: CallbackDeliveryFailureReport,
    ) -> Result<(), HalError> {
        let primary = report.primary.clone();
        let mut failures = FirstErrorCollector::new();
        match report.owner_kind {
            CallbackDeliveryOwnerKind::Filter => {
                self.record_filter_callback_delivery_diagnostic(
                    FilterCallbackDeliveryDiagnosticRecord::new(
                        report.filter_diagnostic_phase(),
                        report.owner_id,
                        report.owner_generation,
                        primary.clone(),
                    ),
                );
                if report.phase != CallbackDeliveryFailurePhase::CallbackArtifactLookup {
                    if let Err(error) = self.mark_filter_callback_delivery_failed_use_case(
                        report.owner_id,
                        report.owner_generation,
                    ) {
                        failures.push_error(error);
                    }
                }
            }
            CallbackDeliveryOwnerKind::Dvr => {
                let phase = report
                    .dvr_post_commit_phase
                    .unwrap_or(DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure);
                self.record_dvr_post_commit_notification_diagnostic(
                    DvrPostCommitNotificationDiagnosticRecord::new(
                        phase,
                        report.owner_id,
                        report.owner_generation,
                        primary.clone(),
                    ),
                );
                if report.phase != CallbackDeliveryFailurePhase::CallbackArtifactLookup {
                    if let Err(error) = self.mark_dvr_callback_delivery_failed_use_case(
                        report.owner_id,
                        report.owner_generation,
                    ) {
                        self.record_dvr_post_commit_notification_diagnostic(
                            DvrPostCommitNotificationDiagnosticRecord::new(
                                phase,
                                report.owner_id,
                                report.owner_generation,
                                error.clone(),
                            ),
                        );
                        failures.push_error(error);
                    }
                }
            }
            CallbackDeliveryOwnerKind::Frontend => {
                if report.phase != CallbackDeliveryFailurePhase::CallbackArtifactLookup {
                    let Some((frontend_id, scan_generation)) = report.frontend_scan_context else {
                        failures.push_error(HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "frontend callback delivery failure report missing scan context",
                        ));
                        return match failures.into_result() {
                            Ok(()) => Err(primary),
                            Err(cleanup) => Err(compose_primary_cleanup_failure(
                                "frontend callback delivery failure accounting failed",
                                primary,
                                cleanup,
                            )),
                        };
                    };
                    if let Err(error) = self.mark_frontend_scan_session_callback_failed(
                        frontend_id,
                        scan_generation,
                    ) {
                        failures.push_error(error);
                    }
                    if let Err(error) = self.mark_frontend_callback_delivery_failed_use_case(
                        report.owner_id,
                        report.owner_generation,
                    ) {
                        failures.push_error(error);
                    }
                }
            }
        }

        match failures.into_result() {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(compose_primary_cleanup_failure(
                "callback delivery failure accounting failed",
                primary,
                cleanup,
            )),
        }
    }

    pub(crate) fn plan_owner_callback_cleanup_artifact_command(
        &self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: Option<AidlApi>,
        cleanup_failure_message: &'static str,
    ) -> OwnerCallbackCleanupArtifactCommand {
        OwnerCallbackCleanupArtifactCommand {
            owner_kind,
            owner_id,
            owner_generation,
            registration_api,
            cleanup_failure_message,
        }
    }

    pub fn finish_owner_callback_cleanup_use_case<T>(
        &mut self,
        command: OwnerCallbackCleanupArtifactCommand,
        primary_result: Result<T, HalError>,
        artifact_cleanup_result: Result<(), HalError>,
    ) -> Result<T, HalError> {
        self.finish_owner_callback_cleanup_use_case_with_phase(
            CallbackArtifactRuntimeSplitPhase::OwnerCleanupFinish,
            command,
            primary_result,
            artifact_cleanup_result,
        )
    }

    fn finish_owner_callback_cleanup_use_case_with_phase<T>(
        &mut self,
        phase: CallbackArtifactRuntimeSplitPhase,
        command: OwnerCallbackCleanupArtifactCommand,
        primary_result: Result<T, HalError>,
        artifact_cleanup_result: Result<(), HalError>,
    ) -> Result<T, HalError> {
        let artifact_error = artifact_cleanup_result.err();

        let value = match (primary_result, artifact_error.clone()) {
            (Ok(value), None) => Some(value),
            (Ok(_), Some(cleanup_error)) => {
                if let Some(outcome) =
                    CallbackArtifactRuntimeSplitOutcome::from_results(Some(cleanup_error.clone()), None)
                {
                    self.record_callback_artifact_runtime_split_diagnostic(
                        CallbackArtifactRuntimeSplitDiagnosticRecord::owner(
                            phase,
                            command.owner_kind,
                            command.owner_id,
                            command.owner_generation,
                            outcome,
                        ),
                    );
                }
                self.mark_owner_callback_cleanup_failed(&command);
                return Err(cleanup_error);
            }
            (Err(primary_error), None) => {
                self.mark_owner_callback_cleanup_failed(&command);
                return Err(primary_error);
            }
            (Err(primary_error), Some(cleanup_error)) => {
                if let Some(outcome) =
                    CallbackArtifactRuntimeSplitOutcome::from_results(Some(cleanup_error.clone()), None)
                {
                    self.record_callback_artifact_runtime_split_diagnostic(
                        CallbackArtifactRuntimeSplitDiagnosticRecord::owner(
                            phase,
                            command.owner_kind,
                            command.owner_id,
                            command.owner_generation,
                            outcome,
                        ),
                    );
                }
                self.mark_owner_callback_cleanup_failed(&command);
                return Err(compose_primary_cleanup_failure(
                    command.cleanup_failure_message,
                    primary_error,
                    cleanup_error,
                ));
            }
        };

        match self
            .callback_registry
            .clear_owner(command.owner_id, command.owner_generation)
        {
            CallbackRegistryUpdate::Updated => Ok(value.expect("value is present for successful cleanup")),
            CallbackRegistryUpdate::Missing => Ok(value.expect("value is present for successful cleanup")),
        }
    }

    fn mark_owner_callback_cleanup_failed(&mut self, command: &OwnerCallbackCleanupArtifactCommand) {
        match command.registration_api {
            Some(api) => {
                self.callback_registry.mark_unhealthy(
                    command.owner_kind,
                    command.owner_id,
                    command.owner_generation,
                    api,
                );
            }
            None => {
                self.callback_registry
                    .mark_owner_unhealthy(command.owner_id, command.owner_generation);
            }
        }
    }

    pub(crate) fn object_table(&self) -> &RuntimeObjectTable {
        &self.object_table
    }

    pub(crate) fn object_table_mut(&mut self) -> &mut RuntimeObjectTable {
        &mut self.object_table
    }

    pub fn aidl_object_lifecycle(&self, object_id: AidlObjectId) -> Option<RuntimeObjectLifecycle> {
        self.object_table
            .entry(object_id)
            .map(|entry| entry.lifecycle)
    }

    pub fn callback_registration_count(&self) -> usize {
        self.callback_registry.registration_count()
    }

    pub fn callback_registration_health(
        &self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) -> Option<CallbackHealthState> {
        self.callback_registry
            .registration_for(owner_kind, owner_id, owner_generation, registration_api)
            .map(|registration| registration.health)
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
        self.descrambler_diagnostics.clear();
        self.child_open_rollback_diagnostics.clear();
        self.dvr_post_commit_notification_diagnostics.clear();
        self.filter_callback_delivery_diagnostics.clear();
        self.callback_artifact_runtime_split_diagnostics.clear();
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

    fn public_runtime_unregister_id(entry: &RuntimeObjectEntry) -> Result<i32, HalError> {
        i32::try_from(entry.ledger_id.0).map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!(
                    "public runtime id is outside i32 range during close cleanup: kind={:?}",
                    entry.object_kind
                ),
            )
        })
    }

    fn validate_public_runtime_for_terminal_aidl_entry(
        &self,
        entry: &RuntimeObjectEntry,
        context: &'static str,
        missing_detail: &'static str,
    ) -> Result<(), HalError> {
        let id = Self::public_runtime_unregister_id(entry)?;
        let exists = match entry.object_kind {
            AidlObjectKind::Demux => {
                self.registry.demux(DemuxRuntimeId(id)).is_some()
                    && self.registry.demux_runtime(DemuxRuntimeId(id)).is_some()
            }
            AidlObjectKind::Filter => self
                .registry
                .filter(FilterRuntimeId(id))
                .and_then(|entry| {
                    self.registry
                        .demux_runtime(DemuxRuntimeId(entry.owner_demux_id))
                        .map(|demux| demux.filter(id).is_some())
                })
                .unwrap_or(false),
            AidlObjectKind::Dvr => self
                .registry
                .dvr(DvrRuntimeId(id))
                .and_then(|entry| {
                    self.registry
                        .demux_runtime(DemuxRuntimeId(entry.owner_demux_id))
                        .map(|demux| demux.dvr(id).is_some())
                })
                .unwrap_or(false),
            AidlObjectKind::Descrambler => {
                self.registry
                    .descrambler(DescramblerRuntimeId(id))
                    .is_some()
                    && self
                        .registry
                        .descrambler_runtime_exists(DescramblerRuntimeId(id))
            }
            _ => {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    format!(
                        "object kind does not own a public runtime unregister entry in this cleanup path: kind={:?} id={id}",
                        entry.object_kind
                    ),
                ));
            }
        };
        if exists {
            Ok(())
        } else {
            Err(HalError::cleanup_failed(
                context,
                format!("{missing_detail}: kind={:?} id={id}", entry.object_kind),
            ))
        }
    }

    pub fn validate_public_runtime_for_closed_aidl_entry(
        &self,
        entry: &RuntimeObjectEntry,
    ) -> Result<(), HalError> {
        self.validate_public_runtime_for_terminal_aidl_entry(
            entry,
            "public runtime unregister preflight after AIDL object close",
            "runtime entry missing before close cleanup commit",
        )
    }

    pub fn validate_public_runtime_for_drop_leak_aidl_entry(
        &self,
        entry: &RuntimeObjectEntry,
    ) -> Result<(), HalError> {
        self.validate_public_runtime_for_terminal_aidl_entry(
            entry,
            "public runtime unregister preflight after Drop leak quarantine",
            "runtime entry missing before Drop leak runtime unregister",
        )
    }

    fn unregister_public_runtime_for_terminal_aidl_entry(
        &mut self,
        entry: &RuntimeObjectEntry,
        context: &'static str,
        missing_detail: &'static str,
    ) -> Result<(), HalError> {
        let id = Self::public_runtime_unregister_id(entry)?;
        let removed = match entry.object_kind {
            AidlObjectKind::Demux => self.unregister_demux_runtime(id)?.is_some(),
            AidlObjectKind::Filter => self.unregister_filter_runtime(id)?.is_some(),
            AidlObjectKind::Dvr => self.unregister_dvr_runtime(id)?.is_some(),
            AidlObjectKind::Descrambler => self.unregister_descrambler_runtime(id)?.is_some(),
            _ => {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    format!(
                        "object kind does not own a public runtime unregister entry in this cleanup path: kind={:?} id={id}",
                        entry.object_kind
                    ),
                ));
            }
        };
        if removed {
            Ok(())
        } else {
            Err(HalError::cleanup_failed(
                context,
                format!("{missing_detail}: kind={:?} id={id}", entry.object_kind),
            ))
        }
    }

    pub fn unregister_public_runtime_for_closed_aidl_entry(
        &mut self,
        entry: &RuntimeObjectEntry,
    ) -> Result<(), HalError> {
        self.unregister_public_runtime_for_terminal_aidl_entry(
            entry,
            "public runtime unregister after AIDL object close",
            "runtime entry missing during close cleanup",
        )
    }

    pub fn unregister_public_runtime_for_drop_leak_aidl_entry(
        &mut self,
        entry: &RuntimeObjectEntry,
    ) -> Result<(), HalError> {
        self.unregister_public_runtime_for_terminal_aidl_entry(
            entry,
            "public runtime unregister after Drop leak quarantine",
            "runtime entry missing during Drop leak terminalization",
        )
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
