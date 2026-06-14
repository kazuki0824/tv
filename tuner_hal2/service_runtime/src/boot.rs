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
    PipelineOpenKind, PipelineReport, PipelineResetReport,
};
use maleicacid_tuner_hal2_demux::runtime::{
    DemuxRuntimeError, DemuxRuntimeErrorKind, DemuxRuntimeState, DvrKind,
};
use maleicacid_tuner_hal2_demux::runtime::{
    DemuxRuntimeSnapshot, DemuxStreamGeneration, GenerationBoundaryReport, GenerationBoundaryTxn,
};
use maleicacid_tuner_hal2_demux::OpenFilterRequest;
use maleicacid_tuner_hal2_demux::{
    DvrRuntime, FilterConfigureTxn, FilterRuntime, FilterRuntimeState, TsInputOrigin,
};
use maleicacid_tuner_hal2_descrambler::{
    DescramblerKeyLookupError, DescramblerKeyToken, DescramblerKeyTokenError, DescramblerPidClaim,
    DescramblerPidClaimError, DescramblerSessionFailureKind, DescramblerSessionTxn,
};
use maleicacid_tuner_hal2_device::{
    FrontendLivePacketSink, FrontendLivePumpOwner, FrontendLivePumpReport,
    FrontendLiveReaderDescriptor, FrontendRuntimeSnapshot, FrontendRuntimeState,
    FrontendSignalState, FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind,
    FrontendWorkerRegistry, FrontendWorkerStartError, FrontendWorkerStopOutcome,
};
use maleicacid_tuner_hal2_domain_request::{
    AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan, DvrOpenKind,
    FilterAvStreamKind, FilterAvStreamTypeRequest, FilterDelayHintKind, FilterDelayHintRequest,
    OpenDvrRequest, RuntimeExecutableRequest, RuntimeTransactionName,
};

