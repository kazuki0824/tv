use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex, Weak,
};
use std::thread;
use std::time::{Duration, Instant};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, PlaybackStatus::PlaybackStatus, RecordStatus::RecordStatus,
};
use binder::Strong;
use maleicacid_tuner_hal2_binder_adapter::{AidlObjectGeneration, AidlObjectId};
use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};
use maleicacid_tuner_hal2_demux::DvrStatusEvent;
use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport, CapabilitySnapshot,
    ClassifiedWorkerTerminalResult, DvrPostCommitNotificationDiagnosticRecord,
    DvrPostCommitNotificationFailureKind, DvrPostCommitNotificationPhase,
    DvrStatusNotifierCleanupDiagnosticRecord, DvrStatusPollSnapshot, WorkerRuntime,
    WorkerRuntimeSupervisor,
};

use crate::object_handle::AidlObjectHandle;
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct DvrStatusNotifierKey {
    object_id: i64,
    generation: u64,
}

impl DvrStatusNotifierKey {
    fn new(handle: AidlObjectHandle) -> Self {
        Self {
            object_id: handle.object_id().0,
            generation: handle.generation().0,
        }
    }
}

pub(crate) struct DvrStatusNotifier {
    worker: WorkerRuntime<()>,
}

fn signal_dvr_status_notifier_stop(notifier: &DvrStatusNotifier) {
    notifier.worker.request_stop_and_wake();
}

fn join_finished_dvr_status_notifier(notifier: DvrStatusNotifier) -> Result<(), HalError> {
    match notifier.worker.join_classified() {
        ClassifiedWorkerTerminalResult::Normal(())
        | ClassifiedWorkerTerminalResult::StopRequested => Ok(()),
        ClassifiedWorkerTerminalResult::Failure { error, .. } => Err(error),
    }
}

struct DvrStatusNotifierReaperJob {
    key: DvrStatusNotifierKey,
    handle: AidlObjectHandle,
    notifier: DvrStatusNotifier,
    transferred_at: Instant,
    deadline_reported: bool,
    restart_requested: bool,
    transfer_reason: DvrStatusNotifierTransferReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DvrStatusNotifierTransferReason {
    Stop,
    Reset,
    WorkerTerminal,
}

enum DvrStatusNotifierSupervisorAction {
    Completed(DvrStatusNotifierReaperJob),
    Deadline(AidlObjectHandle),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DvrStatusNotifierStopDisposition {
    Complete,
    ReaperPending,
}

pub(crate) struct DvrStatusNotifierSupervisor {
    runtime: WorkerRuntimeSupervisor<
        DvrStatusNotifierKey,
        DvrStatusNotifier,
        DvrStatusNotifierReaperJob,
    >,
}

impl DvrStatusNotifierSupervisor {
    pub(crate) fn from_snapshot(snapshot: CapabilitySnapshot) -> Self {
        Self {
            runtime: WorkerRuntimeSupervisor::new(
                snapshot.cleanup_reaper_capacity,
                Duration::from_millis(snapshot.worker_reaper_deadline_ms),
            ),
        }
    }

