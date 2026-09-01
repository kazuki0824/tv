use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::descrambler_key_table::DescramblerKeyLookupError;
#[cfg(test)]
use crate::descrambler_key_table::DescramblerKeySlotId;
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FirstErrorCollector, FrontendBackendKind, FrontendDevicePath,
    FrontendSystem, FrontendTuneRequest, HalError, HalInternalKind, HalInvalidArgumentKind,
    HalInvalidStateKind, TS_PACKET_SIZE,
};
use maleicacid_tuner_hal2_demux::config::{
    AvStreamKind, AvStreamTypeConfig, FilterConfig, FilterDelayHint, FilterOpenType,
};
use maleicacid_tuner_hal2_demux::OpenFilterRequest;
use maleicacid_tuner_hal2_demux::{
    AvDataId, AvMediaEventDescriptor, AvSharedBacking, DemuxRuntimeError, DemuxRuntimeErrorKind,
    DemuxRuntimeRollbackToken, DemuxRuntimeState, DvrKind, DvrRuntimeState, PipelineBoundaryReason,
    PipelineDiagnostic, PipelineReport, PipelineResetReport, StreamBoundaryReport, TsInputOrigin,
    TsPacketValidationError, ValidatedTsPacket,
};
#[cfg(test)]
use maleicacid_tuner_hal2_descrambler::DescramblerKeySlot;
use maleicacid_tuner_hal2_descrambler::{
    DescrambleFailure, DescramblerKeyToken, DescramblerKeyTokenError, DescramblerPid,
    DescramblerPidClaim, DescramblerPidClaimError,
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

use crate::callback_registry::{CallbackRegistryUpdate, RuntimeCallbackRegistry};
use crate::capability_snapshot::{CapabilitySnapshot, CapacityLedger};
use crate::command_dispatch::{
    RuntimeCommandDispatchError, RuntimeCommandDispatchPlan, RuntimeCommandDispatcher,
};
use crate::descrambler_session::{
    DescramblerCleanupTxnError, DescramblerClearKeyOutcome, DescramblerClearKeyTxnError,
    DescramblerReplaceKeyOutcome, DescramblerReplaceKeyTxnError, DescramblerSessionFailureKind,
};
use crate::diagnostics::{
    BoundedDiagnosticStore, CallbackArtifactRuntimeSplitDiagnosticRecord,
    CallbackArtifactRuntimeSplitDiagnosticSnapshot, CallbackArtifactRuntimeSplitOutcome,
    CallbackArtifactRuntimeSplitPhase, CapabilitySuppressionReason,
    ChildOpenRollbackDiagnosticRecord, ChildOpenRollbackDiagnosticSnapshot,
    DemuxTransactionDiagnosticId, DemuxTransactionDiagnosticRecord,
    DemuxTransactionDiagnosticSnapshot, DescramblerDiagnosticKind, DescramblerDiagnosticPhase,
    DescramblerDiagnosticRecord, DescramblerDiagnosticSnapshot,
    DvrPostCommitNotificationDiagnosticRecord, DvrPostCommitNotificationDiagnosticSnapshot,
    DvrPostCommitNotificationFailureKind, DvrPostCommitNotificationPhase,
    DvrStatusNotifierCleanupDiagnosticSnapshot, FilterCallbackDeliveryDiagnosticPhase,
    FilterCallbackDeliveryDiagnosticRecord, FilterCallbackDeliveryDiagnosticSnapshot,
    FrontendCallbackDeliveryDiagnosticPhase, FrontendCallbackDeliveryDiagnosticRecord,
    FrontendCallbackDeliveryDiagnosticSnapshot, QueueDescriptorQueryDiagnosticRecord,
    QueueDescriptorQueryDiagnosticSnapshot, SharedCallbackArtifactRuntimeSplitDiagnostics,
    SharedDvrPostCommitNotificationDiagnostics, SharedDvrStatusNotifierCleanupDiagnostics,
    StartupDiagnosticRecord, StartupDiagnosticSnapshot,
};
use crate::dispatch::{
    adapter_transactions_are_covered, dispatch_target_for, ServiceRuntimeDispatchTarget,
};
use crate::frontend_worker_txn::{
    FrontendWorkerCleanupDiagnosticSnapshot, SharedFrontendWorkerCleanupDiagnostics,
};
use crate::object_close_txn::{
    ObjectCleanupDiagnosticRecord, ObjectCleanupDiagnosticSnapshot, SharedObjectCleanupDiagnostics,
};
use crate::object_lifecycle::aidl_object_live;
use crate::object_method_use_case::ObjectMethodExecutionToken;
use crate::object_table::{
    RuntimeObjectEntry, RuntimeObjectLifecycle, RuntimeObjectTable, RuntimeObjectTableError,
    RuntimeOwnerRelation,
};
use crate::registry::{
    DemuxRuntimeId, DescramblerRuntimeId, DvrRuntimeId, FilterRuntimeId, FrontendRegistryEntry,
    FrontendRuntimeId, LnbRegistryEntry, LnbRegistryProfile, LnbRuntimeId, RegistryCommitError,
    RuntimeRegistry, SatellitePowerTopology,
};
use crate::ServiceState;
use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};

// operation実装はboot child moduleに置き、field visibilityを広げず
// TunerServiceRuntimeのprivate stateを使用できるようにする。
mod query_api;
pub use query_api::DvrStatusPollSnapshot;
pub(crate) use query_api::{
    map_queue_descriptor_query_error, QueueDescriptorExportPlan, RuntimeQuery,
};
mod child_open_context;
pub(crate) use child_open_context::{
    attach_diagnostic_detail_to_public_error, format_dvr_queue_cleanup_report,
    format_filter_runtime_operation_report,
};
mod demux_filter_dvr_ops;
pub use demux_filter_dvr_ops::ChildOpenTxn;
mod descrambler_txn;
mod frontend_txn;
pub(crate) mod lnb_txn;
mod packet_ops;

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
        satellite_power_topology: SatellitePowerTopology,
        capability: crate::registry::FrontendCapabilitySnapshot,
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

