use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind, CommandPlan, RuntimeExecutableRequest, RuntimeTransactionName};
use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem, FrontendDevicePath, FrontendTuneRequest, HalError, HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind, TS_PACKET_SIZE};
use maleicacid_tuner_hal2_device::{FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind, FrontendWorkerRegistry, FrontendWorkerStartError, FrontendWorkerStopOutcome, FrontendLiveReaderDescriptor, FrontendRuntimeState, FrontendRuntimeSnapshot, FrontendSignalState, FrontendLivePacketSink, FrontendLivePumpOwner};
use maleicacid_tuner_hal2_demux::{DemuxRuntime, DemuxRuntimeSnapshot, DemuxStreamGeneration, FilterRuntime, GenerationBoundaryReport, GenerationBoundaryTxn, TsInputOrigin};
use maleicacid_tuner_hal2_demux::packet_pipeline::{PipelineOpenKind, PipelineResetReport};
use maleicacid_tuner_hal2_demux::packet_pipeline::{PipelineBoundaryReason, PipelineReport};

use crate::diagnostics::{CapabilitySuppressionReason, StartupDiagnosticRecord};
use crate::command_dispatch::{RuntimeCommandDispatchError, RuntimeCommandDispatchPlan, RuntimeCommandDispatcher};
use crate::dispatch::{adapter_transactions_are_covered, dispatch_target_for, ServiceRuntimeDispatchTarget};
use crate::registry::{DescramblerRuntimeId, DemuxRuntimeId, DvrRuntimeId, FilterRuntimeId, FrontendRegistryEntry, FrontendRuntimeId, LnbRegistryEntry, LnbRegistryProfile, LnbRuntimeId, RegistryCommitError, RuntimeRegistry};
use crate::object_table::{RuntimeObjectEntry, RuntimeObjectLifecycle, RuntimeObjectTable, RuntimeObjectTableError, RuntimeOwnerRelation};
use maleicacid_tuner_hal2_resource_ledger::{LedgerGeneration, LedgerId};
use crate::ServiceState;
use crate::callback_registry::RuntimeCallbackRegistry;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendProbeOutcome {
    Available { id: FrontendRuntimeId, backend: FrontendBackendKind, system: FrontendSystem, path: PathBuf },
    DeviceMissing { backend: FrontendBackendKind, path: PathBuf },
    DeviceOpenFailed { backend: FrontendBackendKind, path: PathBuf, error: HalError },
    CapabilitySuppressed { backend: FrontendBackendKind, path: PathBuf, reason: CapabilitySuppressionReason },
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

fn live_reader_descriptor_for_frontend_entry(entry: &FrontendRegistryEntry) -> Result<FrontendLiveReaderDescriptor, HalError> {
    match entry.backend {
        FrontendBackendKind::Px4CharDevice => Ok(FrontendLiveReaderDescriptor::px4_from_control_fd(
            entry.id.0,
            FrontendDevicePath::new(entry.device_path.clone()),
        )),
        FrontendBackendKind::LinuxDvb => {
            let dvr_path = dvb_dvr_path_for_frontend_path(&entry.device_path).ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    format!("DVB frontend path has no parent directory: {}", entry.device_path.display()),
                )
            })?;
            Ok(FrontendLiveReaderDescriptor::dvb_dvr_device(entry.id.0, FrontendDevicePath::new(dvr_path)))
        }
    }
}

fn px4_lnb_profile_for_path(path: &std::path::Path) -> LnbRegistryProfile {
    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or_default();
    if name.starts_with("px4video") {
        LnbRegistryProfile::Px4Device15VOnly
    } else {
        LnbRegistryProfile::NoPower
    }
}

fn default_lnb_profile_for_frontend(backend: FrontendBackendKind, system: FrontendSystem, path: &std::path::Path) -> Option<LnbRegistryProfile> {
    if !matches!(system, FrontendSystem::IsdbS) {
        return None;
    }
    Some(match backend {
        FrontendBackendKind::Px4CharDevice => px4_lnb_profile_for_path(path),
        FrontendBackendKind::LinuxDvb => LnbRegistryProfile::EarthPt1FixedLnb,
    })
}

