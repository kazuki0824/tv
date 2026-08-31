use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, Weak};
#[cfg(test)]
use std::sync::MutexGuard;
use std::thread;
use std::time::{Duration, Instant};

use crate::cleanup_execution::{
    CleanupExecutionDiagnosticSnapshot, CleanupExecutionReport, CleanupExecutionStepOutcome,
    SharedCleanupDiagnostics,
};
use crate::registry::FrontendRegistryEntry;
use crate::{
    frontend_ops::{
        FrontendOperationEvent, FrontendTuneScanTxn, FrontendWorkerTerminalEvent,
    },
    object_lifecycle::{aidl_object_live, aidl_public_runtime_id_for_close_cleanup},
    object_method_use_case::ObjectMethodExecutionToken,
    start_frontend_demux_live_pump_from_reader, TunerServiceRuntime,
};
use crate::worker_runtime::WorkerTerminalResult;
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FrontendBackendKind, FrontendDevicePath, FrontendScanMode,
    FrontendIsdbtPartialReceptionRequirement, FrontendTuneRequest, HalError, HalErrorDetail,
    HalInternalKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_demux::DemuxRuntimeRollbackToken;
use maleicacid_tuner_hal2_device::{
    FrontendBackendSession, FrontendBackendSubmitFailure, FrontendBackendSubmitTicket,
    FrontendBackendSubmitWait, FrontendBackendTunePlan, FrontendLivePumpJoinOutcome,
    FrontendLivePumpOwner, FrontendRuntimeSnapshot, FrontendScanPhase, FrontendSignalState,
    FrontendTmccPartialReceptionObservation, FrontendWorkerCancelReason, FrontendWorkerContext,
    FrontendWorkerKind, FrontendWorkerStartError, FrontendWorkerStopOutcome,
    FrontendWorkerStopPoll, FrontendWorkerStopTicket,
};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendScanNotification {
    Locked,
    End,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendTuneNotification {
    Locked,
    LostLock,
    NoSignal,
}

pub type FrontendTuneNotifier = Arc<
    dyn Fn(i32, u64, FrontendTuneNotification) -> Result<(), HalError> + Send + Sync + 'static,
>;

pub type FrontendScanNotifier = Arc<
    dyn Fn(i32, u64, FrontendScanNotification) -> Result<(), HalError> + Send + Sync + 'static,
>;

fn deliver_committed_tune_notification(
    runtime: &SharedRuntime,
    notifier: &FrontendTuneNotifier,
    frontend_id: i32,
    generation: u64,
    notification: FrontendTuneNotification,
) -> Result<(), HalError> {
    match FrontendTuneScanTxn::accept_operation_event(
        runtime,
        frontend_id,
        generation,
        FrontendOperationEvent::Tune {
            notifier: Arc::clone(notifier),
            notification,
        },
    )? {
        crate::frontend_ops::FrontendOperationEventAcceptance::Accepted
        | crate::frontend_ops::FrontendOperationEventAcceptance::AcceptedCallbackFailure
        | crate::frontend_ops::FrontendOperationEventAcceptance::DiscardedStale => Ok(()),
    }
}

fn deliver_committed_scan_notification(
    runtime: &SharedRuntime,
    notifier: &FrontendScanNotifier,
    frontend_id: i32,
    generation: u64,
    notification: FrontendScanNotification,
) -> Result<(), HalError> {
    match FrontendTuneScanTxn::accept_operation_event(
        runtime,
        frontend_id,
        generation,
        FrontendOperationEvent::Scan {
            notifier: Arc::clone(notifier),
            notification,
        },
    )? {
        crate::frontend_ops::FrontendOperationEventAcceptance::Accepted
        | crate::frontend_ops::FrontendOperationEventAcceptance::AcceptedCallbackFailure
        | crate::frontend_ops::FrontendOperationEventAcceptance::DiscardedStale => Ok(()),
    }
}

fn finish_frontend_worker_execution(
    runtime: &SharedRuntime,
    ctx: &FrontendWorkerContext,
    result: Result<(), HalError>,
) -> Result<(), HalError> {
    let terminal_result = match &result {
        Ok(()) if ctx.cancel_requested() => WorkerTerminalResult::StopRequested,
        Ok(()) => WorkerTerminalResult::Normal(()),
        Err(error) => WorkerTerminalResult::RuntimeFailure(error.clone()),
    };
    let acceptance = FrontendTuneScanTxn::accept_worker_terminal(
        runtime,
        FrontendWorkerTerminalEvent::new(
            ctx.frontend_id(),
            ctx.generation(),
            ctx.kind(),
            terminal_result,
        ),
    );
    match (result, acceptance) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), Ok(_)) | (Ok(()), Err(error)) => Err(error),
        (Err(primary), Err(cleanup)) => Err(compose_frontend_cleanup_error(
            "frontend worker failure and terminal acceptance both failed",
            primary,
            cleanup,
        )),
    }
}

type SharedRuntime = Arc<Mutex<TunerServiceRuntime>>;

enum FrontendTuneWorkerActivation {
    Run(FrontendBackendSession),
    Abort,
}

enum FrontendScanWorkerActivation {
    Run(FrontendBackendSession),
    Abort,
}

type FrontendWorkerReaperDeadlineAction =
    Box<dyn FnOnce(&SharedRuntime) + Send + 'static>;
type FrontendWorkerReaperCompletionAction =
    Box<
        dyn FnOnce(
                &SharedRuntime,
                Vec<(FrontendWorkerKind, FrontendWorkerStopOutcome)>,
                bool,
            ) + Send
            + 'static,
    >;

struct FrontendWorkerReaperTicketGroup {
    pending: Vec<(FrontendWorkerKind, FrontendWorkerStopTicket)>,
    completed: Vec<(FrontendWorkerKind, FrontendWorkerStopOutcome)>,
}

impl FrontendWorkerReaperTicketGroup {
    fn new(tickets: Vec<(FrontendWorkerKind, FrontendWorkerStopTicket)>) -> Self {
        let capacity = tickets.len();
        Self {
            pending: tickets,
            completed: Vec::with_capacity(capacity),
        }
    }

    fn try_complete(
        mut self,
    ) -> Result<Vec<(FrontendWorkerKind, FrontendWorkerStopOutcome)>, Self> {
        let mut still_pending = Vec::new();
        for (kind, ticket) in self.pending {
            match ticket.try_complete() {
                FrontendWorkerStopPoll::Completed(outcome) => {
                    self.completed.push((kind, outcome))
                }
                FrontendWorkerStopPoll::Pending(ticket) => still_pending.push((kind, ticket)),
            }
        }
        if still_pending.is_empty() {
            Ok(self.completed)
        } else {
            self.pending = still_pending;
            Err(self)
        }
    }

    fn wait_for_progress(&self, deadline: Option<Instant>) -> Result<Option<usize>, HalError> {
        for (index, (_, ticket)) in self.pending.iter().enumerate() {
            if ticket.wait_until_finished(deadline)? {
                return Ok(Some(index));
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Ok(None);
            }
        }
        Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend worker reaper waited without a pending ticket",
        ))
    }

    fn complete_signalled(&mut self, index: usize) -> Result<(), HalError> {
        if index >= self.pending.len() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend worker completion signal index is out of range",
            ));
        }
        let (kind, ticket) = self.pending.remove(index);
        self.completed.push((kind, ticket.complete()));
        Ok(())
    }
}

struct FrontendWorkerReaperJob {
    keys: Vec<(i32, FrontendWorkerKind)>,
    continuation_kind: Option<FrontendWorkerKind>,
    tickets: FrontendWorkerReaperTicketGroup,
    transferred_at: Instant,
    deadline_action: Option<FrontendWorkerReaperDeadlineAction>,
    completion_action: FrontendWorkerReaperCompletionAction,
}

impl FrontendWorkerReaperJob {
    fn run(
        mut self,
        runtime: &Weak<Mutex<TunerServiceRuntime>>,
        pending: &Mutex<BTreeMap<(i32, FrontendWorkerKind), Option<FrontendWorkerKind>>>,
        deadline: Duration,
    ) {
        let mut deadline_elapsed = false;
        let terminal_deadline = match self.transferred_at.checked_add(deadline) {
            Some(deadline) => deadline,
            None => {
                if let Some(runtime) = runtime.upgrade() {
                    if let Ok(mut guard) = runtime.lock() {
                        guard.mark_service_critical();
                    }
                }
                core::mem::forget(self);
                return;
            }
        };
        loop {
            match self.tickets.try_complete() {
                Ok(outcomes) => {
                    let pending_cleanup_failed = match pending.lock() {
                        Ok(mut pending) => {
                            for key in &self.keys {
                                pending.remove(key);
                            }
                            false
                        }
                        Err(_) => true,
                    };
                    if let Some(runtime) = runtime.upgrade() {
                        if pending_cleanup_failed {
                            if let Ok(mut guard) = runtime.lock() {
                                guard.mark_service_critical();
                            }
                        }
                        (self.completion_action)(&runtime, outcomes, deadline_elapsed);
                    }
                    return;
                }
                Err(tickets) => self.tickets = tickets,
            }
            let wait_deadline = (!deadline_elapsed).then_some(terminal_deadline);
            match self.tickets.wait_for_progress(wait_deadline) {
                Ok(Some(index)) => {
                    if self.tickets.complete_signalled(index).is_err() {
                        if let Some(runtime) = runtime.upgrade() {
                            if let Ok(mut guard) = runtime.lock() {
                                guard.mark_service_critical();
                            }
                        }
                        core::mem::forget(self);
                        return;
                    }
                }
                Ok(None) => {
                    deadline_elapsed = true;
                    if let (Some(runtime), Some(action)) =
                        (runtime.upgrade(), self.deadline_action.take())
                    {
                        action(&runtime);
                    }
                }
                Err(_) => {
                    if let Some(runtime) = runtime.upgrade() {
                        if let Ok(mut guard) = runtime.lock() {
                            guard.mark_service_critical();
                        }
                    }
                    core::mem::forget(self);
                    return;
                }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct FrontendWorkerReaperHandle {
    sender: SyncSender<FrontendWorkerReaperJob>,
    pending: Arc<Mutex<BTreeMap<(i32, FrontendWorkerKind), Option<FrontendWorkerKind>>>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrontendWorkerReaperPendingState {
    NotPending,
    CleanupOnly,
    Replacement(FrontendWorkerKind),
}

impl core::fmt::Debug for FrontendWorkerReaperHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrontendWorkerReaperHandle").finish()
    }
}

impl FrontendWorkerReaperHandle {
    fn start(
        runtime: Weak<Mutex<TunerServiceRuntime>>,
        capacity: usize,
        deadline: Duration,
    ) -> Result<Self, HalError> {
        let capacity = capacity.max(1);
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let receiver: Arc<Mutex<Receiver<FrontendWorkerReaperJob>>> =
            Arc::new(Mutex::new(receiver));
        for lane in 0..capacity {
            let receiver = Arc::clone(&receiver);
            let runtime = Weak::clone(&runtime);
            let pending = Arc::clone(&pending);
            thread::Builder::new()
                .name(format!("maleicacid-frontend-reaper-{lane}"))
                .spawn(move || loop {
                    let job = match receiver.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => return,
                    };
                    match job {
                        Ok(job) => job.run(&runtime, &pending, deadline),
                        Err(_) => return,
                    }
                })
                .map_err(|error| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        format!("frontend worker reaper lane spawn failed: {error}"),
                    )
                })?;
        }
        Ok(Self { sender, pending })
    }

    fn enqueue(&self, job: FrontendWorkerReaperJob) -> Result<(), HalError> {
        let mut pending = match self.pending.lock() {
            Ok(pending) => pending,
            Err(_) => {
                // pending key台帳が利用不能でも、移譲済みJoinHandleの所有権を保持する。
                // ここでjobをdropするとendpoint leaseが有効なままworkerがdetachされる。
                core::mem::forget(job);
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker reaper pending registry lock poisoned",
                ));
            }
        };
        if job.keys.iter().any(|key| pending.contains_key(key)) {
            core::mem::forget(job);
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend worker reaper received a duplicate endpoint lease",
            ));
        }
        for key in &job.keys {
            pending.insert(*key, job.continuation_kind);
        }
        drop(pending);
        self.sender.try_send(job).map_err(|error| match error {
            TrySendError::Full(job) => {
                // 移譲済みJoinHandleをdropまたはdetachしてはならない。
                // 容量枯渇はServiceCriticalとし、不安全なendpoint再利用を防ぐため
                // process lifetime中は所有権を保持する。
                core::mem::forget(job);
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker reaper capacity exhausted",
                )
            }
            TrySendError::Disconnected(job) => {
                core::mem::forget(job);
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker reaper is unavailable",
                )
            }
        })
    }

    fn is_pending(&self, frontend_id: i32, kind: FrontendWorkerKind) -> Result<bool, HalError> {
        self.pending
            .lock()
            .map(|pending| pending.contains_key(&(frontend_id, kind)))
            .map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker reaper pending registry lock poisoned",
                )
            })
    }

    fn pending_state(
        &self,
        frontend_id: i32,
        kind: FrontendWorkerKind,
    ) -> Result<FrontendWorkerReaperPendingState, HalError> {
        self.pending
            .lock()
            .map(|pending| match pending.get(&(frontend_id, kind)).copied() {
                None => FrontendWorkerReaperPendingState::NotPending,
                Some(None) => FrontendWorkerReaperPendingState::CleanupOnly,
                Some(Some(kind)) => FrontendWorkerReaperPendingState::Replacement(kind),
            })
            .map_err(|_| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker reaper pending registry lock poisoned",
                )
            })
    }
}

fn ensure_frontend_worker_reaper(
    runtime: &SharedRuntime,
) -> Result<FrontendWorkerReaperHandle, HalError> {
    let (capacity, deadline) = {
        let guard = lock_runtime(runtime, "service runtime lock poisoned while finding reaper")?;
        if let Some(handle) = guard.frontend_worker_reaper_handle() {
            return Ok(handle);
        }
        (
            guard.frontend_worker_reaper_capacity(),
            Duration::from_millis(guard.capability_snapshot().worker_reaper_deadline_ms),
        )
    };
    let candidate =
        FrontendWorkerReaperHandle::start(Arc::downgrade(runtime), capacity, deadline)?;
    let mut guard = lock_runtime(runtime, "service runtime lock poisoned while installing reaper")?;
    if let Some(handle) = guard.frontend_worker_reaper_handle() {
        return Ok(handle);
    }
    guard.install_frontend_worker_reaper_handle(candidate.clone());
    Ok(candidate)
}