    fn start_or_request_restart(
        self: &Arc<Self>,
        context: &SharedAidlServiceContext,
        handle: AidlObjectHandle,
    ) -> Result<(), HalError> {
        let key = DvrStatusNotifierKey::new(handle);
        let mut state = self.runtime.state().lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier supervisor lock poisoned while starting worker",
            )
        })?;
        if state
            .active
            .get(&key)
            .is_some_and(|notifier| notifier.worker.is_finished())
        {
            let notifier = state.active_mut().remove(&key).ok_or_else(|| {
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "finished DVR status notifier disappeared before reaper transfer",
                )
            })?;
            state.reaping_mut().insert(
                key,
                DvrStatusNotifierReaperJob {
                    key,
                    handle,
                    notifier,
                    transferred_at: Instant::now(),
                    deadline_reported: false,
                    restart_requested: true,
                    transfer_reason: DvrStatusNotifierTransferReason::WorkerTerminal,
                },
            );
            self.runtime.wake().notify_one();
            return Ok(());
        }
        if state.active_mut().contains_key(&key) {
            return Ok(());
        }
        if let Some(job) = state.reaping_mut().get_mut(&key) {
            job.restart_requested = true;
            self.runtime.wake().notify_one();
            return Ok(());
        }
        if state.total_len() >= self.runtime.capacity() {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier supervisor capacity exhausted",
            ));
        }
        let notifier = spawn_dvr_status_notifier(context, handle, Arc::downgrade(self))?;
        state.active_mut().insert(key, notifier);
        self.runtime.wake().notify_one();
        Ok(())
    }

    fn signal_stop(
        &self,
        handle: AidlObjectHandle,
    ) -> Result<DvrStatusNotifierStopDisposition, HalError> {
        let key = DvrStatusNotifierKey::new(handle);
        let mut state = self.runtime.state().lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier supervisor lock poisoned while stopping worker",
            )
        })?;
        if let Some(job) = state.reaping_mut().get_mut(&key) {
            job.restart_requested = false;
            self.runtime.wake().notify_one();
            return Ok(DvrStatusNotifierStopDisposition::ReaperPending);
        }
        let Some(notifier) = state.active_mut().remove(&key) else {
            return Ok(DvrStatusNotifierStopDisposition::Complete);
        };
        signal_dvr_status_notifier_stop(&notifier);
        state.reaping_mut().insert(
            key,
            DvrStatusNotifierReaperJob {
                key,
                handle,
                notifier,
                transferred_at: Instant::now(),
                deadline_reported: false,
                restart_requested: false,
                transfer_reason: DvrStatusNotifierTransferReason::Stop,
            },
        );
        self.runtime.wake().notify_one();
        Ok(DvrStatusNotifierStopDisposition::ReaperPending)
    }

    fn signal_all_for_reset(&self) -> Result<(), HalError> {
        let mut state = self.runtime.state().lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier supervisor lock poisoned while resetting workers",
            )
        })?;
        for job in state.reaping_mut().values_mut() {
            job.restart_requested = false;
        }
        let active = core::mem::take(&mut state.active_mut());
        for (key, notifier) in active {
            signal_dvr_status_notifier_stop(&notifier);
            state.reaping_mut().insert(
                key,
                DvrStatusNotifierReaperJob {
                    key,
                    handle: AidlObjectHandle::new(
                        maleicacid_tuner_hal2_binder_adapter::AidlObjectKind::Dvr,
                        AidlObjectId(key.object_id),
                        AidlObjectGeneration(key.generation),
                    ),
                    notifier,
                    transferred_at: Instant::now(),
                    deadline_reported: false,
                    restart_requested: false,
                    transfer_reason: DvrStatusNotifierTransferReason::Reset,
                },
            );
        }
        self.runtime.wake().notify_all();
        Ok(())
    }

    fn take_next_action(&self) -> Result<DvrStatusNotifierSupervisorAction, HalError> {
        let mut state = self.runtime.state().lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier supervisor lock poisoned in reaper",
            )
        })?;
        loop {
            if let Some(key) = state
                .active_mut()
                .iter()
                .find_map(|(key, notifier)| notifier.worker.is_finished().then_some(*key))
            {
                let Some(notifier) = state.active_mut().remove(&key) else {
                    continue;
                };
                state.reaping_mut().insert(
                    key,
                    DvrStatusNotifierReaperJob {
                        key,
                        handle: AidlObjectHandle::new(
                            maleicacid_tuner_hal2_binder_adapter::AidlObjectKind::Dvr,
                            AidlObjectId(key.object_id),
                            AidlObjectGeneration(key.generation),
                        ),
                        notifier,
                        transferred_at: Instant::now(),
                        deadline_reported: false,
                        restart_requested: false,
                        transfer_reason: DvrStatusNotifierTransferReason::WorkerTerminal,
                    },
                );
                continue;
            }
            if let Some(key) = state
                .reaping_mut()
                .iter()
                .find_map(|(key, job)| job.notifier.worker.is_finished().then_some(*key))
            {
                let Some(job) = state.reaping_mut().remove(&key) else {
                    continue;
                };
                return Ok(DvrStatusNotifierSupervisorAction::Completed(job));
            }
            if let Some(handle) = state.reaping_mut().values_mut().find_map(|job| {
                if !job.deadline_reported && job.transferred_at.elapsed() >= self.runtime.deadline()
                {
                    job.deadline_reported = true;
                    Some(job.handle)
                } else {
                    None
                }
            }) {
                return Ok(DvrStatusNotifierSupervisorAction::Deadline(handle));
            }
            let next_wait = state
                .reaping
                .values()
                .filter(|job| !job.deadline_reported)
                .map(|job| {
                    self.runtime
                        .deadline()
                        .saturating_sub(job.transferred_at.elapsed())
                })
                .min();
            state = match next_wait {
                Some(wait) => {
                    self.runtime
                        .wake()
                        .wait_timeout(state, wait)
                        .map_err(|_| {
                            HalError::internal(
                                HalInternalKind::InvariantViolation,
                                "DVR status notifier supervisor wait lock poisoned in reaper",
                            )
                        })?
                        .0
                }
                None => self.runtime.wake().wait(state).map_err(|_| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "DVR status notifier supervisor wait lock poisoned in reaper",
                    )
                })?,
            };
        }
    }
}

fn dvr_status_event_to_hal_callback(
    callback: &Strong<dyn IDvrCallback>,
    event: DvrStatusEvent,
) -> binder::Result<()> {
    match event {
        DvrStatusEvent::RecordDataReady => callback.onRecordStatus(RecordStatus::DATA_READY),
        DvrStatusEvent::RecordLowWater => callback.onRecordStatus(RecordStatus::LOW_WATER),
        DvrStatusEvent::RecordHighWater => callback.onRecordStatus(RecordStatus::HIGH_WATER),
        DvrStatusEvent::RecordOverflow => callback.onRecordStatus(RecordStatus::OVERFLOW),
        DvrStatusEvent::PlaybackSpaceEmpty => {
            callback.onPlaybackStatus(PlaybackStatus::SPACE_EMPTY)
        }
        DvrStatusEvent::PlaybackSpaceAlmostEmpty => {
            callback.onPlaybackStatus(PlaybackStatus::SPACE_ALMOST_EMPTY)
        }
        DvrStatusEvent::PlaybackSpaceAlmostFull => {
            callback.onPlaybackStatus(PlaybackStatus::SPACE_ALMOST_FULL)
        }
        DvrStatusEvent::PlaybackSpaceFull => callback.onPlaybackStatus(PlaybackStatus::SPACE_FULL),
    }
}