fn default_lnb_entry_for_frontend(entry: &FrontendRegistryEntry) -> Option<LnbRegistryEntry> {
    let profile = entry.lnb_profile?;
    let id = LnbRuntimeId(entry.id.0.checked_add(10_000)?);
    let name = match entry.backend {
        FrontendBackendKind::Px4CharDevice => {
            let dev = entry.device_path.file_name().and_then(|v| v.to_str()).unwrap_or("unknown");
            let rel = entry.id.0.saturating_sub(1_000_000);
            let unit = rel.rem_euclid(10_000).div_euclid(10);
            Some(format!("maleicacid-lnb-px4-{dev}-unit-{unit}"))
        }
        FrontendBackendKind::LinuxDvb => {
            let path = entry.device_path.display().to_string();
            Some(format!("maleicacid-lnb-{path}"))
        }
    };
    Some(LnbRegistryEntry { id, name, owner_frontend_id: entry.id, profile })
}

#[derive(Clone)]
pub struct FrontendDemuxPacketSink {
    runtime: Arc<Mutex<TunerServiceRuntime>>,
    frontend_id: i32,
}

impl FrontendDemuxPacketSink {
    pub fn new(runtime: Arc<Mutex<TunerServiceRuntime>>, frontend_id: i32) -> Self {
        Self { runtime, frontend_id }
    }

    pub fn frontend_id(&self) -> i32 { self.frontend_id }
}

impl FrontendLivePacketSink for FrontendDemuxPacketSink {
    fn deliver_ts_packet(&mut self, packet: &[u8; TS_PACKET_SIZE]) -> Result<(), HalError> {
        self.runtime
            .lock()
            .map_err(|_| HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while delivering frontend TS packet",
            ))?
            .push_frontend_ts_packet_to_bound_demuxes(self.frontend_id, packet)
            .map(|_| ())
    }
}

pub fn start_frontend_demux_live_pump_from_reader(
    runtime: Arc<Mutex<TunerServiceRuntime>>,
    frontend_id: i32,
    reader: Box<dyn Read + Send>,
) -> Result<FrontendLivePumpOwner, HalError> {
    {
        let guard = runtime.lock().map_err(|_| HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while preparing frontend demux live pump",
        ))?;
        guard.ensure_frontend_demux_sink_ready(frontend_id)?;
    }
    let sink: Box<dyn FrontendLivePacketSink> = Box::new(FrontendDemuxPacketSink::new(Arc::clone(&runtime), frontend_id));
    FrontendLivePumpOwner::start(reader, sink)
}