type DemuxRollbackTokenList = Vec<(crate::registry::DemuxRuntimeId, DemuxRuntimeRollbackToken)>;
type SharedDemuxRollbackTokenList = Arc<Mutex<Option<DemuxRollbackTokenList>>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupDiagnosticKind {
    StopTuneObject,
    StopScanObject,
    TuneReplacementStop,
    ScanReplacementStop,
    TuneStartRollback,
    TuneWorkerStartRollback,
    TuneCommitRollback,
    TuneBackendRollbackStateRestore,
    ScanStartRollback,
    ScanWorkerStartRollback,
    ScanBackendRollbackStateRestore,
    WorkerReaperDeadline,
    WorkerReaperCompletion,
    FrontendClose,
    FrontendCloseOwnerLoss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupTarget {
    Object {
        frontend_id: i32,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
    },
    Frontend {
        frontend_id: i32,
    },
}

impl FrontendWorkerCleanupTarget {
    pub const fn object(
        frontend_id: i32,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
    ) -> Self {
        Self::Object {
            frontend_id,
            object_id,
            object_generation,
        }
    }

    pub const fn frontend(frontend_id: i32) -> Self {
        Self::Frontend { frontend_id }
    }

    pub fn frontend_id(&self) -> i32 {
        match *self {
            Self::Object { frontend_id, .. } | Self::Frontend { frontend_id } => frontend_id,
        }
    }

    pub fn object_id(&self) -> Option<AidlObjectId> {
        match *self {
            Self::Object { object_id, .. } => Some(object_id),
            Self::Frontend { .. } => None,
        }
    }

    pub fn object_generation(&self) -> Option<AidlObjectGeneration> {
        match *self {
            Self::Object {
                object_generation, ..
            } => Some(object_generation),
            Self::Frontend { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupWorkerGeneration {
    Known(u64),
    NotAvailable,
}

impl FrontendWorkerCleanupWorkerGeneration {
    pub const fn from_option(generation: Option<u64>) -> Self {
        match generation {
            Some(generation) => Self::Known(generation),
            None => Self::NotAvailable,
        }
    }

    pub const fn as_option(self) -> Option<u64> {
        match self {
            Self::Known(generation) => Some(generation),
            Self::NotAvailable => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendWorkerCleanupStep {
    StopWorker(FrontendWorkerKind),
    RecordScanCancelled,
    ClearLiveReaderDescriptor,
    StopLiveDataAndUnbind,
    CloseLiveDataAndUnbind,
    RestoreFrontendSnapshot,
    TakeDemuxRollbackTokens,
    RestoreBoundDemuxes,
    CompleteReplacement(FrontendWorkerKind),
    CompleteStopObject(FrontendWorkerKind),
    CloseOwnedLnb(i32),
    CloseFrontendWorkersAndLiveData,
}

#[derive(Clone, Debug)]
pub enum FrontendWorkerCleanupStepOutcome {
    StopWorker {
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        worker_generation: FrontendWorkerCleanupWorkerGeneration,
        result: Result<(), HalError>,
    },
    RecordScanCancelled {
        target: FrontendWorkerCleanupTarget,
        worker_generation: FrontendWorkerCleanupWorkerGeneration,
        result: Result<(), HalError>,
    },
    ClearLiveReaderDescriptor {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    StopLiveDataAndUnbind {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    CloseLiveDataAndUnbind {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    RestoreFrontendSnapshot {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    TakeDemuxRollbackTokens {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    RestoreBoundDemuxes {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
    CompleteReplacement {
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        stopped_worker_generation: FrontendWorkerCleanupWorkerGeneration,
        new_worker_generation: u64,
        result: Result<(), HalError>,
    },
    CompleteStopObject {
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        worker_generation: FrontendWorkerCleanupWorkerGeneration,
        result: Result<(), HalError>,
    },
    CloseOwnedLnb {
        target: FrontendWorkerCleanupTarget,
        lnb_id: i32,
        result: Result<(), HalError>,
    },
    CloseFrontendWorkersAndLiveData {
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    },
}

impl FrontendWorkerCleanupStepOutcome {
    fn stop_worker(
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        worker_generation: Option<u64>,
        result: Result<(), HalError>,
    ) -> Self {
        Self::StopWorker {
            target,
            worker_kind,
            worker_generation: FrontendWorkerCleanupWorkerGeneration::from_option(
                worker_generation,
            ),
            result,
        }
    }

    fn record_scan_cancelled(
        target: FrontendWorkerCleanupTarget,
        worker_generation: Option<u64>,
        result: Result<(), HalError>,
    ) -> Self {
        Self::RecordScanCancelled {
            target,
            worker_generation: FrontendWorkerCleanupWorkerGeneration::from_option(
                worker_generation,
            ),
            result,
        }
    }

    fn clear_live_reader_descriptor(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::ClearLiveReaderDescriptor { target, result }
    }

    fn stop_live_data_and_unbind(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::StopLiveDataAndUnbind { target, result }
    }

    fn close_live_data_and_unbind(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CloseLiveDataAndUnbind { target, result }
    }

    fn restore_frontend_snapshot(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::RestoreFrontendSnapshot { target, result }
    }

    fn take_demux_rollback_tokens(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::TakeDemuxRollbackTokens { target, result }
    }

    fn restore_bound_demuxes(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::RestoreBoundDemuxes { target, result }
    }

    fn complete_replacement(
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        stopped_worker_generation: Option<u64>,
        new_worker_generation: u64,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CompleteReplacement {
            target,
            worker_kind,
            stopped_worker_generation: FrontendWorkerCleanupWorkerGeneration::from_option(
                stopped_worker_generation,
            ),
            new_worker_generation,
            result,
        }
    }

    fn complete_stop_object(
        target: FrontendWorkerCleanupTarget,
        worker_kind: FrontendWorkerKind,
        worker_generation: Option<u64>,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CompleteStopObject {
            target,
            worker_kind,
            worker_generation: FrontendWorkerCleanupWorkerGeneration::from_option(
                worker_generation,
            ),
            result,
        }
    }

    fn close_owned_lnb(
        target: FrontendWorkerCleanupTarget,
        lnb_id: i32,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CloseOwnedLnb {
            target,
            lnb_id,
            result,
        }
    }

    fn close_frontend_workers_and_live_data(
        target: FrontendWorkerCleanupTarget,
        result: Result<(), HalError>,
    ) -> Self {
        Self::CloseFrontendWorkersAndLiveData { target, result }
    }

    pub fn target(&self) -> FrontendWorkerCleanupTarget {
        match self {
            Self::StopWorker { target, .. }
            | Self::RecordScanCancelled { target, .. }
            | Self::ClearLiveReaderDescriptor { target, .. }
            | Self::StopLiveDataAndUnbind { target, .. }
            | Self::CloseLiveDataAndUnbind { target, .. }
            | Self::RestoreFrontendSnapshot { target, .. }
            | Self::TakeDemuxRollbackTokens { target, .. }
            | Self::RestoreBoundDemuxes { target, .. }
            | Self::CompleteReplacement { target, .. }
            | Self::CompleteStopObject { target, .. }
            | Self::CloseOwnedLnb { target, .. }
            | Self::CloseFrontendWorkersAndLiveData { target, .. } => *target,
        }
    }

    pub fn frontend_id(&self) -> i32 {
        self.target().frontend_id()
    }

    pub fn object_id(&self) -> Option<AidlObjectId> {
        self.target().object_id()
    }

    pub fn object_generation(&self) -> Option<AidlObjectGeneration> {
        self.target().object_generation()
    }

    pub fn worker_kind(&self) -> Option<FrontendWorkerKind> {
        match self {
            Self::StopWorker { worker_kind, .. }
            | Self::CompleteReplacement { worker_kind, .. }
            | Self::CompleteStopObject { worker_kind, .. } => Some(*worker_kind),
            Self::RecordScanCancelled { .. }
            | Self::ClearLiveReaderDescriptor { .. }
            | Self::StopLiveDataAndUnbind { .. }
            | Self::CloseLiveDataAndUnbind { .. }
            | Self::RestoreFrontendSnapshot { .. }
            | Self::TakeDemuxRollbackTokens { .. }
            | Self::RestoreBoundDemuxes { .. }
            | Self::CloseOwnedLnb { .. }
            | Self::CloseFrontendWorkersAndLiveData { .. } => None,
        }
    }

    pub fn worker_generation(&self) -> Option<u64> {
        match self {
            Self::StopWorker {
                worker_generation, ..
            }
            | Self::RecordScanCancelled {
                worker_generation, ..
            } => worker_generation.as_option(),
            Self::CompleteReplacement {
                stopped_worker_generation,
                ..
            } => stopped_worker_generation.as_option(),
            Self::CompleteStopObject {
                worker_generation, ..
            } => worker_generation.as_option(),
            Self::ClearLiveReaderDescriptor { .. }
            | Self::StopLiveDataAndUnbind { .. }
            | Self::CloseLiveDataAndUnbind { .. }
            | Self::RestoreFrontendSnapshot { .. }
            | Self::TakeDemuxRollbackTokens { .. }
            | Self::RestoreBoundDemuxes { .. }
            | Self::CloseOwnedLnb { .. }
            | Self::CloseFrontendWorkersAndLiveData { .. } => None,
        }
    }

    pub fn step(&self) -> FrontendWorkerCleanupStep {
        match self {
            Self::StopWorker { worker_kind, .. } => {
                FrontendWorkerCleanupStep::StopWorker(*worker_kind)
            }
            Self::RecordScanCancelled { .. } => FrontendWorkerCleanupStep::RecordScanCancelled,
            Self::ClearLiveReaderDescriptor { .. } => {
                FrontendWorkerCleanupStep::ClearLiveReaderDescriptor
            }
            Self::StopLiveDataAndUnbind { .. } => FrontendWorkerCleanupStep::StopLiveDataAndUnbind,
            Self::CloseLiveDataAndUnbind { .. } => {
                FrontendWorkerCleanupStep::CloseLiveDataAndUnbind
            }
            Self::RestoreFrontendSnapshot { .. } => {
                FrontendWorkerCleanupStep::RestoreFrontendSnapshot
            }
            Self::TakeDemuxRollbackTokens { .. } => {
                FrontendWorkerCleanupStep::TakeDemuxRollbackTokens
            }
            Self::RestoreBoundDemuxes { .. } => FrontendWorkerCleanupStep::RestoreBoundDemuxes,
            Self::CompleteReplacement { worker_kind, .. } => {
                FrontendWorkerCleanupStep::CompleteReplacement(*worker_kind)
            }
            Self::CompleteStopObject { worker_kind, .. } => {
                FrontendWorkerCleanupStep::CompleteStopObject(*worker_kind)
            }
            Self::CloseOwnedLnb { lnb_id, .. } => FrontendWorkerCleanupStep::CloseOwnedLnb(*lnb_id),
            Self::CloseFrontendWorkersAndLiveData { .. } => {
                FrontendWorkerCleanupStep::CloseFrontendWorkersAndLiveData
            }
        }
    }

    pub fn result(&self) -> Result<(), HalError> {
        match self {
            Self::StopWorker { result, .. }
            | Self::RecordScanCancelled { result, .. }
            | Self::ClearLiveReaderDescriptor { result, .. }
            | Self::StopLiveDataAndUnbind { result, .. }
            | Self::CloseLiveDataAndUnbind { result, .. }
            | Self::RestoreFrontendSnapshot { result, .. }
            | Self::TakeDemuxRollbackTokens { result, .. }
            | Self::RestoreBoundDemuxes { result, .. }
            | Self::CompleteReplacement { result, .. }
            | Self::CompleteStopObject { result, .. }
            | Self::CloseOwnedLnb { result, .. }
            | Self::CloseFrontendWorkersAndLiveData { result, .. } => result.clone(),
        }
    }

    pub fn into_result(self) -> Result<(), HalError> {
        match self {
            Self::StopWorker { result, .. }
            | Self::RecordScanCancelled { result, .. }
            | Self::ClearLiveReaderDescriptor { result, .. }
            | Self::StopLiveDataAndUnbind { result, .. }
            | Self::CloseLiveDataAndUnbind { result, .. }
            | Self::RestoreFrontendSnapshot { result, .. }
            | Self::TakeDemuxRollbackTokens { result, .. }
            | Self::RestoreBoundDemuxes { result, .. }
            | Self::CompleteReplacement { result, .. }
            | Self::CompleteStopObject { result, .. }
            | Self::CloseOwnedLnb { result, .. }
            | Self::CloseFrontendWorkersAndLiveData { result, .. } => result,
        }
    }
}

impl CleanupExecutionStepOutcome for FrontendWorkerCleanupStepOutcome {
    type Failure = HalError;

    fn result(&self) -> Result<(), Self::Failure> {
        FrontendWorkerCleanupStepOutcome::result(self)
    }

    fn into_result(self) -> Result<(), Self::Failure> {
        FrontendWorkerCleanupStepOutcome::into_result(self)
    }
}

pub type FrontendWorkerCleanupExecutionReport =
    CleanupExecutionReport<FrontendWorkerCleanupStepOutcome, HalError>;

#[derive(Clone, Debug)]
pub struct FrontendWorkerCleanupDiagnosticRecord {
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    report: FrontendWorkerCleanupExecutionReport,
    public_error: Option<HalError>,
}

impl FrontendWorkerCleanupDiagnosticRecord {
    pub fn new(
        kind: FrontendWorkerCleanupDiagnosticKind,
        target: FrontendWorkerCleanupTarget,
        report: FrontendWorkerCleanupExecutionReport,
        public_error: Option<HalError>,
    ) -> Self {
        Self {
            kind,
            target,
            report,
            public_error,
        }
    }

    pub fn kind(&self) -> FrontendWorkerCleanupDiagnosticKind {
        self.kind
    }

    pub fn target(&self) -> FrontendWorkerCleanupTarget {
        self.target
    }

    pub fn frontend_id(&self) -> i32 {
        self.target.frontend_id()
    }

    pub fn object_id(&self) -> Option<AidlObjectId> {
        self.target.object_id()
    }

    pub fn object_generation(&self) -> Option<AidlObjectGeneration> {
        self.target.object_generation()
    }

    pub fn report(&self) -> &FrontendWorkerCleanupExecutionReport {
        &self.report
    }

    pub fn public_error(&self) -> Option<&HalError> {
        self.public_error.as_ref()
    }
}

pub type FrontendWorkerCleanupDiagnosticSnapshot =
    CleanupExecutionDiagnosticSnapshot<FrontendWorkerCleanupDiagnosticRecord>;
pub type SharedFrontendWorkerCleanupDiagnostics =
    SharedCleanupDiagnostics<FrontendWorkerCleanupDiagnosticRecord>;

fn record_frontend_cleanup_diagnostic_after_terminal(
    sink: &SharedFrontendWorkerCleanupDiagnostics,
    record: FrontendWorkerCleanupDiagnosticRecord,
) {
    if sink.record(record).is_err() {
        // sink内のrecord failure counterを残す。呼び出し元を失った終端結果は再実行しない。
    }
}

type BoundDemuxGenerationSnapshot = Vec<(crate::registry::DemuxRuntimeId, u64)>;

fn share_demux_rollback_tokens(tokens: DemuxRollbackTokenList) -> SharedDemuxRollbackTokenList {
    Arc::new(Mutex::new(Some(tokens)))
}

fn take_demux_rollback_tokens(
    tokens: &SharedDemuxRollbackTokenList,
    context: &'static str,
) -> Result<DemuxRollbackTokenList, HalError> {
    let mut guard = tokens.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "demux rollback token list lock poisoned",
        )
    })?;
    guard
        .take()
        .ok_or_else(|| HalError::internal(HalInternalKind::InvariantViolation, context))
}

#[cfg(test)]
struct FrontendWorkerReplacementTicket {
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    stopped_worker_generation: Option<u64>,
    new_worker_generation: u64,
    frontend_snapshot: FrontendRuntimeSnapshot,
    demux_rollback_tokens: DemuxRollbackTokenList,
    bound_demux_generations: BoundDemuxGenerationSnapshot,
    stop_ticket: FrontendWorkerStopTicket,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FrontendWorkerReplacementRollbackContext {
    worker_kind: FrontendWorkerKind,
    stopped_worker_generation: Option<u64>,
    new_worker_generation: u64,
}

#[cfg(test)]
struct FrontendWorkerStopObjectTicket {
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    reason: FrontendWorkerCancelReason,
    worker_generation: Option<u64>,
    frontend_snapshot: FrontendRuntimeSnapshot,
    bound_demux_generations: BoundDemuxGenerationSnapshot,
    stop_ticket: FrontendWorkerStopTicket,
}

fn ensure_frontend_ticket_still_targets_object(
    guard: &TunerServiceRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
) -> Result<(), HalError> {
    ensure_frontend_object_still_live(guard, object_id, object_generation)?;
    let (resolved_frontend_id, _) =
        resolve_frontend_object_for_method(guard, object_id, object_generation)?;
    if resolved_frontend_id != frontend_id {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker ticket target changed after external join",
        ));
    }
    Ok(())
}

fn frontend_worker_stop_outcome_generation(outcome: &FrontendWorkerStopOutcome) -> Option<u64> {
    match outcome {
        FrontendWorkerStopOutcome::NotRunning => None,
        FrontendWorkerStopOutcome::CancelRequested { generation, .. }
        | FrontendWorkerStopOutcome::Completed { generation, .. }
        | FrontendWorkerStopOutcome::StopRequestFailed { generation, .. } => Some(*generation),
    }
}

fn bound_demux_generation_snapshot(
    tokens: &DemuxRollbackTokenList,
) -> BoundDemuxGenerationSnapshot {
    let mut generations = tokens
        .iter()
        .map(|(demux_id, token)| (*demux_id, token.generation()))
        .collect::<Vec<_>>();
    generations.sort();
    generations
}

fn current_bound_demux_generation_snapshot(
    guard: &TunerServiceRuntime,
    frontend_id: i32,
) -> Result<BoundDemuxGenerationSnapshot, HalError> {
    let mut generations = guard.query().bound_demux_runtime_generations(frontend_id)?;
    generations.sort();
    Ok(generations)
}

fn bound_demux_generations_are_fenced_at_or_after(
    current: &BoundDemuxGenerationSnapshot,
    expected: &BoundDemuxGenerationSnapshot,
) -> bool {
    expected.iter().all(|(expected_id, expected_generation)| {
        match current
            .iter()
            .find(|(current_id, _)| current_id == expected_id)
        {
            Some((_, current_generation)) => current_generation >= expected_generation,
            None => true,
        }
    })
}

fn ensure_frontend_join_snapshot_still_matches(
    guard: &TunerServiceRuntime,
    frontend_id: i32,
    expected_frontend: &FrontendRuntimeSnapshot,
    expected_demux_generations: &BoundDemuxGenerationSnapshot,
) -> Result<(), HalError> {
    let current_frontend = guard.query().frontend_runtime_snapshot(frontend_id)?;
    if current_frontend.generation != expected_frontend.generation
        || current_frontend.live_reader_descriptor != expected_frontend.live_reader_descriptor
        || current_frontend.scan_session != expected_frontend.scan_session
    {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker ticket runtime snapshot changed during external join",
        ));
    }
    let current_demux_generations = current_bound_demux_generation_snapshot(guard, frontend_id)?;
    if current_demux_generations != *expected_demux_generations {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker ticket bound demux snapshot changed during external join",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn complete_frontend_worker_replacement_ticket<'a>(
    runtime: &'a SharedRuntime,
    ticket: FrontendWorkerReplacementTicket,
    cleanup_diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
    diagnostic_kind: FrontendWorkerCleanupDiagnosticKind,
    context: &'static str,
) -> Result<
    (
        MutexGuard<'a, TunerServiceRuntime>,
        i32,
        u64,
        FrontendWorkerStopOutcome,
        FrontendRuntimeSnapshot,
        DemuxRollbackTokenList,
    ),
    HalError,
> {
    let FrontendWorkerReplacementTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        stopped_worker_generation,
        new_worker_generation,
        frontend_snapshot,
        demux_rollback_tokens,
        bound_demux_generations,
        stop_ticket,
    } = ticket;
    let stop_outcome = stop_ticket.complete();
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let record_stop_outcome_for_failure = |primary: HalError| -> HalError {
        match record_frontend_worker_replacement_stop_diagnostic(
            cleanup_diagnostic_sink.clone(),
            diagnostic_kind,
            target,
            kind,
            &stop_outcome,
            None,
            Some((stopped_worker_generation, new_worker_generation, primary.clone())),
        ) {
            Ok(()) => primary,
            Err(record_error) => compose_frontend_worker_cleanup_record_failure(
                "frontend worker replacement stop diagnostic record failed after replacement failure",
                primary,
                record_error,
            ),
        }
    };
    if let Some(error) = frontend_worker_stop_failure(&stop_outcome) {
        return Err(record_stop_outcome_for_failure(error));
    }
    let guard = match lock_runtime(runtime, context) {
        Ok(guard) => guard,
        Err(error) => return Err(record_stop_outcome_for_failure(error)),
    };
    if let Err(error) = ensure_frontend_ticket_still_targets_object(
        &guard,
        object_id,
        object_generation,
        frontend_id,
    ) {
        return Err(record_stop_outcome_for_failure(error));
    }
    if let Err(error) = ensure_frontend_join_snapshot_still_matches(
        &guard,
        frontend_id,
        &frontend_snapshot,
        &bound_demux_generations,
    ) {
        return Err(record_stop_outcome_for_failure(error));
    }
    if frontend_worker_stop_outcome_generation(&stop_outcome) != stopped_worker_generation {
        return Err(record_stop_outcome_for_failure(HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend worker replacement ticket generation mismatch",
        )));
    }
    if !matches!(stop_outcome, FrontendWorkerStopOutcome::NotRunning) {
        match &stop_outcome {
            FrontendWorkerStopOutcome::CancelRequested {
                kind: outcome_kind, ..
            }
            | FrontendWorkerStopOutcome::Completed {
                kind: outcome_kind, ..
            } => {
                if *outcome_kind != kind {
                    return Err(record_stop_outcome_for_failure(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend worker replacement ticket kind mismatch",
                    )));
                }
            }
            FrontendWorkerStopOutcome::NotRunning
            | FrontendWorkerStopOutcome::StopRequestFailed { .. } => {}
        }
    }
    Ok((
        guard,
        frontend_id,
        new_worker_generation,
        stop_outcome,
        frontend_snapshot,
        demux_rollback_tokens,
    ))
}

#[cfg(test)]
fn prepare_frontend_worker_stop_object_ticket(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    kind: FrontendWorkerKind,
    reason: FrontendWorkerCancelReason,
) -> Result<FrontendWorkerStopObjectTicket, HalError> {
    let (frontend_id, _) =
        resolve_frontend_object_for_method(runtime, object_id, object_generation)?;
    let frontend_snapshot = runtime.query().frontend_runtime_snapshot(frontend_id)?;
    let demux_rollback_tokens = runtime.prepare_bound_demux_runtime_rollback_tokens(frontend_id)?;
    let bound_demux_generations = bound_demux_generation_snapshot(&demux_rollback_tokens);
    let stop_ticket =
        runtime
            .frontend_txn()
            .request_worker_stop_for_join(frontend_id, kind, reason);
    let worker_generation = stop_ticket.worker_generation();
    Ok(FrontendWorkerStopObjectTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        reason,
        worker_generation,
        frontend_snapshot,
        bound_demux_generations,
        stop_ticket,
    })
}

#[cfg(test)]
fn complete_frontend_worker_stop_object_ticket<'a>(
    runtime: &'a SharedRuntime,
    ticket: FrontendWorkerStopObjectTicket,
    cleanup_diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
    diagnostic_kind: FrontendWorkerCleanupDiagnosticKind,
    context: &'static str,
) -> Result<
    (
        MutexGuard<'a, TunerServiceRuntime>,
        i32,
        FrontendWorkerCancelReason,
        FrontendWorkerStopOutcome,
    ),
    HalError,
> {
    let FrontendWorkerStopObjectTicket {
        object_id,
        object_generation,
        frontend_id,
        kind,
        reason,
        worker_generation,
        frontend_snapshot,
        bound_demux_generations,
        stop_ticket,
    } = ticket;
    let stop_outcome = stop_ticket.complete();
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let record_stop_outcome_for_failure =
        |primary: HalError, include_complete_step: bool| -> HalError {
            let mut report = FrontendWorkerCleanupExecutionReport::new();
            report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
                target,
                kind,
                frontend_worker_stop_outcome_generation(&stop_outcome),
                frontend_worker_stop_result_from_outcome(&stop_outcome),
            ));
            if include_complete_step {
                report.push(FrontendWorkerCleanupStepOutcome::complete_stop_object(
                    target,
                    kind,
                    frontend_worker_stop_outcome_generation(&stop_outcome),
                    Err(primary.clone()),
                ));
            }
            let record = FrontendWorkerCleanupDiagnosticRecord::new(
                diagnostic_kind,
                target,
                report,
                Some(primary.clone()),
            );
            match cleanup_diagnostic_sink.record(record) {
                Ok(()) => primary,
                Err(record_error) => compose_frontend_worker_cleanup_record_failure(
                    "frontend worker stop object diagnostic record failed after stop failure",
                    primary,
                    record_error,
                ),
            }
        };
    if let Some(error) = frontend_worker_stop_failure(&stop_outcome) {
        return Err(record_stop_outcome_for_failure(error, false));
    }
    let guard = match lock_runtime(runtime, context) {
        Ok(guard) => guard,
        Err(error) => return Err(record_stop_outcome_for_failure(error, true)),
    };
    if let Err(error) = ensure_frontend_ticket_still_targets_object(
        &guard,
        object_id,
        object_generation,
        frontend_id,
    ) {
        return Err(record_stop_outcome_for_failure(error, true));
    }
    if let Err(error) = ensure_frontend_join_snapshot_still_matches(
        &guard,
        frontend_id,
        &frontend_snapshot,
        &bound_demux_generations,
    ) {
        return Err(record_stop_outcome_for_failure(error, true));
    }
    if frontend_worker_stop_outcome_generation(&stop_outcome) != worker_generation {
        return Err(record_stop_outcome_for_failure(
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend worker stop ticket generation mismatch",
            ),
            true,
        ));
    }
    if !matches!(stop_outcome, FrontendWorkerStopOutcome::NotRunning) {
        match &stop_outcome {
            FrontendWorkerStopOutcome::CancelRequested {
                kind: outcome_kind, ..
            }
            | FrontendWorkerStopOutcome::Completed {
                kind: outcome_kind, ..
            } => {
                if *outcome_kind != kind {
                    return Err(record_stop_outcome_for_failure(
                        HalError::internal(
                            HalInternalKind::InvariantViolation,
                            "frontend worker stop ticket kind mismatch",
                        ),
                        true,
                    ));
                }
            }
            FrontendWorkerStopOutcome::NotRunning
            | FrontendWorkerStopOutcome::StopRequestFailed { .. } => {}
        }
    }
    Ok((guard, frontend_id, reason, stop_outcome))
}

#[derive(Debug)]
pub struct FrontendCloseCleanupReport {
    pub frontend_id: i32,
    pub closed_lnb_ids: Vec<i32>,
    pub cleanup_result: Result<(), HalError>,
}

fn lock_runtime<'a>(
    runtime: &'a SharedRuntime,
    context: &'static str,
) -> Result<std::sync::MutexGuard<'a, TunerServiceRuntime>, HalError> {
    runtime
        .lock()
        .map_err(|_| HalError::internal(HalInternalKind::InvariantViolation, context))
}