#[derive(Clone, Debug)]
enum DvrCallbackArtifactLookup {
    Present,
    Missing,
    StoreFailure(HalError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DvrStatusCallbackDeliveryOutcome {
    Delivered,
    ArtifactMissing,
    StoreFailure,
    BinderFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DvrCallbackNotifierAvailability {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DvrStatusNotificationPreflight {
    Ready,
    NotStarted,
    CallbackMissing,
    CallbackUnhealthy,
    StatusReportingDisabled,
}

impl DvrStatusNotificationPreflight {
    fn should_skip_delivery(self) -> bool {
        !matches!(self, Self::Ready)
    }
}

fn dvr_callback_artifact_lookup(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    delivery_context: &'static str,
) -> DvrCallbackArtifactLookup {
    match context.dvr_callback_for_owner(handle) {
        Ok(Some(_)) => DvrCallbackArtifactLookup::Present,
        Ok(None) => DvrCallbackArtifactLookup::Missing,
        Err(_) => DvrCallbackArtifactLookup::StoreFailure(HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("{delivery_context}: callback store lock poisoned"),
        )),
    }
}

fn poll_dvr_status_snapshot(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<DvrStatusPollSnapshot, HalError> {
    let guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while querying DVR status",
        )
    })?;
    guard.dvr_status_poll_snapshot_for_aidl_object(handle.object_id(), handle.generation())
}

fn dvr_status_metadata_snapshot(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<DvrStatusPollSnapshot, HalError> {
    let guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while querying DVR status metadata",
        )
    })?;
    guard.dvr_status_metadata_snapshot_for_aidl_object(handle.object_id(), handle.generation())
}

pub fn is_playback_dvr(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<bool, HalError> {
    dvr_status_metadata_snapshot(&context.runtime(), handle).map(|snapshot| snapshot.is_playback)
}

fn consume_playback_dvr_once(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while consuming playback DVR data",
        )
    })?;
    guard
        .consume_playback_dvr_for_object(handle.object_id(), handle.generation())
        .map(|_| ())
}

fn record_dvr_artifact_lookup_failure(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    dvr_phase: DvrPostCommitNotificationPhase,
    primary: HalError,
) {
    record_dvr_callback_delivery_failure(
        context,
        handle,
        CallbackDeliveryFailurePhase::CallbackArtifactLookup,
        dvr_phase,
        primary,
    );
}

fn dvr_status_notification_preflight(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    snapshot: &DvrStatusPollSnapshot,
    dvr_phase: DvrPostCommitNotificationPhase,
    delivery_context: &'static str,
) -> Result<DvrStatusNotificationPreflight, HalError> {
    if !snapshot.started {
        return Ok(DvrStatusNotificationPreflight::NotStarted);
    }
    if !snapshot.callback_present {
        record_dvr_artifact_lookup_failure(
            context,
            handle,
            dvr_phase,
            HalError::callback_failed(delivery_context, "DVR callback is not registered"),
        );
        return Ok(DvrStatusNotificationPreflight::CallbackMissing);
    }
    if snapshot.callback_unhealthy {
        record_dvr_callback_delivery_failure(
            context,
            handle,
            CallbackDeliveryFailurePhase::RuntimePolicySkip,
            dvr_phase,
            HalError::callback_failed(delivery_context, "DVR callback is unhealthy"),
        );
        return Ok(DvrStatusNotificationPreflight::CallbackUnhealthy);
    }
    if !snapshot.status_reporting_enabled {
        record_dvr_callback_delivery_failure(
            context,
            handle,
            CallbackDeliveryFailurePhase::RuntimePolicySkip,
            dvr_phase,
            HalError::callback_failed(delivery_context, "DVR status reporting is disabled"),
        );
        return Ok(DvrStatusNotificationPreflight::StatusReportingDisabled);
    }
    Ok(DvrStatusNotificationPreflight::Ready)
}

fn dvr_callback_notifier_availability(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<DvrCallbackNotifierAvailability, HalError> {
    match dvr_callback_artifact_lookup(context, handle, "IDvrCallback.notifier_preflight") {
        DvrCallbackArtifactLookup::Present => Ok(DvrCallbackNotifierAvailability::Available),
        DvrCallbackArtifactLookup::Missing => {
            record_dvr_callback_delivery_failure(
                context,
                handle,
                CallbackDeliveryFailurePhase::NotifierPreflight,
                DvrPostCommitNotificationPhase::StatusNotifierStart,
                HalError::callback_failed(
                    "IDvrCallback.notifier_preflight",
                    "DVR callback artifact missing before notifier start",
                ),
            );
            Ok(DvrCallbackNotifierAvailability::Unavailable)
        }
        DvrCallbackArtifactLookup::StoreFailure(error) => {
            record_dvr_callback_delivery_failure(
                context,
                handle,
                CallbackDeliveryFailurePhase::NotifierPreflight,
                DvrPostCommitNotificationPhase::StatusNotifierStart,
                error,
            );
            Ok(DvrCallbackNotifierAvailability::Unavailable)
        }
    }
}

fn record_dvr_callback_delivery_failure(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    phase: CallbackDeliveryFailurePhase,
    dvr_phase: DvrPostCommitNotificationPhase,
    primary: HalError,
) {
    let finish_result = (|| -> Result<(), HalError> {
        let runtime = context.runtime();
        let mut guard = runtime.lock().map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "service runtime lock poisoned while finishing DVR callback delivery failure",
            )
        })?;
        if phase == CallbackDeliveryFailurePhase::PostCommitNotification {
            guard.finish_dvr_post_commit_notification_failure_use_case(
                handle.object_id(),
                handle.generation(),
                dvr_phase,
                primary.clone(),
            )
        } else {
            guard.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::dvr(
                handle.object_id(),
                handle.generation(),
                phase,
                dvr_phase,
                primary.clone(),
            ))
        }
    })();
    if let Err(accounting_error) = finish_result {
        record_post_commit_accounting_failure_fallback(
            context,
            handle,
            dvr_phase,
            DvrPostCommitNotificationFailureKind::CallbackRegistryAccounting,
            primary,
            "DVR post-commit callback delivery accounting failed",
            accounting_error,
        );
    }
}

