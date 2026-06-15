use crate::boot::TunerServiceRuntime;
use crate::registry::DemuxRuntimeId;
use maleicacid_tuner_hal2_common::{FrontendTuneRequest, HalError};
use maleicacid_tuner_hal2_demux::runtime::{
    DemuxRuntimeSnapshot, GenerationBoundaryReport,
};
use maleicacid_tuner_hal2_device::{
    FrontendLivePumpReport, FrontendRuntimeSnapshot, FrontendSignalState,
    FrontendWorkerCancelReason, FrontendWorkerContext, FrontendWorkerKind,
    FrontendWorkerStartError, FrontendWorkerStopOutcome,
};

use std::sync::{Arc, Mutex};

pub use crate::frontend_worker_txn::FrontendScanEndNotifier;

pub type SharedFrontendRuntime = Arc<Mutex<TunerServiceRuntime>>;

pub fn start_frontend_tune_use_case(
    runtime: SharedFrontendRuntime,
    frontend_id: i32,
    entry: crate::registry::FrontendRegistryEntry,
    request: FrontendTuneRequest,
    kind: FrontendWorkerKind,
) -> Result<(), HalError> {
    crate::frontend_worker_txn::start_frontend_backend_tune_worker(
        runtime,
        frontend_id,
        entry,
        request,
        kind,
    )
}

pub fn start_frontend_scan_use_case(
    runtime: SharedFrontendRuntime,
    frontend_id: i32,
    entry: crate::registry::FrontendRegistryEntry,
    request: FrontendTuneRequest,
    scan_mode: maleicacid_tuner_hal2_common::FrontendScanMode,
    candidates: Vec<FrontendTuneRequest>,
    scan_end_notifier: FrontendScanEndNotifier,
) -> Result<(), HalError> {
    crate::frontend_worker_txn::start_frontend_backend_scan_session_worker(
        runtime,
        frontend_id,
        entry,
        request,
        scan_mode,
        candidates,
        scan_end_notifier,
    )
}

pub fn stop_frontend_tune_use_case(
    runtime: SharedFrontendRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    crate::frontend_worker_txn::stop_frontend_tune_worker(runtime, frontend_id, reason)
}

pub fn stop_frontend_scan_use_case(
    runtime: SharedFrontendRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    crate::frontend_worker_txn::stop_frontend_scan_worker(runtime, frontend_id, reason)
}

pub fn stop_frontend_live_data_use_case(
    runtime: SharedFrontendRuntime,
    frontend_id: i32,
) -> Result<(), HalError> {
    crate::frontend_worker_txn::stop_frontend_live_data_and_unbind(runtime, frontend_id)
}

pub fn close_frontend_workers_and_live_data_use_case(
    runtime: SharedFrontendRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    crate::frontend_worker_txn::close_frontend_workers_and_live_data(runtime, frontend_id, reason)
}

impl TunerServiceRuntime {
    pub fn restore_frontend_runtime_snapshot(
        &mut self,
        frontend_id: i32,
        snapshot: FrontendRuntimeSnapshot,
    ) -> Result<(), HalError> {
        self.frontend_txn().restore_frontend_runtime_snapshot(frontend_id, snapshot)
    }

    pub fn restore_bound_demux_runtime_snapshots(
        &mut self,
        snapshots: Vec<(DemuxRuntimeId, DemuxRuntimeSnapshot)>,
    ) -> Result<(), HalError> {
        self.frontend_txn().restore_bound_demux_runtime_snapshots(snapshots)
    }

    pub fn commit_frontend_active_tune_request(
        &mut self,
        frontend_id: i32,
        generation: u64,
        request: FrontendTuneRequest,
    ) -> Result<(), HalError> {
        self.frontend_txn().commit_frontend_active_tune_request(frontend_id, generation, request)
    }