fn map_frontend_worker_start_error(error: FrontendWorkerStartError) -> HalError {
    match error {
        FrontendWorkerStartError::AlreadyRunning { .. } => HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend worker is already running",
        ),
        FrontendWorkerStartError::CompletedFailurePending { detail, .. } => HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("frontend worker previous failure is pending and must be reported before replacement: {detail}"),
        ),
        FrontendWorkerStartError::SpawnFailed { detail } => HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("frontend worker spawn failed: {detail}"),
        ),
    }
}

fn compose_frontend_cleanup_error(
    context: &'static str,
    primary: HalError,
    cleanup: HalError,
) -> HalError {
    compose_primary_cleanup_failure(context, primary, cleanup)
}

fn compose_frontend_worker_cleanup_record_failure(
    context: &'static str,
    primary: HalError,
    record_error: HalError,
) -> HalError {
    compose_primary_cleanup_failure(context, primary, record_error)
}

fn finish_frontend_worker_rollback_report(
    sink: Result<SharedFrontendWorkerCleanupDiagnostics, HalError>,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    report: FrontendWorkerCleanupExecutionReport,
    primary: HalError,
    context: &'static str,
) -> HalError {
    let rollback_error = report.first_error();
    let public_error = match rollback_error {
        Some(cleanup) => compose_frontend_cleanup_error(context, primary.clone(), cleanup),
        None => primary,
    };
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        kind,
        target,
        report,
        Some(public_error.clone()),
    );
    match sink.and_then(|sink| sink.record(record)) {
        Ok(()) => public_error,
        Err(record_error) => compose_frontend_worker_cleanup_record_failure(
            "frontend worker cleanup diagnostic record failed after rollback",
            public_error,
            record_error,
        ),
    }
}

fn restore_frontend_state_after_primary_failure(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    frontend_snapshot: FrontendRuntimeSnapshot,
    demux_rollback_tokens: DemuxRollbackTokenList,
    primary: HalError,
    context: &'static str,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> HalError {
    let sink = Ok(guard.frontend_worker_cleanup_diagnostic_sink());
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    if let Some(replacement_context) = replacement_context {
        report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
            target,
            replacement_context.worker_kind,
            replacement_context.stopped_worker_generation,
            replacement_context.new_worker_generation,
            Ok(()),
        ));
    }
    let restore_frontend_result = guard
        .frontend_txn()
        .restore_frontend_runtime_snapshot(frontend_id, frontend_snapshot);
    report.push(FrontendWorkerCleanupStepOutcome::restore_frontend_snapshot(
        target,
        restore_frontend_result,
    ));
    let restore_demux_result = guard
        .frontend_txn()
        .restore_bound_demux_runtime_rollback_tokens(demux_rollback_tokens);
    report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demuxes(
        target,
        restore_demux_result,
    ));
    finish_frontend_worker_rollback_report(sink, kind, target, report, primary, context)
}

fn restore_frontend_state_after_primary_failure_with_shared_demux_tokens(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    frontend_snapshot: FrontendRuntimeSnapshot,
    demux_rollback_tokens: &SharedDemuxRollbackTokenList,
    primary: HalError,
    context: &'static str,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> HalError {
    match take_demux_rollback_tokens(
        demux_rollback_tokens,
        "demux rollback token list was already consumed",
    ) {
        Ok(tokens) => restore_frontend_state_after_primary_failure(
            guard,
            frontend_id,
            frontend_snapshot,
            tokens,
            primary,
            context,
            kind,
            target,
            replacement_context,
        ),
        Err(take_error) => {
            let mut report = FrontendWorkerCleanupExecutionReport::new();
            if let Some(replacement_context) = replacement_context {
                report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
                    target,
                    replacement_context.worker_kind,
                    replacement_context.stopped_worker_generation,
                    replacement_context.new_worker_generation,
                    Ok(()),
                ));
            }
            report.push(
                FrontendWorkerCleanupStepOutcome::take_demux_rollback_tokens(
                    target,
                    Err(take_error.clone()),
                ),
            );
            finish_frontend_worker_rollback_report(
                Ok(guard.frontend_worker_cleanup_diagnostic_sink()),
                kind,
                target,
                report,
                primary,
                context,
            )
        }
    }
}

fn finish_frontend_state_restore_lock_failure_report(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    primary: HalError,
    lock_error: HalError,
    context: &'static str,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> HalError {
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    if let Some(replacement_context) = replacement_context {
        report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
            target,
            replacement_context.worker_kind,
            replacement_context.stopped_worker_generation,
            replacement_context.new_worker_generation,
            Ok(()),
        ));
    }
    report.push(FrontendWorkerCleanupStepOutcome::restore_frontend_snapshot(
        target,
        Err(lock_error),
    ));
    finish_frontend_worker_rollback_report(Ok(sink), kind, target, report, primary, context)
}

fn finish_backend_session_after_worker_body(
    session: FrontendBackendSession,
    body_result: Result<(), HalError>,
) -> Result<(), HalError> {
    let stop_result = session.stop();
    match (body_result, stop_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(stop_error)) => Err(stop_error),
        (Err(primary), Ok(())) => Err(primary),
        (Err(primary), Err(stop_error)) => Err(compose_frontend_cleanup_error(
            "frontend backend session stop failed after worker body error",
            primary,
            stop_error,
        )),
    }
}

fn finish_backend_session_before_frontend_commit_failure(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    generation: u64,
    session: FrontendBackendSession,
    primary: HalError,
    context: &'static str,
) -> HalError {
    let stop_result = session.stop();
    let backend_stopped = stop_result.is_ok();
    let public_error = match stop_result {
        Ok(()) => primary,
        Err(stop_error) => compose_frontend_cleanup_error(context, primary, stop_error),
    };
    match guard
        .frontend_txn()
        .record_frontend_backend_request_failure_after_fence(
            frontend_id,
            generation,
            public_error.clone(),
            backend_stopped,
        ) {
        Ok(()) => public_error,
        Err(record_error) => compose_frontend_cleanup_error(
            "frontend backend failure state record failed",
            public_error,
            record_error,
        ),
    }
}

fn finish_backend_session_after_frontend_commit_activation_failure(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    generation: u64,
    session: FrontendBackendSession,
    primary: HalError,
    context: &'static str,
) -> HalError {
    let stop_result = session.stop();
    let backend_stopped = stop_result.is_ok();
    let public_error = match stop_result {
        Ok(()) => primary,
        Err(stop_error) => compose_frontend_cleanup_error(context, primary, stop_error),
    };
    match guard
        .frontend_txn()
        .record_frontend_backend_activation_failure_after_commit(
            frontend_id,
            generation,
            public_error.clone(),
            backend_stopped,
        ) {
        Ok(()) => public_error,
        Err(record_error) => compose_frontend_cleanup_error(
            "frontend backend activation failure state record failed",
            public_error,
            record_error,
        ),
    }
}

fn record_backend_submit_failure_after_fence(
    guard: &mut TunerServiceRuntime,
    frontend_id: i32,
    generation: u64,
    backend_stopped: bool,
    public_error: HalError,
) -> HalError {
    match guard
        .frontend_txn()
        .record_frontend_backend_request_failure_after_fence(
            frontend_id,
            generation,
            public_error.clone(),
            backend_stopped,
        ) {
        Ok(()) => public_error,
        Err(record_error) => compose_frontend_cleanup_error(
            "frontend backend submission failure state record failed",
            public_error,
            record_error,
        ),
    }
}

enum FrontendBackendSubmitDeadlineOutcome {
    Completed(Result<FrontendBackendSession, FrontendBackendSubmitFailure>),
    TimedOut(FrontendBackendSubmitTicket),
}

const FRONTEND_BACKEND_SUBMIT_TIMEOUT_ERRNO: i32 = 110;

fn submit_frontend_backend_with_deadline(
    plan: FrontendBackendTunePlan,
    previous_request: Option<FrontendTuneRequest>,
    generation: u64,
    deadline_ms: u64,
) -> Result<FrontendBackendSubmitDeadlineOutcome, HalError> {
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(deadline_ms))
        .ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend backend submit deadline overflow",
            )
        })?;
    let ticket = FrontendBackendSubmitTicket::start(plan, previous_request)?;
    match ticket.wait_until(deadline) {
        Ok(FrontendBackendSubmitWait::Completed(result)) => {
            Ok(FrontendBackendSubmitDeadlineOutcome::Completed(result))
        }
        Ok(FrontendBackendSubmitWait::TimedOut(ticket)) => {
            Ok(FrontendBackendSubmitDeadlineOutcome::TimedOut(ticket))
        }
        Err(error) => Ok(FrontendBackendSubmitDeadlineOutcome::Completed(Err(
            FrontendBackendSubmitFailure::indeterminate(generation, error),
        ))),
    }
}

fn frontend_backend_submit_timeout_error(deadline_ms: u64) -> HalError {
    HalError::Io {
        backend: "frontend",
        operation: "backend submit",
        path: None,
        errno: Some(FRONTEND_BACKEND_SUBMIT_TIMEOUT_ERRNO),
        detail: HalErrorDetail::new(format!(
            "backend request did not finish within worker I/O deadline ({deadline_ms} ms)"
        )),
    }
}

fn transfer_timed_out_frontend_backend_submit(
    reaper: &FrontendWorkerReaperHandle,
    guard: &mut TunerServiceRuntime,
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    generation: u64,
    expected_demux_generations: BoundDemuxGenerationSnapshot,
    ticket: FrontendBackendSubmitTicket,
    diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
    deadline_ms: u64,
) -> HalError {
    let timeout_error = frontend_backend_submit_timeout_error(deadline_ms);
    let public_error = record_backend_submit_failure_after_fence(
        guard,
        target.frontend_id(),
        generation,
        false,
        timeout_error,
    );
    enqueue_timed_out_frontend_backend_submit(
        reaper,
        guard,
        target,
        worker_kind,
        generation,
        expected_demux_generations,
        ticket,
        diagnostic_sink,
        public_error,
    )
}

fn transfer_timed_out_active_scan_submit(
    reaper: &FrontendWorkerReaperHandle,
    guard: &mut TunerServiceRuntime,
    target: FrontendWorkerCleanupTarget,
    generation: u64,
    expected_demux_generations: BoundDemuxGenerationSnapshot,
    ticket: FrontendBackendSubmitTicket,
    diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
    deadline_ms: u64,
) -> HalError {
    let timeout_error = frontend_backend_submit_timeout_error(deadline_ms);
    let public_error = match guard
        .frontend_txn()
        .mark_frontend_scan_session_backend_failed(target.frontend_id(), generation)
    {
        Ok(()) => timeout_error,
        Err(mark_error) => compose_frontend_cleanup_error(
            "frontend scan submit timeout state commit failed",
            timeout_error,
            mark_error,
        ),
    };
    enqueue_timed_out_frontend_backend_submit(
        reaper,
        guard,
        target,
        FrontendWorkerKind::Scan,
        generation,
        expected_demux_generations,
        ticket,
        diagnostic_sink,
        public_error,
    )
}

fn enqueue_timed_out_frontend_backend_submit(
    reaper: &FrontendWorkerReaperHandle,
    guard: &mut TunerServiceRuntime,
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    generation: u64,
    expected_demux_generations: BoundDemuxGenerationSnapshot,
    ticket: FrontendBackendSubmitTicket,
    diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
    public_error: HalError,
) -> HalError {
    let deadline_sink = diagnostic_sink.clone();
    let completion_sink = diagnostic_sink;
    let completion_error = public_error.clone();
    let cleanup_ticket = FrontendWorkerStopTicket::backend_submit_cleanup(
        target.frontend_id(),
        worker_kind,
        generation,
        ticket,
    );
    let job = FrontendWorkerReaperJob {
        keys: vec![(target.frontend_id(), worker_kind)],
        continuation_kind: None,
        tickets: FrontendWorkerReaperTicketGroup::new(vec![(worker_kind, cleanup_ticket)]),
        transferred_at: Instant::now(),
        deadline_action: Some(Box::new(move |runtime| {
            handle_frontend_worker_reaper_deadline(
                runtime,
                target,
                worker_kind,
                generation,
                expected_demux_generations,
                deadline_sink,
            );
        })),
        completion_action: Box::new(move |runtime, outcomes, deadline_elapsed| {
            accept_frontend_worker_terminal_outcomes(runtime, &outcomes);
            let recorded_error = if deadline_elapsed {
                compose_frontend_cleanup_error(
                    "frontend backend submit reaper deadline elapsed",
                    completion_error.clone(),
                    HalError::cleanup_failed(
                        "frontend backend submit reaper",
                        "submit operation did not exit before the reaper deadline",
                    ),
                )
            } else {
                completion_error.clone()
            };
            if record_aborted_frontend_replacement_after_reap(
                completion_sink,
                target,
                worker_kind,
                generation,
                &outcomes,
                recorded_error,
            )
            .is_err()
            {
                if let Ok(mut guard) = runtime.lock() {
                    guard.mark_service_critical();
                }
            }
        }),
    };
    if let Err(transfer_error) = reaper.enqueue(job) {
        guard.mark_service_critical();
        return compose_frontend_cleanup_error(
            "frontend backend submit reaper transfer failed",
            public_error,
            transfer_error,
        );
    }
    public_error
}