fn record_post_commit_accounting_failure_fallback(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    phase: DvrPostCommitNotificationPhase,
    failure_kind: DvrPostCommitNotificationFailureKind,
    primary: HalError,
    accounting_context: &'static str,
    accounting_error: HalError,
) {
    let fallback_error =
        compose_primary_cleanup_failure(accounting_context, primary, accounting_error);
    let fallback_record_result = context.record_dvr_post_commit_notification_diagnostic_fallback(
        DvrPostCommitNotificationDiagnosticRecord::new(
            phase,
            failure_kind,
            handle.object_id(),
            handle.generation(),
            fallback_error,
        ),
    );
    if fallback_record_result.is_err() {
        // The fallback helper increments the context-owned failure counter; post-commit public
        // methods must not be reversed by this diagnostic-store failure.
    }
}

fn record_dvr_status_notifier_lifecycle_outcome(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    record: DvrStatusNotifierCleanupDiagnosticRecord,
) {
    let phase = record.phase;
    if let Err(error) = context.record_dvr_status_notifier_cleanup_diagnostic(record) {
        record_post_commit_accounting_failure_fallback(
            context,
            handle,
            phase,
            DvrPostCommitNotificationFailureKind::NotifierCleanup,
            error,
            "DVR status notifier lifecycle diagnostic failed",
            HalError::cleanup_failed(
                "DVR status notifier lifecycle diagnostic",
                "recording failed",
            ),
        );
    }
}

fn record_superseded_dvr_notifier_cleanup_failure(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    primary: HalError,
) {
    record_dvr_callback_delivery_failure(
        context,
        handle,
        CallbackDeliveryFailurePhase::NotifierCleanup,
        DvrPostCommitNotificationPhase::StatusNotifierStop,
        primary,
    );
}

pub fn record_dvr_notifier_cleanup_outcome(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    phase: DvrPostCommitNotificationPhase,
    outcome: Result<(), HalError>,
) {
    let Err(primary) = outcome else {
        return;
    };
    record_dvr_callback_delivery_failure(
        context,
        handle,
        CallbackDeliveryFailurePhase::NotifierCleanup,
        phase,
        primary,
    );
}

fn deliver_dvr_status_event(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    event: DvrStatusEvent,
    delivery_context: &'static str,
    dvr_phase: DvrPostCommitNotificationPhase,
) -> Result<DvrStatusCallbackDeliveryOutcome, HalError> {
    let callback = match context.dvr_callback_for_owner(handle) {
        Ok(Some(callback)) => callback,
        Ok(None) => {
            record_dvr_artifact_lookup_failure(
                context,
                handle,
                dvr_phase,
                HalError::callback_failed(delivery_context, "DVR callback artifact missing"),
            );
            return Ok(DvrStatusCallbackDeliveryOutcome::ArtifactMissing);
        }
        Err(_) => {
            let primary = HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{delivery_context}: callback store lock poisoned"),
            );
            record_dvr_artifact_lookup_failure(context, handle, dvr_phase, primary);
            return Ok(DvrStatusCallbackDeliveryOutcome::StoreFailure);
        }
    };
    if let Err(error) = dvr_status_event_to_hal_callback(&callback, event) {
        let primary =
            HalError::callback_failed(delivery_context, format!("binder failure: {error:?}"));
        record_dvr_callback_delivery_failure(
            context,
            handle,
            CallbackDeliveryFailurePhase::BinderDelivery,
            dvr_phase,
            primary,
        );
        return Ok(DvrStatusCallbackDeliveryOutcome::BinderFailure);
    }
    Ok(DvrStatusCallbackDeliveryOutcome::Delivered)
}