    pub fn record_frontend_signal_state(
        &mut self,
        frontend_id: i32,
        generation: u64,
        signal_state: FrontendSignalState,
    ) -> Result<(), HalError> {
        self.frontend_txn().record_frontend_signal_state(frontend_id, generation, signal_state)
    }

    pub fn record_live_pump_report(
        &mut self,
        frontend_id: i32,
        generation: u64,
        report: FrontendLivePumpReport,
        cancel_reason: Option<FrontendWorkerCancelReason>,
    ) -> Result<(), HalError> {
        self.frontend_txn().record_live_pump_report(frontend_id, generation, report, cancel_reason)
    }

    pub fn prepare_frontend_worker_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Result<u64, HalError> {
        self.frontend_txn().prepare_frontend_worker_generation(frontend_id, kind)
    }

    pub fn install_frontend_live_reader_descriptor_for_generation(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        generation: u64,
    ) -> Result<(), HalError> {
        self.frontend_txn().install_frontend_live_reader_descriptor_for_generation(
            frontend_id,
            kind,
            generation,
        )
    }

    pub fn clear_frontend_live_reader_descriptor_and_idle(
        &mut self,
        frontend_id: i32,
    ) -> Result<(), HalError> {
        self.frontend_txn().clear_frontend_live_reader_descriptor_and_idle(frontend_id)
    }

    pub fn stop_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.frontend_txn().stop_frontend_live_data_and_unbind(frontend_id)
    }

    pub fn close_frontend_live_data_and_unbind(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<GenerationBoundaryReport>, HalError> {
        self.frontend_txn().close_frontend_live_data_and_unbind(frontend_id)
    }

    pub fn record_frontend_scan_cancelled(
        &mut self,
        frontend_id: i32,
        generation: u64,
        reason: FrontendWorkerCancelReason,
    ) -> Result<(), HalError> {
        self.frontend_txn().record_frontend_scan_cancelled(frontend_id, generation, reason)
    }

    pub fn begin_frontend_scan_session(
        &mut self,
        frontend_id: i32,
        generation: u64,
        fingerprint: String,
        candidates: Vec<FrontendTuneRequest>,
    ) -> Result<(), HalError> {
        self.frontend_txn().begin_frontend_scan_session(frontend_id, generation, fingerprint, candidates)
    }

    pub fn cancel_frontend_scan_session(
        &mut self,
        frontend_id: i32,
        generation: u64,
        reason: FrontendWorkerCancelReason,
    ) -> Result<(), HalError> {
        self.frontend_txn().cancel_frontend_scan_session(frontend_id, generation, reason)
    }

    pub fn advance_frontend_scan_session_after_candidate(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<bool, HalError> {
        self.frontend_txn().advance_frontend_scan_session_after_candidate(frontend_id, generation)
    }

    pub fn mark_frontend_tune_worker_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
        error: HalError,
    ) -> Result<(), HalError> {
        self.frontend_txn().mark_frontend_tune_worker_failed(frontend_id, generation, error)
    }

    pub fn mark_frontend_scan_session_backend_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        self.frontend_txn().mark_frontend_scan_session_backend_failed(frontend_id, generation)
    }

    pub fn mark_frontend_scan_session_callback_failed(
        &mut self,
        frontend_id: i32,
        generation: u64,
    ) -> Result<(), HalError> {
        self.frontend_txn().mark_frontend_scan_session_callback_failed(frontend_id, generation)
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
        self.frontend_txn().start_worker(frontend_id, kind, generation, job)
    }

    pub fn request_frontend_worker_stop(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopOutcome {
        self.frontend_txn().request_worker_stop(frontend_id, kind, reason)
    }

    pub fn request_frontend_worker_stop_and_join(
        &mut self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
        reason: FrontendWorkerCancelReason,
    ) -> FrontendWorkerStopOutcome {
        self.frontend_txn().request_worker_stop_and_join(frontend_id, kind, reason)
    }

    pub fn clear_finished_frontend_workers(&mut self) {
        self.frontend_txn().clear_finished_workers();
    }
}