fn stop_live_pump_after_worker_error(
    live_pump: &mut Option<FrontendLivePumpOwner>,
    body_result: &mut Result<(), HalError>,
) {
    if body_result.is_ok() {
        return;
    }
    let Some(owner) = live_pump.take() else {
        return;
    };
    if let Err(stop_error) = owner.join_after_stop() {
        let primary = match std::mem::replace(body_result, Ok(())) {
            Err(error) => error,
            Ok(()) => return,
        };
        *body_result = Err(compose_frontend_cleanup_error(
            "frontend live pump stop failed after worker body error",
            primary,
            stop_error,
        ));
    }
}

fn frontend_worker_stop_failure(outcome: &FrontendWorkerStopOutcome) -> Option<HalError> {
    match outcome {
        FrontendWorkerStopOutcome::StopRequestFailed { error, .. }
        | FrontendWorkerStopOutcome::Completed {
            result: Err(error), ..
        } => Some(error.clone()),
        _ => None,
    }
}

fn frontend_worker_stop_result(
    outcome: &Result<FrontendWorkerStopOutcome, HalError>,
) -> Result<(), HalError> {
    match outcome {
        Ok(outcome) => frontend_worker_stop_failure(outcome).map_or(Ok(()), Err),
        Err(error) => Err(error.clone()),
    }
}

fn frontend_worker_stop_result_from_outcome(
    outcome: &FrontendWorkerStopOutcome,
) -> Result<(), HalError> {
    frontend_worker_stop_failure(outcome).map_or(Ok(()), Err)
}

fn frontend_worker_stop_result_generation(
    outcome: &Result<FrontendWorkerStopOutcome, HalError>,
) -> Option<u64> {
    outcome
        .as_ref()
        .ok()
        .and_then(frontend_worker_stop_outcome_generation)
}

fn compose_frontend_worker_cleanup_finish_result(
    cleanup_result: Result<(), HalError>,
    record_result: Result<(), HalError>,
) -> Result<(), HalError> {
    match (cleanup_result, record_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(()), Err(record_error)) => Err(record_error),
        (Err(primary), Err(record_error)) => Err(compose_primary_cleanup_failure(
            "frontend worker cleanup diagnostic record failed",
            primary,
            record_error,
        )),
    }
}

fn finish_frontend_worker_cleanup_report(
    sink: Result<SharedFrontendWorkerCleanupDiagnostics, HalError>,
    record: FrontendWorkerCleanupDiagnosticRecord,
) -> Result<(), HalError> {
    let cleanup_result = record.report().clone().into_result();
    let record_result = sink.and_then(|sink| sink.record(record));
    compose_frontend_worker_cleanup_finish_result(cleanup_result, record_result)
}

fn build_frontend_worker_replacement_stop_report(
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    stop_outcome: &FrontendWorkerStopOutcome,
    scan_cancel_result: Option<Result<(), HalError>>,
) -> FrontendWorkerCleanupExecutionReport {
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
        target,
        worker_kind,
        frontend_worker_stop_outcome_generation(stop_outcome),
        frontend_worker_stop_result_from_outcome(stop_outcome),
    ));
    if let Some(scan_cancel_result) = scan_cancel_result {
        report.push(FrontendWorkerCleanupStepOutcome::record_scan_cancelled(
            target,
            frontend_worker_stop_outcome_generation(stop_outcome),
            scan_cancel_result,
        ));
    }
    report
}

fn record_frontend_worker_replacement_stop_diagnostic(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    stop_outcome: &FrontendWorkerStopOutcome,
    scan_cancel_result: Option<Result<(), HalError>>,
    post_stop_failure: Option<(Option<u64>, u64, HalError)>,
) -> Result<(), HalError> {
    let mut report = build_frontend_worker_replacement_stop_report(
        target,
        worker_kind,
        stop_outcome,
        scan_cancel_result,
    );
    let public_error =
        if let Some((stopped_generation, new_generation, primary)) = post_stop_failure {
            report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
                target,
                worker_kind,
                stopped_generation,
                new_generation,
                Err(primary.clone()),
            ));
            Some(primary)
        } else {
            report.clone().into_result().err()
        };
    let record = FrontendWorkerCleanupDiagnosticRecord::new(kind, target, report, public_error);
    sink.record(record)
}

fn record_frontend_worker_replacement_stop_report(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    kind: FrontendWorkerCleanupDiagnosticKind,
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    stop_outcome: &FrontendWorkerStopOutcome,
    scan_cancel_result: Option<Result<(), HalError>>,
) -> Result<(), HalError> {
    let report = build_frontend_worker_replacement_stop_report(
        target,
        worker_kind,
        stop_outcome,
        scan_cancel_result,
    );
    let cleanup_result = report.clone().into_result();
    let public_error = cleanup_result.clone().err();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(kind, target, report, public_error);
    let record_result = sink.record(record);
    compose_frontend_worker_cleanup_finish_result(cleanup_result, record_result)
}

fn resolve_frontend_object_for_method(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<(i32, FrontendRegistryEntry), HalError> {
    let entry = runtime.frontend_entry_for_aidl_object(object_id, generation)?;
    Ok((entry.id.0, entry))
}

fn ensure_frontend_object_still_live(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<(), HalError> {
    aidl_object_live(runtime, object_id, generation, AidlObjectKind::Frontend)
}

fn resolve_frontend_object_for_close_cleanup(
    runtime: &TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<(i32, FrontendRegistryEntry), HalError> {
    let frontend_id = aidl_public_runtime_id_for_close_cleanup(
        runtime,
        object_id,
        generation,
        AidlObjectKind::Frontend,
    )?;
    let entry = runtime
        .frontend_entry(frontend_id)
        .ok_or_else(|| HalError::Unsupported("frontend runtime entry is not available"))?;
    Ok((frontend_id, entry))
}

fn record_scan_cancelled_from_stop_outcome_locked(
    runtime: &mut TunerServiceRuntime,
    frontend_id: i32,
    outcome: &FrontendWorkerStopOutcome,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    let generation = match outcome {
        FrontendWorkerStopOutcome::NotRunning => {
            let snapshot = runtime.query().frontend_runtime_snapshot(frontend_id)?;
            let Some(session) = snapshot.scan_session else {
                return Ok(());
            };
            if session.phase() != FrontendScanPhase::LockedReported {
                return Ok(());
            }
            session.generation()
        }
        FrontendWorkerStopOutcome::StopRequestFailed { error, .. } => return Err(error.clone()),
        FrontendWorkerStopOutcome::CancelRequested { generation, .. }
        | FrontendWorkerStopOutcome::Completed { generation, .. } => *generation,
    };
    runtime
        .frontend_txn()
        .cancel_frontend_scan_session(frontend_id, generation, reason)
}

fn mark_tune_worker_failed(
    runtime: &SharedRuntime,
    frontend_id: i32,
    generation: u64,
    error: HalError,
) -> Result<(), HalError> {
    let mut guard = lock_runtime(
        runtime,
        "service runtime lock poisoned while marking tune worker failure",
    )?;
    guard
        .frontend_txn()
        .mark_frontend_tune_worker_failed(frontend_id, generation, error)
}

#[cfg(test)]
fn rollback_started_tune_worker_after_commit_failure(
    runtime: &SharedRuntime,
    cleanup_diagnostic_sink: Result<SharedFrontendWorkerCleanupDiagnostics, HalError>,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    snapshot: FrontendRuntimeSnapshot,
    demux_rollback_tokens: &SharedDemuxRollbackTokenList,
    commit_error: HalError,
    target: FrontendWorkerCleanupTarget,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> HalError {
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    if let Some(replacement_context) = replacement_context {
        report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
            target,
            replacement_context.worker_kind,
            replacement_context.stopped_worker_generation,
            replacement_context.new_worker_generation,
            Ok(()),
        ));
    }
    match stop_frontend_worker(
        Arc::clone(runtime),
        frontend_id,
        kind,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    ) {
        Ok(outcome) => report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            kind,
            frontend_worker_stop_outcome_generation(&outcome),
            frontend_worker_stop_result_from_outcome(&outcome),
        )),
        Err(error) => report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            kind,
            None,
            Err(error),
        )),
    }

    match lock_runtime(
        runtime,
        "service runtime lock poisoned while rolling back tune commit failure",
    ) {
        Ok(mut guard) => {
            let restore_frontend_result = guard
                .frontend_txn()
                .restore_frontend_runtime_snapshot(frontend_id, snapshot);
            report.push(FrontendWorkerCleanupStepOutcome::restore_frontend_snapshot(
                target,
                restore_frontend_result,
            ));
            let take_result = take_demux_rollback_tokens(
                demux_rollback_tokens,
                "demux rollback token list was already consumed during tune commit rollback",
            );
            match take_result {
                Ok(tokens) => {
                    report.push(
                        FrontendWorkerCleanupStepOutcome::take_demux_rollback_tokens(
                            target,
                            Ok(()),
                        ),
                    );
                    let demux_restore_result = guard
                        .frontend_txn()
                        .restore_bound_demux_runtime_rollback_tokens(tokens);
                    report.push(FrontendWorkerCleanupStepOutcome::restore_bound_demuxes(
                        target,
                        demux_restore_result,
                    ));
                }
                Err(error) => {
                    report.push(
                        FrontendWorkerCleanupStepOutcome::take_demux_rollback_tokens(
                            target,
                            Err(error),
                        ),
                    );
                }
            }
        }
        Err(error) => report.push(FrontendWorkerCleanupStepOutcome::restore_frontend_snapshot(
            target,
            Err(error),
        )),
    }

    finish_frontend_worker_rollback_report(
        cleanup_diagnostic_sink,
        FrontendWorkerCleanupDiagnosticKind::TuneCommitRollback,
        target,
        report,
        commit_error,
        "frontend tune commit failed after worker start",
    )
}