fn dvr_status_notifier_loop(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
    cancel: Arc<AtomicBool>,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let initial_snapshot = dvr_status_metadata_snapshot(&runtime, handle)?;
    let mut callback_delivery_active = false;
    if initial_snapshot.callback_present && initial_snapshot.status_reporting_enabled {
        let initial_preflight = dvr_status_notification_preflight(
            &context,
            handle,
            &initial_snapshot,
            DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure,
            "IDvrCallback.poll_status.initial",
        )?;
        callback_delivery_active = !initial_preflight.should_skip_delivery();
    }
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if initial_snapshot.is_playback {
            consume_playback_dvr_once(&runtime, handle)?;
        }
        let snapshot = poll_dvr_status_snapshot(&runtime, handle)?;
        if !snapshot.started {
            return Ok(());
        }
        if callback_delivery_active {
            let preflight = dvr_status_notification_preflight(
                &context,
                handle,
                &snapshot,
                DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure,
                "IDvrCallback.poll_status.terminal",
            )?;
            callback_delivery_active = !preflight.should_skip_delivery();
        }
        if callback_delivery_active {
            if let Some(event) = snapshot.event {
                let delivery_outcome = deliver_dvr_status_event(
                    &context,
                    handle,
                    event,
                    "IDvrCallback.poll_status",
                    DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure,
                )?;
                match delivery_outcome {
                    DvrStatusCallbackDeliveryOutcome::Delivered => {}
                    DvrStatusCallbackDeliveryOutcome::ArtifactMissing
                    | DvrStatusCallbackDeliveryOutcome::StoreFailure
                    | DvrStatusCallbackDeliveryOutcome::BinderFailure => {
                        callback_delivery_active = false;
                    }
                }
            }
        }
        if !snapshot.is_playback && !callback_delivery_active {
            return Ok(());
        }
        let interval_ms = if snapshot.interval_ms == 0 {
            10
        } else {
            snapshot.interval_ms
        };
        thread::park_timeout(Duration::from_millis(interval_ms));
    }
}

fn run_dvr_status_notifier_with_terminal_diagnostic(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
    cancel: Arc<AtomicBool>,
) -> Result<(), HalError> {
    let terminal_error = match dvr_status_notifier_loop(Arc::clone(&context), handle, cancel) {
        Ok(()) => {
            record_dvr_status_notifier_lifecycle_outcome(
                &context,
                handle,
                DvrStatusNotifierCleanupDiagnosticRecord::worker_terminal(
                    handle.object_id(),
                    handle.generation(),
                    Ok(()),
                ),
            );
            return Ok(());
        }
        Err(error) => error,
    };
    record_dvr_status_notifier_lifecycle_outcome(
        &context,
        handle,
        DvrStatusNotifierCleanupDiagnosticRecord::worker_terminal(
            handle.object_id(),
            handle.generation(),
            Err(terminal_error.clone()),
        ),
    );
    record_dvr_callback_delivery_failure(
        &context,
        handle,
        CallbackDeliveryFailurePhase::NotifierTerminal,
        DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure,
        terminal_error.clone(),
    );
    Err(terminal_error)
}

pub fn record_dvr_post_commit_notification_outcome(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    phase: DvrPostCommitNotificationPhase,
    outcome: Result<(), HalError>,
) {
    let Err(primary) = outcome else {
        return;
    };
    record_dvr_callback_delivery_failure(
        context,
        handle,
        CallbackDeliveryFailurePhase::PostCommitNotification,
        phase,
        primary,
    );
}

pub fn deliver_started_dvr_status(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let snapshot = poll_dvr_status_snapshot(&runtime, handle)?;
    if !snapshot.started {
        return Ok(());
    }
    if !snapshot.callback_present || !snapshot.status_reporting_enabled {
        return Ok(());
    }
    let preflight = dvr_status_notification_preflight(
        context,
        handle,
        &snapshot,
        DvrPostCommitNotificationPhase::InitialStatusDelivery,
        "IDvrCallback.start_status",
    )?;
    if preflight.should_skip_delivery() {
        return Ok(());
    }
    let Some(event) = snapshot.event else {
        return Ok(());
    };
    match deliver_dvr_status_event(
        context,
        handle,
        event,
        "IDvrCallback.start_status",
        DvrPostCommitNotificationPhase::InitialStatusDelivery,
    )? {
        DvrStatusCallbackDeliveryOutcome::Delivered
        | DvrStatusCallbackDeliveryOutcome::ArtifactMissing
        | DvrStatusCallbackDeliveryOutcome::StoreFailure
        | DvrStatusCallbackDeliveryOutcome::BinderFailure => Ok(()),
    }
}

fn spawn_dvr_status_notifier(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    supervisor: Weak<DvrStatusNotifierSupervisor>,
) -> Result<DvrStatusNotifier, HalError> {
    let thread_context = Arc::clone(context);
    let worker = WorkerRuntime::spawn(
        format!(
            "tuner-hal2-dvr-status-{}-{}",
            handle.object_id().0,
            handle.generation().0
        ),
        handle.object_id().0,
        handle.generation().0,
        move |cancel| {
            run_dvr_status_notifier_with_terminal_diagnostic(thread_context, handle, cancel)
        },
        move || {
            if let Some(supervisor) = supervisor.upgrade() {
                supervisor.wake.notify_one();
            }
        },
    )
    .map_err(|error| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            format!("failed to spawn DVR status notifier: {error}"),
        )
    })?;
    Ok(DvrStatusNotifier { worker })
}