use crate::callback_registry::RuntimeCallbackRegistry;
use crate::command_dispatch::{
    RuntimeCommandDispatchError, RuntimeCommandDispatchPlan, RuntimeCommandDispatcher,
};
use crate::diagnostics::{
    CapabilitySuppressionReason, DescramblerDiagnosticKind, DescramblerDiagnosticPhase,
    DescramblerDiagnosticRecord, StartupDiagnosticRecord,
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
        DescramblerSessionFailureKind::ExpiredToken => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
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
        DescramblerKeyLookupError::ExpiredToken => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
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
        guard.ensure_frontend_demux_sink_ready(frontend_id)?;
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

    pub(crate) fn registry_mut(&mut self) -> &mut RuntimeRegistry {
        &mut self.registry
    }

    pub fn diagnostics(&self) -> &[StartupDiagnosticRecord] {
        &self.diagnostics
    }

    pub fn descrambler_diagnostics(&self) -> &[DescramblerDiagnosticRecord] {
        &self.descrambler_diagnostics
    }

    fn record_descrambler_diagnostic(&mut self, record: DescramblerDiagnosticRecord) {
        eprintln!(
            "maleicacid-tuner-hal2-descrambler-diagnostic: phase={:?} kind={:?} descrambler_id={:?} demux_id={:?} pid={:?} filter_id={:?} error={:?}",
            record.phase,
            record.kind,
            record.descrambler_id,
            record.demux_id,
            record.pid,
            record.filter_id,
            record.error,
        );
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

    pub fn frontend_worker_running_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Option<u64> {
        self.frontend_workers.running_generation(frontend_id, kind)
    }

    pub fn frontend_runtime_snapshot(
        &self,
        frontend_id: i32,
    ) -> Result<FrontendRuntimeSnapshot, HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        Ok(runtime.snapshot())
    }

    pub fn restore_frontend_runtime_snapshot(
        &mut self,
        frontend_id: i32,
        snapshot: FrontendRuntimeSnapshot,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.restore_snapshot(snapshot);
        Ok(())
    }

    pub fn bound_demux_runtime_snapshots(
        &self,
        frontend_id: i32,
    ) -> Result<Vec<(DemuxRuntimeId, DemuxRuntimeSnapshot)>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        let mut snapshots = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            let demux = self.registry.demux_runtime(demux_id).ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing while taking tune rollback snapshot",
                )
            })?;
            snapshots.push((demux_id, demux.snapshot()));
        }
        Ok(snapshots)
    }

    pub fn restore_bound_demux_runtime_snapshots(
        &mut self,
        snapshots: Vec<(DemuxRuntimeId, DemuxRuntimeSnapshot)>,
    ) -> Result<(), HalError> {
        for (demux_id, snapshot) in snapshots {
            let demux = self.registry.demux_runtime_mut(demux_id).ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing while restoring tune rollback snapshot",
                )
            })?;
            demux.restore(snapshot);
        }
        Ok(())
    }

    pub fn frontend_has_same_active_tune(
        &self,
        frontend_id: i32,
        request: &FrontendTuneRequest,
    ) -> Result<bool, HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        Ok(runtime.same_active_tune(request))
    }

    pub fn commit_frontend_active_tune_request(
        &mut self,
        frontend_id: i32,
        generation: u64,
        request: FrontendTuneRequest,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.commit_active_tune_request(generation, request)
    }

    pub fn record_frontend_signal_state(
        &mut self,
        frontend_id: i32,
        generation: u64,
        signal_state: FrontendSignalState,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.record_signal_state(generation, signal_state)
    }

    pub fn record_live_pump_report(
        &mut self,
        frontend_id: i32,
        generation: u64,
        report: FrontendLivePumpReport,
        cancel_reason: Option<FrontendWorkerCancelReason>,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.record_live_pump_report(generation, report, cancel_reason)
    }

    pub fn frontend_signal_state(&self, frontend_id: i32) -> Result<FrontendSignalState, HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        Ok(runtime.signal_state())
    }

    pub fn prepare_frontend_worker_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Result<u64, HalError> {
        if self
            .frontend_workers
            .running_generation(frontend_id, kind)
            .is_some()
        {
            return Err(HalError::invalid_state(
                maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                "frontend worker is already running",
            ));
        }
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.checked_next_generation()
    }

    pub fn install_frontend_live_reader_descriptor_for_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
    ) -> Result<(), HalError> {
        let entry = self
            .registry
            .frontend(crate::registry::FrontendRuntimeId(frontend_id))
            .cloned()
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend registry entry is missing for advertised frontend",
                )
            })?;
        let reader = live_reader_descriptor_for_frontend_entry(&entry)?;
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.commit_generation(generation)?;
        runtime.set_live_reader_descriptor(reader);
        match kind {
            FrontendWorkerKind::Tune => runtime.mark_tuning(generation),
            FrontendWorkerKind::Scan => runtime.mark_scanning(generation),
        }
        Ok(())
    }

    pub fn frontend_live_reader_descriptor_for_live_pump(
        &self,
        frontend_id: i32,
    ) -> Result<Option<FrontendLiveReaderDescriptor>, HalError> {
        let frontend_key = crate::registry::FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported(
                "frontend id is not available for live pump",
            ));
        }
        if self
            .registry
            .frontend_bound_demux_ids(frontend_key)
            .is_empty()
        {
            return Ok(None);
        }
        let runtime = self
            .registry
            .frontend_runtime(frontend_key)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime
            .live_reader_descriptor()
            .cloned()
            .map(Some)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "frontend has bound demux but no live reader descriptor",
                )
            })
    }

    pub fn clear_frontend_live_reader_descriptor_and_idle(
        &mut self,
        frontend_id: i32,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.clear_live_reader_descriptor();
        runtime.mark_idle();
        Ok(())
    }

    pub fn stop_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.clear_frontend_live_reader_descriptor_and_idle(frontend_id)?;
        self.reset_and_unbind_bound_demuxes_for_frontend(
            frontend_id,
            PipelineBoundaryReason::FrontendUnbind,
        )
    }

    pub fn close_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let frontend_key = crate::registry::FrontendRuntimeId(frontend_id);
        let runtime = self
            .registry
            .frontend_runtime_mut(frontend_key)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.clear_live_reader_descriptor();
        runtime.mark_closing();
        self.reset_and_unbind_bound_demuxes_for_frontend(
            frontend_id,
            PipelineBoundaryReason::FrontendClose,
        )
    }

    pub fn record_frontend_scan_cancelled(
        &mut self,
        frontend_id: i32,
        generation: u64,
        reason: FrontendWorkerCancelReason,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.record_scan_cancelled(generation, reason)
    }

    pub fn begin_frontend_scan_session(
        &mut self,
        frontend_id: i32,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.begin_scan_session(generation, fingerprint, candidates)
    }

    pub fn cancel_frontend_scan_session(
        &mut self,
        frontend_id: i32,
        generation: u64,
        reason: FrontendWorkerCancelReason,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.cancel_scan_session(generation, reason)
    }

    pub fn advance_frontend_scan_session_after_candidate(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<bool, HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.advance_scan_session_after_candidate(generation)
    }

    pub fn mark_frontend_tune_worker_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        {
            let runtime = self
                .registry
                .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend runtime is missing for advertised frontend",
                    )
                })?;
            runtime.mark_tune_worker_failed(generation, error)?;
        }
        self.registry
            .quarantine_bound_demuxes_for_frontend(crate::registry::FrontendRuntimeId(frontend_id));
        Ok(())
    }

    pub fn mark_frontend_scan_session_backend_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        {
            let runtime = self
                .registry
                .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend runtime is missing for advertised frontend",
                    )
                })?;
            runtime.mark_scan_session_backend_failed(generation)?;
        }
        self.registry
            .quarantine_bound_demuxes_for_frontend(crate::registry::FrontendRuntimeId(frontend_id));
        Ok(())
    }

    pub fn mark_frontend_scan_session_callback_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        runtime.mark_scan_session_callback_failed(generation)
    }

    pub fn frontend_terminal_events(
        &self,
        frontend_id: i32,
    ) -> Result<&[maleicacid_tuner_hal2_device::FrontendTerminalEvent], HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                )
            })?;
        Ok(runtime.terminal_events())
    }

    pub fn start_frontend_worker<F>(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
        job: F,
    ) -> Result<(), FrontendWorkerStartError>
    where
        F: FnOnce(FrontendWorkerContext) -> Result<(), HalError> + Send + 'static,
    {
        self.frontend_workers
            .start(frontend_id, kind, generation, job)
    }

    pub fn request_frontend_worker_stop(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopOutcome {
        self.frontend_workers
            .request_stop(frontend_id, kind, reason)
    }

    pub fn request_frontend_worker_stop_and_join(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopOutcome {
        self.frontend_workers
            .request_stop_and_join(frontend_id, kind, reason)
    }

    pub fn clear_finished_frontend_workers(&mut self) {
        self.frontend_workers.clear_finished();
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
                                        StartupDiagnosticRecord::duplicate_frontend_id(
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

    pub fn frontend_ids(&self) -> Vec<i32> {
        self.registry
            .frontend_ids()
            .into_iter()
            .map(|id| id.0)
            .collect()
    }

    pub fn has_frontend_id(&self, id: i32) -> bool {
        self.registry
            .frontend(crate::registry::FrontendRuntimeId(id))
            .is_some()
    }

    pub fn frontend_entry(&self, id: i32) -> Option<crate::registry::FrontendRegistryEntry> {
        self.registry
            .frontend(crate::registry::FrontendRuntimeId(id))
            .cloned()
    }

    pub fn allocate_demux_runtime(
        &mut self,
    ) -> Result<crate::registry::DemuxRegistryEntry, RegistryCommitError> {
        self.registry.allocate_demux()
    }

    pub fn unregister_demux_runtime(
        &mut self,
        id: i32,
    ) -> Option<crate::registry::DemuxRegistryEntry> {
        self.cleanup_descramblers_for_demux_owner_loss(id);
        self.registry.unregister_demux(DemuxRuntimeId(id))
    }

    pub fn allocate_filter_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<crate::registry::FilterRegistryEntry, RegistryCommitError> {
        self.registry.allocate_filter(owner_demux_id)
    }

    pub fn unregister_filter_runtime(
        &mut self,
        id: i32,
    ) -> Option<crate::registry::FilterRegistryEntry> {
        let entry = self.registry.unregister_filter(FilterRuntimeId(id));
        if let Some(entry_ref) = entry.as_ref() {
            if let Some(demux_runtime) = self
                .registry
                .demux_runtime_mut(DemuxRuntimeId(entry_ref.owner_demux_id))
            {
                if demux_runtime.remove_filter(id).is_err() {
                    demux_runtime.quarantine();
                }
            }
        }
        entry
    }

    pub fn register_demux_filter_runtime(
        &mut self,
        owner_demux_id: i32,
        filter_id: i32,
        request: &OpenFilterRequest,
    ) -> Result<(), HalError> {
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .register_filter(FilterRuntime::new_open_request(
                filter_id,
                demux_runtime.generation(),
                request,
            ))
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter runtime registration failed",
                )
            })
    }

    pub fn filter_open_kind(&self, filter_id: i32) -> Option<PipelineOpenKind> {
        let entry = self.registry.filter(FilterRuntimeId(filter_id))?;
        let demux = self
            .registry
            .demux_runtime(DemuxRuntimeId(entry.owner_demux_id))?;
        demux.filter(filter_id).map(|filter| filter.open_kind())
    }

    pub fn filter_open_type(&self, filter_id: i32) -> Option<FilterOpenType> {
        let entry = self.registry.filter(FilterRuntimeId(filter_id))?;
        let demux = self
            .registry
            .demux_runtime(DemuxRuntimeId(entry.owner_demux_id))?;
        demux.filter(filter_id).map(|filter| filter.open_type())
    }

    fn map_filter_runtime_error(error: DemuxRuntimeError) -> HalError {
        match error.kind {
            DemuxRuntimeErrorKind::FilterMissing => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter runtime is missing",
            ),
            DemuxRuntimeErrorKind::SourceLifecycle
            | DemuxRuntimeErrorKind::SinkLifecycle
            | DemuxRuntimeErrorKind::InvalidState => HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter lifecycle is invalid for requested operation",
            ),
            DemuxRuntimeErrorKind::InvalidSourceSubtype
            | DemuxRuntimeErrorKind::InvalidSinkSubtype => {
                HalError::Unsupported("filter subtype is unsupported for requested operation")
            }
            DemuxRuntimeErrorKind::PidMismatch => HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "filter PID does not match requested operation",
            ),
            DemuxRuntimeErrorKind::GenerationExhausted => HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter generation exhausted",
            ),
            DemuxRuntimeErrorKind::PipelineFailed
            | DemuxRuntimeErrorKind::DvrMissing
            | DemuxRuntimeErrorKind::QueueMissing => HalError::internal(
                HalInternalKind::InvariantViolation,
                "filter runtime pipeline operation failed",
            ),
        }
    }

    fn owner_demux_id_for_filter(&self, filter_id: i32) -> Result<i32, HalError> {
        self.registry
            .filter(FilterRuntimeId(filter_id))
            .map(|entry| entry.owner_demux_id)
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter registry entry is missing",
                )
            })
    }

    pub fn configure_filter_runtime_request(
        &mut self,
        filter_id: i32,
        config: FilterConfig,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let (_txn, result) = FilterConfigureTxn::new(filter_id).configure(
            demux_runtime,
            config.open_type.pipeline_open_kind(),
            config.pipeline_config(),
        );
        result.map(|_| ()).map_err(Self::map_filter_runtime_error)
    }

    pub fn start_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .start_filter_runtime(filter_id)
            .map_err(Self::map_filter_runtime_error)
    }

    pub fn stop_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .stop_filter_runtime(filter_id)
            .map_err(Self::map_filter_runtime_error)
    }

    pub fn flush_filter_runtime(&mut self, filter_id: i32) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .flush_filter_runtime(filter_id)
            .map_err(Self::map_filter_runtime_error)
    }

    pub fn configure_filter_av_stream_type_request(
        &mut self,
        filter_id: i32,
        request: FilterAvStreamTypeRequest,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let snapshot = demux_runtime
            .filter_snapshot(filter_id)
            .map_err(Self::map_filter_runtime_error)?;
        match snapshot.state {
            FilterRuntimeState::Configured
            | FilterRuntimeState::Started
            | FilterRuntimeState::Stopped => {}
            FilterRuntimeState::Open => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "AV stream type can be configured only after filter configure",
                ));
            }
            FilterRuntimeState::Closing
            | FilterRuntimeState::CleanupFailed
            | FilterRuntimeState::Closed
            | FilterRuntimeState::Failed => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "filter is not live",
                ));
            }
        }
        let expected_kind = match snapshot.open_type {
            FilterOpenType::TsAudio => AvStreamKind::Audio,
            FilterOpenType::TsVideo => AvStreamKind::Video,
            _ => {
                return Err(HalError::Unsupported(
                    "configureAvStreamType is available only for AV filters",
                ));
            }
        };
        if snapshot.state == FilterRuntimeState::Started {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "AV stream type cannot be changed while filter is started",
            ));
        }
        let requested_kind = match request.kind {
            FilterAvStreamKind::Audio => AvStreamKind::Audio,
            FilterAvStreamKind::Video => AvStreamKind::Video,
        };
        if requested_kind != expected_kind {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::UnsupportedStreamSelector,
                "AV stream type kind must match filter open subtype",
            ));
        }
        demux_runtime
            .configure_filter_av_stream_type(
                filter_id,
                AvStreamTypeConfig {
                    kind: requested_kind,
                    stream_type: request.stream_type,
                },
            )
            .map_err(Self::map_filter_runtime_error)
    }

    pub fn set_filter_delay_hint_request(
        &mut self,
        filter_id: i32,
        request: FilterDelayHintRequest,
    ) -> Result<(), HalError> {
        let owner_demux_id = self.owner_demux_id_for_filter(filter_id)?;
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        let snapshot = demux_runtime
            .filter_snapshot(filter_id)
            .map_err(Self::map_filter_runtime_error)?;
        if snapshot.state.is_closed_or_failed() {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "filter is not live",
            ));
        }
        if matches!(
            snapshot.open_type,
            FilterOpenType::TsAudio | FilterOpenType::TsVideo
        ) {
            return Err(HalError::Unsupported(
                "FilterDelayHint is not available for media filters",
            ));
        }
        let hint = match request.kind {
            FilterDelayHintKind::TimeDelayMs => {
                FilterDelayHint::TimeDelayMs(u64::try_from(request.value).map_err(|_| {
                    HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "filter delay hint value must be non-negative",
                    )
                })?)
            }
            FilterDelayHintKind::DataSizeDelayBytes => {
                if snapshot.open_type == FilterOpenType::TsRecord {
                    return Err(HalError::invalid_argument(
                        HalInvalidArgumentKind::NumericRange,
                        "record filters do not accept data-size delay hints",
                    ));
                }
                FilterDelayHint::DataSizeDelayBytes(usize::try_from(request.value).map_err(
                    |_| {
                        HalError::invalid_argument(
                            HalInvalidArgumentKind::NumericRange,
                            "filter delay hint value is too large",
                        )
                    },
                )?)
            }
        };
        demux_runtime
            .set_filter_delay_hint(filter_id, hint)
            .map_err(Self::map_filter_runtime_error)
    }

    pub fn set_filter_data_source_non_null(
        &mut self,
        demux_id: i32,
        sink_filter_id: i32,
        source_filter_id: i32,
    ) -> Result<PipelineResetReport, HalError> {
        let sink_entry = self
            .registry
            .filter(FilterRuntimeId(sink_filter_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "sink filter registry entry is missing",
                )
            })?;
        let source_entry = self
            .registry
            .filter(FilterRuntimeId(source_filter_id))
            .ok_or_else(|| {
                HalError::invalid_argument(
                    HalInvalidArgumentKind::NumericRange,
                    "source filter registry entry is missing",
                )
            })?;
        if sink_entry.owner_demux_id != demux_id || source_entry.owner_demux_id != demux_id {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "source filter owner demux mismatch",
            ));
        }
        let Some(demux_runtime) = self.registry.demux_runtime_mut(DemuxRuntimeId(demux_id)) else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "owner demux runtime is missing",
            ));
        };
        demux_runtime
            .set_filter_source_non_null(sink_filter_id, source_filter_id)
            .map_err(|err| match err.kind {
                maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::FilterMissing => {
                    HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "source or sink filter runtime is missing")
                }
                maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::SourceLifecycle
                | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::SinkLifecycle
                | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidState => {
                    HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, "source or sink filter lifecycle is invalid")
                }
                maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidSourceSubtype
                | maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::InvalidSinkSubtype => {
                    HalError::Unsupported("source or sink filter subtype is unsupported")
                }
                maleicacid_tuner_hal2_demux::runtime::DemuxRuntimeErrorKind::PidMismatch => {
                    HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "source and sink filter PID mismatch")
                }
                _ => HalError::internal(maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation, "filter source boundary failed"),
            })
    }

    pub fn allocate_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
    ) -> Result<crate::registry::DvrRegistryEntry, RegistryCommitError> {
        self.registry.allocate_dvr(owner_demux_id)
    }

    pub fn unregister_dvr_runtime(&mut self, id: i32) -> Option<crate::registry::DvrRegistryEntry> {
        let entry = self.registry.unregister_dvr(DvrRuntimeId(id));
        if let Some(entry_ref) = entry.as_ref() {
            if let Some(demux_runtime) = self
                .registry
                .demux_runtime_mut(DemuxRuntimeId(entry_ref.owner_demux_id))
            {
                if demux_runtime.remove_dvr(id).is_err() {
                    demux_runtime.quarantine();
                }
            }
        }
        entry
    }

    pub fn register_demux_dvr_runtime(
        &mut self,
        owner_demux_id: i32,
        dvr_id: i32,
        request: &OpenDvrRequest,
        callback_present: bool,
    ) -> Result<(), HalError> {
        let Some(demux_runtime) = self
            .registry
            .demux_runtime_mut(DemuxRuntimeId(owner_demux_id))
        else {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "owner demux runtime is missing",
            ));
        };
        let kind = match request.kind {
            DvrOpenKind::Record => DvrKind::Record,
            DvrOpenKind::Playback => DvrKind::Playback,
        };
        demux_runtime
            .register_dvr(DvrRuntime::new_open_request(
                dvr_id,
                kind,
                demux_runtime.generation(),
                request.buffer_size,
                callback_present,
            ))
            .map_err(|_| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "DVR runtime registration failed",
                )
            })
    }

    pub fn allocate_descrambler_runtime(
        &mut self,
    ) -> Result<crate::registry::DescramblerRegistryEntry, RegistryCommitError> {
        self.registry.allocate_descrambler()
    }

    fn descrambler_runtime_mut(
        &mut self,
        descrambler_id: i32,
    ) -> Result<&mut maleicacid_tuner_hal2_descrambler::DescramblerRuntime, HalError> {
        self.registry
            .descrambler_runtime_mut(DescramblerRuntimeId(descrambler_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "descrambler runtime is missing",
                )
            })
    }

    fn descrambler_bound_demux(&self, descrambler_id: i32) -> Result<(i32, u64), HalError> {
        let runtime = self
            .registry
            .descrambler_runtime(DescramblerRuntimeId(descrambler_id))
            .ok_or_else(|| {
                HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "descrambler runtime is missing",
                )
            })?;
        let demux_id = runtime.session().demux_id().ok_or_else(|| {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler demux source is not bound",
            )
        })?;
        let demux_generation = runtime.session().demux_generation().ok_or_else(|| {
            HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler demux generation is not bound",
            )
        })?;
        Ok((demux_id, demux_generation))
    }

    fn validate_descrambler_source_filter(
        &self,
        expected_demux_id: i32,
        expected_demux_generation: u64,
        source_filter_id: i32,
        pid: u16,
    ) -> Result<u64, HalError> {
        let filter_entry = self
            .registry
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
        let Some(demux_runtime) = self
            .registry
            .demux_runtime(DemuxRuntimeId(filter_entry.owner_demux_id))
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
            .map_err(Self::map_filter_runtime_error)?;
        if source_snapshot.state == FilterRuntimeState::Open
            || source_snapshot.state.is_closed_or_failed()
            || source_snapshot.tpid.is_none()
        {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "source filter is not configured",
            ));
        }
        if source_snapshot.tpid != Some(i32::from(pid)) {
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

    pub fn set_descrambler_demux_source(
        &mut self,
        descrambler_id: i32,
        demux_id: i32,
    ) -> Result<(), HalError> {
        let demux_runtime = self
            .registry
            .demux_runtime(DemuxRuntimeId(demux_id))
            .ok_or(HalError::Unsupported("demux id is not available"))?;
        match demux_runtime.state() {
            DemuxRuntimeState::Open => {}
            DemuxRuntimeState::Closing
            | DemuxRuntimeState::CleanupFailed
            | DemuxRuntimeState::Closed
            | DemuxRuntimeState::Failed
            | DemuxRuntimeState::Quarantined => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "demux runtime is not live",
                ));
            }
        }
        let demux_generation = demux_runtime.generation();
        let runtime = self.descrambler_runtime_mut(descrambler_id)?;
        let mut txn = DescramblerSessionTxn::new();
        txn.bind_demux(runtime.session_mut(), demux_id, demux_generation)
            .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))
    }

    pub fn set_descrambler_key_token(
        &mut self,
        descrambler_id: i32,
        key_token: &[u8],
    ) -> Result<(), HalError> {
        if key_token == [0x00].as_slice() {
            let clear_result = {
                let runtime = self.descrambler_runtime_mut(descrambler_id)?;
                let mut txn = DescramblerSessionTxn::new();
                txn.clear_key(runtime.session_mut())
                    .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))
            };
            let old_token = match clear_result {
                Ok(old_token) => old_token,
                Err(error) => {
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                        descrambler_id,
                        DescramblerDiagnosticKind::SessionClosed,
                        error.clone(),
                    ));
                    return Err(error);
                }
            };
            if let Some(old_token) = old_token {
                if let Err(error) = self
                    .registry
                    .descrambler_key_table_mut()
                    .release(&old_token)
                    .map_err(descrambler_key_release_error_to_hal)
                {
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                        descrambler_id,
                        DescramblerDiagnosticKind::KeyTokenReleaseFailed,
                        error.clone(),
                    ));
                    return Err(error);
                }
            }
            return Ok(());
        }
        let token = match DescramblerKeyToken::try_from_bytes(key_token.to_vec()) {
            Ok(token) => token,
            Err(error) => {
                let kind = match error {
                    DescramblerKeyTokenError::Empty => DescramblerDiagnosticKind::KeyTokenEmpty,
                    DescramblerKeyTokenError::InvalidLength { .. } => {
                        DescramblerDiagnosticKind::KeyTokenInvalidLength
                    }
                };
                let hal_error = descrambler_key_token_error_to_hal(error);
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    kind,
                    hal_error.clone(),
                ));
                return Err(hal_error);
            }
        };
        let old_token_result = {
            let runtime = self.descrambler_runtime_mut(descrambler_id)?;
            if runtime.session().is_closed() {
                Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "descrambler session is closed",
                ))
            } else if runtime.session().key_token() == Some(&token) {
                return Ok(());
            } else {
                Ok(runtime.session().key_token().cloned())
            }
        };
        let old_token = match old_token_result {
            Ok(old_token) => old_token,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    DescramblerDiagnosticKind::SessionClosed,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        if self.registry.descrambler_key_table().is_empty() {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler CAS token producer is not connected",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                descrambler_id,
                DescramblerDiagnosticKind::CasTokenProducerUnavailable,
                error.clone(),
            ));
            return Err(error);
        }
        let key_slot = match self.registry.descrambler_key_table_mut().acquire(&token) {
            Ok(key_slot) => key_slot,
            Err(error) => {
                let kind = match error {
                    DescramblerKeyLookupError::UnknownToken => {
                        DescramblerDiagnosticKind::KeyTokenUnknown
                    }
                    DescramblerKeyLookupError::ExpiredToken => {
                        DescramblerDiagnosticKind::KeyTokenExpired
                    }
                };
                let hal_error = descrambler_key_lookup_error_to_hal(error);
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    kind,
                    hal_error.clone(),
                ));
                return Err(hal_error);
            }
        };
        if let Some(old_token) = old_token {
            if let Err(error) = self
                .registry
                .descrambler_key_table_mut()
                .release(&old_token)
            {
                let hal_error = descrambler_key_release_error_to_hal(error);
                if let Err(rollback_error) =
                    self.registry.descrambler_key_table_mut().release(&token)
                {
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                        descrambler_id,
                        DescramblerDiagnosticKind::KeyTokenReleaseFailed,
                        descrambler_key_release_error_to_hal(rollback_error),
                    ));
                }
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::set_key_token(
                    descrambler_id,
                    DescramblerDiagnosticKind::KeyTokenReleaseFailed,
                    hal_error.clone(),
                ));
                return Err(hal_error);
            }
        }
        let runtime = self.descrambler_runtime_mut(descrambler_id)?;
        runtime.session_mut().replace_key(token, key_slot);
        Ok(())
    }

    pub fn add_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        let (demux_id, demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::AddPid,
                    descrambler_id,
                    None,
                    pid,
                    source_filter_id,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        let source_generation = match self.validate_descrambler_source_filter(
            demux_id,
            demux_generation,
            source_filter_id,
            pid,
        ) {
            Ok(source_generation) => source_generation,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::AddPid,
                    descrambler_id,
                    Some(demux_id),
                    pid,
                    source_filter_id,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        if self.registry.descrambler_pid_claimed_by_other(
            DescramblerRuntimeId(descrambler_id),
            demux_id,
            demux_generation,
            pid,
        ) {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "descrambler PID is already claimed by another session",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                Some(demux_id),
                pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        let claim =
            match DescramblerPidClaim::from_source_filter(pid, source_filter_id, source_generation)
            {
                Ok(claim) => claim,
                Err(error) => {
                    let hal_error = descrambler_pid_claim_error_to_hal(error);
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                        DescramblerDiagnosticPhase::AddPid,
                        descrambler_id,
                        Some(demux_id),
                        pid,
                        source_filter_id,
                        hal_error.clone(),
                    ));
                    return Err(hal_error);
                }
            };
        let add_result = {
            let runtime = self.descrambler_runtime_mut(descrambler_id)?;
            let mut txn = DescramblerSessionTxn::new();
            txn.add_pid_claim(runtime.session_mut(), claim)
                .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))
        };
        if let Err(error) = add_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::AddPid,
                descrambler_id,
                Some(demux_id),
                pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        Ok(())
    }

    pub fn remove_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        let (demux_id, demux_generation) = match self.descrambler_bound_demux(descrambler_id) {
            Ok(bound) => bound,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::RemovePid,
                    descrambler_id,
                    None,
                    pid,
                    source_filter_id,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        let source_generation = match self.validate_descrambler_source_filter(
            demux_id,
            demux_generation,
            source_filter_id,
            pid,
        ) {
            Ok(source_generation) => source_generation,
            Err(error) => {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                    DescramblerDiagnosticPhase::RemovePid,
                    descrambler_id,
                    Some(demux_id),
                    pid,
                    source_filter_id,
                    error.clone(),
                ));
                return Err(error);
            }
        };
        let claim =
            match DescramblerPidClaim::from_source_filter(pid, source_filter_id, source_generation)
            {
                Ok(claim) => claim,
                Err(error) => {
                    let hal_error = descrambler_pid_claim_error_to_hal(error);
                    self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                        DescramblerDiagnosticPhase::RemovePid,
                        descrambler_id,
                        Some(demux_id),
                        pid,
                        source_filter_id,
                        hal_error.clone(),
                    ));
                    return Err(hal_error);
                }
            };
        let stale_source_generation = {
            let runtime = self.descrambler_runtime_mut(descrambler_id)?;
            runtime.session().pid_claims().iter().any(|stored| {
                stored.pid().0 == pid
                    && stored.source_filter().filter_id == source_filter_id
                    && stored.source_filter().generation != source_generation
            })
        };
        if stale_source_generation {
            let error = HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "source filter generation changed before PID removal",
            );
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::RemovePid,
                descrambler_id,
                Some(demux_id),
                pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        let remove_result = {
            let runtime = self.descrambler_runtime_mut(descrambler_id)?;
            let mut txn = DescramblerSessionTxn::new();
            txn.remove_pid_claim(runtime.session_mut(), claim)
                .map_err(|failure| descrambler_session_failure_to_hal(failure.kind))
        };
        if let Err(error) = remove_result {
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::pid_claim(
                DescramblerDiagnosticPhase::RemovePid,
                descrambler_id,
                Some(demux_id),
                pid,
                source_filter_id,
                error.clone(),
            ));
            return Err(error);
        }
        Ok(())
    }

    pub fn unregister_descrambler_runtime(
        &mut self,
        id: i32,
    ) -> Option<crate::registry::DescramblerRegistryEntry> {
        self.cleanup_descrambler_session(id);
        self.registry
            .unregister_descrambler(DescramblerRuntimeId(id))
    }

    fn cleanup_descramblers_for_demux_owner_loss(&mut self, demux_id: i32) {
        let descrambler_ids = self.registry.descrambler_ids_bound_to_demux(demux_id);
        for descrambler_id in descrambler_ids {
            self.cleanup_descrambler_session(descrambler_id.0);
        }
    }

    fn cleanup_descrambler_session(&mut self, id: i32) {
        let mut cleanup_failure = None;
        let old_token = if let Some(runtime) = self
            .registry
            .descrambler_runtime_mut(DescramblerRuntimeId(id))
        {
            let mut txn = DescramblerSessionTxn::new();
            let old_token = runtime.session().key_token().cloned();
            let cleanup_report = txn.cleanup_all(runtime.session_mut());
            cleanup_failure = cleanup_report
                .failure()
                .map(|failure| descrambler_session_failure_to_hal(failure.kind));
            old_token
        } else {
            None
        };
        if let Some(error) = cleanup_failure {
            self.record_descrambler_diagnostic(
                DescramblerDiagnosticRecord::cleanup_release_failed(id, error),
            );
        }
        if let Some(old_token) = old_token {
            if let Err(error) = self
                .registry
                .descrambler_key_table_mut()
                .release(&old_token)
                .map_err(descrambler_key_release_error_to_hal)
            {
                self.record_descrambler_diagnostic(
                    DescramblerDiagnosticRecord::cleanup_release_failed(id, error),
                );
            }
        }
    }

    pub fn demux_ids(&self) -> Vec<i32> {
        self.registry
            .demux_ids()
            .into_iter()
            .map(|id| id.0)
            .collect()
    }

    pub fn has_demux_id(&self, id: i32) -> bool {
        self.registry.demux(DemuxRuntimeId(id)).is_some()
    }

    pub fn lnb_ids(&self) -> Vec<i32> {
        self.registry.lnb_ids().into_iter().map(|id| id.0).collect()
    }

    pub fn has_lnb_id(&self, id: i32) -> bool {
        self.registry.lnb(LnbRuntimeId(id)).is_some()
    }

    pub fn lnb_id_by_name(&self, name: &str) -> Option<i32> {
        self.registry.lnb_by_name(name).map(|entry| entry.id.0)
    }

    pub fn lnb_for_frontend_id(
        &self,
        frontend_id: i32,
    ) -> Option<crate::registry::LnbRegistryEntry> {
        self.registry
            .lnb_for_frontend(FrontendRuntimeId(frontend_id))
            .cloned()
    }

    pub fn set_demux_frontend_data_source(
        &mut self,
        demux_id: i32,
        frontend_id: i32,
    ) -> Result<GenerationBoundaryReport, HalError> {
        let demux_key = DemuxRuntimeId(demux_id);
        let frontend_key = FrontendRuntimeId(frontend_id);

        let Some(frontend_runtime) = self.registry.frontend_runtime(frontend_key) else {
            return Err(HalError::Unsupported(
                "frontend id is not available for demux source binding",
            ));
        };
        match frontend_runtime.state() {
            FrontendRuntimeState::Closing | FrontendRuntimeState::Failed => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "frontend runtime is closing or failed",
                ));
            }
            FrontendRuntimeState::Idle
            | FrontendRuntimeState::Tuning { .. }
            | FrontendRuntimeState::Scanning { .. } => {}
        }

        let Some(demux_runtime) = self.registry.demux_runtime_mut(demux_key) else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtime is missing",
            ));
        };
        let generation = DemuxStreamGeneration(demux_runtime.generation());
        let (_, report) =
            GenerationBoundaryTxn::for_reason(generation, PipelineBoundaryReason::TuneStart)
                .apply(demux_runtime);
        let report = report.map_err(demux_runtime_error_to_hal)?;
        self.registry.bind_demux_frontend(demux_key, frontend_key);
        Ok(report)
    }

    pub fn reset_bound_demuxes_for_frontend_tune_start(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported(
                "frontend id is not available for tune boundary reset",
            ));
        }
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        let mut reports = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            let Some(demux_runtime) = self.registry.demux_runtime_mut(demux_id) else {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing during tune boundary reset",
                ));
            };
            let generation = DemuxStreamGeneration(demux_runtime.generation());
            let (_, report) =
                GenerationBoundaryTxn::for_reason(generation, PipelineBoundaryReason::TuneStart)
                    .apply(demux_runtime);
            reports.push(report.map_err(demux_runtime_error_to_hal)?);
        }
        Ok(reports)
    }

    pub fn reset_and_unbind_bound_demuxes_for_frontend(
        &mut self,
        frontend_id: i32,
        reason: PipelineBoundaryReason,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported(
                "frontend id is not available for demux unbind",
            ));
        }
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        let mut reports = Vec::with_capacity(demux_ids.len());
        for demux_id in &demux_ids {
            let Some(demux_runtime) = self.registry.demux_runtime_mut(*demux_id) else {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing during frontend unbind",
                ));
            };
            let generation = DemuxStreamGeneration(demux_runtime.generation());
            let (_, report) =
                GenerationBoundaryTxn::for_reason(generation, reason).apply(demux_runtime);
            reports.push(report.map_err(demux_runtime_error_to_hal)?);
        }
        self.registry.unbind_frontend_demuxes(frontend_key);
        Ok(reports)
    }

    pub fn quarantine_frontend_and_bound_demuxes(
        &mut self,
        frontend_id: i32,
        error: HalError,
    ) -> Result<Vec<DemuxRuntimeId>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let demux_ids = self
            .registry
            .quarantine_bound_demuxes_for_frontend(frontend_key);
        let runtime = self
            .registry
            .frontend_runtime_mut(frontend_key)
            .ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for quarantine",
                )
            })?;
        runtime.mark_failed(error);
        Ok(demux_ids)
    }

    pub fn ensure_frontend_demux_sink_ready(
        &self,
        frontend_id: i32,
    ) -> Result<Vec<DemuxRuntimeId>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported(
                "frontend id is not available for live TS delivery",
            ));
        }
        let demux_ids = self.registry.frontend_bound_demux_ids(frontend_key);
        if demux_ids.is_empty() {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend has no bound demux for live TS delivery",
            ));
        }
        Ok(demux_ids)
    }

    pub fn push_frontend_ts_packet_to_bound_demuxes(
        &mut self,
        frontend_id: i32,
        packet: &[u8],
    ) -> Result<Vec<PipelineReport>, HalError> {
        let demux_ids = self.ensure_frontend_demux_sink_ready(frontend_id)?;
        let mut reports = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            let (demux_generation, report) = {
                let Some(demux_runtime) = self.registry.demux_runtime_mut(demux_id) else {
                    return Err(HalError::invalid_state(
                        HalInvalidStateKind::InvalidLifecycle,
                        "bound demux runtime is missing",
                    ));
                };
                let demux_generation = demux_runtime.generation();
                let report =
                    demux_runtime.push_ts_packet_from_origin(packet, TsInputOrigin::Frontend);
                (demux_generation, report)
            };
            self.record_descrambler_packet_diagnostics(demux_id.0, demux_generation, &report);
            reports.push(report);
        }
        Ok(reports)
    }

    fn record_descrambler_packet_diagnostics(
        &mut self,
        demux_id: i32,
        demux_generation: u64,
        report: &PipelineReport,
    ) {
        let keyless_suppressed = report
            .assembly_suppression_reasons
            .contains(&PipelineAssemblySuppressionReason::KeylessScrambledWithoutDescrambler);
        if !keyless_suppressed {
            return;
        }
        let pids = report.diagnostics.iter().filter_map(|diagnostic| {
            if diagnostic.kind != PipelineDiagnosticKind::KeylessScrambledAssemblySuppressed {
                return None;
            }
            diagnostic.pid.and_then(|pid| u16::try_from(pid).ok())
        });
        for pid in pids {
            let keyless_claim = self
                .registry
                .descrambler_key_slot_for_demux_pid(demux_id, demux_generation, pid)
                .is_some_and(|key_slot| key_slot.is_none());
            if keyless_claim {
                self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy(
                    demux_id,
                    pid,
                    DescramblerDiagnosticKind::PacketScrambledWithoutKey,
                ));
            }
            self.record_descrambler_diagnostic(DescramblerDiagnosticRecord::packet_policy(
                demux_id,
                pid,
                DescramblerDiagnosticKind::PacketAssemblySuppressed,
            ));
        }
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

    pub fn begin_aidl_object_close(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        step: maleicacid_tuner_hal2_resource_ledger::CleanupStep,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        self.object_table.begin_close(object_id, generation, step)
    }

    pub fn mark_aidl_object_cleanup_failed(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        step: maleicacid_tuner_hal2_resource_ledger::CleanupStep,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        self.object_table
            .mark_cleanup_failed(object_id, generation, step)
    }

    pub fn commit_aidl_object_close(
        &mut self,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
    ) -> Result<RuntimeObjectEntry, RuntimeObjectTableError> {
        self.object_table.commit_close(object_id, generation)
    }

    pub fn unregister_public_runtime_for_closed_aidl_entry(&mut self, entry: &RuntimeObjectEntry) {
        match entry.object_kind {
            AidlObjectKind::Demux => {
                if let Ok(id) = i32::try_from(entry.ledger_id.0) {
                    self.unregister_demux_runtime(id);
                }
            }
            AidlObjectKind::Filter => {
                if let Ok(id) = i32::try_from(entry.ledger_id.0) {
                    self.unregister_filter_runtime(id);
                }
            }
            AidlObjectKind::Dvr => {
                if let Ok(id) = i32::try_from(entry.ledger_id.0) {
                    self.unregister_dvr_runtime(id);
                }
            }
            AidlObjectKind::Descrambler => {
                if let Ok(id) = i32::try_from(entry.ledger_id.0) {
                    self.unregister_descrambler_runtime(id);
                }
            }
            _ => {}
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