fn frontend_capability_is_consistent(
    backend: FrontendBackendKind,
    system: FrontendSystem,
    capability: crate::registry::FrontendCapabilitySnapshot,
) -> bool {
    let scalar = capability.scalar;
    let expected_tag = match backend {
        FrontendBackendKind::Px4CharDevice => 0x1000_0000,
        FrontendBackendKind::LinuxDvb => 0x2000_0000,
    };
    if capability.exclusive_group_id < 0
        || (capability.exclusive_group_id & 0x7000_0000) != expected_tag
        || scalar.min_frequency_hz <= 0
        || scalar.max_frequency_hz < scalar.min_frequency_hz
        || scalar.min_symbol_rate < 0
        || scalar.max_symbol_rate < scalar.min_symbol_rate
        || scalar.acquire_range_hz < 0
    {
        return false;
    }
    match system {
        FrontendSystem::IsdbT => capability
            .isdbt_segment
            .is_some_and(|segment| segment.is_segment_auto),
        FrontendSystem::IsdbS => capability.isdbt_segment.is_none() && scalar.max_symbol_rate > 0,
        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => false,
    }
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
    let profile = match entry.lnb_profile? {
        // ExternalOrShared is product wiring evidence for keeping the
        // satellite frontend powered.  It is not evidence for any
        // caller-controllable ILnb operation, so it must not create a public
        // LNB endpoint.
        LnbRegistryProfile::NoPower => return None,
        profile => profile,
    };
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterEventDeliverySnapshot {
    pub object_id: AidlObjectId,
    pub generation: AidlObjectGeneration,
    pub filter_id: i32,
    pub event: FilterEventDelivery,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilterEventDelivery {
    StartId(i32),
    Status(maleicacid_tuner_hal2_demux::FilterStatusEvent),
    Media(AvMediaEventDescriptor),
    Section { data_length: usize },
    Pes { stream_id: i32, data_length: usize },
    RecordIndex(maleicacid_tuner_hal2_demux::TsRecordEventData),
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
    error: maleicacid_tuner_hal2_demux::DemuxRuntimeError,
) -> HalError {
    match error.kind {
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::GenerationExhausted => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtime generation exhausted",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::FilterMissing
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::DvrMissing
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueMissing => {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtime object is missing",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidState
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidDvrFilter
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SourceLifecycle
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SinkLifecycle => {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtime lifecycle is invalid",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidSourceSubtype
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::InvalidSinkSubtype => {
            HalError::Unsupported("demux source/sink subtype is unsupported")
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::UnsupportedDvrOperation => {
            HalError::Unsupported("DVR operation is unavailable for this DVR kind")
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::PidMismatch => {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "demux source/sink PID mismatch",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SelfReference => {
            HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "a filter cannot use itself as its data source",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::PipelineFailed
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::RelationCommitUnknown => {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "demux runtime pipeline or relation operation failed",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::SourceBoundaryRollbackFailed => {
            HalError::cleanup_failed(
                "demux source boundary rollback",
                "demux runtime was quarantined after source boundary rollback failure",
            )
        }
        maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::QueueRuntimeFailure
        | maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::AvBackingFailure => {
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
        DescramblerSessionFailureKind::DemuxAlreadyBound => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "descrambler demux source is already bound",
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
    capability_snapshot: CapabilitySnapshot,
    capacity_ledger: CapacityLedger,
    release_only_filter_av_backings: BTreeMap<i32, AvSharedBacking>,
    release_only_filter_types: BTreeMap<i32, FilterOpenType>,
    released_filter_av_shared_handle_leases:
        BTreeMap<i32, maleicacid_tuner_hal2_demux::AvFileIdentity>,
    registry: RuntimeRegistry,
    object_table: RuntimeObjectTable,
    diagnostics: BoundedDiagnosticStore<StartupDiagnosticRecord>,
    descrambler_diagnostics: BoundedDiagnosticStore<DescramblerDiagnosticRecord>,
    child_open_rollback_diagnostics: BoundedDiagnosticStore<ChildOpenRollbackDiagnosticRecord>,
    dvr_post_commit_notification_diagnostics: SharedDvrPostCommitNotificationDiagnostics,
    dvr_status_notifier_cleanup_diagnostics: SharedDvrStatusNotifierCleanupDiagnostics,
    queue_descriptor_query_diagnostics:
        BoundedDiagnosticStore<QueueDescriptorQueryDiagnosticRecord>,
    filter_callback_delivery_diagnostics:
        BoundedDiagnosticStore<FilterCallbackDeliveryDiagnosticRecord>,
    frontend_callback_delivery_diagnostics:
        BoundedDiagnosticStore<FrontendCallbackDeliveryDiagnosticRecord>,
    demux_transaction_diagnostics: BoundedDiagnosticStore<DemuxTransactionDiagnosticRecord>,
    object_cleanup_diagnostics: SharedObjectCleanupDiagnostics,
    frontend_worker_cleanup_diagnostics: SharedFrontendWorkerCleanupDiagnostics,
    next_demux_transaction_diagnostic_id: u64,
    demux_transaction_diagnostic_id_saturation_reported: bool,
    callback_artifact_runtime_split_diagnostics: SharedCallbackArtifactRuntimeSplitDiagnostics,
    filter_event_dispatcher: Option<FilterEventDispatcherHandle>,
    callback_registry: RuntimeCallbackRegistry,
    frontend_workers: FrontendWorkerRegistry,
    playback_consume_txns: BTreeMap<i32, crate::playback_consume_txn::PlaybackConsumeTxn>,
    frontend_worker_reaper: Option<crate::frontend_worker_txn::FrontendWorkerReaperHandle>,
    frontend_current_max: BTreeMap<FrontendSystem, i32>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackArtifactCleanupResult {
    Cleared,
    NoArtifact,
}

#[derive(Debug)]
pub struct OwnerCallbackCleanupUseCaseOutcome<T> {
    command: OwnerCallbackCleanupArtifactCommand,
    primary_result: Result<T, HalError>,
}

impl<T> OwnerCallbackCleanupUseCaseOutcome<T> {
    fn new(
        command: OwnerCallbackCleanupArtifactCommand,
        primary_result: Result<T, HalError>,
    ) -> Self {
        Self {
            command,
            primary_result,
        }
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
    owner_kind: AidlObjectKind,
    owner_id: AidlObjectId,
    owner_generation: AidlObjectGeneration,
    registration_api: AidlApi,
    rollback_command: Option<OwnerCallbackCleanupArtifactCommand>,
    primary_result: Result<(), HalError>,
    prepared_artifact: bool,
}

impl CallbackRegistrationArtifactOutcome {
    fn new(
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
        rollback_command: Option<OwnerCallbackCleanupArtifactCommand>,
        primary_result: Result<(), HalError>,
        prepared_artifact: bool,
    ) -> Self {
        Self {
            owner_kind,
            owner_id,
            owner_generation,
            registration_api,
            rollback_command,
            primary_result,
            prepared_artifact,
        }
    }

    pub fn artifact_key(&self) -> (AidlObjectKind, AidlObjectId, AidlObjectGeneration, AidlApi) {
        (
            self.owner_kind,
            self.owner_id,
            self.owner_generation,
            self.registration_api,
        )
    }

    pub const fn uses_prepared_artifact(&self) -> bool {
        self.prepared_artifact
    }

    pub fn rollback_command(&self) -> Option<&OwnerCallbackCleanupArtifactCommand> {
        self.rollback_command.as_ref()
    }

    pub fn finish_lock_failure_command(&self) -> OwnerCallbackCleanupArtifactCommand {
        OwnerCallbackCleanupArtifactCommand {
            owner_kind: self.owner_kind,
            owner_id: self.owner_id,
            owner_generation: self.owner_generation,
            registration_api: Some(self.registration_api),
            cleanup_failure_message:
                "callback artifact rollback failed after runtime registration finish lock failure",
        }
    }

    pub fn requires_runtime_finish(&self) -> bool {
        self.rollback_command.is_some() || self.primary_result.is_err()
    }

    pub fn primary_error(&self) -> Option<&HalError> {
        self.primary_result.as_ref().err()
    }

    pub fn into_primary_result(self) -> Result<(), HalError> {
        self.primary_result
    }

    fn into_parts(
        self,
    ) -> (
        OwnerCallbackCleanupArtifactCommand,
        Option<OwnerCallbackCleanupArtifactCommand>,
        Result<(), HalError>,
    ) {
        (
            self.finish_lock_failure_command(),
            self.rollback_command,
            self.primary_result,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackDeliveryFailurePhase {
    CallbackArtifactLookup,
    RuntimePolicySkip,
    EventConversion,
    BinderDelivery,
    ScanEndDelivery,
    PostCommitNotification,
    NotifierTerminal,
    NotifierCleanup,
    NotifierPreflight,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallbackDeliveryFailureReport {
    Filter {
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        phase: CallbackDeliveryFailurePhase,
        primary: HalError,
    },
    Dvr {
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        phase: CallbackDeliveryFailurePhase,
        dvr_post_commit_phase: DvrPostCommitNotificationPhase,
        primary: HalError,
    },
    FrontendEvent {
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        frontend_id: i32,
        frontend_generation: u64,
        phase: CallbackDeliveryFailurePhase,
        primary: HalError,
    },
    FrontendScanEnd {
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        frontend_id: i32,
        scan_generation: u64,
        phase: CallbackDeliveryFailurePhase,
        primary: HalError,
    },
}

impl CallbackDeliveryFailureReport {
    pub fn filter(
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        phase: CallbackDeliveryFailurePhase,
        primary: HalError,
    ) -> Self {
        Self::Filter {
            owner_id,
            owner_generation,
            phase,
            primary,
        }
    }

    pub fn dvr(
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        phase: CallbackDeliveryFailurePhase,
        dvr_post_commit_phase: DvrPostCommitNotificationPhase,
        primary: HalError,
    ) -> Self {
        Self::Dvr {
            owner_id,
            owner_generation,
            phase,
            dvr_post_commit_phase,
            primary,
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
        Self::FrontendScanEnd {
            owner_id,
            owner_generation,
            frontend_id,
            scan_generation,
            phase,
            primary,
        }
    }

    pub fn frontend_event(
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        frontend_id: i32,
        frontend_generation: u64,
        phase: CallbackDeliveryFailurePhase,
        primary: HalError,
    ) -> Self {
        Self::FrontendEvent {
            owner_id,
            owner_generation,
            frontend_id,
            frontend_generation,
            phase,
            primary,
        }
    }

    pub fn phase(&self) -> CallbackDeliveryFailurePhase {
        match self {
            Self::Filter { phase, .. }
            | Self::Dvr { phase, .. }
            | Self::FrontendEvent { phase, .. }
            | Self::FrontendScanEnd { phase, .. } => *phase,
        }
    }
}

pub(crate) fn filter_callback_failure_diagnostic_phase(
    phase: CallbackDeliveryFailurePhase,
) -> FilterCallbackDeliveryDiagnosticPhase {
    match phase {
        CallbackDeliveryFailurePhase::CallbackArtifactLookup
        | CallbackDeliveryFailurePhase::RuntimePolicySkip
        | CallbackDeliveryFailurePhase::NotifierCleanup
        | CallbackDeliveryFailurePhase::NotifierPreflight => {
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

pub(crate) fn frontend_callback_failure_diagnostic_phase(
    phase: CallbackDeliveryFailurePhase,
) -> FrontendCallbackDeliveryDiagnosticPhase {
    match phase {
        CallbackDeliveryFailurePhase::CallbackArtifactLookup
        | CallbackDeliveryFailurePhase::RuntimePolicySkip
        | CallbackDeliveryFailurePhase::NotifierCleanup
        | CallbackDeliveryFailurePhase::NotifierPreflight => {
            FrontendCallbackDeliveryDiagnosticPhase::CallbackArtifactLookup
        }
        CallbackDeliveryFailurePhase::EventConversion
        | CallbackDeliveryFailurePhase::BinderDelivery
        | CallbackDeliveryFailurePhase::ScanEndDelivery
        | CallbackDeliveryFailurePhase::PostCommitNotification
        | CallbackDeliveryFailurePhase::NotifierTerminal => {
            FrontendCallbackDeliveryDiagnosticPhase::ScanEndDelivery
        }
    }
}

pub(crate) fn dvr_post_commit_notification_failure_kind(
    phase: CallbackDeliveryFailurePhase,
) -> DvrPostCommitNotificationFailureKind {
    match phase {
        CallbackDeliveryFailurePhase::CallbackArtifactLookup => {
            DvrPostCommitNotificationFailureKind::CallbackArtifactLookup
        }
        CallbackDeliveryFailurePhase::RuntimePolicySkip => {
            DvrPostCommitNotificationFailureKind::RuntimePolicySkip
        }
        CallbackDeliveryFailurePhase::EventConversion => {
            DvrPostCommitNotificationFailureKind::EventConversion
        }
        CallbackDeliveryFailurePhase::BinderDelivery => {
            DvrPostCommitNotificationFailureKind::BinderDelivery
        }
        CallbackDeliveryFailurePhase::PostCommitNotification => {
            DvrPostCommitNotificationFailureKind::PostCommitNotification
        }
        CallbackDeliveryFailurePhase::NotifierTerminal => {
            DvrPostCommitNotificationFailureKind::NotifierTerminal
        }
        CallbackDeliveryFailurePhase::NotifierCleanup => {
            DvrPostCommitNotificationFailureKind::NotifierCleanup
        }
        CallbackDeliveryFailurePhase::NotifierPreflight => {
            DvrPostCommitNotificationFailureKind::NotifierPreflight
        }
        CallbackDeliveryFailurePhase::ScanEndDelivery => {
            DvrPostCommitNotificationFailureKind::EventConversion
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

fn callback_runtime_registry_missing_error(
    command: &OwnerCallbackCleanupArtifactCommand,
    action: &'static str,
) -> HalError {
    HalError::internal(
        HalInternalKind::InvariantViolation,
        format!(
            "callback runtime registry entry missing while {action}: {:?} {:?} {:?}",
            command.owner_kind, command.owner_id, command.owner_generation
        ),
    )
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
    pub fn plan_callback_artifact_reset_before_boot_use_case(
        &self,
    ) -> CallbackArtifactResetCommand {
        CallbackArtifactResetCommand::new("callback artifact reset failed before runtime boot")
    }

    fn filter_event_delivery_snapshots(
        &mut self,
        reports: &[PipelineReport],
    ) -> Vec<FilterEventDeliverySnapshot> {
        let mut snapshots = Vec::new();
        let mut start_id_snapshot_emitted = BTreeSet::new();
        for event in reports
            .iter()
            .flat_map(|report| report.generated_events.iter())
        {
            use maleicacid_tuner_hal2_demux::PipelineGeneratedEvent;
            let (filter_id, event) = match event {
                PipelineGeneratedEvent::FilterStatus { filter_id, status } => {
                    (*filter_id, FilterEventDelivery::Status(*status))
                }
                PipelineGeneratedEvent::AvMedia {
                    filter_id,
                    descriptor,
                } => (*filter_id, FilterEventDelivery::Media(descriptor.clone())),
                PipelineGeneratedEvent::SectionPayloadReady {
                    filter_id,
                    raw,
                    bytes,
                    ..
                } => {
                    if *raw {
                        continue;
                    }
                    (
                        *filter_id,
                        FilterEventDelivery::Section {
                            data_length: bytes.len(),
                        },
                    )
                }
                PipelineGeneratedEvent::PesPacketReady {
                    filter_id,
                    raw,
                    packet,
                    ..
                } => {
                    if *raw {
                        continue;
                    }
                    (
                        *filter_id,
                        FilterEventDelivery::Pes {
                            stream_id: i32::from(packet.stream_id),
                            data_length: packet.raw_bytes.len(),
                        },
                    )
                }
                PipelineGeneratedEvent::RecordIndex { filter_id, data } => {
                    (*filter_id, FilterEventDelivery::RecordIndex(*data))
                }
                _ => continue,
            };
            let Some(entry) = self
                .object_table
                .live_entry_for_runtime(AidlObjectKind::Filter, LedgerId(i64::from(filter_id)))
            else {
                continue;
            };
            let object_id = entry.object_id;
            let generation = entry.generation;
            let owner_demux_id = self
                .registry
                .filter(FilterRuntimeId(filter_id))
                .map(|filter| filter.owner_demux_id);
            if start_id_snapshot_emitted.insert(filter_id) {
                if let Some(start_id) = owner_demux_id
                    .and_then(|demux_id| self.registry.demux_runtime(DemuxRuntimeId(demux_id)))
                    .and_then(|demux| demux.pending_filter_start_id(filter_id).ok())
                    .flatten()
                {
                    snapshots.push(FilterEventDeliverySnapshot {
                        object_id,
                        generation,
                        filter_id,
                        event: FilterEventDelivery::StartId(start_id),
                    });
                }
            }
            snapshots.push(FilterEventDeliverySnapshot {
                object_id,
                generation,
                filter_id,
                event,
            });
        }
        snapshots
    }

    pub fn commit_filter_start_id_delivery(
        &mut self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        filter_id: i32,
        start_id: i32,
    ) -> Result<(), HalError> {
        let entry = self
            .object_table
            .live_entry_for_runtime(AidlObjectKind::Filter, LedgerId(i64::from(filter_id)))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter object is no longer live while committing startId delivery",
                )
            })?;
        if entry.object_id != object_id || entry.generation != object_generation {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter object generation changed while committing startId delivery",
            ));
        }
        let owner_demux_id = self
            .registry
            .filter(FilterRuntimeId(filter_id))
            .map(|filter| filter.owner_demux_id)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter registry entry is missing while committing startId delivery",
                )
            })?;
        let committed = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "owner demux is missing while committing startId delivery",
                )
            })?
            .commit_pending_filter_start_id(filter_id, start_id)
            .map_err(demux_runtime_error_to_hal)?;
        if committed {
            Ok(())
        } else {
            Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "pending filter startId changed before successful delivery commit",
            ))
        }
    }
}

#[cfg(test)]
mod raw_filter_event_projection_tests {
    use super::*;
    use maleicacid_tuner_hal2_demux::{
        FilterConfigKind, FilterStatusEvent, PesSettings, PipelineGeneratedEvent, SectionCondition,
        SectionConditionKind, ValidatedPacketIngressRequest,
    };

    fn packet_with_payload(
        pid: u16,
        payload_unit_start: bool,
        payload: &[u8],
    ) -> [u8; TS_PACKET_SIZE] {
        let mut packet = [0xffu8; TS_PACKET_SIZE];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) & 0x1f;
        if payload_unit_start {
            packet[1] |= 0x40;
        }
        packet[2] = pid as u8;
        let adaptation_len = TS_PACKET_SIZE - 5 - payload.len();
        packet[3] = 0x30;
        packet[4] = adaptation_len as u8;
        if adaptation_len > 0 {
            packet[5] = 0;
        }
        let payload_offset = 5 + adaptation_len;
        packet[payload_offset..payload_offset + payload.len()].copy_from_slice(payload);
        packet
    }

    fn configured_raw_filter(
        open_type: FilterOpenType,
        pid: i32,
        kind: FilterConfigKind,
    ) -> (TunerServiceRuntime, i32, i32) {
        let mut runtime = TunerServiceRuntime::new();
        let demux = runtime
            .allocate_demux_runtime()
            .expect("test demux allocation succeeds");
        let filter = runtime
            .allocate_filter_runtime(demux.id.0)
            .expect("test filter allocation succeeds");
        runtime
            .register_demux_filter_runtime(
                demux.id.0,
                filter.id.0,
                &OpenFilterRequest {
                    open_type,
                    buffer_size: 4096,
                    callback_present: true,
                },
            )
            .expect("test filter registration succeeds");
        runtime
            .configure_filter_runtime_request(
                filter.id.0,
                FilterConfig {
                    open_type,
                    tpid: pid,
                    kind,
                },
            )
            .expect("test filter configuration succeeds");
        runtime
            .start_filter_runtime(filter.id.0)
            .expect("test filter start succeeds");
        let demux_object_id = AidlObjectId(10_000 + i64::from(demux.id.0));
        runtime
            .object_table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Demux,
                object_id: demux_object_id,
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(i64::from(demux.id.0)),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Root,
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("test demux object registration succeeds");
        runtime
            .object_table
            .insert(RuntimeObjectEntry {
                object_kind: AidlObjectKind::Filter,
                object_id: AidlObjectId(20_000 + i64::from(filter.id.0)),
                generation: AidlObjectGeneration(1),
                ledger_id: LedgerId(i64::from(filter.id.0)),
                ledger_generation: LedgerGeneration(1),
                owner: RuntimeOwnerRelation::Demux {
                    demux: demux_object_id,
                    generation: AidlObjectGeneration(1),
                },
                lifecycle: RuntimeObjectLifecycle::Live,
            })
            .expect("test filter object registration succeeds");
        (runtime, demux.id.0, filter.id.0)
    }

    fn push_and_project(
        runtime: &mut TunerServiceRuntime,
        demux_id: i32,
        packet: &[u8; TS_PACKET_SIZE],
    ) -> (PipelineReport, Vec<FilterEventDeliverySnapshot>) {
        let validated = ValidatedTsPacket::validate(packet).expect("test TS packet is valid");
        let report = runtime
            .registry
            .demux_runtime_mut(DemuxRuntimeId(demux_id))
            .expect("test demux runtime exists")
            .push_validated_ts_packet_from_typed_request(ValidatedPacketIngressRequest::new(
                &validated,
                TsInputOrigin::frontend(1),
            ));
        let snapshots = runtime.filter_event_delivery_snapshots(std::slice::from_ref(&report));
        (report, snapshots)
    }

    fn assert_data_ready_without_typed_event(snapshots: &[FilterEventDeliverySnapshot]) {
        assert!(snapshots.iter().any(|snapshot| {
            snapshot.event == FilterEventDelivery::Status(FilterStatusEvent::DataReady)
        }));
        assert!(!snapshots.iter().any(|snapshot| {
            matches!(
                snapshot.event,
                FilterEventDelivery::Section { .. } | FilterEventDelivery::Pes { .. }
            )
        }));
    }

    #[test]
    fn raw_section_fmq_commit_projects_data_ready_without_typed_event() {
        let pid = 0x0123;
        let (mut runtime, demux_id, filter_id) = configured_raw_filter(
            FilterOpenType::TsSection,
            pid,
            FilterConfigKind::TsSection {
                check_crc: false,
                repeat: true,
                raw: true,
                length_field_bits: 12,
                condition: SectionCondition {
                    kind: SectionConditionKind::SectionBits,
                    filter: Vec::new(),
                    mask: Vec::new(),
                    mode: Vec::new(),
                    table_id: None,
                    version: None,
                },
            },
        );
        // raw + isCheckCrc=false はreserved bitsをsemantic rejectせず、
        // pointer/section_lengthでframingした生バイト列を配送する。
        let section = [0x7f, 0x00, 0x05, 0xaa, 0xbb, 0xcc, 0xdd, 0xee];
        let mut payload = vec![0x00];
        payload.extend_from_slice(&section);

        let (report, snapshots) = push_and_project(
            &mut runtime,
            demux_id,
            &packet_with_payload(pid as u16, true, &payload),
        );

        assert!(report.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::SectionPayloadReady {
                filter_id: event_filter_id,
                raw: true,
                bytes,
                ..
            } if *event_filter_id == filter_id && bytes.as_slice() == section.as_slice()
        )));
        assert_data_ready_without_typed_event(&snapshots);
    }

    #[test]
    fn raw_pes_fmq_commit_projects_data_ready_without_typed_event() {
        let pid = 0x0100;
        let (mut runtime, demux_id, filter_id) = configured_raw_filter(
            FilterOpenType::TsPes,
            pid,
            FilterConfigKind::TsPes(PesSettings {
                stream_id: 0xffff,
                raw: true,
            }),
        );
        let pes = [0x00, 0x00, 0x01, 0xe0, 0x00, 0x04, 0x80, 0x00, 0x00, 0xde];

        let (report, snapshots) = push_and_project(
            &mut runtime,
            demux_id,
            &packet_with_payload(pid as u16, true, &pes),
        );

        assert!(report.generated_events.iter().any(|event| matches!(
            event,
            PipelineGeneratedEvent::PesPacketReady {
                filter_id: event_filter_id,
                raw: true,
                packet,
                ..
            } if *event_filter_id == filter_id && packet.raw_bytes.as_slice() == pes.as_slice()
        )));
        assert_data_ready_without_typed_event(&snapshots);
    }
}

impl Default for TunerServiceRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl TunerServiceRuntime {
    pub(crate) fn default_max_number_of_frontends(&self, system: FrontendSystem) -> i32 {
        match i32::try_from(
            self.registry
                .frontend_ids()
                .into_iter()
                .filter(|id| {
                    self.registry
                        .frontend(*id)
                        .is_some_and(|entry| entry.system == system)
                })
                .count(),
        ) {
            Ok(count) => count,
            Err(_) => i32::MAX,
        }
    }

    pub(crate) fn current_max_number_of_frontends(&self, system: FrontendSystem) -> i32 {
        match self.frontend_current_max.get(&system) {
            Some(max_number) => *max_number,
            None => self.default_max_number_of_frontends(system),
        }
    }

    pub(crate) fn set_current_max_number_of_frontends(
        &mut self,
        system: FrontendSystem,
        max_number: i32,
    ) {
        self.frontend_current_max.insert(system, max_number);
    }

    pub(crate) fn active_frontend_lease_count(&self, system: FrontendSystem) -> i32 {
        match i32::try_from(
            self.object_table
                .active_public_runtime_ids(AidlObjectKind::Frontend)
                .into_iter()
                .filter_map(|id| i32::try_from(id.0).ok())
                .filter(|id| {
                    self.registry
                        .frontend(FrontendRuntimeId(*id))
                        .is_some_and(|entry| entry.system == system)
                })
                .count(),
        ) {
            Ok(count) => count,
            Err(_) => i32::MAX,
        }
    }

    pub(crate) fn has_active_frontend_lease(&self, frontend_id: i32) -> bool {
        self.object_table
            .active_public_runtime_ids(AidlObjectKind::Frontend)
            .contains(&LedgerId(i64::from(frontend_id)))
    }

    pub(crate) fn has_active_frontend_group_lease(&self, exclusive_group_id: i32) -> bool {
        self.object_table
            .active_public_runtime_ids(AidlObjectKind::Frontend)
            .into_iter()
            .filter_map(|id| i32::try_from(id.0).ok())
            .any(|id| {
                self.registry
                    .frontend(FrontendRuntimeId(id))
                    .is_some_and(|entry| entry.capability.exclusive_group_id == exclusive_group_id)
            })
    }

    pub fn new() -> Self {
        Self::from_capability_snapshot(CapabilitySnapshot::product_default())
    }

    pub fn try_new() -> Result<Self, HalError> {
        let capability_snapshot = CapabilitySnapshot::product_default();
        capability_snapshot.validate_dependency_closures()?;
        Ok(Self::from_capability_snapshot(capability_snapshot))
    }

    fn from_capability_snapshot(capability_snapshot: CapabilitySnapshot) -> Self {
        Self {
            state: ServiceState::Booting,
            capability_snapshot,
            capacity_ledger: CapacityLedger::default(),
            release_only_filter_av_backings: BTreeMap::new(),
            release_only_filter_types: BTreeMap::new(),
            released_filter_av_shared_handle_leases: BTreeMap::new(),
            registry: RuntimeRegistry::with_av_runtime_limits(
                capability_snapshot.av_max_event_bytes,
                capability_snapshot.av_max_outstanding_events_per_filter,
                capability_snapshot.av_per_filter_live_bytes,
                capability_snapshot.av_runtime_budget_bytes,
            ),
            object_table: RuntimeObjectTable::default(),
            diagnostics: BoundedDiagnosticStore::default(),
            descrambler_diagnostics: BoundedDiagnosticStore::default(),
            child_open_rollback_diagnostics: BoundedDiagnosticStore::default(),
            dvr_post_commit_notification_diagnostics:
                SharedDvrPostCommitNotificationDiagnostics::default(),
            dvr_status_notifier_cleanup_diagnostics:
                SharedDvrStatusNotifierCleanupDiagnostics::default(),
            queue_descriptor_query_diagnostics: BoundedDiagnosticStore::default(),
            filter_callback_delivery_diagnostics: BoundedDiagnosticStore::default(),
            frontend_callback_delivery_diagnostics: BoundedDiagnosticStore::default(),
            demux_transaction_diagnostics: BoundedDiagnosticStore::default(),
            object_cleanup_diagnostics: SharedObjectCleanupDiagnostics::default(),
            frontend_worker_cleanup_diagnostics: SharedFrontendWorkerCleanupDiagnostics::default(),
            next_demux_transaction_diagnostic_id: 1,
            demux_transaction_diagnostic_id_saturation_reported: false,
            callback_artifact_runtime_split_diagnostics:
                SharedCallbackArtifactRuntimeSplitDiagnostics::default(),
            filter_event_dispatcher: None,
            callback_registry: RuntimeCallbackRegistry::default(),
            frontend_workers: FrontendWorkerRegistry::default(),
            playback_consume_txns: BTreeMap::new(),
            frontend_worker_reaper: None,
            frontend_current_max: BTreeMap::new(),
            next_aidl_generation: 0,
            next_aidl_object_id: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_capability_snapshot_for_test(
        capability_snapshot: CapabilitySnapshot,
    ) -> Self {
        Self::from_capability_snapshot(capability_snapshot)
    }

    pub fn state(&self) -> ServiceState {
        self.state
    }

    pub(crate) fn frontend_worker_reaper_handle(
        &self,
    ) -> Option<crate::frontend_worker_txn::FrontendWorkerReaperHandle> {
        self.frontend_worker_reaper.clone()
    }

    pub(crate) fn install_frontend_worker_reaper_handle(
        &mut self,
        handle: crate::frontend_worker_txn::FrontendWorkerReaperHandle,
    ) {
        self.frontend_worker_reaper = Some(handle);
    }

    pub(crate) fn frontend_worker_reaper_capacity(&self) -> usize {
        self.registry.frontend_count().max(1).saturating_mul(2)
    }

    pub fn mark_service_critical(&mut self) {
        self.state = ServiceState::ServiceCritical;
    }

    pub const fn capability_snapshot(&self) -> CapabilitySnapshot {
        self.capability_snapshot
    }

    #[cfg(test)]
    pub(crate) fn reserve_filter_capacity_for_test(
        &mut self,
        filter_id: i32,
        open_type: FilterOpenType,
        buffer_size: i32,
    ) -> Result<(), HalError> {
        self.capacity_ledger.reserve_filter(
            self.capability_snapshot,
            filter_id,
            open_type,
            buffer_size,
        )
    }

    #[cfg(test)]
    pub(crate) fn release_filter_capacity_for_test(
        &mut self,
        filter_id: i32,
    ) -> Result<(), HalError> {
        self.capacity_ledger.release_filter(filter_id)
    }

    #[cfg(test)]
    pub(crate) fn reserve_dvr_capacity_for_test(
        &mut self,
        dvr_id: i32,
        buffer_size: i32,
    ) -> Result<(), HalError> {
        self.capacity_ledger
            .reserve_dvr(self.capability_snapshot, dvr_id, buffer_size)
    }

    #[cfg(test)]
    pub(crate) fn release_dvr_capacity_for_test(&mut self, dvr_id: i32) -> Result<(), HalError> {
        self.capacity_ledger.release_dvr(dvr_id)
    }

    pub(crate) fn registry(&self) -> &RuntimeRegistry {
        &self.registry
    }

    pub(crate) fn registry_mut(&mut self) -> &mut RuntimeRegistry {
        &mut self.registry
    }

    #[cfg(test)]
    pub(crate) fn registry_mut_for_test(&mut self) -> &mut RuntimeRegistry {
        &mut self.registry
    }

    #[cfg(test)]
    pub(crate) fn diagnostics(&self) -> &[StartupDiagnosticRecord] {
        self.diagnostics.as_slice()
    }

    #[cfg(test)]
    pub(crate) fn descrambler_diagnostics(&self) -> &[DescramblerDiagnosticRecord] {
        self.descrambler_diagnostics.as_slice()
    }

    pub fn startup_diagnostic_snapshot(&self) -> StartupDiagnosticSnapshot {
        StartupDiagnosticSnapshot::new(
            self.diagnostics.as_slice().to_vec(),
            self.diagnostics.dropped_count(),
        )
    }

    pub fn descrambler_diagnostic_snapshot(&self) -> DescramblerDiagnosticSnapshot {
        DescramblerDiagnosticSnapshot::new(
            self.descrambler_diagnostics.as_slice().to_vec(),
            self.descrambler_diagnostics.dropped_count(),
        )
    }

    pub fn child_open_rollback_diagnostic_snapshot(&self) -> ChildOpenRollbackDiagnosticSnapshot {
        ChildOpenRollbackDiagnosticSnapshot::new(
            self.child_open_rollback_diagnostics.as_slice().to_vec(),
            self.child_open_rollback_diagnostics.dropped_count(),
        )
    }

    pub fn dvr_post_commit_notification_diagnostics(
        &self,
    ) -> Result<DvrPostCommitNotificationDiagnosticSnapshot, HalError> {
        self.dvr_post_commit_notification_diagnostics.snapshot()
    }

    pub fn dvr_post_commit_notification_diagnostic_sink(
        &self,
    ) -> SharedDvrPostCommitNotificationDiagnostics {
        self.dvr_post_commit_notification_diagnostics.clone()
    }

    pub fn dvr_status_notifier_cleanup_diagnostics(
        &self,
    ) -> Result<DvrStatusNotifierCleanupDiagnosticSnapshot, HalError> {
        self.dvr_status_notifier_cleanup_diagnostics.snapshot()
    }

    pub fn dvr_status_notifier_cleanup_diagnostic_sink(
        &self,
    ) -> SharedDvrStatusNotifierCleanupDiagnostics {
        self.dvr_status_notifier_cleanup_diagnostics.clone()
    }

    #[cfg(test)]
    pub(crate) fn queue_descriptor_query_diagnostics(
        &self,
    ) -> &[QueueDescriptorQueryDiagnosticRecord] {
        self.queue_descriptor_query_diagnostics.as_slice()
    }

    pub fn queue_descriptor_query_diagnostic_snapshot(
        &self,
    ) -> QueueDescriptorQueryDiagnosticSnapshot {
        QueueDescriptorQueryDiagnosticSnapshot::new(
            self.queue_descriptor_query_diagnostics.as_slice().to_vec(),
            self.queue_descriptor_query_diagnostics.dropped_count(),
        )
    }

    #[cfg(test)]
    pub(crate) fn filter_callback_delivery_diagnostics(
        &self,
    ) -> &[FilterCallbackDeliveryDiagnosticRecord] {
        self.filter_callback_delivery_diagnostics.as_slice()
    }

    pub fn filter_callback_delivery_diagnostic_snapshot(
        &self,
    ) -> FilterCallbackDeliveryDiagnosticSnapshot {
        FilterCallbackDeliveryDiagnosticSnapshot::new(
            self.filter_callback_delivery_diagnostics
                .as_slice()
                .to_vec(),
            self.filter_callback_delivery_diagnostics.dropped_count(),
        )
    }

    #[cfg(test)]
    pub(crate) fn frontend_callback_delivery_diagnostics(
        &self,
    ) -> &[FrontendCallbackDeliveryDiagnosticRecord] {
        self.frontend_callback_delivery_diagnostics.as_slice()
    }

    pub fn frontend_callback_delivery_diagnostic_snapshot(
        &self,
    ) -> FrontendCallbackDeliveryDiagnosticSnapshot {
        FrontendCallbackDeliveryDiagnosticSnapshot::new(
            self.frontend_callback_delivery_diagnostics
                .as_slice()
                .to_vec(),
            self.frontend_callback_delivery_diagnostics.dropped_count(),
        )
    }

    pub fn demux_transaction_diagnostics(&self) -> DemuxTransactionDiagnosticSnapshot {
        DemuxTransactionDiagnosticSnapshot::new(
            self.demux_transaction_diagnostics.as_slice().to_vec(),
            self.demux_transaction_diagnostics.dropped_count(),
        )
    }

    pub fn object_cleanup_diagnostics(&self) -> Result<ObjectCleanupDiagnosticSnapshot, HalError> {
        self.object_cleanup_diagnostics.snapshot()
    }

    pub fn object_cleanup_diagnostic_sink(&self) -> SharedObjectCleanupDiagnostics {
        self.object_cleanup_diagnostics.clone()
    }

    pub fn frontend_worker_cleanup_diagnostics(
        &self,
    ) -> Result<FrontendWorkerCleanupDiagnosticSnapshot, HalError> {
        self.frontend_worker_cleanup_diagnostics.snapshot()
    }

    pub fn frontend_worker_cleanup_diagnostic_sink(
        &self,
    ) -> SharedFrontendWorkerCleanupDiagnostics {
        self.frontend_worker_cleanup_diagnostics.clone()
    }

    pub fn callback_artifact_runtime_split_diagnostics(
        &self,
    ) -> Result<CallbackArtifactRuntimeSplitDiagnosticSnapshot, HalError> {
        self.callback_artifact_runtime_split_diagnostics.snapshot()
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

    pub(crate) fn register_prepared_aidl_object_for_runtime_auto_generation(
        &mut self,
        object_kind: AidlObjectKind,
        runtime_id: i64,
        owner: RuntimeOwnerRelation,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        let generation = self.object_table.next_generation()?;
        let object_id = self.object_table.next_object_id()?;
        let entry = RuntimeObjectEntry {
            object_kind,
            object_id,
            generation,
            ledger_id: LedgerId(runtime_id),
            ledger_generation: LedgerGeneration(generation.0),
            owner,
            lifecycle: RuntimeObjectLifecycle::Prepared,
        };
        self.object_table.insert_prepared(entry.clone())?;
        Ok(entry)
    }

    pub fn commit_prepared_child_object(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<RuntimeObjectEntry, HalError> {
        self.object_table
            .commit_prepared(object_id, generation)
            .map_err(object_table_error_to_hal)
    }

    pub(crate) fn record_child_open_rollback_diagnostic(
        &mut self,
        record: ChildOpenRollbackDiagnosticRecord,
    ) {
        self.child_open_rollback_diagnostics.push(record);
    }

    pub(crate) fn record_dvr_post_commit_notification_diagnostic(
        &mut self,
        record: DvrPostCommitNotificationDiagnosticRecord,
    ) {
        if let Err(error) = self.dvr_post_commit_notification_diagnostics.record(record) {
            self.diagnostics.push(
                StartupDiagnosticRecord::dvr_post_commit_notification_diagnostic_record_failed(
                    error,
                ),
            );
        }
    }

    pub(crate) fn record_queue_descriptor_query_diagnostic(
        &mut self,
        record: QueueDescriptorQueryDiagnosticRecord,
    ) {
        self.queue_descriptor_query_diagnostics.push(record);
    }

    pub(crate) fn record_filter_callback_delivery_diagnostic(
        &mut self,
        record: FilterCallbackDeliveryDiagnosticRecord,
    ) {
        self.filter_callback_delivery_diagnostics.push(record);
    }

    pub(crate) fn record_frontend_callback_delivery_diagnostic(
        &mut self,
        record: FrontendCallbackDeliveryDiagnosticRecord,
    ) {
        self.frontend_callback_delivery_diagnostics.push(record);
    }

    pub(crate) fn allocate_demux_transaction_diagnostic_id(
        &mut self,
    ) -> DemuxTransactionDiagnosticId {
        let id = DemuxTransactionDiagnosticId(self.next_demux_transaction_diagnostic_id);
        if let Some(next) = self.next_demux_transaction_diagnostic_id.checked_add(1) {
            self.next_demux_transaction_diagnostic_id = next;
        } else if !self.demux_transaction_diagnostic_id_saturation_reported {
            self.demux_transaction_diagnostic_id_saturation_reported = true;
            eprintln!(
                "maleicacid-tuner-hal2-diagnostic: diagnostic_counter_saturated counter=demux_transaction_diagnostic_id owner=service_runtime"
            );
        }
        id
    }

    pub(crate) fn record_demux_transaction_diagnostic(
        &mut self,
        record: DemuxTransactionDiagnosticRecord,
    ) {
        self.demux_transaction_diagnostics.push(record);
    }

    pub(crate) fn discard_playback_consume_for_queue_cleanup(&mut self, dvr_id: i32) -> usize {
        self.playback_consume_txns
            .get_mut(&dvr_id)
            .map(|txn| txn.discard_for_boundary())
            .unwrap_or(0)
    }

    pub fn record_object_cleanup_diagnostic(
        &mut self,
        record: ObjectCleanupDiagnosticRecord,
    ) -> Result<(), HalError> {
        self.object_cleanup_diagnostics.record(record)
    }

    pub(crate) fn record_callback_artifact_runtime_split_diagnostic(
        &mut self,
        record: CallbackArtifactRuntimeSplitDiagnosticRecord,
    ) -> Result<(), HalError> {
        self.callback_artifact_runtime_split_diagnostics
            .record(record)
    }

    pub fn callback_artifact_runtime_split_diagnostic_sink(
        &self,
    ) -> SharedCallbackArtifactRuntimeSplitDiagnostics {
        self.callback_artifact_runtime_split_diagnostics.clone()
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
        match self
            .child_open_txn()
            .rollback_filter_child_open_after_aidl_failure(object_id, generation, filter_id)
        {
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
        match self
            .child_open_txn()
            .rollback_dvr_child_open_after_aidl_failure(object_id, generation, dvr_id)
        {
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
        let primary_result = self
            .child_open_txn()
            .rollback_filter_child_open_after_aidl_failure(owner_id, owner_generation, filter_id);
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
        let primary_result = self
            .child_open_txn()
            .rollback_dvr_child_open_after_aidl_failure(owner_id, owner_generation, dvr_id);
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
                .clear_lnb_callback_registration_for_object(owner_id, owner_generation, dispatch),
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
                let prepared = crate::callback_registry::CallbackRegistrationUseCase::prepare(
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
                if commit_result.is_ok() {
                    crate::callback_registry::CallbackRegistrationUseCase::commit(
                        &mut self.callback_registry,
                        prepared,
                    );
                } else {
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
        CallbackRegistrationArtifactOutcome::new(
            owner_kind,
            owner_id,
            owner_generation,
            registration_api,
            rollback_command,
            primary_result,
            true,
        )
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
                let prepared = crate::callback_registry::CallbackRegistrationUseCase::prepare(
                    owner_kind,
                    owner_id,
                    owner_generation,
                    registration_api,
                );
                let record_result = aidl_object_live(self, owner_id, owner_generation, owner_kind)
                    .map(|_| {
                        crate::callback_registry::CallbackRegistrationUseCase::commit(
                            &mut self.callback_registry,
                            prepared,
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
        CallbackRegistrationArtifactOutcome::new(
            owner_kind,
            owner_id,
            owner_generation,
            registration_api,
            rollback_command,
            primary_result,
            false,
        )
    }

    pub fn finish_owner_callback_cleanup_outcome<T>(
        &mut self,
        outcome: OwnerCallbackCleanupUseCaseOutcome<T>,
        artifact_cleanup_result: Result<CallbackArtifactCleanupResult, HalError>,
    ) -> Result<T, HalError> {
        let (command, primary_result) = outcome.into_parts();
        self.finish_owner_callback_cleanup_use_case(
            command,
            primary_result,
            artifact_cleanup_result,
        )
    }

    pub fn finish_object_close_callback_cleanup_outcome(
        &mut self,
        command: OwnerCallbackCleanupArtifactCommand,
        artifact_cleanup_result: Result<CallbackArtifactCleanupResult, HalError>,
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
        rollback_result: Option<Result<CallbackArtifactCleanupResult, HalError>>,
    ) -> Result<(), HalError> {
        let (finish_command, rollback_command, primary_result) = outcome.into_parts();
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
            None => match primary_result {
                Ok(()) => Ok(()),
                Err(artifact_error) => Err(self.record_callback_artifact_cleanup_split_failure(
                    CallbackArtifactRuntimeSplitPhase::RegistrationRollbackFinish,
                    &finish_command,
                    artifact_error,
                )),
            },
        }
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
        let mut failures = FirstErrorCollector::new();
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
            failures.push_error(error);
        }
        if let Err(error) =
            self.mark_filter_callback_unhealthy_for_object(owner_id, owner_generation)
        {
            self.record_filter_callback_delivery_diagnostic(
                FilterCallbackDeliveryDiagnosticRecord::new(
                    FilterCallbackDeliveryDiagnosticPhase::RuntimeCallbackAccounting,
                    owner_id,
                    owner_generation,
                    error.clone(),
                ),
            );
            failures.push_error(error);
        }
        failures.into_result()
    }

    pub(crate) fn mark_dvr_callback_delivery_failed_use_case(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        diagnostic_phase: DvrPostCommitNotificationPhase,
    ) -> Result<(), HalError> {
        let mut failures = FirstErrorCollector::new();
        if self.mark_callback_registration_unhealthy(
            AidlObjectKind::Dvr,
            owner_id,
            owner_generation,
            AidlApi::DemuxOpenDvr,
        ) == CallbackRegistryUpdate::Missing
        {
            let error = HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR callback registry entry missing while marking unhealthy",
            );
            self.record_dvr_post_commit_notification_diagnostic(
                DvrPostCommitNotificationDiagnosticRecord::new(
                    diagnostic_phase,
                    DvrPostCommitNotificationFailureKind::CallbackRegistryAccounting,
                    owner_id,
                    owner_generation,
                    error.clone(),
                ),
            );
            failures.push_error(error);
        }
        if let Err(error) = self.mark_dvr_callback_unhealthy_for_object(owner_id, owner_generation)
        {
            self.record_dvr_post_commit_notification_diagnostic(
                DvrPostCommitNotificationDiagnosticRecord::new(
                    diagnostic_phase,
                    DvrPostCommitNotificationFailureKind::CallbackRegistryAccounting,
                    owner_id,
                    owner_generation,
                    error.clone(),
                ),
            );
            failures.push_error(error);
        }
        failures.into_result()
    }

    pub fn finish_dvr_post_commit_notification_failure_use_case(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        phase: DvrPostCommitNotificationPhase,
        primary: HalError,
    ) -> Result<(), HalError> {
        let service_critical = phase == DvrPostCommitNotificationPhase::StatusNotifierStart
            && self
                .dvr_status_metadata_snapshot_for_aidl_object(object_id, generation)
                .map(|snapshot| snapshot.is_playback)
                .unwrap_or(true);
        self.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::dvr(
            object_id,
            generation,
            CallbackDeliveryFailurePhase::PostCommitNotification,
            phase,
            primary,
        ))?;
        if service_critical {
            self.mark_service_critical();
        }
        Ok(())
    }

    pub fn finish_callback_delivery_failure_use_case(
        &mut self,
        report: CallbackDeliveryFailureReport,
    ) -> Result<(), HalError> {
        let classified =
            crate::worker_failure_classifier::WorkerFailureClassifier::classify_callback(report);
        crate::post_commit_callback_failure_txn::PostCommitCallbackFailureTxn::new(self)
            .execute(classified)
    }

    pub(crate) fn commit_post_callback_failure_effects(
        &mut self,
        report: CallbackDeliveryFailureReport,
        health_effect: crate::post_commit_callback_failure_txn::CallbackHealthEffect,
    ) -> Result<(), HalError> {
        let mut failures = FirstErrorCollector::new();
        match report {
            CallbackDeliveryFailureReport::Filter {
                owner_id,
                owner_generation,
                phase,
                primary,
            } => {
                self.record_filter_callback_delivery_diagnostic(
                    FilterCallbackDeliveryDiagnosticRecord::new(
                        filter_callback_failure_diagnostic_phase(phase),
                        owner_id,
                        owner_generation,
                        primary.clone(),
                    ),
                );
                if health_effect
                    == crate::post_commit_callback_failure_txn::CallbackHealthEffect::MarkUnhealthy
                {
                    if let Err(error) = self
                        .mark_filter_callback_delivery_failed_use_case(owner_id, owner_generation)
                    {
                        failures.push_error(error);
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
            CallbackDeliveryFailureReport::Dvr {
                owner_id,
                owner_generation,
                phase,
                dvr_post_commit_phase,
                primary,
            } => {
                self.record_dvr_post_commit_notification_diagnostic(
                    DvrPostCommitNotificationDiagnosticRecord::new(
                        dvr_post_commit_phase,
                        dvr_post_commit_notification_failure_kind(phase),
                        owner_id,
                        owner_generation,
                        primary.clone(),
                    ),
                );
                if health_effect
                    == crate::post_commit_callback_failure_txn::CallbackHealthEffect::MarkUnhealthy
                {
                    if let Err(error) = self.mark_dvr_callback_delivery_failed_use_case(
                        owner_id,
                        owner_generation,
                        dvr_post_commit_phase,
                    ) {
                        failures.push_error(error);
                    }
                }
                match failures.into_result() {
                    Ok(()) => Ok(()),
                    Err(cleanup) => Err(compose_primary_cleanup_failure(
                        "callback delivery failure accounting failed",
                        primary,
                        cleanup,
                    )),
                }
            }
            CallbackDeliveryFailureReport::FrontendEvent {
                owner_id,
                owner_generation,
                frontend_id,
                frontend_generation,
                phase,
                primary,
            } => {
                let record = if phase == CallbackDeliveryFailurePhase::CallbackArtifactLookup {
                    FrontendCallbackDeliveryDiagnosticRecord::callback_artifact_lookup(
                        owner_id,
                        owner_generation,
                        primary.clone(),
                    )
                } else {
                    FrontendCallbackDeliveryDiagnosticRecord::frontend_event_delivery(
                        owner_id,
                        owner_generation,
                        frontend_id,
                        frontend_generation,
                        primary.clone(),
                    )
                };
                self.record_frontend_callback_delivery_diagnostic(record);
                if health_effect
                    == crate::post_commit_callback_failure_txn::CallbackHealthEffect::MarkUnhealthy
                {
                    if let Err(error) = self
                        .mark_frontend_callback_delivery_failed_use_case(owner_id, owner_generation)
                    {
                        self.record_frontend_callback_delivery_diagnostic(
                            FrontendCallbackDeliveryDiagnosticRecord::callback_registry_accounting(
                                owner_id,
                                owner_generation,
                                frontend_id,
                                frontend_generation,
                                error.clone(),
                            ),
                        );
                        failures.push_error(error);
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
            CallbackDeliveryFailureReport::FrontendScanEnd {
                owner_id,
                owner_generation,
                frontend_id,
                scan_generation,
                phase,
                primary,
            } => {
                self.record_frontend_callback_delivery_diagnostic(
                    match frontend_callback_failure_diagnostic_phase(phase) {
                        FrontendCallbackDeliveryDiagnosticPhase::CallbackArtifactLookup => {
                            FrontendCallbackDeliveryDiagnosticRecord::callback_artifact_lookup(
                                owner_id,
                                owner_generation,
                                primary.clone(),
                            )
                        }
                        FrontendCallbackDeliveryDiagnosticPhase::FrontendEventDelivery => {
                            FrontendCallbackDeliveryDiagnosticRecord::frontend_event_delivery(
                                owner_id,
                                owner_generation,
                                frontend_id,
                                scan_generation,
                                primary.clone(),
                            )
                        }
                        FrontendCallbackDeliveryDiagnosticPhase::ScanEndDelivery => {
                            FrontendCallbackDeliveryDiagnosticRecord::scan_end_delivery(
                                owner_id,
                                owner_generation,
                                frontend_id,
                                scan_generation,
                                primary.clone(),
                            )
                        }
                        FrontendCallbackDeliveryDiagnosticPhase::ScanSessionAccounting => {
                            FrontendCallbackDeliveryDiagnosticRecord::scan_session_accounting(
                                owner_id,
                                owner_generation,
                                frontend_id,
                                scan_generation,
                                primary.clone(),
                            )
                        }
                        FrontendCallbackDeliveryDiagnosticPhase::CallbackRegistryAccounting => {
                            FrontendCallbackDeliveryDiagnosticRecord::callback_registry_accounting(
                                owner_id,
                                owner_generation,
                                frontend_id,
                                scan_generation,
                                primary.clone(),
                            )
                        }
                    },
                );
                if let Err(error) =
                    self.mark_frontend_scan_session_callback_failed(frontend_id, scan_generation)
                {
                    self.record_frontend_callback_delivery_diagnostic(
                        FrontendCallbackDeliveryDiagnosticRecord::scan_session_accounting(
                            owner_id,
                            owner_generation,
                            frontend_id,
                            scan_generation,
                            error.clone(),
                        ),
                    );
                    failures.push_error(error);
                }
                if health_effect
                    == crate::post_commit_callback_failure_txn::CallbackHealthEffect::MarkUnhealthy
                {
                    if let Err(error) = self
                        .mark_frontend_callback_delivery_failed_use_case(owner_id, owner_generation)
                    {
                        self.record_frontend_callback_delivery_diagnostic(
                            FrontendCallbackDeliveryDiagnosticRecord::callback_registry_accounting(
                                owner_id,
                                owner_generation,
                                frontend_id,
                                scan_generation,
                                error.clone(),
                            ),
                        );
                        failures.push_error(error);
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

    pub fn plan_callback_registration_runtime_finish_lock_failure_cleanup_command(
        &self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) -> OwnerCallbackCleanupArtifactCommand {
        self.plan_owner_callback_cleanup_artifact_command(
            owner_kind,
            owner_id,
            owner_generation,
            Some(registration_api),
            "callback artifact rollback failed after runtime registration finish lock failure",
        )
    }

    pub fn finish_owner_callback_cleanup_use_case<T>(
        &mut self,
        command: OwnerCallbackCleanupArtifactCommand,
        primary_result: Result<T, HalError>,
        artifact_cleanup_result: Result<CallbackArtifactCleanupResult, HalError>,
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
        artifact_cleanup_result: Result<CallbackArtifactCleanupResult, HalError>,
    ) -> Result<T, HalError> {
        let artifact_error = artifact_cleanup_result.err();

        let value = match (primary_result, artifact_error.clone()) {
            (Ok(value), None) => value,
            (Ok(_), Some(cleanup_error)) => {
                let cleanup_error = self.record_callback_artifact_cleanup_split_failure(
                    phase,
                    &command,
                    cleanup_error,
                );
                let cleanup_error = match self.mark_owner_callback_cleanup_failed(phase, &command) {
                    Ok(()) => cleanup_error,
                    Err(mark_error) => compose_primary_cleanup_failure(
                        command.cleanup_failure_message,
                        cleanup_error,
                        mark_error,
                    ),
                };
                return Err(cleanup_error);
            }
            (Err(primary_error), None) => {
                let primary_error = match self.mark_owner_callback_cleanup_failed(phase, &command) {
                    Ok(()) => primary_error,
                    Err(mark_error) => compose_primary_cleanup_failure(
                        command.cleanup_failure_message,
                        primary_error,
                        mark_error,
                    ),
                };
                return Err(primary_error);
            }
            (Err(primary_error), Some(cleanup_error)) => {
                let cleanup_error = self.record_callback_artifact_cleanup_split_failure(
                    phase,
                    &command,
                    cleanup_error,
                );
                let primary_error = compose_primary_cleanup_failure(
                    command.cleanup_failure_message,
                    primary_error,
                    cleanup_error,
                );
                let primary_error = match self.mark_owner_callback_cleanup_failed(phase, &command) {
                    Ok(()) => primary_error,
                    Err(mark_error) => compose_primary_cleanup_failure(
                        command.cleanup_failure_message,
                        primary_error,
                        mark_error,
                    ),
                };
                return Err(primary_error);
            }
        };

        match self
            .callback_registry
            .clear_owner(command.owner_id, command.owner_generation)
        {
            CallbackRegistryUpdate::Updated => Ok(value),
            CallbackRegistryUpdate::Missing => {
                let registry_error = callback_runtime_registry_missing_error(
                    &command,
                    "clearing owner callback runtime registry",
                );
                let registry_error = self.record_callback_runtime_registry_missing_split_failure(
                    phase,
                    &command,
                    registry_error,
                );
                Err(registry_error)
            }
        }
    }

    fn record_callback_artifact_cleanup_split_failure(
        &mut self,
        phase: CallbackArtifactRuntimeSplitPhase,
        command: &OwnerCallbackCleanupArtifactCommand,
        cleanup_error: HalError,
    ) -> HalError {
        let Some(outcome) =
            CallbackArtifactRuntimeSplitOutcome::from_results(Some(cleanup_error.clone()), None)
        else {
            return cleanup_error;
        };
        match self.record_callback_artifact_runtime_split_diagnostic(
            CallbackArtifactRuntimeSplitDiagnosticRecord::owner(
                phase,
                command.owner_kind,
                command.owner_id,
                command.owner_generation,
                outcome,
            ),
        ) {
            Ok(()) => cleanup_error,
            Err(record_error) => compose_primary_cleanup_failure(
                "callback artifact/runtime split diagnostic record failed after artifact cleanup failure",
                cleanup_error,
                record_error,
            ),
        }
    }

    fn record_callback_runtime_registry_missing_split_failure(
        &mut self,
        phase: CallbackArtifactRuntimeSplitPhase,
        command: &OwnerCallbackCleanupArtifactCommand,
        registry_error: HalError,
    ) -> HalError {
        match self.record_callback_artifact_runtime_split_diagnostic(
            CallbackArtifactRuntimeSplitDiagnosticRecord::owner(
                phase,
                command.owner_kind,
                command.owner_id,
                command.owner_generation,
                CallbackArtifactRuntimeSplitOutcome::RuntimeRegistryMissing,
            ),
        ) {
            Ok(()) => registry_error,
            Err(record_error) => compose_primary_cleanup_failure(
                "callback artifact/runtime split diagnostic record failed after runtime registry missing",
                registry_error,
                record_error,
            ),
        }
    }

    fn mark_owner_callback_cleanup_failed(
        &mut self,
        phase: CallbackArtifactRuntimeSplitPhase,
        command: &OwnerCallbackCleanupArtifactCommand,
    ) -> Result<(), HalError> {
        let update = match command.registration_api {
            Some(api) => self.callback_registry.mark_unhealthy(
                command.owner_kind,
                command.owner_id,
                command.owner_generation,
                api,
            ),
            None => self
                .callback_registry
                .mark_owner_unhealthy(command.owner_id, command.owner_generation),
        };
        match update {
            CallbackRegistryUpdate::Updated => Ok(()),
            CallbackRegistryUpdate::Missing => {
                let registry_error = callback_runtime_registry_missing_error(
                    command,
                    "marking owner callback runtime registry unhealthy",
                );
                Err(self.record_callback_runtime_registry_missing_split_failure(
                    phase,
                    command,
                    registry_error,
                ))
            }
        }
    }

    pub(crate) fn object_table(&self) -> &RuntimeObjectTable {
        &self.object_table
    }

    pub(crate) fn object_table_mut(&mut self) -> &mut RuntimeObjectTable {
        &mut self.object_table
    }

    pub(crate) fn has_callback_registration(
        &self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) -> bool {
        self.callback_registry
            .registration_for(owner_kind, owner_id, owner_generation, registration_api)
            .is_some()
    }

    pub fn finish_service_boot_reset_after_artifact_result_use_case(
        &mut self,
        outcome: ServiceBootOutcome,
        dvr_notifier_result: Result<(), HalError>,
        artifact_result: Result<(), HalError>,
        drop_leak_result: Result<(), HalError>,
        callback_fallback_clear_result: Result<(), HalError>,
        diagnostic_clear_result: Result<(), HalError>,
    ) -> Result<ServiceBootOutcome, HalError> {
        let mut record_error: Option<HalError> = None;
        for split_outcome in
            CallbackArtifactRuntimeSplitOutcome::service_boot_reset_from_attempt_results(
                dvr_notifier_result.clone(),
                artifact_result.clone(),
                drop_leak_result.clone(),
                callback_fallback_clear_result.clone(),
                diagnostic_clear_result.clone(),
                Ok(()),
            )
        {
            if let Err(error) = self.record_callback_artifact_runtime_split_diagnostic(
                CallbackArtifactRuntimeSplitDiagnosticRecord::service_boot_reset(split_outcome),
            ) {
                record_error = Some(match record_error {
                    Some(primary) => compose_primary_cleanup_failure(
                        "service boot split diagnostic record failed repeatedly",
                        primary,
                        error,
                    ),
                    None => error,
                });
            }
        }
        let mut reset_failures = FirstErrorCollector::new();
        reset_failures.push_result(dvr_notifier_result);
        reset_failures.push_result(artifact_result);
        reset_failures.push_result(drop_leak_result);
        reset_failures.push_result(callback_fallback_clear_result);
        reset_failures.push_result(diagnostic_clear_result);
        let result = match reset_failures.into_result() {
            Ok(()) => Ok(outcome),
            Err(error) => Err(error),
        };
        match (result, record_error) {
            (Ok(outcome), None) => Ok(outcome),
            (Ok(_), Some(record_error)) => Err(record_error),
            (Err(primary), None) => Err(primary),
            (Err(primary), Some(record_error)) => Err(compose_primary_cleanup_failure(
                "service boot split diagnostic record failed after artifact/drop-leak failure",
                primary,
                record_error,
            )),
        }
    }

    pub fn boot_from_probe_results<I>(&mut self, results: I) -> ServiceBootOutcome
    where
        I: IntoIterator<Item = FrontendProbeOutcome>,
    {
        self.boot_from_probe_results_with_diagnostic_clear_result(results)
            .0
    }

    pub fn boot_from_probe_results_with_diagnostic_clear_result<I>(
        &mut self,
        results: I,
    ) -> (ServiceBootOutcome, Result<(), HalError>)
    where
        I: IntoIterator<Item = FrontendProbeOutcome>,
    {
        self.state = ServiceState::Booting;
        self.registry.clear_frontends();
        self.registry.clear_lnbs();
        self.registry.clear_transient_objects();
        self.object_table.clear();
        self.capacity_ledger.clear();
        self.release_only_filter_av_backings.clear();
        self.release_only_filter_types.clear();
        self.released_filter_av_shared_handle_leases.clear();
        self.diagnostics.clear();
        self.descrambler_diagnostics.clear();
        self.child_open_rollback_diagnostics.clear();
        let mut diagnostic_clear_failures = FirstErrorCollector::new();
        if let Err(error) = self.dvr_post_commit_notification_diagnostics.clear() {
            self.diagnostics.push(
                StartupDiagnosticRecord::dvr_post_commit_notification_diagnostic_clear_failed(
                    error.clone(),
                ),
            );
            diagnostic_clear_failures.push_error(error);
        }
        if let Err(error) = self.dvr_status_notifier_cleanup_diagnostics.clear() {
            self.diagnostics.push(
                StartupDiagnosticRecord::dvr_status_notifier_cleanup_diagnostic_clear_failed(
                    error.clone(),
                ),
            );
            diagnostic_clear_failures.push_error(error);
        }
        self.queue_descriptor_query_diagnostics.clear();
        self.filter_callback_delivery_diagnostics.clear();
        self.frontend_callback_delivery_diagnostics.clear();
        self.demux_transaction_diagnostics.clear();
        if let Err(error) = self.object_cleanup_diagnostics.clear() {
            self.diagnostics.push(
                StartupDiagnosticRecord::object_cleanup_diagnostic_clear_failed(error.clone()),
            );
            diagnostic_clear_failures.push_error(error);
        }
        if let Err(error) = self.frontend_worker_cleanup_diagnostics.clear() {
            self.diagnostics.push(
                StartupDiagnosticRecord::frontend_worker_cleanup_diagnostic_clear_failed(
                    error.clone(),
                ),
            );
            diagnostic_clear_failures.push_error(error);
        }
        if let Err(error) = self.callback_artifact_runtime_split_diagnostics.clear() {
            self.diagnostics.push(
                StartupDiagnosticRecord::callback_artifact_runtime_split_diagnostic_clear_failed(
                    error.clone(),
                ),
            );
            diagnostic_clear_failures.push_error(error);
        }
        let diagnostic_clear_result = diagnostic_clear_failures.into_result();
        self.callback_registry = RuntimeCallbackRegistry::default();
        self.frontend_workers = FrontendWorkerRegistry::default();
        self.frontend_current_max.clear();
        self.next_aidl_generation = 0;
        self.next_aidl_object_id = 0;

        if !adapter_transactions_are_covered() {
            self.diagnostics
                .push(StartupDiagnosticRecord::runtime_dispatch_missing());
        }

        let mut physical_group_by_path: BTreeMap<PathBuf, (FrontendBackendKind, i32)> =
            BTreeMap::new();
        let mut px4_path_by_group: BTreeMap<i32, PathBuf> = BTreeMap::new();
        for result in results {
            match result {
                FrontendProbeOutcome::Available {
                    id,
                    backend,
                    system,
                    path,
                    lnb_profile,
                    satellite_power_topology,
                    capability,
                } => {
                    let path_group_mismatch = physical_group_by_path.get(&path).is_some_and(
                        |(known_backend, known_group)| {
                            *known_backend != backend
                                || *known_group != capability.exclusive_group_id
                        },
                    );
                    let px4_group_collision = backend == FrontendBackendKind::Px4CharDevice
                        && px4_path_by_group
                            .get(&capability.exclusive_group_id)
                            .is_some_and(|known_path| known_path != &path);
                    let satellite_power_is_consistent = match system {
                        FrontendSystem::IsdbS => {
                            satellite_power_topology != SatellitePowerTopology::UnknownOrDisabled
                        }
                        FrontendSystem::IsdbT => {
                            satellite_power_topology == SatellitePowerTopology::UnknownOrDisabled
                        }
                        FrontendSystem::IsdbS3 | FrontendSystem::DvbS => false,
                    };
                    if !frontend_capability_is_consistent(backend, system, capability)
                        || !satellite_power_is_consistent
                        || path_group_mismatch
                        || px4_group_collision
                    {
                        self.diagnostics
                            .push(StartupDiagnosticRecord::capability_suppressed(
                                backend,
                                path,
                                CapabilitySuppressionReason::InvalidCapabilityProfile,
                            ));
                        continue;
                    }
                    let entry = FrontendRegistryEntry {
                        id,
                        backend,
                        system,
                        device_path: path.clone(),
                        lnb_profile,
                        satellite_power_topology,
                        capability,
                    };
                    match self.registry.register_frontend(entry.clone()) {
                        Ok(()) => {
                            physical_group_by_path
                                .insert(path.clone(), (backend, capability.exclusive_group_id));
                            if backend == FrontendBackendKind::Px4CharDevice {
                                px4_path_by_group
                                    .insert(capability.exclusive_group_id, path.clone());
                            }
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

        for system in [
            FrontendSystem::IsdbT,
            FrontendSystem::IsdbS,
            FrontendSystem::IsdbS3,
            FrontendSystem::DvbS,
        ] {
            let count = self.default_max_number_of_frontends(system);
            self.frontend_current_max.insert(system, count);
        }

        if self.registry.frontend_count() > 0 && self.diagnostics.is_empty() {
            self.state = ServiceState::Ready;
            (ServiceBootOutcome::Ready, diagnostic_clear_result)
        } else {
            self.state = ServiceState::Degraded;
            (ServiceBootOutcome::Degraded, diagnostic_clear_result)
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
            .ok_or(RuntimeObjectTableError::ObjectIdOverflow)?;
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
                        .map(|demux| demux.filter_snapshot(id).is_ok())
                })
                .unwrap_or(false),
            AidlObjectKind::Dvr => self
                .registry
                .dvr(DvrRuntimeId(id))
                .and_then(|entry| {
                    self.registry
                        .demux_runtime(DemuxRuntimeId(entry.owner_demux_id))
                        .map(|demux| demux.dvr_snapshot(id).is_ok())
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
        if self.state == ServiceState::ServiceCritical {
            return Err(RuntimeCommandDispatchError::ServiceCritical);
        }
        let plan = RuntimeCommandDispatcher::plan(command_plan, executable_request);
        if plan.is_err() {
            self.diagnostics
                .push(StartupDiagnosticRecord::runtime_dispatch_missing());
        }
        plan
    }
}