fn dvr_notifier_owner_generation_is_fenced(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> bool {
    let runtime = context.runtime();
    let Ok(runtime) = runtime.lock() else {
        return false;
    };
    runtime
        .dvr_status_metadata_snapshot_for_aidl_object(handle.object_id(), handle.generation())
        .is_err()
}

fn mark_dvr_notifier_service_critical(context: &SharedAidlServiceContext) {
    let runtime = context.runtime();
    if let Ok(mut runtime) = runtime.lock() {
        runtime.mark_service_critical();
    }
}

fn record_dvr_notifier_cleanup_control_failure(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    error: HalError,
) {
    record_superseded_dvr_notifier_cleanup_failure(context, handle, error);
    mark_dvr_notifier_service_critical(context);
}

fn fence_dvr_notifier_owner_after_cleanup_failure(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) {
    if dvr_notifier_owner_generation_is_fenced(context, handle) {
        return;
    }
    if let Err(error) = crate::object_runtime::drop_leak_object(context, handle) {
        record_dvr_notifier_cleanup_control_failure(context, handle, error);
        return;
    }
    if !dvr_notifier_owner_generation_is_fenced(context, handle) {
        record_dvr_notifier_cleanup_control_failure(
            context,
            handle,
            HalError::cleanup_failed(
                "DVR notifier owner fencing",
                "owner generation remained live after drop cleanup",
            ),
        );
    }
}

fn enqueue_cleanup_retry_after_notifier_reap(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) {
    match context.cleanup_dependency_for_handle(handle) {
        Ok(_) => {
            if let Err(error) = context.enqueue_cleanup_retry(handle) {
                record_dvr_notifier_cleanup_control_failure(context, handle, error);
            }
        }
        Err(dependency_error) => match context.cleanup_is_terminal_for_handle(handle) {
            Ok(true) => {}
            Ok(false) => {
                record_dvr_notifier_cleanup_control_failure(context, handle, dependency_error);
            }
            Err(terminal_error) => {
                record_dvr_notifier_cleanup_control_failure(
                    context,
                    handle,
                    compose_primary_cleanup_failure(
                        "DVR notifier cleanup dependency resolution failed",
                        dependency_error,
                        terminal_error,
                    ),
                );
            }
        },
    }
}

fn finish_reaped_dvr_status_notifier(
    context: Option<SharedAidlServiceContext>,
    job: DvrStatusNotifierReaperJob,
) {
    let handle = job.handle;
    let restart_requested = job.restart_requested;
    let cleanup_result = join_finished_dvr_status_notifier(job.notifier);
    let Some(context) = context else {
        return;
    };
    let record = if restart_requested {
        DvrStatusNotifierCleanupDiagnosticRecord::supersede_cleanup(
            AidlObjectId(job.key.object_id),
            AidlObjectGeneration(job.key.generation),
            cleanup_result.clone(),
        )
    } else {
        match job.transfer_reason {
            DvrStatusNotifierTransferReason::Stop => {
                DvrStatusNotifierCleanupDiagnosticRecord::reaper_completion(
                    AidlObjectId(job.key.object_id),
                    AidlObjectGeneration(job.key.generation),
                    cleanup_result.clone(),
                )
            }
            DvrStatusNotifierTransferReason::Reset => {
                DvrStatusNotifierCleanupDiagnosticRecord::reset_notifier_cleanup(
                    AidlObjectId(job.key.object_id),
                    AidlObjectGeneration(job.key.generation),
                    cleanup_result.clone(),
                )
            }
            DvrStatusNotifierTransferReason::WorkerTerminal => {
                DvrStatusNotifierCleanupDiagnosticRecord::reaper_completion(
                    AidlObjectId(job.key.object_id),
                    AidlObjectGeneration(job.key.generation),
                    cleanup_result.clone(),
                )
            }
        }
    };
    record_dvr_status_notifier_lifecycle_outcome(&context, handle, record);

    match cleanup_result {
        Ok(()) if restart_requested => {
            if let Err(error) = start_dvr_status_notifier(&context, handle) {
                record_superseded_dvr_notifier_cleanup_failure(&context, handle, error);
                mark_dvr_notifier_service_critical(&context);
            }
        }
        Ok(()) => {}
        Err(error) => {
            record_superseded_dvr_notifier_cleanup_failure(&context, handle, error);
            fence_dvr_notifier_owner_after_cleanup_failure(&context, handle);
        }
    }

    enqueue_cleanup_retry_after_notifier_reap(&context, handle);
}

fn handle_dvr_status_notifier_reaper_deadline(
    context: Option<SharedAidlServiceContext>,
    handle: AidlObjectHandle,
) {
    let Some(context) = context else {
        return;
    };
    let deadline_error = HalError::cleanup_failed(
        "DVR status notifier reaper deadline",
        "worker did not exit within the configured worker reaper deadline",
    );
    record_dvr_status_notifier_lifecycle_outcome(
        &context,
        handle,
        DvrStatusNotifierCleanupDiagnosticRecord::reaper_deadline(
            handle.object_id(),
            handle.generation(),
            Err(deadline_error),
        ),
    );

    if dvr_notifier_owner_generation_is_fenced(&context, handle) {
        return;
    }
    fence_dvr_notifier_owner_after_cleanup_failure(&context, handle);
}

pub(crate) fn start_dvr_status_notifier_reaper(
    context: Weak<crate::service_context::AidlServiceContext>,
    supervisor: Arc<DvrStatusNotifierSupervisor>,
) -> Result<(), HalError> {
    thread::Builder::new()
        .name("tuner-hal2-dvr-notifier-reaper".to_owned())
        .spawn(move || loop {
            match supervisor.take_next_action() {
                Ok(DvrStatusNotifierSupervisorAction::Completed(job)) => {
                    finish_reaped_dvr_status_notifier(context.upgrade(), job);
                }
                Ok(DvrStatusNotifierSupervisorAction::Deadline(handle)) => {
                    handle_dvr_status_notifier_reaper_deadline(context.upgrade(), handle);
                }
                Err(_supervisor_error) => {
                    if let Some(context) = context.upgrade() {
                        mark_dvr_notifier_service_critical(&context);
                    }
                    return;
                }
            }
        })
        .map(|_| ())
        .map_err(|error| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("failed to spawn DVR status notifier reaper: {error}"),
            )
        })
}