pub(crate) fn request_tune_worker_replacement_stop(
    runtime: &mut TunerServiceRuntime,
    frontend_id: i32,
) -> FrontendWorkerStopTicket {
    runtime.frontend_txn().request_worker_stop_for_join(
        frontend_id,
        FrontendWorkerKind::Tune,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrontendLockQualification {
    Locked,
    Unlocked,
    TmccPending,
    TmccMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrontendLockWaitOutcome {
    Locked,
    NoSignal,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrontendLockTransition {
    None,
    Locked,
    LostLock,
    NoSignal,
}

fn frontend_terminal_deadline(backend: FrontendBackendKind) -> Duration {
    match backend {
        FrontendBackendKind::LinuxDvb => Duration::from_millis(4_000),
        FrontendBackendKind::Px4CharDevice => Duration::from_millis(7_000),
    }
}

fn classify_frontend_lock_qualification(
    signal_state: FrontendSignalState,
    requirement: FrontendIsdbtPartialReceptionRequirement,
    tmcc_observation: Option<FrontendTmccPartialReceptionObservation>,
) -> Result<FrontendLockQualification, HalError> {
    if signal_state != FrontendSignalState::Locked {
        return Ok(FrontendLockQualification::Unlocked);
    }
    let FrontendIsdbtPartialReceptionRequirement::Required(expected) = requirement else {
        return Ok(FrontendLockQualification::Locked);
    };
    match tmcc_observation {
        Some(FrontendTmccPartialReceptionObservation::Available(observed))
            if observed == expected =>
        {
            Ok(FrontendLockQualification::Locked)
        }
        Some(FrontendTmccPartialReceptionObservation::Available(_)) => {
            Ok(FrontendLockQualification::TmccMismatch)
        }
        Some(FrontendTmccPartialReceptionObservation::Pending) => {
            Ok(FrontendLockQualification::TmccPending)
        }
        None => Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "locked explicit partial reception request lacks a TMCC observation",
        )),
    }
}

fn frontend_lock_transition(
    lock_announced: bool,
    signal_state: FrontendSignalState,
    qualification: FrontendLockQualification,
) -> FrontendLockTransition {
    if lock_announced {
        return if matches!(
            signal_state,
            FrontendSignalState::NoSignal | FrontendSignalState::SignalDetected
        ) {
            FrontendLockTransition::LostLock
        } else {
            FrontendLockTransition::None
        };
    }
    match qualification {
        FrontendLockQualification::Locked => FrontendLockTransition::Locked,
        FrontendLockQualification::TmccMismatch => FrontendLockTransition::NoSignal,
        FrontendLockQualification::Unlocked | FrontendLockQualification::TmccPending => {
            FrontendLockTransition::None
        }
    }
}

fn observe_frontend_lock_qualification(
    session: &FrontendBackendSession,
) -> Result<(FrontendSignalState, FrontendLockQualification), HalError> {
    let signal_state = session.observe_signal_state()?;
    let requirement = session.partial_reception_requirement();
    let tmcc_observation = if signal_state == FrontendSignalState::Locked
        && matches!(
            requirement,
            FrontendIsdbtPartialReceptionRequirement::Required(_)
        ) {
        Some(session.observe_tmcc_partial_reception()?)
    } else {
        None
    };
    let qualification =
        classify_frontend_lock_qualification(signal_state, requirement, tmcc_observation)?;
    Ok((signal_state, qualification))
}

fn record_frontend_signal_observation(
    runtime: &SharedRuntime,
    ctx: &FrontendWorkerContext,
    frontend_id: i32,
    generation: u64,
    signal_state: FrontendSignalState,
) -> Result<(), HalError> {
    if ctx.cancel_requested() {
        return Ok(());
    }
    let mut guard = lock_runtime(
        runtime,
        "service runtime lock poisoned while recording frontend signal state",
    )?;
    guard
        .frontend_txn()
        .record_frontend_signal_state(frontend_id, generation, signal_state)
}

fn record_frontend_tune_lock_qualification(
    runtime: &SharedRuntime,
    ctx: &FrontendWorkerContext,
    frontend_id: i32,
    generation: u64,
) -> Result<bool, HalError> {
    if ctx.cancel_requested() {
        return Ok(false);
    }
    let mut guard = lock_runtime(
        runtime,
        "service runtime lock poisoned while recording qualified frontend tune lock",
    )?;
    if ctx.cancel_requested() {
        return Ok(false);
    }
    guard
        .frontend_txn()
        .record_frontend_tune_lock_qualified(frontend_id, generation)?;
    Ok(true)
}

fn wait_for_frontend_qualified_lock(
    runtime: &SharedRuntime,
    ctx: &FrontendWorkerContext,
    session: &FrontendBackendSession,
    backend: FrontendBackendKind,
    frontend_id: i32,
    generation: u64,
) -> Result<FrontendLockWaitOutcome, HalError> {
    let started = Instant::now();
    let deadline = frontend_terminal_deadline(backend);
    loop {
        if ctx.cancel_requested() {
            return Ok(FrontendLockWaitOutcome::Cancelled);
        }
        let (signal_state, qualification) = observe_frontend_lock_qualification(session)?;
        if ctx.cancel_requested() {
            return Ok(FrontendLockWaitOutcome::Cancelled);
        }
        record_frontend_signal_observation(runtime, ctx, frontend_id, generation, signal_state)?;
        if ctx.cancel_requested() {
            return Ok(FrontendLockWaitOutcome::Cancelled);
        }
        match qualification {
            FrontendLockQualification::Locked => return Ok(FrontendLockWaitOutcome::Locked),
            FrontendLockQualification::TmccMismatch => {
                return Ok(FrontendLockWaitOutcome::NoSignal)
            }
            FrontendLockQualification::Unlocked | FrontendLockQualification::TmccPending => {}
        }
        if started.elapsed() >= deadline {
            return Ok(FrontendLockWaitOutcome::NoSignal);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn record_frontend_tune_no_signal(
    runtime: &SharedRuntime,
    frontend_id: i32,
    generation: u64,
    tune_notifier: &FrontendTuneNotifier,
) -> Result<(), HalError> {
    deliver_committed_tune_notification(
        runtime,
        tune_notifier,
        frontend_id,
        generation,
        FrontendTuneNotification::NoSignal,
    )?;
    let mut guard = lock_runtime(
        runtime,
        "service runtime lock poisoned while recording tune no-signal",
    )?;
    guard
        .frontend_txn()
        .mark_frontend_tune_no_signal(frontend_id, generation)
}

#[cfg(test)]
mod frontend_readback_tests {
    use super::*;

    #[test]
    fn unspecified_partial_reception_needs_only_demod_lock() {
        assert_eq!(
            classify_frontend_lock_qualification(
                FrontendSignalState::Locked,
                FrontendIsdbtPartialReceptionRequirement::Unspecified,
                None,
            ),
            Ok(FrontendLockQualification::Locked)
        );
        assert_eq!(
            classify_frontend_lock_qualification(
                FrontendSignalState::NoSignal,
                FrontendIsdbtPartialReceptionRequirement::Unspecified,
                None,
            ),
            Ok(FrontendLockQualification::Unlocked)
        );
    }

    #[test]
    fn explicit_partial_reception_requires_matching_fresh_tmcc() {
        for expected in [false, true] {
            assert_eq!(
                classify_frontend_lock_qualification(
                    FrontendSignalState::Locked,
                    FrontendIsdbtPartialReceptionRequirement::Required(expected),
                    Some(FrontendTmccPartialReceptionObservation::Available(expected)),
                ),
                Ok(FrontendLockQualification::Locked)
            );
            assert_eq!(
                classify_frontend_lock_qualification(
                    FrontendSignalState::Locked,
                    FrontendIsdbtPartialReceptionRequirement::Required(expected),
                    Some(FrontendTmccPartialReceptionObservation::Available(
                        !expected
                    )),
                ),
                Ok(FrontendLockQualification::TmccMismatch)
            );
        }
        assert_eq!(
            classify_frontend_lock_qualification(
                FrontendSignalState::Locked,
                FrontendIsdbtPartialReceptionRequirement::Required(true),
                Some(FrontendTmccPartialReceptionObservation::Pending),
            ),
            Ok(FrontendLockQualification::TmccPending)
        );
    }

    #[test]
    fn lock_transition_reports_loss_once_and_relock_once() {
        assert_eq!(
            frontend_lock_transition(
                true,
                FrontendSignalState::NoSignal,
                FrontendLockQualification::Unlocked,
            ),
            FrontendLockTransition::LostLock
        );
        assert_eq!(
            frontend_lock_transition(
                false,
                FrontendSignalState::NoSignal,
                FrontendLockQualification::Unlocked,
            ),
            FrontendLockTransition::None
        );
        assert_eq!(
            frontend_lock_transition(
                false,
                FrontendSignalState::Locked,
                FrontendLockQualification::Locked,
            ),
            FrontendLockTransition::Locked
        );
        assert_eq!(
            frontend_lock_transition(
                true,
                FrontendSignalState::Locked,
                FrontendLockQualification::Locked,
            ),
            FrontendLockTransition::None
        );
    }
}

fn run_frontend_backend_tune_session_worker(
    runtime: SharedRuntime,
    ctx: &FrontendWorkerContext,
    session: FrontendBackendSession,
    backend: FrontendBackendKind,
    frontend_id: i32,
    generation: u64,
    tune_notifier: FrontendTuneNotifier,
) -> Result<(), HalError> {
    if ctx.frontend_id() != frontend_id || ctx.generation() != generation {
        return Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend tune worker activation target mismatch",
        ));
    }
    let mut live_pump = None;
    let mut body_result = (|| {
        match wait_for_frontend_qualified_lock(
            &runtime,
            ctx,
            &session,
            backend,
            frontend_id,
            generation,
        )? {
            FrontendLockWaitOutcome::Locked => {
                if !record_frontend_tune_lock_qualification(&runtime, ctx, frontend_id, generation)?
                {
                    return Ok(());
                }
                deliver_committed_tune_notification(
                    &runtime,
                    &tune_notifier,
                    frontend_id,
                    generation,
                    FrontendTuneNotification::Locked,
                )?;
            }
            FrontendLockWaitOutcome::NoSignal => {
                record_frontend_tune_no_signal(&runtime, frontend_id, generation, &tune_notifier)?;
                return Ok(());
            }
            FrontendLockWaitOutcome::Cancelled => return Ok(()),
        }
        let mut lock_announced = true;
        while !ctx.cancel_requested() {
            let (signal_state, qualification) = if lock_announced {
                let signal_state = session.observe_signal_state()?;
                let qualification = if signal_state == FrontendSignalState::Locked {
                    FrontendLockQualification::Locked
                } else {
                    FrontendLockQualification::Unlocked
                };
                (signal_state, qualification)
            } else {
                observe_frontend_lock_qualification(&session)?
            };
            if ctx.cancel_requested() {
                break;
            }
            record_frontend_signal_observation(
                &runtime,
                ctx,
                frontend_id,
                generation,
                signal_state,
            )?;
            if ctx.cancel_requested() {
                break;
            }
            match frontend_lock_transition(lock_announced, signal_state, qualification) {
                FrontendLockTransition::Locked => {
                    if !record_frontend_tune_lock_qualification(
                        &runtime,
                        ctx,
                        frontend_id,
                        generation,
                    )? {
                        break;
                    }
                    deliver_committed_tune_notification(
                        &runtime,
                        &tune_notifier,
                        frontend_id,
                        generation,
                        FrontendTuneNotification::Locked,
                    )?;
                    lock_announced = true;
                }
                FrontendLockTransition::LostLock => {
                    deliver_committed_tune_notification(
                        &runtime,
                        &tune_notifier,
                        frontend_id,
                        generation,
                        FrontendTuneNotification::LostLock,
                    )?;
                    lock_announced = false;
                }
                FrontendLockTransition::NoSignal => {
                    record_frontend_tune_no_signal(
                        &runtime,
                        frontend_id,
                        generation,
                        &tune_notifier,
                    )?;
                    break;
                }
                FrontendLockTransition::None => {}
            }
            if live_pump.is_none() {
                let live_reader_descriptor = {
                    let guard = lock_runtime(
                        &runtime,
                        "service runtime lock poisoned while checking frontend live pump readiness",
                    )?;
                    guard
                        .query()
                        .frontend_live_reader_descriptor_for_live_pump(frontend_id)?
                };
                if let Some(descriptor) = live_reader_descriptor {
                    let reader = session.open_live_reader(&descriptor)?;
                    live_pump = Some(start_frontend_demux_live_pump_from_reader(
                        Arc::clone(&runtime),
                        frontend_id,
                        reader,
                    )?);
                }
            }
            let completed_live_pump =
                live_pump
                    .as_mut()
                    .and_then(|owner| match owner.collect_if_finished() {
                        FrontendLivePumpJoinOutcome::Running => None,
                        FrontendLivePumpJoinOutcome::Completed(result) => Some(result),
                    });
            if let Some(result) = completed_live_pump {
                live_pump = None;
                let report = result?;
                let mut guard = lock_runtime(
                    &runtime,
                    "service runtime lock poisoned while recording completed live pump report",
                )?;
                guard.frontend_txn().record_live_pump_report(
                    frontend_id,
                    generation,
                    report,
                    ctx.cancel_reason()?,
                )?;
            }
            thread::sleep(Duration::from_millis(20));
        }
        if let Some(owner) = live_pump.take() {
            let report = owner.join_after_stop()?;
            let mut guard = lock_runtime(
                &runtime,
                "service runtime lock poisoned while recording stopped live pump report",
            )?;
            guard.frontend_txn().record_live_pump_report(
                frontend_id,
                generation,
                report,
                ctx.cancel_reason()?,
            )?;
        }
        Ok(())
    })();
    stop_live_pump_after_worker_error(&mut live_pump, &mut body_result);
    finish_backend_session_after_worker_body(session, body_result)
}

struct CommittedTuneReplacement {
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
    generation: u64,
    entry: FrontendRegistryEntry,
    request: FrontendTuneRequest,
    kind: FrontendWorkerKind,
    tune_notifier: FrontendTuneNotifier,
    cleanup_diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
}

fn first_reaped_worker_generation(
    outcomes: &[(FrontendWorkerKind, FrontendWorkerStopOutcome)],
) -> Option<u64> {
    outcomes
        .iter()
        .find_map(|(_, outcome)| frontend_worker_stop_outcome_generation(outcome))
}

fn accept_frontend_worker_terminal_outcomes(
    runtime: &SharedRuntime,
    outcomes: &[(FrontendWorkerKind, FrontendWorkerStopOutcome)],
) {
    for (_, outcome) in outcomes {
        if let Some(event) = FrontendWorkerTerminalEvent::from_stop_outcome(outcome) {
            let _ = FrontendTuneScanTxn::accept_worker_terminal(runtime, event);
        }
    }
}

fn record_frontend_reaper_completion(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    target: FrontendWorkerCleanupTarget,
    replacement_kind: FrontendWorkerKind,
    new_generation: u64,
    outcomes: &[(FrontendWorkerKind, FrontendWorkerStopOutcome)],
    result: Result<(), HalError>,
) -> Result<(), HalError> {
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    for (kind, outcome) in outcomes {
        report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            *kind,
            frontend_worker_stop_outcome_generation(outcome),
            frontend_worker_stop_result_from_outcome(outcome),
        ));
    }
    report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
        target,
        replacement_kind,
        first_reaped_worker_generation(outcomes),
        new_generation,
        result.clone(),
    ));
    let primary = result.err();
    sink.record(FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::WorkerReaperCompletion,
        target,
        report,
        primary.clone(),
    ))?;
    match primary {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn record_aborted_frontend_replacement_after_reap(
    sink: SharedFrontendWorkerCleanupDiagnostics,
    target: FrontendWorkerCleanupTarget,
    replacement_kind: FrontendWorkerKind,
    new_generation: u64,
    outcomes: &[(FrontendWorkerKind, FrontendWorkerStopOutcome)],
    public_error: HalError,
) -> Result<(), HalError> {
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    for (kind, outcome) in outcomes {
        report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            *kind,
            frontend_worker_stop_outcome_generation(outcome),
            frontend_worker_stop_result_from_outcome(outcome),
        ));
    }
    report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
        target,
        replacement_kind,
        first_reaped_worker_generation(outcomes),
        new_generation,
        Err(public_error.clone()),
    ));
    sink.record(FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::WorkerReaperCompletion,
        target,
        report,
        Some(public_error),
    ))
}

fn handle_frontend_worker_reaper_deadline(
    runtime: &SharedRuntime,
    target: FrontendWorkerCleanupTarget,
    worker_kind: FrontendWorkerKind,
    fenced_generation: u64,
    expected_demux_generations: BoundDemuxGenerationSnapshot,
    diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
) {
    let deadline_error = HalError::cleanup_failed(
        "frontend worker reaper deadline",
        "worker did not exit within 10 seconds after cancellation",
    );
    let result = (|| {
        let mut guard = lock_runtime(
            runtime,
            "service runtime lock poisoned at frontend worker reaper deadline",
        )?;
        let snapshot = guard
            .query()
            .frontend_runtime_snapshot(target.frontend_id())?;
        let current_demux_generations =
            current_bound_demux_generation_snapshot(&guard, target.frontend_id())?;
        let fenced = snapshot.generation >= fenced_generation
            && snapshot.live_reader_descriptor.is_none()
            && bound_demux_generations_are_fenced_at_or_after(
                &current_demux_generations,
                &expected_demux_generations,
            );
        if !fenced {
            guard.mark_service_critical();
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend worker remained live without a valid generation fence",
            ));
        }
        if let (Some(object_id), Some(object_generation)) =
            (target.object_id(), target.object_generation())
        {
            let owner_generation_is_present = guard
                .object_table()
                .entry(object_id)
                .is_some_and(|entry| entry.generation == object_generation);
            if owner_generation_is_present
                && crate::object_close_txn::quarantine_object_cascade(
                    &mut guard,
                    object_id,
                    object_generation,
                )
                .is_err()
            {
                guard.mark_service_critical();
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend worker deadline quarantine failed",
                ));
            }
        }
        Ok(())
    })();
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    report.push(FrontendWorkerCleanupStepOutcome::complete_replacement(
        target,
        worker_kind,
        None,
        fenced_generation,
        Err(deadline_error.clone()),
    ));
    let public_error = match result {
        Ok(()) => deadline_error,
        Err(error) => error,
    };
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::WorkerReaperDeadline,
        target,
        report,
        Some(public_error),
    );
    if diagnostic_sink.record(record).is_err() {
        if let Ok(mut guard) = runtime.lock() {
            guard.mark_service_critical();
        }
    }
}

fn finish_committed_tune_replacement(
    runtime: &SharedRuntime,
    transition: CommittedTuneReplacement,
    outcomes: Vec<(FrontendWorkerKind, FrontendWorkerStopOutcome)>,
    deadline_elapsed: bool,
) -> Result<(), HalError> {
    accept_frontend_worker_terminal_outcomes(runtime, &outcomes);
    let frontend_id = transition.frontend_id;
    let generation = transition.generation;
    let replacement_kind = transition.kind;
    let completion_diagnostic_sink = transition.cleanup_diagnostic_sink.clone();
    let target = FrontendWorkerCleanupTarget::object(
        frontend_id,
        transition.object_id,
        transition.object_generation,
    );
    let reaper = ensure_frontend_worker_reaper(runtime)?;
    let result = (|| {
        if deadline_elapsed {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend worker replacement was quarantined after reaper deadline",
            ));
        }
        for (_, outcome) in &outcomes {
            if let Some(error) = frontend_worker_stop_failure(outcome) {
                return Err(error);
            }
        }
        let mut guard = lock_runtime(
            runtime,
            "service runtime lock poisoned while completing tune replacement",
        )?;
        ensure_frontend_ticket_still_targets_object(
            &guard,
            transition.object_id,
            transition.object_generation,
            frontend_id,
        )?;
        let snapshot = guard
            .query()
            .frontend_runtime_snapshot(frontend_id)?;
        if snapshot.generation > generation && snapshot.live_reader_descriptor.is_none() {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend replacement was cancelled by a later stop or close",
            ));
        }
        if snapshot.generation != generation || snapshot.live_reader_descriptor.is_some()
        {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend replacement fence changed before old worker exit",
            ));
        }
        let plan = FrontendBackendTunePlan::new(
            frontend_id,
            generation,
            transition.entry.backend,
            FrontendDevicePath::new(transition.entry.device_path.clone()),
            transition.request.clone(),
        );
        let worker_io_deadline_ms = guard.capability_snapshot().worker_io_deadline_ms;
        let session = match submit_frontend_backend_with_deadline(
            plan,
            None,
            generation,
            worker_io_deadline_ms,
        ) {
            Ok(FrontendBackendSubmitDeadlineOutcome::Completed(Ok(session))) => session,
            Ok(FrontendBackendSubmitDeadlineOutcome::Completed(Err(failure))) => {
                let backend_stopped = failure.rollback_succeeded;
                let public_error = failure.into_error();
                return Err(record_backend_submit_failure_after_fence(
                    &mut guard,
                    frontend_id,
                    generation,
                    backend_stopped,
                    public_error,
                ));
            }
            Ok(FrontendBackendSubmitDeadlineOutcome::TimedOut(ticket)) => {
                let (expected_demux_generations, snapshot_error) =
                    match current_bound_demux_generation_snapshot(&guard, frontend_id) {
                        Ok(snapshot) => (snapshot, None),
                        Err(error) => {
                            guard.mark_service_critical();
                            (Vec::new(), Some(error))
                        }
                    };
                let timeout_error = transfer_timed_out_frontend_backend_submit(
                    &reaper,
                    &mut guard,
                    target,
                    replacement_kind,
                    generation,
                    expected_demux_generations,
                    ticket,
                    transition.cleanup_diagnostic_sink.clone(),
                    worker_io_deadline_ms,
                );
                return Err(match snapshot_error {
                    Some(snapshot_error) => compose_frontend_cleanup_error(
                        "frontend backend submit timeout demux snapshot failed",
                        timeout_error,
                        snapshot_error,
                    ),
                    None => timeout_error,
                });
            }
            Err(start_error) => {
                return Err(record_backend_submit_failure_after_fence(
                    &mut guard,
                    frontend_id,
                    generation,
                    true,
                    start_error,
                ));
            }
        };
        let runtime_for_worker = Arc::clone(runtime);
        let backend = transition.entry.backend;
        let tune_notifier = transition.tune_notifier;
        let (activation_sender, activation_receiver) = mpsc::sync_channel(1);
        if let Err(start_error) = guard.frontend_txn().start_worker(
            frontend_id,
            replacement_kind,
            generation,
            move |ctx| {
                let result = match activation_receiver.recv() {
                    Ok(FrontendTuneWorkerActivation::Run(session)) => {
                        run_frontend_backend_tune_session_worker(
                            Arc::clone(&runtime_for_worker),
                            &ctx,
                            session,
                            backend,
                            frontend_id,
                            generation,
                            tune_notifier,
                        )
                    }
                    Ok(FrontendTuneWorkerActivation::Abort) => Ok(()),
                    Err(_) => Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend tune worker activation channel disconnected",
                    )),
                };
                finish_frontend_worker_execution(&runtime_for_worker, &ctx, result)
            },
        ) {
            return Err(finish_backend_session_before_frontend_commit_failure(
                &mut guard,
                frontend_id,
                generation,
                session,
                map_frontend_worker_start_error(start_error),
                "frontend backend stop failed after tune worker preparation failure",
            ));
        }
        if let Err(commit_error) = guard.frontend_txn().commit_frontend_tune_after_fence(
            frontend_id,
            generation,
            transition.request,
        ) {
            let activation_error = activation_sender
                .send(FrontendTuneWorkerActivation::Abort)
                .err()
                .map(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend tune worker abort activation failed",
                    )
                });
            let mut error = finish_backend_session_before_frontend_commit_failure(
                &mut guard,
                frontend_id,
                generation,
                session,
                commit_error,
                "frontend backend stop failed after tune commit failure",
            );
            if let Some(activation_error) = activation_error {
                error = compose_frontend_cleanup_error(
                    "frontend tune worker abort failed after commit failure",
                    error,
                    activation_error,
                );
            }
            return Err(error);
        }
        match activation_sender.send(FrontendTuneWorkerActivation::Run(session)) {
            Ok(()) => Ok(()),
            Err(error) => match error.0 {
                FrontendTuneWorkerActivation::Run(session) => {
                    let primary = HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend tune worker ended before backend activation",
                    );
                    Err(finish_backend_session_after_frontend_commit_activation_failure(
                        &mut guard,
                        frontend_id,
                        generation,
                        session,
                        primary,
                        "frontend backend stop failed after tune activation failure",
                    ))
                }
                FrontendTuneWorkerActivation::Abort => Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend tune worker returned an unexpected abort activation",
                )),
            },
        }
    })();
    record_frontend_reaper_completion(
        completion_diagnostic_sink,
        target,
        replacement_kind,
        generation,
        &outcomes,
        result,
    )
}