#[derive(Debug)]
pub struct TunerServiceRuntime {
    state: ServiceState,
    registry: RuntimeRegistry,
    object_table: RuntimeObjectTable,
    diagnostics: Vec<StartupDiagnosticRecord>,
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
        Self { state: ServiceState::Booting, registry: RuntimeRegistry::default(), object_table: RuntimeObjectTable::default(), diagnostics: Vec::new(), callback_registry: RuntimeCallbackRegistry::default(), frontend_workers: FrontendWorkerRegistry::default(), next_aidl_generation: 0, next_aidl_object_id: 0 }
    }

    pub fn state(&self) -> ServiceState {
        self.state
    }

    pub fn registry(&self) -> &RuntimeRegistry {
        &self.registry
    }

    pub fn diagnostics(&self) -> &[StartupDiagnosticRecord] {
        &self.diagnostics
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

    pub fn frontend_worker_running_generation(&mut self, frontend_id: i32, kind: FrontendWorkerKind) -> Option<u64> {
        self.frontend_workers.running_generation(frontend_id, kind)
    }

    pub fn frontend_runtime_snapshot(&self, frontend_id: i32) -> Result<FrontendRuntimeSnapshot, HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
            let demux = self.registry.demux_runtime(demux_id).ok_or_else(|| HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "bound demux runtime is missing while taking tune rollback snapshot",
            ))?;
            snapshots.push((demux_id, demux.snapshot()));
        }
        Ok(snapshots)
    }

    pub fn restore_bound_demux_runtime_snapshots(
        &mut self,
        snapshots: Vec<(DemuxRuntimeId, DemuxRuntimeSnapshot)>,
    ) -> Result<(), HalError> {
        for (demux_id, snapshot) in snapshots {
            let demux = self.registry.demux_runtime_mut(demux_id).ok_or_else(|| HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "bound demux runtime is missing while restoring tune rollback snapshot",
            ))?;
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
        runtime.record_signal_state(generation, signal_state)
    }

    pub fn frontend_signal_state(&self, frontend_id: i32) -> Result<FrontendSignalState, HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
        Ok(runtime.signal_state())
    }

    pub fn prepare_frontend_worker_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Result<u64, HalError> {
        if self.frontend_workers.running_generation(frontend_id, kind).is_some() {
            return Err(HalError::invalid_state(
                maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                "frontend worker is already running",
            ));
        }
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend registry entry is missing for advertised frontend",
            ))?;
        let reader = live_reader_descriptor_for_frontend_entry(&entry)?;
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
        runtime.commit_generation(generation)?;
        runtime.set_live_reader_descriptor(reader);
        match kind {
            FrontendWorkerKind::Tune => runtime.mark_tuning(generation),
            FrontendWorkerKind::Scan => runtime.mark_scanning(generation),
        }
        Ok(())
    }

    pub fn frontend_live_reader_descriptor_for_live_pump(&self, frontend_id: i32) -> Result<Option<FrontendLiveReaderDescriptor>, HalError> {
        let frontend_key = crate::registry::FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported("frontend id is not available for live pump"));
        }
        if self.registry.frontend_bound_demux_ids(frontend_key).is_empty() {
            return Ok(None);
        }
        let runtime = self
            .registry
            .frontend_runtime(frontend_key)
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
        runtime
            .live_reader_descriptor()
            .cloned()
            .map(Some)
            .ok_or_else(|| HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend has bound demux but no live reader descriptor",
            ))
    }

    pub fn clear_frontend_live_reader_descriptor_and_idle(&mut self, frontend_id: i32) -> Result<(), HalError> {
        let runtime = self
            .registry
            .frontend_runtime_mut(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
        runtime.clear_live_reader_descriptor();
        runtime.mark_idle();
        Ok(())
    }

    pub fn stop_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.clear_frontend_live_reader_descriptor_and_idle(frontend_id)?;
        self.reset_and_unbind_bound_demuxes_for_frontend(frontend_id, PipelineBoundaryReason::FrontendUnbind)
    }

    pub fn close_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let frontend_key = crate::registry::FrontendRuntimeId(frontend_id);
        let runtime = self
            .registry
            .frontend_runtime_mut(frontend_key)
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
        runtime.clear_live_reader_descriptor();
        runtime.mark_closing();
        self.reset_and_unbind_bound_demuxes_for_frontend(frontend_id, PipelineBoundaryReason::FrontendClose)
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
                .ok_or_else(|| HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                ))?;
            runtime.mark_tune_worker_failed(generation, error)?;
        }
        self.registry.quarantine_bound_demuxes_for_frontend(crate::registry::FrontendRuntimeId(frontend_id));
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
                .ok_or_else(|| HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend runtime is missing for advertised frontend",
                ))?;
            runtime.mark_scan_session_backend_failed(generation)?;
        }
        self.registry.quarantine_bound_demuxes_for_frontend(crate::registry::FrontendRuntimeId(frontend_id));
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
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
        runtime.mark_scan_session_callback_failed(generation)
    }

    pub fn frontend_terminal_events(
        &self,
        frontend_id: i32,
    ) -> Result<&[maleicacid_tuner_hal2_device::FrontendTerminalEvent], HalError> {
        let runtime = self
            .registry
            .frontend_runtime(crate::registry::FrontendRuntimeId(frontend_id))
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for advertised frontend",
            ))?;
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
        self.frontend_workers.start(frontend_id, kind, generation, job)
    }

    pub fn request_frontend_worker_stop(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopOutcome {
        self.frontend_workers.request_stop(frontend_id, kind, reason)
    }

    pub fn request_frontend_worker_stop_and_join(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopOutcome {
        self.frontend_workers.request_stop_and_join(frontend_id, kind, reason)
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
            self.diagnostics.push(StartupDiagnosticRecord::runtime_dispatch_missing());
        }

        for result in results {
            match result {
                FrontendProbeOutcome::Available { id, backend, system, path } => {
                    let lnb_profile = default_lnb_profile_for_frontend(backend, system, &path);
                    let entry = FrontendRegistryEntry { id, backend, system, device_path: path.clone(), lnb_profile };
                    match self.registry.register_frontend(entry.clone()) {
                        Ok(()) => {
                            if let Some(lnb_entry) = default_lnb_entry_for_frontend(&entry) {
                                if let Err(RegistryCommitError::DuplicateLnbId { .. }) = self.registry.register_lnb(lnb_entry) {
                                    self.diagnostics.push(StartupDiagnosticRecord::duplicate_frontend_id(backend, path.clone()));
                                }
                            }
                        }
                        Err(RegistryCommitError::DuplicateFrontendId { .. }) => {
                            self.diagnostics.push(StartupDiagnosticRecord::duplicate_frontend_id(backend, path));
                        }
                        Err(_) => {
                            self.diagnostics.push(StartupDiagnosticRecord::duplicate_frontend_id(backend, path));
                        }
                    }
                }
                FrontendProbeOutcome::DeviceMissing { backend, path } => {
                    self.diagnostics.push(StartupDiagnosticRecord::device_missing(backend, path));
                }
                FrontendProbeOutcome::DeviceOpenFailed { backend, path, error } => {
                    self.diagnostics.push(StartupDiagnosticRecord::device_open_failed(backend, path, error));
                }
                FrontendProbeOutcome::CapabilitySuppressed { backend, path, reason } => {
                    self.diagnostics.push(StartupDiagnosticRecord::capability_suppressed(backend, path, reason));
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

    pub fn dispatch_target(&mut self, transaction: RuntimeTransactionName) -> Option<ServiceRuntimeDispatchTarget> {
        let target = dispatch_target_for(transaction);
        if target.is_none() {
            self.diagnostics.push(StartupDiagnosticRecord::runtime_dispatch_missing());
        }
        target
    }


    pub fn frontend_ids(&self) -> Vec<i32> {
        self.registry.frontend_ids().into_iter().map(|id| id.0).collect()
    }

    pub fn has_frontend_id(&self, id: i32) -> bool {
        self.registry.frontend(crate::registry::FrontendRuntimeId(id)).is_some()
    }

    pub fn frontend_entry(&self, id: i32) -> Option<crate::registry::FrontendRegistryEntry> {
        self.registry.frontend(crate::registry::FrontendRuntimeId(id)).cloned()
    }


    pub fn allocate_demux_runtime(&mut self) -> Result<crate::registry::DemuxRegistryEntry, RegistryCommitError> {
        self.registry.allocate_demux()
    }

    pub fn unregister_demux_runtime(&mut self, id: i32) -> Option<crate::registry::DemuxRegistryEntry> {
        self.registry.unregister_demux(DemuxRuntimeId(id))
    }

    pub fn allocate_filter_runtime(&mut self, owner_demux_id: i32) -> Result<crate::registry::FilterRegistryEntry, RegistryCommitError> {
        self.registry.allocate_filter(owner_demux_id)
    }

    pub fn unregister_filter_runtime(&mut self, id: i32) -> Option<crate::registry::FilterRegistryEntry> {
        let entry = self.registry.unregister_filter(FilterRuntimeId(id));
        if let Some(entry_ref) = entry.as_ref() {
            if let Some(demux_runtime) = self.registry.demux_runtime_mut(DemuxRuntimeId(entry_ref.owner_demux_id)) {
                if demux_runtime.remove_filter(id).is_err() {
                    demux_runtime.quarantine();
                }
            }
        }
        entry
    }

    pub fn register_demux_filter_runtime(&mut self, owner_demux_id: i32, filter_id: i32, open_kind: PipelineOpenKind) -> Result<(), HalError> {
        let Some(demux_runtime) = self.registry.demux_runtime_mut(DemuxRuntimeId(owner_demux_id)) else {
            return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "owner demux runtime is missing"));
        };
        demux_runtime
            .register_filter(FilterRuntime::new(filter_id, demux_runtime.generation(), open_kind))
            .map_err(|_| HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, "filter runtime registration failed"))
    }

    pub fn set_filter_data_source_non_null(&mut self, demux_id: i32, sink_filter_id: i32, source_filter_id: i32) -> Result<PipelineResetReport, HalError> {
        let sink_entry = self.registry.filter(FilterRuntimeId(sink_filter_id)).ok_or_else(|| {
            HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, "sink filter registry entry is missing")
        })?;
        let source_entry = self.registry.filter(FilterRuntimeId(source_filter_id)).ok_or_else(|| {
            HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "source filter registry entry is missing")
        })?;
        if sink_entry.owner_demux_id != demux_id || source_entry.owner_demux_id != demux_id {
            return Err(HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "source filter owner demux mismatch"));
        }
        let Some(demux_runtime) = self.registry.demux_runtime_mut(DemuxRuntimeId(demux_id)) else {
            return Err(HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, "owner demux runtime is missing"));
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

    pub fn allocate_dvr_runtime(&mut self, owner_demux_id: i32) -> Result<crate::registry::DvrRegistryEntry, RegistryCommitError> {
        self.registry.allocate_dvr(owner_demux_id)
    }

    pub fn unregister_dvr_runtime(&mut self, id: i32) -> Option<crate::registry::DvrRegistryEntry> {
        self.registry.unregister_dvr(DvrRuntimeId(id))
    }

    pub fn allocate_descrambler_runtime(&mut self) -> Result<crate::registry::DescramblerRegistryEntry, RegistryCommitError> {
        self.registry.allocate_descrambler()
    }

    pub fn unregister_descrambler_runtime(&mut self, id: i32) -> Option<crate::registry::DescramblerRegistryEntry> {
        self.registry.unregister_descrambler(DescramblerRuntimeId(id))
    }

    pub fn demux_ids(&self) -> Vec<i32> {
        self.registry.demux_ids().into_iter().map(|id| id.0).collect()
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

    pub fn lnb_for_frontend_id(&self, frontend_id: i32) -> Option<crate::registry::LnbRegistryEntry> {
        self.registry.lnb_for_frontend(FrontendRuntimeId(frontend_id)).cloned()
    }

    pub fn set_demux_frontend_data_source(&mut self, demux_id: i32, frontend_id: i32) -> Result<GenerationBoundaryReport, HalError> {
        let demux_key = DemuxRuntimeId(demux_id);
        let frontend_key = FrontendRuntimeId(frontend_id);

        let Some(frontend_runtime) = self.registry.frontend_runtime(frontend_key) else {
            return Err(HalError::Unsupported("frontend id is not available for demux source binding"));
        };
        match frontend_runtime.state() {
            FrontendRuntimeState::Closing | FrontendRuntimeState::Failed => {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "frontend runtime is closing or failed",
                ));
            }
            FrontendRuntimeState::Idle | FrontendRuntimeState::Tuning { .. } | FrontendRuntimeState::Scanning { .. } => {}
        }

        let Some(demux_runtime) = self.registry.demux_runtime_mut(demux_key) else {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "demux runtime is missing",
            ));
        };
        let generation = DemuxStreamGeneration(demux_runtime.generation());
        let (_, report) = GenerationBoundaryTxn::for_reason(generation, PipelineBoundaryReason::TuneStart).apply(demux_runtime);
        self.registry.bind_demux_frontend(demux_key, frontend_key);
        Ok(report)
    }

    pub fn reset_bound_demuxes_for_frontend_tune_start(&mut self, frontend_id: i32) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported("frontend id is not available for tune boundary reset"));
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
            let (_, report) = GenerationBoundaryTxn::for_reason(generation, PipelineBoundaryReason::TuneStart).apply(demux_runtime);
            reports.push(report);
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
            return Err(HalError::Unsupported("frontend id is not available for demux unbind"));
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
            let (_, report) = GenerationBoundaryTxn::for_reason(generation, reason).apply(demux_runtime);
            reports.push(report);
        }
        self.registry.unbind_frontend_demuxes(frontend_key);
        Ok(reports)
    }

    pub fn quarantine_frontend_and_bound_demuxes(&mut self, frontend_id: i32, error: HalError) -> Result<Vec<DemuxRuntimeId>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let demux_ids = self.registry.quarantine_bound_demuxes_for_frontend(frontend_key);
        let runtime = self
            .registry
            .frontend_runtime_mut(frontend_key)
            .ok_or_else(|| HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend runtime is missing for quarantine",
            ))?;
        runtime.mark_failed(error);
        Ok(demux_ids)
    }

    pub fn ensure_frontend_demux_sink_ready(&self, frontend_id: i32) -> Result<Vec<DemuxRuntimeId>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        if self.registry.frontend(frontend_key).is_none() {
            return Err(HalError::Unsupported("frontend id is not available for live TS delivery"));
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

    pub fn push_frontend_ts_packet_to_bound_demuxes(&mut self, frontend_id: i32, packet: &[u8]) -> Result<Vec<PipelineReport>, HalError> {
        let demux_ids = self.ensure_frontend_demux_sink_ready(frontend_id)?;
        let mut reports = Vec::with_capacity(demux_ids.len());
        for demux_id in demux_ids {
            let Some(demux_runtime) = self.registry.demux_runtime_mut(demux_id) else {
                return Err(HalError::invalid_state(
                    HalInvalidStateKind::InvalidLifecycle,
                    "bound demux runtime is missing",
                ));
            };
            reports.push(demux_runtime.push_ts_packet_from_origin(packet, TsInputOrigin::Frontend));
        }
        Ok(reports)
    }
    fn allocate_aidl_generation(&mut self) -> Result<AidlObjectGeneration, RuntimeObjectTableError> {
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
        self.register_aidl_object_for_runtime(object_kind, object_id, generation, public_runtime_id, owner)
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
        self.object_table.mark_cleanup_failed(object_id, generation, step)
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
            self.diagnostics.push(StartupDiagnosticRecord::runtime_dispatch_missing());
        }
        plan
    }

}