pub fn start_dvr_status_notifier(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let snapshot = dvr_status_metadata_snapshot(&runtime, handle)?;
    if !snapshot.started {
        return Ok(());
    }
    if !snapshot.is_playback || (snapshot.callback_present && snapshot.status_reporting_enabled) {
        let preflight = dvr_status_notification_preflight(
            context,
            handle,
            &snapshot,
            DvrPostCommitNotificationPhase::StatusNotifierStart,
            "IDvrCallback.notifier_start",
        )?;
        if preflight.should_skip_delivery() && !snapshot.is_playback {
            return Ok(());
        }
    }
    if !snapshot.is_playback
        && dvr_callback_notifier_availability(context, handle)?
            != DvrCallbackNotifierAvailability::Available
    {
        return Ok(());
    }
    context
        .dvr_status_notifier_supervisor()
        .start_or_request_restart(context, handle)
}

pub fn stop_dvr_status_notifier(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    context
        .dvr_status_notifier_supervisor()
        .signal_stop(handle)
        .map(|_| ())
}

pub(crate) fn finish_dvr_status_notifier_cleanup(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    match context
        .dvr_status_notifier_supervisor()
        .signal_stop(handle)?
    {
        DvrStatusNotifierStopDisposition::Complete => Ok(()),
        DvrStatusNotifierStopDisposition::ReaperPending => Err(HalError::cleanup_failed(
            "DVR status notifier cleanup",
            "worker ownership transferred to the DVR notifier reaper",
        )),
    }
}