pub(crate) fn start_frontend_backend_tune_worker(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    request: FrontendTuneRequest,
    kind: FrontendWorkerKind,
    tune_notifier: FrontendTuneNotifier,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let reaper = ensure_frontend_worker_reaper(&runtime)?;
    let request = request.normalized_for_non_blind_operation();
    let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
    dispatch.consume_for_object(
        &mut guard,
        object_id,
        object_generation,
        AidlObjectKind::Frontend,
    )?;
    let (frontend_id, _) =
        resolve_frontend_object_for_method(&guard, object_id, object_generation)?;
    if reaper.is_pending(frontend_id, FrontendWorkerKind::Tune)?
        || reaper.is_pending(frontend_id, FrontendWorkerKind::Scan)?
    {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend endpoint remains owned by the worker reaper",
        ));
    }
    let entry = guard.validate_frontend_request_for_id(frontend_id, &request)?;
    if guard
        .frontend_txn()
        .is_stable_locked_tune_reentry(frontend_id, &request)?
    {
        let generation = guard
            .query()
            .frontend_runtime_snapshot(frontend_id)?
            .generation;
        drop(guard);
        deliver_committed_tune_notification(
            &runtime,
            &tune_notifier,
            frontend_id,
            generation,
            FrontendTuneNotification::Locked,
        )?;
        return Ok(());
    }

    let frontend_snapshot = guard.query().frontend_runtime_snapshot(frontend_id)?;
    let demux_rollback_tokens = guard.prepare_bound_demux_runtime_rollback_tokens(frontend_id)?;
    let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
    let generation = guard
        .frontend_txn()
        .prepare_frontend_worker_replacement_generation(frontend_id, kind)?;
    if let Err(error) = guard
        .frontend_txn()
        .fence_frontend_worker_replacement_generation(frontend_id, generation)
    {
        return match guard
            .frontend_txn()
            .restore_bound_demux_runtime_rollback_tokens(demux_rollback_tokens)
        {
            Ok(()) => Err(error),
            Err(restore_error) => Err(compose_frontend_cleanup_error(
                "frontend demux rollback failed after tune fence failure",
                error,
                restore_error,
            )),
        };
    }
    if frontend_snapshot
        .scan_session
        .as_ref()
        .is_some_and(|session| session.phase() == FrontendScanPhase::Running)
    {
        if let Err(error) = guard.frontend_txn().cancel_frontend_scan_session(
            frontend_id,
            frontend_snapshot.generation,
            FrontendWorkerCancelReason::SupersededByNewRequest,
        ) {
            let mut public_error = error;
            if let Err(commit_error) = guard
                .frontend_txn()
                .commit_bound_demux_runtime_rollback_tokens(demux_rollback_tokens)
            {
                public_error = compose_frontend_cleanup_error(
                    "frontend demux rollback-token commit failed after scan cancellation failure",
                    public_error,
                    commit_error,
                );
            }
            if let Err(quarantine_error) = crate::object_close_txn::quarantine_object_cascade(
                &mut guard,
                object_id,
                object_generation,
            ) {
                public_error = compose_frontend_cleanup_error(
                    "frontend quarantine failed after scan cancellation failure",
                    public_error,
                    quarantine_error,
                );
                guard.mark_service_critical();
            }
            return Err(public_error);
        }
    }
    let scan_stop_ticket = guard.frontend_txn().request_worker_stop_for_join(
        frontend_id,
        FrontendWorkerKind::Scan,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    );
    let tune_stop_ticket = guard.frontend_txn().request_worker_stop_for_join(
        frontend_id,
        FrontendWorkerKind::Tune,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    );
    let tickets = FrontendWorkerReaperTicketGroup::new(vec![
        (FrontendWorkerKind::Scan, scan_stop_ticket),
        (FrontendWorkerKind::Tune, tune_stop_ticket),
    ]);
    let tickets = match tickets.try_complete() {
        Ok(outcomes) => {
            for (_, outcome) in &outcomes {
                if let Some(error) = frontend_worker_stop_failure(outcome) {
                    let mut public_error = error;
                    if let Err(commit_error) = guard
                        .frontend_txn()
                        .commit_bound_demux_runtime_rollback_tokens(demux_rollback_tokens)
                    {
                        public_error = compose_frontend_cleanup_error(
                            "frontend demux rollback-token commit failed after worker stop failure",
                            public_error,
                            commit_error,
                        );
                    }
                    if let Err(quarantine_error) = crate::object_close_txn::quarantine_object_cascade(
                        &mut guard,
                        object_id,
                        object_generation,
                    ) {
                        public_error = compose_frontend_cleanup_error(
                            "frontend quarantine failed after worker stop failure",
                            public_error,
                            quarantine_error,
                        );
                        guard.mark_service_critical();
                    }
                    return Err(public_error);
                }
            }
            Ok(outcomes)
        }
        Err(tickets) => {
            if let Some(error) = tickets
                .completed
                .iter()
                .find_map(|(_, outcome)| frontend_worker_stop_failure(outcome))
            {
                let mut public_error = error;
                if let Err(commit_error) = guard
                    .frontend_txn()
                    .commit_bound_demux_runtime_rollback_tokens(demux_rollback_tokens)
                {
                    public_error = compose_frontend_cleanup_error(
                        "frontend demux rollback-token commit failed with pending worker stop",
                        public_error,
                        commit_error,
                    );
                }
                guard.mark_service_critical();
                core::mem::forget(tickets);
                return Err(public_error);
            }
            Err(tickets)
        }
    };

    let boundary_result = guard.reset_bound_demuxes_for_frontend_tune_start(frontend_id);
    let commit_tokens_result = guard
        .frontend_txn()
        .commit_bound_demux_runtime_rollback_tokens(demux_rollback_tokens);
    if let Err(error) = commit_tokens_result {
        guard.mark_service_critical();
        core::mem::forget(tickets);
        return Err(error);
    }
    if let Err(error) = boundary_result {
        let public_error = match crate::object_close_txn::quarantine_object_cascade(
            &mut guard,
            object_id,
            object_generation,
        ) {
            Ok(_) => error,
            Err(quarantine_error) => {
                guard.mark_service_critical();
                compose_frontend_cleanup_error(
                    "frontend quarantine failed after tune boundary failure",
                    error,
                    quarantine_error,
                )
            }
        };
        core::mem::forget(tickets);
        return Err(public_error);
    }
    let mut pending_stop_error = tickets.is_err().then(|| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend backend stop did not complete before tune returned",
        )
    });
    if let Some(error) = pending_stop_error.as_mut() {
        if let Err(mark_error) = guard
            .frontend_txn()
            .mark_frontend_worker_stop_pending_failure(frontend_id, generation, error.clone())
        {
            guard.mark_service_critical();
            *error = compose_frontend_cleanup_error(
                "frontend pending worker-stop failure state commit failed",
                error.clone(),
                mark_error,
            );
        }
    }
    let fenced_demux_generations = if tickets.is_err() {
        match current_bound_demux_generation_snapshot(&guard, frontend_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                guard.mark_service_critical();
                core::mem::forget(tickets);
                return Err(error);
            }
        }
    } else {
        Vec::new()
    };
    drop(guard);

    match tickets {
        Ok(outcomes) => {
            let transition = CommittedTuneReplacement {
                object_id,
                object_generation,
                frontend_id,
                generation,
                entry,
                request,
                kind,
                tune_notifier,
                cleanup_diagnostic_sink: cleanup_diagnostic_sink.clone(),
            };
            finish_committed_tune_replacement(&runtime, transition, outcomes, false)
        }
        Err(tickets) => {
            let pending_stop_error = match pending_stop_error {
                Some(error) => error,
                None => {
                    let error = HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "pending frontend worker tickets lost their public failure",
                    );
                    if let Ok(mut guard) = runtime.lock() {
                        guard.mark_service_critical();
                    }
                    core::mem::forget(tickets);
                    return Err(error);
                }
            };
            let target = FrontendWorkerCleanupTarget::object(
                frontend_id,
                object_id,
                object_generation,
            );
            let deadline_demux_generations = fenced_demux_generations;
            let completion_public_error = pending_stop_error.clone();
            let deadline_diagnostic_sink = cleanup_diagnostic_sink.clone();
            let job = FrontendWorkerReaperJob {
                keys: vec![
                    (frontend_id, FrontendWorkerKind::Scan),
                    (frontend_id, FrontendWorkerKind::Tune),
                ],
                continuation_kind: None,
                tickets,
                transferred_at: Instant::now(),
                deadline_action: Some(Box::new(move |runtime| {
                    handle_frontend_worker_reaper_deadline(
                        runtime,
                        target,
                        kind,
                        generation,
                        deadline_demux_generations,
                        deadline_diagnostic_sink,
                    );
                })),
                completion_action: Box::new(move |runtime, outcomes, deadline_elapsed| {
                    let completion_error = if deadline_elapsed {
                        compose_frontend_cleanup_error(
                            "frontend tune replacement reaper deadline elapsed",
                            completion_public_error.clone(),
                            HalError::cleanup_failed(
                                "frontend tune replacement reaper",
                                "old worker did not exit before the reaper deadline",
                            ),
                        )
                    } else {
                        completion_public_error.clone()
                    };
                    if record_aborted_frontend_replacement_after_reap(
                        cleanup_diagnostic_sink,
                        target,
                        kind,
                        generation,
                        &outcomes,
                        completion_error,
                    )
                    .is_err()
                    {
                        if let Ok(mut guard) = runtime.lock() {
                            guard.mark_service_critical();
                        }
                    }
                }),
            };
            if let Err(error) = reaper.enqueue(job) {
                if let Ok(mut guard) = runtime.lock() {
                    guard.mark_service_critical();
                }
                return Err(error);
            }
            Err(pending_stop_error)
        }
    }
}

fn run_frontend_backend_scan_session_worker(
    runtime: SharedRuntime,
    ctx: &FrontendWorkerContext,
    backend: FrontendBackendKind,
    device_path: FrontendDevicePath,
    candidates: Vec<FrontendTuneRequest>,
    initial_session: Option<FrontendBackendSession>,
    previous_request: Option<FrontendTuneRequest>,
    target_for_worker: FrontendWorkerCleanupTarget,
    scan_notifier: FrontendScanNotifier,
    cleanup_diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
    replacement_context: Option<FrontendWorkerReplacementRollbackContext>,
) -> Result<(), HalError> {
    let reaper = ensure_frontend_worker_reaper(&runtime)?;
    let worker_io_deadline_ms = {
        let guard = lock_runtime(
            &runtime,
            "service runtime lock poisoned while reading worker I/O deadline",
        )?;
        guard.capability_snapshot().worker_io_deadline_ms
    };
    let mut initial_session = initial_session;
    for candidate in candidates {
        if ctx.cancel_requested() {
            return Ok(());
        }
        let plan = FrontendBackendTunePlan::new(
            ctx.frontend_id(),
            ctx.generation(),
            backend,
            device_path.clone(),
            candidate,
        );
        if let Err(error) = plan.validate_worker_generation(ctx.generation()) {
            return Err(error);
        }
        let session = match initial_session.take() {
            Some(session) => session,
            None => match submit_frontend_backend_with_deadline(
                plan,
                previous_request.clone(),
                ctx.generation(),
                worker_io_deadline_ms,
            ) {
                Ok(FrontendBackendSubmitDeadlineOutcome::Completed(Ok(session))) => session,
                Ok(FrontendBackendSubmitDeadlineOutcome::Completed(Err(failure)))
                    if failure.rollback_succeeded => {
                    let primary = failure.error;
                    let mut guard = match lock_runtime(
                        &runtime,
                        "service runtime lock poisoned while recording rejected scan submission",
                    ) {
                        Ok(guard) => guard,
                        Err(lock_error) => {
                            let error = finish_frontend_state_restore_lock_failure_report(
                                cleanup_diagnostic_sink.clone(),
                                primary,
                                lock_error,
                                "frontend scan submission failure marking failed",
                                FrontendWorkerCleanupDiagnosticKind::ScanBackendRollbackStateRestore,
                                target_for_worker,
                                replacement_context,
                            );
                            return Err(error);
                        }
                    };
                    if let Err(mark_error) = guard
                        .frontend_txn()
                        .mark_frontend_scan_submit_rejected_after_boundary(
                            ctx.frontend_id(),
                            ctx.generation(),
                            primary.clone(),
                        )
                    {
                        let error = compose_frontend_cleanup_error(
                            "frontend scan submission failure marking failed",
                            primary,
                            mark_error,
                        );
                        return Err(error);
                    }
                    return Err(primary);
                }
                Ok(FrontendBackendSubmitDeadlineOutcome::Completed(Err(failure))) => {
                    let primary = failure.error;
                    let mut guard = match lock_runtime(
                        &runtime,
                        "service runtime lock poisoned while marking scan backend failure",
                    ) {
                        Ok(guard) => guard,
                        Err(lock_error) => {
                            let error = finish_frontend_state_restore_lock_failure_report(
                                cleanup_diagnostic_sink.clone(),
                                primary,
                                lock_error,
                                "frontend scan backend failure marking failed",
                                FrontendWorkerCleanupDiagnosticKind::ScanBackendRollbackStateRestore,
                                target_for_worker,
                                replacement_context,
                            );
                            return Err(error);
                        }
                    };
                    if let Err(mark_error) = guard
                        .frontend_txn()
                        .mark_frontend_scan_session_backend_failed(
                            ctx.frontend_id(),
                            ctx.generation(),
                        )
                    {
                        let error = compose_frontend_cleanup_error(
                            "frontend scan backend failure marking failed",
                            primary,
                            mark_error,
                        );
                        return Err(error);
                    }
                    return Err(primary);
                }
                Ok(FrontendBackendSubmitDeadlineOutcome::TimedOut(ticket)) => {
                    let mut guard = lock_runtime(
                        &runtime,
                        "service runtime lock poisoned while recording scan submit timeout",
                    )?;
                    let (expected_demux_generations, snapshot_error) =
                        match current_bound_demux_generation_snapshot(&guard, ctx.frontend_id()) {
                            Ok(snapshot) => (snapshot, None),
                            Err(error) => {
                                guard.mark_service_critical();
                                (Vec::new(), Some(error))
                            }
                        };
                    let timeout_error = transfer_timed_out_active_scan_submit(
                        &reaper,
                        &mut guard,
                        target_for_worker,
                        ctx.generation(),
                        expected_demux_generations,
                        ticket,
                        cleanup_diagnostic_sink.clone(),
                        worker_io_deadline_ms,
                    );
                    return Err(match snapshot_error {
                        Some(snapshot_error) => compose_frontend_cleanup_error(
                            "frontend scan submit timeout demux snapshot failed",
                            timeout_error,
                            snapshot_error,
                        ),
                        None => timeout_error,
                    });
                }
                Err(start_error) => {
                    let mut guard = lock_runtime(
                        &runtime,
                        "service runtime lock poisoned while recording scan submit start failure",
                    )?;
                    if let Err(mark_error) = guard
                        .frontend_txn()
                        .mark_frontend_scan_submit_rejected_after_boundary(
                            ctx.frontend_id(),
                            ctx.generation(),
                            start_error.clone(),
                        )
                    {
                        return Err(compose_frontend_cleanup_error(
                            "frontend scan submit start failure state commit failed",
                            start_error,
                            mark_error,
                        ));
                    }
                    return Err(start_error);
                }
            },
        };
        let mut signal_state = FrontendSignalState::NoSignal;
        let body_result = (|| {
            match wait_for_frontend_qualified_lock(
                &runtime,
                ctx,
                &session,
                backend,
                ctx.frontend_id(),
                ctx.generation(),
            )? {
                FrontendLockWaitOutcome::Locked => signal_state = FrontendSignalState::Locked,
                FrontendLockWaitOutcome::NoSignal | FrontendLockWaitOutcome::Cancelled => {}
            }
            Ok(())
        })();
        finish_backend_session_after_worker_body(session, body_result)?;
        if ctx.cancel_requested() {
            return Ok(());
        }
        if signal_state == FrontendSignalState::Locked {
            deliver_committed_scan_notification(
                &runtime,
                &scan_notifier,
                ctx.frontend_id(),
                ctx.generation(),
                FrontendScanNotification::Locked,
            )?;
            let mut guard = lock_runtime(
                &runtime,
                "service runtime lock poisoned while recording scan lock delivery",
            )?;
            guard
                .frontend_txn()
                .mark_frontend_scan_session_locked_reported(
                    ctx.frontend_id(),
                    ctx.generation(),
                )?;
            return Ok(());
        }
        let mut guard = lock_runtime(
            &runtime,
            "service runtime lock poisoned while advancing scan session",
        )?;
        let has_next = guard
            .frontend_txn()
            .advance_frontend_scan_session_after_candidate(ctx.frontend_id(), ctx.generation())?;
        drop(guard);
        if !has_next {
            deliver_committed_scan_notification(
                &runtime,
                &scan_notifier,
                ctx.frontend_id(),
                ctx.generation(),
                FrontendScanNotification::End,
            )?;
            return Ok(());
        }
    }
    Ok(())
}

struct CommittedScanReplacement {
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    frontend_id: i32,
    generation: u64,
    entry: FrontendRegistryEntry,
    fingerprint: String,
    candidates: Vec<FrontendTuneRequest>,
    locked_continuation: bool,
    scan_notifier: FrontendScanNotifier,
    cleanup_diagnostic_sink: SharedFrontendWorkerCleanupDiagnostics,
}

