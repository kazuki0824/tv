use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_common::{
    FrontendBackendKind, FrontendDevicePath, FrontendSystem, FrontendTuneRequest, HalError,
    HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind, TS_PACKET_SIZE,
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
#[cfg(test)]
use maleicacid_tuner_hal2_descrambler::DescramblerKeyRegistrationError;
use maleicacid_tuner_hal2_descrambler::{
    add_pid_claim_with_session_txn, bind_demux_with_session_txn, cleanup_all_with_session_txn,
    descramble_ts_packet_in_place, packet_policy_for_descramble_failure,
    remove_pid_claim_with_session_txn, DescrambleFailure, DescrambleOutcome,
    DescramblerClearKeyTxnError, DescramblerKeyLookupError, DescramblerKeySlot,
    DescramblerKeyToken, DescramblerKeyTokenError, DescramblerPidClaim, DescramblerPidClaimError,
    DescramblerReplaceKeyOutcome, DescramblerReplaceKeyTxnError, DescramblerSessionFailureKind,
    PacketPolicyAction,
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
use crate::diagnostics::{
    BoundedDiagnosticStore, CapabilitySuppressionReason, ChildOpenRollbackDiagnosticRecord,
    DescramblerDiagnosticKind, DescramblerDiagnosticPhase, DescramblerDiagnosticRecord,
    DvrPostCommitNotificationDiagnosticRecord, FilterCallbackDeliveryDiagnosticRecord,
    StartupDiagnosticRecord,
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
pub use query_api::{
    DvrStatusPollSnapshot, RuntimeObjectPublicEntry, RuntimeObjectQueryError, RuntimeQuery,
};
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
    pids: BTreeSet<u16>,
    key_slot: Option<DescramblerKeySlot>,
    source_filter_ids_by_pid: BTreeMap<u16, BTreeSet<i32>>,
}

impl ActiveDescramblerSnapshot {
    fn targets_pid(&self, pid: u16) -> bool {
        self.pids.contains(&pid)
    }

    fn source_filter_ids_for_pid(&self, pid: u16) -> Option<&BTreeSet<i32>> {
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
    filter_event_dispatcher: Option<FilterEventDispatcherHandle>,
    callback_registry: RuntimeCallbackRegistry,
    frontend_workers: FrontendWorkerRegistry,
    next_aidl_generation: u64,
    next_aidl_object_id: i64,
}

impl TunerServiceRuntime {
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
                            stream_id: pid.get(),
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

    #[cfg(test)]
    pub(crate) fn register_descrambler_key_slot(
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

    pub fn record_callback_registration_for_object(
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

    pub fn mark_callback_registration_unhealthy(
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

    pub fn mark_callback_registration_owner_unhealthy(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
    ) -> CallbackRegistryUpdate {
        self.callback_registry
            .mark_owner_unhealthy(owner_id, owner_generation)
    }

    pub fn clear_callback_registration_owner(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
    ) -> CallbackRegistryUpdate {
        self.callback_registry
            .clear_owner(owner_id, owner_generation)
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
                        .descrambler_runtime(DescramblerRuntimeId(id))
                        .is_some()
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