pub fn stop_all_dvr_status_notifiers(
    context: &crate::service_context::AidlServiceContext,
) -> Result<(), HalError> {
    let result = context
        .dvr_status_notifier_supervisor()
        .signal_all_for_reset();
    if let Err(error) = result {
        return match context.record_dvr_status_notifier_cleanup_diagnostic(
            DvrStatusNotifierCleanupDiagnosticRecord::reset_store_recovered_after_poison(
                error.clone(),
            ),
        ) {
            Ok(()) => Err(error),
            Err(record_error) => Err(compose_primary_cleanup_failure(
                "DVR status notifier reset store recovery diagnostic failed",
                error,
                record_error,
            )),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
        IDvrCallback::{BnDvrCallback, IDvrCallback},
        PlaybackStatus::PlaybackStatus,
        RecordStatus::RecordStatus,
    };
    use binder::{BinderFeatures, Interface, StatusCode};
    use maleicacid_tuner_hal2_binder_adapter::{
        AidlApi, AidlMethodCall, AidlObjectKind, DvrConfigureKind, DvrConfigureRequest,
        DvrDataFormat, DvrOpenKind, OpenDvrRequest,
    };
    use maleicacid_tuner_hal2_service_runtime::ObjectMethodUseCase;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };

    #[derive(Default)]
    struct CallbackState {
        playback_statuses: Mutex<Vec<PlaybackStatus>>,
        record_statuses: Mutex<Vec<RecordStatus>>,
        fail_delivery: AtomicBool,
    }

    struct TestDvrCallback {
        state: Arc<CallbackState>,
    }

    impl Interface for TestDvrCallback {}

    impl IDvrCallback for TestDvrCallback {
        fn onPlaybackStatus(&self, status: PlaybackStatus) -> binder::Result<()> {
            if self.state.fail_delivery.load(Ordering::Relaxed) {
                return Err(StatusCode::FAILED_TRANSACTION.into());
            }
            self.state.playback_statuses.lock().unwrap().push(status);
            Ok(())
        }

        fn onRecordStatus(&self, status: RecordStatus) -> binder::Result<()> {
            if self.state.fail_delivery.load(Ordering::Relaxed) {
                return Err(StatusCode::FAILED_TRANSACTION.into());
            }
            self.state.record_statuses.lock().unwrap().push(status);
            Ok(())
        }
    }

    fn new_test_callback(state: Arc<CallbackState>) -> Strong<dyn IDvrCallback> {
        BnDvrCallback::new_binder(TestDvrCallback { state }, BinderFeatures::default())
    }

    fn record_dvr_callback_registration_for_test(
        runtime: &SharedTunerRuntime,
        handle: AidlObjectHandle,
    ) {
        let mut guard = runtime.lock().unwrap();
        let outcome = guard.record_callback_artifact_after_owner_ready_use_case(
            AidlObjectKind::Dvr,
            handle.object_id(),
            handle.generation(),
            AidlApi::DemuxOpenDvr,
            Ok(()),
        );
        guard
            .finish_callback_registration_after_artifact_result_use_case(outcome, None)
            .unwrap();
    }

    fn build_started_playback_dvr_context() -> (
        SharedAidlServiceContext,
        SharedTunerRuntime,
        AidlObjectHandle,
    ) {
        let runtime = Arc::new(Mutex::new(
            maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::new(),
        ));
        let context = crate::service_context::AidlServiceContext::from_shared_runtime_for_test(
            runtime.clone(),
        );
        let demux_entry = {
            let mut guard = runtime.lock().unwrap();
            guard
                .root_open_txn()
                .open_demux_root_object(AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenDemux,
                })
                .unwrap()
        };
        let dvr_open = ObjectMethodUseCase::execute_after_live(
            &runtime,
            demux_entry.object_id(),
            demux_entry.generation(),
            AidlObjectKind::Demux,
            || -> Result<_, maleicacid_tuner_hal2_common::HalError> {
                let request = OpenDvrRequest {
                    kind: DvrOpenKind::Playback,
                    buffer_size: 188,
                };
                Ok((AidlMethodCall::DemuxOpenDvr(request.clone()), request))
            },
            |runtime, dispatch, request| {
                runtime
                    .child_open_txn()
                    .open_dvr_child_runtime_for_demux_object(
                        demux_entry.object_id(),
                        demux_entry.generation(),
                        request,
                        dispatch,
                    )
            },
        )
        .unwrap();
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Dvr,
            dvr_open.runtime_entry.object_id(),
            dvr_open.runtime_entry.generation(),
        );
        ObjectMethodUseCase::execute_after_live(
            &runtime,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            || -> Result<_, maleicacid_tuner_hal2_common::HalError> {
                let request = DvrConfigureRequest {
                    kind: DvrConfigureKind::Playback,
                    status_mask: i32::from(PlaybackStatus::SPACE_EMPTY.0),
                    low_threshold_bytes: 0,
                    high_threshold_bytes: 188,
                    data_format: DvrDataFormat::Ts,
                    packet_size: 188,
                };
                Ok((AidlMethodCall::DvrConfigure(request.clone()), request))
            },
            |runtime, dispatch, request| {
                runtime.configure_dvr_runtime_for_object(
                    handle.object_id(),
                    handle.generation(),
                    request,
                    dispatch,
                )
            },
        )
        .unwrap();
        ObjectMethodUseCase::execute_after_live(
            &runtime,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            || -> Result<_, maleicacid_tuner_hal2_common::HalError> {
                Ok((AidlMethodCall::DvrStart, ()))
            },
            |runtime, dispatch, ()| {
                runtime.start_dvr_for_object(handle.object_id(), handle.generation(), dispatch)
            },
        )
        .unwrap();
        (context, runtime, handle)
    }

    #[test]
    fn status_event_to_hal_callback_routes_record_and_playback_statuses() {
        let state = Arc::new(CallbackState::default());
        let callback = new_test_callback(Arc::clone(&state));

        dvr_status_event_to_hal_callback(&callback, DvrStatusEvent::RecordHighWater).unwrap();
        dvr_status_event_to_hal_callback(&callback, DvrStatusEvent::PlaybackSpaceAlmostEmpty)
            .unwrap();

        assert_eq!(
            *state.record_statuses.lock().unwrap(),
            vec![RecordStatus::HIGH_WATER]
        );
        assert_eq!(
            *state.playback_statuses.lock().unwrap(),
            vec![PlaybackStatus::SPACE_ALMOST_EMPTY]
        );
    }

    #[test]
    fn deliver_started_dvr_status_emits_current_playback_status() {
        let (context, runtime, handle) = build_started_playback_dvr_context();
        let state = Arc::new(CallbackState::default());
        let callback = new_test_callback(Arc::clone(&state));
        context.clear_owner_callbacks_for_test(handle).unwrap();
        context.retain_dvr_callback(handle, &callback).unwrap();
        record_dvr_callback_registration_for_test(&runtime, handle);

        deliver_started_dvr_status(&context, handle).unwrap();

        assert_eq!(
            *state.playback_statuses.lock().unwrap(),
            vec![PlaybackStatus::SPACE_EMPTY]
        );
        let snapshot = runtime
            .lock()
            .unwrap()
            .dvr_status_poll_snapshot_for_aidl_object(handle.object_id(), handle.generation())
            .unwrap();
        assert!(!snapshot.callback_unhealthy);
        context.clear_owner_callbacks_for_test(handle).unwrap();
    }

    #[test]
    fn deliver_started_dvr_status_marks_unhealthy_on_binder_failure() {
        let (context, runtime, handle) = build_started_playback_dvr_context();
        let state = Arc::new(CallbackState::default());
        state.fail_delivery.store(true, Ordering::Relaxed);
        let callback = new_test_callback(Arc::clone(&state));
        context.clear_owner_callbacks_for_test(handle).unwrap();
        context.retain_dvr_callback(handle, &callback).unwrap();
        record_dvr_callback_registration_for_test(&runtime, handle);

        assert!(deliver_started_dvr_status(&context, handle).is_err());

        let snapshot = runtime
            .lock()
            .unwrap()
            .dvr_status_poll_snapshot_for_aidl_object(handle.object_id(), handle.generation())
            .unwrap();
        assert!(snapshot.callback_unhealthy);
        context.clear_owner_callbacks_for_test(handle).unwrap();
    }
}