fn finish_committed_scan_replacement(
    runtime: &SharedRuntime,
    transition: CommittedScanReplacement,
    outcomes: Vec<(FrontendWorkerKind, FrontendWorkerStopOutcome)>,
    deadline_elapsed: bool,
) -> Result<(), HalError> {
    accept_frontend_worker_terminal_outcomes(runtime, &outcomes);
    let frontend_id = transition.frontend_id;
    let generation = transition.generation;
    let completion_diagnostic_sink = transition.cleanup_diagnostic_sink.clone();
    let target = FrontendWorkerCleanupTarget::object(
        frontend_id,
        transition.object_id,
        transition.object_generation,
    );
    let stopped_worker_generation = first_reaped_worker_generation(&outcomes);
    let replacement_context = Some(FrontendWorkerReplacementRollbackContext {
        worker_kind: FrontendWorkerKind::Scan,
        stopped_worker_generation,
        new_worker_generation: generation,
    });
    let reaper = ensure_frontend_worker_reaper(runtime)?;
    let result = (|| {
        if deadline_elapsed {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend scan replacement was quarantined after reaper deadline",
            ));
        }
        for (_, outcome) in &outcomes {
            if let Some(error) = frontend_worker_stop_failure(outcome) {
                return Err(error);
            }
        }
        let mut guard = lock_runtime(
            runtime,
            "service runtime lock poisoned while completing scan replacement",
        )?;
        ensure_frontend_ticket_still_targets_object(
            &guard,
            transition.object_id,
            transition.object_generation,
            frontend_id,
        )?;
        let snapshot = guard
            .query()
            .frontend_runtime_snapshot(frontend_id)?;
        if snapshot.generation > generation && snapshot.live_reader_descriptor.is_none() {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend scan replacement was cancelled by a later stop or close",
            ));
        }
        if snapshot.generation != generation || snapshot.live_reader_descriptor.is_some() {
            return Err(HalError::invalid_state(
                HalInvalidStateKind::InvalidLifecycle,
                "frontend scan fence changed before old worker exit",
            ));
        }
        if transition.locked_continuation {
            guard
                .frontend_txn()
                .complete_locked_frontend_scan_continuation_after_fence(
                    frontend_id,
                    generation,
                    transition.fingerprint,
                    transition.candidates,
                )?;
            drop(guard);
            deliver_committed_scan_notification(
                runtime,
                &transition.scan_notifier,
                frontend_id,
                generation,
                FrontendScanNotification::End,
            )?;
            crate::lnb_ops::release_frontend_fixed_power_after_operation(
                runtime,
                crate::registry::FrontendRuntimeId(frontend_id),
            )?;
            return Ok(());
        }
        let first_candidate = transition.candidates.first().cloned().ok_or_else(|| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend scan candidate list is empty after preflight",
            )
        })?;
        let runtime_for_worker = Arc::clone(runtime);
        let backend = transition.entry.backend;
        let device_path = FrontendDevicePath::new(transition.entry.device_path);
        let plan = FrontendBackendTunePlan::new(
            frontend_id,
            generation,
            backend,
            device_path.clone(),
            first_candidate,
        );
        let worker_io_deadline_ms = guard.capability_snapshot().worker_io_deadline_ms;
        let session = match submit_frontend_backend_with_deadline(
            plan,
            None,
            generation,
            worker_io_deadline_ms,
        ) {
            Ok(FrontendBackendSubmitDeadlineOutcome::Completed(Ok(session))) => session,
            Ok(FrontendBackendSubmitDeadlineOutcome::Completed(Err(failure))) => {
                let backend_stopped = failure.rollback_succeeded;
                let public_error = failure.into_error();
                return Err(record_backend_submit_failure_after_fence(
                    &mut guard,
                    frontend_id,
                    generation,
                    backend_stopped,
                    public_error,
                ));
            }
            Ok(FrontendBackendSubmitDeadlineOutcome::TimedOut(ticket)) => {
                let (expected_demux_generations, snapshot_error) =
                    match current_bound_demux_generation_snapshot(&guard, frontend_id) {
                        Ok(snapshot) => (snapshot, None),
                        Err(error) => {
                            guard.mark_service_critical();
                            (Vec::new(), Some(error))
                        }
                    };
                let timeout_error = transfer_timed_out_frontend_backend_submit(
                    &reaper,
                    &mut guard,
                    target,
                    FrontendWorkerKind::Scan,
                    generation,
                    expected_demux_generations,
                    ticket,
                    transition.cleanup_diagnostic_sink.clone(),
                    worker_io_deadline_ms,
                );
                return Err(match snapshot_error {
                    Some(snapshot_error) => compose_frontend_cleanup_error(
                        "frontend scan backend submit timeout demux snapshot failed",
                        timeout_error,
                        snapshot_error,
                    ),
                    None => timeout_error,
                });
            }
            Err(start_error) => {
                return Err(record_backend_submit_failure_after_fence(
                    &mut guard,
                    frontend_id,
                    generation,
                    true,
                    start_error,
                ));
            }
        };
        let scan_notifier = transition.scan_notifier;
        let cleanup_diagnostic_sink = transition.cleanup_diagnostic_sink.clone();
        let candidates_for_worker = transition.candidates.clone();
        let (activation_sender, activation_receiver) = mpsc::sync_channel(1);
        if let Err(start_error) = guard.frontend_txn().start_worker(
            frontend_id,
            FrontendWorkerKind::Scan,
            generation,
            move |ctx| {
                let result = match activation_receiver.recv() {
                    Ok(FrontendScanWorkerActivation::Run(session)) => {
                        run_frontend_backend_scan_session_worker(
                            Arc::clone(&runtime_for_worker),
                            &ctx,
                            backend,
                            device_path,
                            candidates_for_worker,
                            Some(session),
                            None,
                            target,
                            scan_notifier,
                            cleanup_diagnostic_sink,
                            replacement_context,
                        )
                    }
                    Ok(FrontendScanWorkerActivation::Abort) => Ok(()),
                    Err(_) => Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend scan worker activation channel disconnected",
                    )),
                };
                finish_frontend_worker_execution(&runtime_for_worker, &ctx, result)
            },
        ) {
            return Err(finish_backend_session_before_frontend_commit_failure(
                &mut guard,
                frontend_id,
                generation,
                session,
                map_frontend_worker_start_error(start_error),
                "frontend backend stop failed after scan worker preparation failure",
            ));
        }
        if let Err(commit_error) = guard.frontend_txn().commit_frontend_scan_after_fence(
            frontend_id,
            generation,
            transition.fingerprint,
            transition.candidates,
        ) {
            let activation_error = activation_sender
                .send(FrontendScanWorkerActivation::Abort)
                .err()
                .map(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend scan worker abort activation failed",
                    )
                });
            let mut error = finish_backend_session_before_frontend_commit_failure(
                &mut guard,
                frontend_id,
                generation,
                session,
                commit_error,
                "frontend backend stop failed after scan commit failure",
            );
            if let Some(activation_error) = activation_error {
                error = compose_frontend_cleanup_error(
                    "frontend scan worker abort failed after commit failure",
                    error,
                    activation_error,
                );
            }
            return Err(error);
        }
        match activation_sender.send(FrontendScanWorkerActivation::Run(session)) {
            Ok(()) => Ok(()),
            Err(error) => match error.0 {
                FrontendScanWorkerActivation::Run(session) => {
                    let primary = HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "frontend scan worker ended before backend activation",
                    );
                    Err(finish_backend_session_after_frontend_commit_activation_failure(
                        &mut guard,
                        frontend_id,
                        generation,
                        session,
                        primary,
                        "frontend backend stop failed after scan activation failure",
                    ))
                }
                FrontendScanWorkerActivation::Abort => Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "frontend scan worker returned an unexpected abort activation",
                )),
            },
        }
    })();
    record_frontend_reaper_completion(
        completion_diagnostic_sink,
        target,
        FrontendWorkerKind::Scan,
        generation,
        &outcomes,
        result,
    )
}

pub(crate) fn start_frontend_backend_scan_session_worker(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    request: FrontendTuneRequest,
    scan_mode: FrontendScanMode,
    scan_notifier: FrontendScanNotifier,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let reaper = ensure_frontend_worker_reaper(&runtime)?;
    let request = request.normalized_for_non_blind_operation();
    let fingerprint = format!("{:?}:{:?}", scan_mode, request);
    let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
    dispatch.consume_for_object(
        &mut guard,
        object_id,
        object_generation,
        AidlObjectKind::Frontend,
    )?;
    let (frontend_id, _) =
        resolve_frontend_object_for_method(&guard, object_id, object_generation)?;
    if reaper.is_pending(frontend_id, FrontendWorkerKind::Tune)?
        || reaper.is_pending(frontend_id, FrontendWorkerKind::Scan)?
    {
        return Err(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "frontend endpoint remains owned by the worker reaper",
        ));
    }
    let entry = guard.validate_frontend_request_for_id(frontend_id, &request)?;
    let candidates = guard.scan_candidates_for_frontend_entry(&entry, &request, scan_mode)?;
    let frontend_snapshot = guard.query().frontend_runtime_snapshot(frontend_id)?;
    let is_locked_continuation = frontend_snapshot.scan_session.as_ref().is_some_and(|session| {
        session.phase() == FrontendScanPhase::LockedReported
            && session.fingerprint() == fingerprint
    });
    let demux_rollback_tokens = guard.prepare_bound_demux_runtime_rollback_tokens(frontend_id)?;
    let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
    let generation = guard
        .frontend_txn()
        .prepare_frontend_worker_replacement_generation(frontend_id, FrontendWorkerKind::Scan)?;
    if !is_locked_continuation
        && frontend_snapshot
            .scan_session
            .as_ref()
            .is_some_and(|session| session.phase() == FrontendScanPhase::Running)
    {
        if let Err(error) = guard.frontend_txn().cancel_frontend_scan_session(
            frontend_id,
            frontend_snapshot.generation,
            FrontendWorkerCancelReason::SupersededByNewRequest,
        ) {
            return match guard
                .frontend_txn()
                .restore_bound_demux_runtime_rollback_tokens(demux_rollback_tokens)
            {
                Ok(()) => Err(error),
                Err(restore_error) => Err(compose_frontend_cleanup_error(
                    "frontend demux rollback failed after scan cancellation failure",
                    error,
                    restore_error,
                )),
            };
        }
    }
    if let Err(error) = guard
        .frontend_txn()
        .fence_frontend_worker_replacement_generation(frontend_id, generation)
    {
        return match guard
            .frontend_txn()
            .restore_bound_demux_runtime_rollback_tokens(demux_rollback_tokens)
        {
            Ok(()) => Err(error),
            Err(restore_error) => Err(compose_frontend_cleanup_error(
                "frontend demux rollback failed after scan fence failure",
                error,
                restore_error,
            )),
        };
    }
    let tune_stop_ticket = guard.frontend_txn().request_worker_stop_for_join(
        frontend_id,
        FrontendWorkerKind::Tune,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    );
    let scan_stop_ticket = guard.frontend_txn().request_worker_stop_for_join(
        frontend_id,
        FrontendWorkerKind::Scan,
        FrontendWorkerCancelReason::SupersededByNewRequest,
    );
    let tickets = FrontendWorkerReaperTicketGroup::new(vec![
        (FrontendWorkerKind::Tune, tune_stop_ticket),
        (FrontendWorkerKind::Scan, scan_stop_ticket),
    ]);
    let tickets = match tickets.try_complete() {
        Ok(outcomes) => {
            for (_, outcome) in &outcomes {
                if let Some(error) = frontend_worker_stop_failure(outcome) {
                    let mut public_error = error;
                    if let Err(commit_error) = guard
                        .frontend_txn()
                        .commit_bound_demux_runtime_rollback_tokens(demux_rollback_tokens)
                    {
                        public_error = compose_frontend_cleanup_error(
                            "frontend demux rollback-token commit failed after scan worker stop failure",
                            public_error,
                            commit_error,
                        );
                    }
                    if let Err(quarantine_error) = crate::object_close_txn::quarantine_object_cascade(
                        &mut guard,
                        object_id,
                        object_generation,
                    ) {
                        public_error = compose_frontend_cleanup_error(
                            "frontend quarantine failed after scan worker stop failure",
                            public_error,
                            quarantine_error,
                        );
                        guard.mark_service_critical();
                    }
                    return Err(public_error);
                }
            }
            Ok(outcomes)
        }
        Err(tickets) => {
            if let Some(error) = tickets
                .completed
                .iter()
                .find_map(|(_, outcome)| frontend_worker_stop_failure(outcome))
            {
                let mut public_error = error;
                if let Err(commit_error) = guard
                    .frontend_txn()
                    .commit_bound_demux_runtime_rollback_tokens(demux_rollback_tokens)
                {
                    public_error = compose_frontend_cleanup_error(
                        "frontend demux rollback-token commit failed with pending scan worker stop",
                        public_error,
                        commit_error,
                    );
                }
                guard.mark_service_critical();
                core::mem::forget(tickets);
                return Err(public_error);
            }
            Err(tickets)
        }
    };

    let boundary_result = if is_locked_continuation {
        Ok(())
    } else {
        guard
            .reset_bound_demuxes_for_frontend_tune_start(frontend_id)
            .map(|_| ())
    };
    let commit_tokens_result = guard
        .frontend_txn()
        .commit_bound_demux_runtime_rollback_tokens(demux_rollback_tokens);
    if let Err(error) = commit_tokens_result {
        guard.mark_service_critical();
        core::mem::forget(tickets);
        return Err(error);
    }
    if let Err(error) = boundary_result {
        let public_error = match crate::object_close_txn::quarantine_object_cascade(
            &mut guard,
            object_id,
            object_generation,
        ) {
            Ok(_) => error,
            Err(quarantine_error) => {
                guard.mark_service_critical();
                compose_frontend_cleanup_error(
                    "frontend quarantine failed after scan boundary failure",
                    error,
                    quarantine_error,
                )
            }
        };
        core::mem::forget(tickets);
        return Err(public_error);
    }
    let mut pending_stop_error = tickets.is_err().then(|| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend backend stop did not complete before scan returned",
        )
    });
    if let Some(error) = pending_stop_error.as_mut() {
        if let Err(mark_error) = guard
            .frontend_txn()
            .mark_frontend_worker_stop_pending_failure(frontend_id, generation, error.clone())
        {
            guard.mark_service_critical();
            *error = compose_frontend_cleanup_error(
                "frontend pending scan worker-stop failure state commit failed",
                error.clone(),
                mark_error,
            );
        }
    }
    let fenced_demux_generations = if tickets.is_err() {
        match current_bound_demux_generation_snapshot(&guard, frontend_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                guard.mark_service_critical();
                core::mem::forget(tickets);
                return Err(error);
            }
        }
    } else {
        Vec::new()
    };
    drop(guard);

    match tickets {
        Ok(outcomes) => {
            let transition = CommittedScanReplacement {
                object_id,
                object_generation,
                frontend_id,
                generation,
                entry,
                fingerprint,
                candidates,
                locked_continuation: is_locked_continuation,
                scan_notifier,
                cleanup_diagnostic_sink: cleanup_diagnostic_sink.clone(),
            };
            finish_committed_scan_replacement(&runtime, transition, outcomes, false)
        }
        Err(tickets) => {
            let pending_stop_error = match pending_stop_error {
                Some(error) => error,
                None => {
                    let error = HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "pending frontend scan tickets lost their public failure",
                    );
                    if let Ok(mut guard) = runtime.lock() {
                        guard.mark_service_critical();
                    }
                    core::mem::forget(tickets);
                    return Err(error);
                }
            };
            let target = FrontendWorkerCleanupTarget::object(
                frontend_id,
                object_id,
                object_generation,
            );
            let completion_public_error = pending_stop_error.clone();
            let deadline_diagnostic_sink = cleanup_diagnostic_sink.clone();
            let job = FrontendWorkerReaperJob {
                keys: vec![
                    (frontend_id, FrontendWorkerKind::Tune),
                    (frontend_id, FrontendWorkerKind::Scan),
                ],
                continuation_kind: None,
                tickets,
                transferred_at: Instant::now(),
                deadline_action: Some(Box::new(move |runtime| {
                    handle_frontend_worker_reaper_deadline(
                        runtime,
                        target,
                        FrontendWorkerKind::Scan,
                        generation,
                        fenced_demux_generations,
                        deadline_diagnostic_sink,
                    );
                })),
                completion_action: Box::new(move |runtime, outcomes, deadline_elapsed| {
                    let completion_error = if deadline_elapsed {
                        compose_frontend_cleanup_error(
                            "frontend scan replacement reaper deadline elapsed",
                            completion_public_error.clone(),
                            HalError::cleanup_failed(
                                "frontend scan replacement reaper",
                                "old worker did not exit before the reaper deadline",
                            ),
                        )
                    } else {
                        completion_public_error.clone()
                    };
                    if record_aborted_frontend_replacement_after_reap(
                        cleanup_diagnostic_sink,
                        target,
                        FrontendWorkerKind::Scan,
                        generation,
                        &outcomes,
                        completion_error,
                    )
                    .is_err()
                    {
                        if let Ok(mut guard) = runtime.lock() {
                            guard.mark_service_critical();
                        }
                    }
                }),
            };
            if let Err(error) = reaper.enqueue(job) {
                if let Ok(mut guard) = runtime.lock() {
                    guard.mark_service_critical();
                }
                return Err(error);
            }
            Err(pending_stop_error)
        }
    }
}

#[cfg(test)]
pub fn stop_frontend_worker(
    runtime: SharedRuntime,
    frontend_id: i32,
    kind: FrontendWorkerKind,
    reason: FrontendWorkerCancelReason,
) -> Result<FrontendWorkerStopOutcome, HalError> {
    let ticket = {
        let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
        guard
            .frontend_txn()
            .request_worker_stop_for_join(frontend_id, kind, reason)
    };
    Ok(ticket.complete())
}

fn record_scan_cancelled_from_stop_outcome(
    runtime: &SharedRuntime,
    frontend_id: i32,
    outcome: &FrontendWorkerStopOutcome,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    let mut guard = lock_runtime(runtime, "service runtime lock poisoned")?;
    record_scan_cancelled_from_stop_outcome_locked(&mut guard, frontend_id, outcome, reason)
}

fn record_frontend_stop_reaper_completion(
    runtime: &SharedRuntime,
    sink: SharedFrontendWorkerCleanupDiagnostics,
    target: FrontendWorkerCleanupTarget,
    kind: FrontendWorkerKind,
    outcomes: Vec<(FrontendWorkerKind, FrontendWorkerStopOutcome)>,
    deadline_elapsed: bool,
) {
    accept_frontend_worker_terminal_outcomes(runtime, &outcomes);
    let outcome = outcomes
        .iter()
        .find(|(outcome_kind, _)| *outcome_kind == kind)
        .map(|(_, outcome)| outcome);
    let mut result = match outcome {
        Some(outcome) => frontend_worker_stop_result_from_outcome(outcome),
        None => Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "frontend reaper completion omitted the requested worker kind",
            )),
    };
    if deadline_elapsed && result.is_ok() {
        result = Err(HalError::cleanup_failed(
            "frontend worker reaper deadline",
            "worker exit was observed only after the reaper deadline",
        ));
    }
    if deadline_elapsed || result.is_err() {
        if let (Some(object_id), Some(object_generation)) =
            (target.object_id(), target.object_generation())
        {
            let quarantine_result = match runtime.lock() {
                Ok(mut guard) => {
                    let owner_generation_is_present = guard
                        .object_table()
                        .entry(object_id)
                        .is_some_and(|entry| entry.generation == object_generation);
                    if owner_generation_is_present {
                        crate::object_close_txn::quarantine_object_cascade(
                            &mut guard,
                            object_id,
                            object_generation,
                        )
                        .map(|_| ())
                    } else {
                        Ok(())
                    }
                }
                Err(_) => Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "service runtime lock poisoned while quarantining a reaped frontend worker",
                )),
            };
            if let Err(quarantine_error) = quarantine_result {
                result = Err(match result {
                    Ok(()) => quarantine_error,
                    Err(primary) => compose_frontend_cleanup_error(
                        "frontend quarantine failed after worker reaper completion",
                        primary,
                        quarantine_error,
                    ),
                });
            }
        }
    }
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
        target,
        kind,
        outcome.and_then(frontend_worker_stop_outcome_generation),
        result.clone(),
    ));
    report.push(FrontendWorkerCleanupStepOutcome::complete_stop_object(
        target,
        kind,
        outcome.and_then(frontend_worker_stop_outcome_generation),
        result.clone(),
    ));
    record_frontend_cleanup_diagnostic_after_terminal(
        &sink,
        FrontendWorkerCleanupDiagnosticRecord::new(
            FrontendWorkerCleanupDiagnosticKind::WorkerReaperCompletion,
            target,
            report,
            result.err(),
        ),
    );
}

fn stop_frontend_object_without_join(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    kind: FrontendWorkerKind,
    reason: FrontendWorkerCancelReason,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let reaper = ensure_frontend_worker_reaper(&runtime)?;
    let mut guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
    dispatch.consume_for_object(
        &mut guard,
        object_id,
        object_generation,
        AidlObjectKind::Frontend,
    )?;
    let (frontend_id, _) =
        resolve_frontend_object_for_method(&guard, object_id, object_generation)?;
    let replacement_pending_for_kind = match reaper.pending_state(frontend_id, kind)? {
        FrontendWorkerReaperPendingState::NotPending => false,
        FrontendWorkerReaperPendingState::CleanupOnly => return Ok(()),
        FrontendWorkerReaperPendingState::Replacement(replacement_kind)
            if replacement_kind != kind =>
        {
            return Ok(())
        }
        FrontendWorkerReaperPendingState::Replacement(_) => true,
    };
    let snapshot = guard.query().frontend_runtime_snapshot(frontend_id)?;
    let generation = guard
        .frontend_txn()
        .prepare_frontend_worker_replacement_generation(frontend_id, kind)?;
    if kind == FrontendWorkerKind::Scan
        && snapshot
            .scan_session
            .as_ref()
            .is_some_and(|session| session.phase() == FrontendScanPhase::Running)
    {
        guard
            .frontend_txn()
            .cancel_frontend_scan_session(frontend_id, snapshot.generation, reason)?;
    }
    guard
        .frontend_txn()
        .fence_frontend_worker_replacement_generation(frontend_id, generation)?;
    if replacement_pending_for_kind {
        let boundary_result = match kind {
            FrontendWorkerKind::Tune => guard
                .frontend_txn()
                .stop_frontend_live_data_and_unbind(frontend_id)
                .map(|_| ()),
            FrontendWorkerKind::Scan => guard
                .frontend_txn()
                .clear_frontend_live_reader_descriptor_and_idle(frontend_id),
        };
        let target =
            FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
        let mut report = FrontendWorkerCleanupExecutionReport::new();
        report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            kind,
            None,
            Ok(()),
        ));
        match kind {
            FrontendWorkerKind::Tune => report.push(
                FrontendWorkerCleanupStepOutcome::stop_live_data_and_unbind(
                    target,
                    boundary_result.clone(),
                ),
            ),
            FrontendWorkerKind::Scan => report.push(
                FrontendWorkerCleanupStepOutcome::clear_live_reader_descriptor(
                    target,
                    boundary_result.clone(),
                ),
            ),
        }
        guard.frontend_worker_cleanup_diagnostic_sink().record(
            FrontendWorkerCleanupDiagnosticRecord::new(
                match kind {
                    FrontendWorkerKind::Tune => {
                        FrontendWorkerCleanupDiagnosticKind::StopTuneObject
                    }
                    FrontendWorkerKind::Scan => {
                        FrontendWorkerCleanupDiagnosticKind::StopScanObject
                    }
                },
                target,
                report,
                boundary_result.clone().err(),
            ),
        )?;
        return boundary_result;
    }
    let ticket = guard
        .frontend_txn()
        .request_worker_stop_for_join(frontend_id, kind, reason);
    let worker_generation = ticket.worker_generation();
    let tickets = FrontendWorkerReaperTicketGroup::new(vec![(kind, ticket)]);
    let tickets = match tickets.try_complete() {
        Ok(outcomes) => {
            if let Some(error) = outcomes
                .iter()
                .find_map(|(_, outcome)| frontend_worker_stop_failure(outcome))
            {
                let public_error = match crate::object_close_txn::quarantine_object_cascade(
                    &mut guard,
                    object_id,
                    object_generation,
                ) {
                    Ok(_) => error,
                    Err(quarantine_error) => {
                        guard.mark_service_critical();
                        compose_frontend_cleanup_error(
                            "frontend quarantine failed after stop worker failure",
                            error,
                            quarantine_error,
                        )
                    }
                };
                return Err(public_error);
            }
            Ok(outcomes)
        }
        Err(tickets) => Err(tickets),
    };
    let mut boundary_result = match kind {
        FrontendWorkerKind::Tune => guard
            .frontend_txn()
            .stop_frontend_live_data_and_unbind(frontend_id)
            .map(|_| ()),
        FrontendWorkerKind::Scan => guard
            .frontend_txn()
            .clear_frontend_live_reader_descriptor_and_idle(frontend_id),
    };
    let target =
        FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
        target,
        kind,
        worker_generation,
        Ok(()),
    ));
    match kind {
        FrontendWorkerKind::Tune => report.push(
            FrontendWorkerCleanupStepOutcome::stop_live_data_and_unbind(
                target,
                boundary_result.clone(),
            ),
        ),
        FrontendWorkerKind::Scan => report.push(
            FrontendWorkerCleanupStepOutcome::clear_live_reader_descriptor(
                target,
                boundary_result.clone(),
            ),
        ),
    }
    let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
    cleanup_diagnostic_sink.record(FrontendWorkerCleanupDiagnosticRecord::new(
        match kind {
            FrontendWorkerKind::Tune => FrontendWorkerCleanupDiagnosticKind::StopTuneObject,
            FrontendWorkerKind::Scan => FrontendWorkerCleanupDiagnosticKind::StopScanObject,
        },
        target,
        report,
        boundary_result.clone().err(),
    ))?;
    if let Err(primary) = boundary_result.clone() {
        if let Err(quarantine_error) = crate::object_close_txn::quarantine_object_cascade(
            &mut guard,
            object_id,
            object_generation,
        ) {
            guard.mark_service_critical();
            boundary_result = Err(compose_frontend_cleanup_error(
                "frontend quarantine failed after stop boundary failure",
                primary,
                quarantine_error,
            ));
        }
    }
    let fenced_demux_generations = if tickets.is_err() {
        match current_bound_demux_generation_snapshot(&guard, frontend_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                guard.mark_service_critical();
                core::mem::forget(tickets);
                return Err(error);
            }
        }
    } else {
        Vec::new()
    };
    drop(guard);

    match tickets {
        Ok(outcomes) => record_frontend_stop_reaper_completion(
            &runtime,
            cleanup_diagnostic_sink,
            target,
            kind,
            outcomes,
            false,
        ),
        Err(tickets) => {
            let completion_sink = cleanup_diagnostic_sink.clone();
            let deadline_diagnostic_sink = cleanup_diagnostic_sink.clone();
            let job = FrontendWorkerReaperJob {
                keys: vec![(frontend_id, kind)],
                continuation_kind: None,
                tickets,
                transferred_at: Instant::now(),
                deadline_action: Some(Box::new(move |runtime| {
                    handle_frontend_worker_reaper_deadline(
                        runtime,
                        target,
                        kind,
                        generation,
                        fenced_demux_generations,
                        deadline_diagnostic_sink,
                    );
                })),
                completion_action: Box::new(move |runtime, outcomes, deadline_elapsed| {
                    record_frontend_stop_reaper_completion(
                        runtime,
                        completion_sink,
                        target,
                        kind,
                        outcomes,
                        deadline_elapsed,
                    );
                }),
            };
            if let Err(error) = reaper.enqueue(job) {
                if let Ok(mut guard) = runtime.lock() {
                    guard.mark_service_critical();
                }
                return Err(error);
            }
        }
    }
    boundary_result
}

pub(crate) fn stop_frontend_tune_object(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    reason: FrontendWorkerCancelReason,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    stop_frontend_object_without_join(
        runtime,
        object_id,
        object_generation,
        FrontendWorkerKind::Tune,
        reason,
        dispatch,
    )
}

pub(crate) fn stop_frontend_scan_object(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    reason: FrontendWorkerCancelReason,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    stop_frontend_object_without_join(
        runtime,
        object_id,
        object_generation,
        FrontendWorkerKind::Scan,
        reason,
        dispatch,
    )
}


pub fn close_frontend_live_data_and_unbind(
    runtime: SharedRuntime,
    frontend_id: i32,
) -> Result<(), HalError> {
    lock_runtime(&runtime, "service runtime lock poisoned")?
        .frontend_txn()
        .close_frontend_live_data_and_unbind(frontend_id)
        .map(|_| ())
}

pub(crate) fn cleanup_frontend_object_after_close_begin(
    runtime: SharedRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    reason: FrontendWorkerCancelReason,
) -> Result<FrontendCloseCleanupReport, HalError> {
    let (frontend_id, cleanup_diagnostic_sink) = {
        let guard = lock_runtime(&runtime, "service runtime lock poisoned")?;
        let (frontend_id, _) =
            resolve_frontend_object_for_close_cleanup(&guard, object_id, object_generation)?;
        let cleanup_diagnostic_sink = guard.frontend_worker_cleanup_diagnostic_sink();
        (frontend_id, cleanup_diagnostic_sink)
    };
    let target = FrontendWorkerCleanupTarget::object(frontend_id, object_id, object_generation);
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    let worker_cleanup_result = close_frontend_workers_and_live_data_with_sink(
        Arc::clone(&runtime),
        frontend_id,
        reason,
        Ok(cleanup_diagnostic_sink.clone()),
        target,
    );
    report.push(
        FrontendWorkerCleanupStepOutcome::close_frontend_workers_and_live_data(
            target,
            worker_cleanup_result,
        ),
    );
    let lnb_outcomes = crate::lnb_ops::close_lnbs_from_frontend_owner_loss_report(
        Arc::clone(&runtime),
        frontend_id,
    );
    let mut closed_lnb_ids = Vec::with_capacity(lnb_outcomes.len());
    for (lnb_id, result) in lnb_outcomes {
        if result.is_ok() {
            closed_lnb_ids.push(lnb_id);
        }
        report.push(FrontendWorkerCleanupStepOutcome::close_owned_lnb(
            target, lnb_id, result,
        ));
    }
    let cleanup_result = report.clone().into_result();
    let public_error = cleanup_result.clone().err();
    let record = FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::FrontendCloseOwnerLoss,
        target,
        report,
        public_error,
    );
    let record_result = cleanup_diagnostic_sink.record(record);
    let cleanup_result =
        compose_frontend_worker_cleanup_finish_result(cleanup_result, record_result);
    Ok(FrontendCloseCleanupReport {
        frontend_id,
        closed_lnb_ids,
        cleanup_result,
    })
}

#[cfg(test)]
pub fn close_frontend_workers_and_live_data(
    runtime: SharedRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
) -> Result<(), HalError> {
    let cleanup_diagnostic_sink = lock_runtime(
        &runtime,
        "service runtime lock poisoned while preparing frontend worker cleanup diagnostic",
    )
    .map(|guard| guard.frontend_worker_cleanup_diagnostic_sink());
    close_frontend_workers_and_live_data_with_sink(
        runtime,
        frontend_id,
        reason,
        cleanup_diagnostic_sink,
        FrontendWorkerCleanupTarget::frontend(frontend_id),
    )
}

fn close_frontend_workers_and_live_data_with_sink(
    runtime: SharedRuntime,
    frontend_id: i32,
    reason: FrontendWorkerCancelReason,
    cleanup_diagnostic_sink: Result<SharedFrontendWorkerCleanupDiagnostics, HalError>,
    target: FrontendWorkerCleanupTarget,
) -> Result<(), HalError> {
    let reaper = ensure_frontend_worker_reaper(&runtime)?;
    if reaper.is_pending(frontend_id, FrontendWorkerKind::Tune)?
        || reaper.is_pending(frontend_id, FrontendWorkerKind::Scan)?
    {
        return Err(HalError::cleanup_failed(
            "frontend worker cleanup pending",
            "frontend worker exit is still owned by the reaper",
        ));
    }
    let sink = cleanup_diagnostic_sink?;
    let (generation, tickets, fenced_demux_generations, close_result) = {
        let mut guard = lock_runtime(
            &runtime,
            "service runtime lock poisoned while preparing frontend close reaping",
        )?;
        let generation = guard
            .frontend_txn()
            .prepare_frontend_worker_replacement_generation(
                frontend_id,
                FrontendWorkerKind::Tune,
            )?;
        guard
            .frontend_txn()
            .fence_frontend_worker_replacement_generation(frontend_id, generation)?;
        let tune_ticket = guard.frontend_txn().request_worker_stop_for_join(
            frontend_id,
            FrontendWorkerKind::Tune,
            reason,
        );
        let scan_ticket = guard.frontend_txn().request_worker_stop_for_join(
            frontend_id,
            FrontendWorkerKind::Scan,
            reason,
        );
        let tickets = FrontendWorkerReaperTicketGroup::new(vec![
            (FrontendWorkerKind::Tune, tune_ticket),
            (FrontendWorkerKind::Scan, scan_ticket),
        ]);
        let close_result = guard
            .frontend_txn()
            .close_frontend_live_data_and_unbind(frontend_id)
            .map(|_| ());
        let fenced_demux_generations = match current_bound_demux_generation_snapshot(
            &guard,
            frontend_id,
        ) {
            Ok(snapshot) => snapshot,
            Err(snapshot_error) => {
                guard.mark_service_critical();
                core::mem::forget(tickets);
                return Err(match close_result {
                    Ok(()) => snapshot_error,
                    Err(primary) => compose_frontend_cleanup_error(
                        "frontend demux snapshot failed after close boundary failure",
                        primary,
                        snapshot_error,
                    ),
                });
            }
        };
        (generation, tickets, fenced_demux_generations, close_result)
    };

    let tickets = match tickets.try_complete() {
        Ok(outcomes) => Ok(outcomes),
        Err(tickets) => Err(tickets),
    };
    let mut report = FrontendWorkerCleanupExecutionReport::new();
    if let Ok(outcomes) = &tickets {
        for (kind, outcome) in outcomes {
            report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
                target,
                *kind,
                frontend_worker_stop_outcome_generation(outcome),
                frontend_worker_stop_result_from_outcome(outcome),
            ));
        }
        if let Some((_, scan_outcome)) = outcomes
            .iter()
            .find(|(kind, _)| *kind == FrontendWorkerKind::Scan)
        {
            let scan_cancel_result = record_scan_cancelled_from_stop_outcome(
                &runtime,
                frontend_id,
                scan_outcome,
                reason,
            );
            report.push(FrontendWorkerCleanupStepOutcome::record_scan_cancelled(
                target,
                frontend_worker_stop_outcome_generation(scan_outcome),
                scan_cancel_result,
            ));
        }
    } else {
        report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            FrontendWorkerKind::Tune,
            None,
            Ok(()),
        ));
        report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
            target,
            FrontendWorkerKind::Scan,
            None,
            Ok(()),
        ));
    }
    report.push(FrontendWorkerCleanupStepOutcome::close_live_data_and_unbind(
        target,
        close_result.clone(),
    ));
    sink.record(FrontendWorkerCleanupDiagnosticRecord::new(
        FrontendWorkerCleanupDiagnosticKind::FrontendClose,
        target,
        report,
        close_result.clone().err(),
    ))?;
    close_result?;

    match tickets {
        Ok(outcomes) => {
            accept_frontend_worker_terminal_outcomes(&runtime, &outcomes);
            let mut terminal_result = Ok(());
            for (_, outcome) in outcomes {
                if let Some(error) = frontend_worker_stop_failure(&outcome) {
                    terminal_result = Err(error);
                    break;
                }
            }
            let fixed_power_result = crate::lnb_ops::release_frontend_fixed_power_after_operation(
                &runtime,
                crate::registry::FrontendRuntimeId(frontend_id),
            );
            match (terminal_result, fixed_power_result) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
                (Err(primary), Err(cleanup)) => Err(compose_frontend_cleanup_error(
                    "frontend worker termination and fixed LNB power cleanup both failed",
                    primary,
                    cleanup,
                )),
            }
        }
        Err(tickets) => {
            let completion_sink = sink.clone();
            let deadline_diagnostic_sink = sink.clone();
            let job = FrontendWorkerReaperJob {
                keys: vec![
                    (frontend_id, FrontendWorkerKind::Tune),
                    (frontend_id, FrontendWorkerKind::Scan),
                ],
                continuation_kind: None,
                tickets,
                transferred_at: Instant::now(),
                deadline_action: Some(Box::new(move |runtime| {
                    handle_frontend_worker_reaper_deadline(
                        runtime,
                        target,
                        FrontendWorkerKind::Tune,
                        generation,
                        fenced_demux_generations,
                        deadline_diagnostic_sink,
                    );
                })),
                completion_action: Box::new(move |runtime, outcomes, _deadline_elapsed| {
                    accept_frontend_worker_terminal_outcomes(runtime, &outcomes);
                    let fixed_power_result =
                        crate::lnb_ops::release_frontend_fixed_power_after_operation(
                            runtime,
                            crate::registry::FrontendRuntimeId(frontend_id),
                        );
                    let mut report = FrontendWorkerCleanupExecutionReport::new();
                    for (kind, outcome) in outcomes {
                        report.push(FrontendWorkerCleanupStepOutcome::stop_worker(
                            target,
                            kind,
                            frontend_worker_stop_outcome_generation(&outcome),
                            frontend_worker_stop_result_from_outcome(&outcome),
                        ));
                    }
                    if let Err(error) = fixed_power_result {
                        report.push(
                            FrontendWorkerCleanupStepOutcome::close_frontend_workers_and_live_data(
                                target,
                                Err(error),
                            ),
                        );
                    }
                    let public_error = report.first_error();
                    record_frontend_cleanup_diagnostic_after_terminal(
                        &completion_sink,
                        FrontendWorkerCleanupDiagnosticRecord::new(
                            FrontendWorkerCleanupDiagnosticKind::WorkerReaperCompletion,
                            target,
                            report,
                            public_error,
                        ),
                    );
                }),
            };
            if let Err(error) = reaper.enqueue(job) {
                if let Ok(mut guard) = runtime.lock() {
                    guard.mark_service_critical();
                }
                return Err(error);
            }
            Err(HalError::cleanup_failed(
                "frontend worker cleanup pending",
                "frontend worker ownership transferred to the reaper",
            ))
        }
    }
}
