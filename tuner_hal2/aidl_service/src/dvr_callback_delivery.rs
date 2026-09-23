#[cfg(test)]
use std::sync::Mutex;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, PlaybackStatus::PlaybackStatus, RecordStatus::RecordStatus,
};
use binder::Strong;
use maleicacid_tuner_hal2_binder_adapter::{AidlObjectGeneration, AidlObjectId};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalErrorDetail, HalInternalKind,
};
use maleicacid_tuner_hal2_demux::DvrStatusEvent;
use maleicacid_tuner_hal2_service_runtime::{
    join_worker_classified, CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    CapabilitySnapshot, ClassifiedWorkerTerminalResult, DvrPostCommitNotificationDiagnosticRecord,
    DvrPostCommitNotificationFailureKind, DvrPostCommitNotificationPhase,
    DvrStatusNotifierCleanupDiagnosticRecord, DvrStatusPollSnapshot, WorkerFailureClassifier,
    WorkerRuntime, WorkerRuntimeSupervisor, WorkerRuntimeSupervisorAction,
    WorkerRuntimeSupervisorActiveEntry, WorkerRuntimeSupervisorReapingEntry,
    WorkerRuntimeSupervisorStartDisposition, WorkerRuntimeSupervisorStartOperation,
    WorkerRuntimeSupervisorStopDisposition, WorkerTerminalResult,
};

use crate::filter_callback_delivery::dispatch_filter_event_snapshots;
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

struct DvrStatusNotifierReaperJob {
    key: DvrStatusNotifierKey,
    handle: AidlObjectHandle,
    notifier: DvrStatusNotifier,
    transferred_at: Instant,
    deadline_reported: bool,
    restart_requested: bool,
    transfer_reason: DvrStatusNotifierTransferReason,
}

impl WorkerRuntimeSupervisorActiveEntry for DvrStatusNotifier {
    fn supervisor_is_finished(&self) -> bool {
        self.worker.is_finished()
    }

    fn supervisor_request_stop(&self) {
        self.worker.request_stop();
    }
}

struct DvrStatusNotifierStartOperation {
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
    supervisor: Weak<DvrStatusNotifierSupervisor>,
}

impl WorkerRuntimeSupervisorStartOperation<DvrStatusNotifier> for DvrStatusNotifierStartOperation {
    fn start(self) -> Result<DvrStatusNotifier, HalError> {
        spawn_dvr_status_notifier(&self.context, self.handle, self.supervisor)
    }
}

impl DvrStatusNotifierReaperJob {
    fn from_supervisor_transfer(
        key: DvrStatusNotifierKey,
        notifier: DvrStatusNotifier,
        transfer_reason: DvrStatusNotifierTransferReason,
    ) -> Self {
        Self {
            key,
            handle: AidlObjectHandle::new(
                maleicacid_tuner_hal2_domain_request::AidlObjectKind::Dvr,
                AidlObjectId(key.object_id),
                AidlObjectGeneration(key.generation),
            ),
            notifier,
            transferred_at: Instant::now(),
            deadline_reported: false,
            restart_requested: false,
            transfer_reason,
        }
    }
}

impl WorkerRuntimeSupervisorReapingEntry<DvrStatusNotifierKey, DvrStatusNotifier>
    for DvrStatusNotifierReaperJob
{
    type DeadlineTarget = AidlObjectHandle;

    fn from_terminal(key: DvrStatusNotifierKey, active: DvrStatusNotifier) -> Self {
        Self::from_supervisor_transfer(key, active, DvrStatusNotifierTransferReason::WorkerTerminal)
    }

    fn from_stop(key: DvrStatusNotifierKey, active: DvrStatusNotifier) -> Self {
        Self::from_supervisor_transfer(key, active, DvrStatusNotifierTransferReason::Stop)
    }

    fn from_reset(key: DvrStatusNotifierKey, active: DvrStatusNotifier) -> Self {
        Self::from_supervisor_transfer(key, active, DvrStatusNotifierTransferReason::Reset)
    }

    fn supervisor_is_finished(&self) -> bool {
        self.notifier.worker.is_finished()
    }

    fn supervisor_set_restart_requested(&mut self, requested: bool) {
        self.restart_requested = requested;
    }

    fn supervisor_deadline_reported(&self) -> bool {
        self.deadline_reported
    }

    fn supervisor_mark_deadline_reported(&mut self) {
        self.deadline_reported = true;
    }

    fn supervisor_transferred_at(&self) -> Instant {
        self.transferred_at
    }

    fn supervisor_deadline_target(&self) -> Self::DeadlineTarget {
        self.handle
    }
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
    StartPending,
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
            runtime: WorkerRuntime::supervisor(
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
        if let Some(terminal) = self.runtime.worker_terminal_result()? {
            mark_dvr_notifier_service_critical(context);
            return Err(
                match WorkerFailureClassifier::classify_terminal(
                    terminal,
                    "DVR通知回収ワーカーがpanicしたか、終了待ちに失敗しました",
                ) {
                    ClassifiedWorkerTerminalResult::Failure { error, .. } => error,
                    ClassifiedWorkerTerminalResult::Normal(())
                    | ClassifiedWorkerTerminalResult::StopRequested => HalError::cleanup_failed(
                        "DVR通知回収ワーカー",
                        "回収ワーカーは既に終了しています",
                    ),
                },
            );
        }
        let key = DvrStatusNotifierKey::new(handle);
        match self.runtime.start_supervised(
            key,
            DvrStatusNotifierStartOperation {
                context: Arc::clone(context),
                handle,
                supervisor: Arc::downgrade(self),
            },
        )? {
            WorkerRuntimeSupervisorStartDisposition::Started
            | WorkerRuntimeSupervisorStartDisposition::Active
            | WorkerRuntimeSupervisorStartDisposition::ReapingPending => Ok(()),
            WorkerRuntimeSupervisorStartDisposition::StartPending => Err(HalError::invalid_state(
                maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                "DVR状態通知ワーカーの開始処理が完了していません",
            )),
        }
    }

    fn signal_stop(
        &self,
        handle: AidlObjectHandle,
    ) -> Result<DvrStatusNotifierStopDisposition, HalError> {
        let key = DvrStatusNotifierKey::new(handle);
        match self.runtime.request_supervised_stop(key)? {
            WorkerRuntimeSupervisorStopDisposition::Complete => {
                Ok(DvrStatusNotifierStopDisposition::Complete)
            }
            WorkerRuntimeSupervisorStopDisposition::StartPending => {
                Ok(DvrStatusNotifierStopDisposition::StartPending)
            }
            WorkerRuntimeSupervisorStopDisposition::ReapingPending => {
                Ok(DvrStatusNotifierStopDisposition::ReaperPending)
            }
        }
    }

    fn signal_all_for_reset(&self) -> Result<(), HalError> {
        self.runtime.request_supervised_reset()
    }

    fn take_next_action(
        &self,
    ) -> Result<(Option<DvrStatusNotifierSupervisorAction>, Option<Instant>), HalError> {
        let (action, next_wait) = self.runtime.take_supervisor_action()?;
        let action = action.map(|action| match action {
            WorkerRuntimeSupervisorAction::Completed(job) => {
                DvrStatusNotifierSupervisorAction::Completed(job)
            }
            WorkerRuntimeSupervisorAction::Deadline(handle) => {
                DvrStatusNotifierSupervisorAction::Deadline(handle)
            }
        });
        Ok((action, next_wait))
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
        Err(error) => {
            DvrCallbackArtifactLookup::StoreFailure(error.into_hal_error(delivery_context))
        }
    }
}

fn poll_dvr_status_snapshot(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<DvrStatusPollSnapshot, HalError> {
    let guard = maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::lock_shared(
        runtime.as_ref(),
        "DVR状態の照会中にservice runtimeのロックが汚染されました",
    )?;
    guard.dvr_status_poll_snapshot_for_aidl_object(handle.object_id(), handle.generation())
}

fn dvr_status_metadata_snapshot(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<DvrStatusPollSnapshot, HalError> {
    let guard = maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::lock_shared(
        runtime.as_ref(),
        "DVR状態メタデータの照会中にservice runtimeのロックが汚染されました",
    )?;
    guard.dvr_status_metadata_snapshot_for_aidl_object(handle.object_id(), handle.generation())
}

pub fn is_playback_dvr(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<bool, HalError> {
    dvr_status_metadata_snapshot(&context.runtime(), handle).map(|snapshot| snapshot.is_playback)
}

fn consume_playback_dvr_once(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let events = maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::consume_playback_dvr_for_object(
        &runtime, handle.object_id(), handle.generation())?;
    maleicacid_tuner_hal2_service_runtime::notify_filter_delivery_change(&runtime)?;
    let _recorded_failure = dispatch_filter_event_snapshots(context, events);
    Ok(())
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
            HalError::callback_failed(delivery_context, "DVR callbackが登録されていません"),
        );
        return Ok(DvrStatusNotificationPreflight::CallbackMissing);
    }
    if snapshot.callback_unhealthy {
        record_dvr_callback_delivery_failure(
            context,
            handle,
            CallbackDeliveryFailurePhase::RuntimePolicySkip,
            dvr_phase,
            HalError::callback_failed(delivery_context, "DVR callbackは利用できない状態です"),
        );
        return Ok(DvrStatusNotificationPreflight::CallbackUnhealthy);
    }
    if !snapshot.status_reporting_enabled {
        record_dvr_callback_delivery_failure(
            context,
            handle,
            CallbackDeliveryFailurePhase::RuntimePolicySkip,
            dvr_phase,
            HalError::callback_failed(delivery_context, "DVR状態通知は無効です"),
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
                    "notifier起動前にDVR callback artifactが見つかりません",
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
        let mut guard = maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::lock_shared(
            runtime.as_ref(),
            "DVR callback配送失敗の完了処理中にservice runtimeのロックが汚染されました",
        )?;
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
            "確定後のDVR callback配送記録に失敗しました",
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
    let phase = record.phase();
    if let Err(error) = context.record_dvr_status_notifier_cleanup_diagnostic(record) {
        record_post_commit_accounting_failure_fallback(
            context,
            handle,
            phase,
            DvrPostCommitNotificationFailureKind::NotifierCleanup,
            error,
            "DVR status notifierのライフサイクル診断に失敗しました",
            HalError::cleanup_failed(
                "DVR status notifierのライフサイクル診断",
                "録画に失敗しました",
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
                HalError::callback_failed(
                    delivery_context,
                    "DVR callback artifactが見つかりません",
                ),
            );
            return Ok(DvrStatusCallbackDeliveryOutcome::ArtifactMissing);
        }
        Err(error) => {
            let primary = error.into_hal_error(delivery_context);
            record_dvr_artifact_lookup_failure(context, handle, dvr_phase, primary);
            return Ok(DvrStatusCallbackDeliveryOutcome::StoreFailure);
        }
    };
    if let Err(error) = dvr_status_event_to_hal_callback(&callback, event) {
        let primary = HalError::callback_failed(
            delivery_context,
            format!("Binder呼び出しに失敗しました: {error:?}"),
        );
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
    cancel: maleicacid_tuner_hal2_service_runtime::WorkerContext,
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
        if cancel.stop_requested() {
            return Ok(());
        }
        if initial_snapshot.is_playback {
            consume_playback_dvr_once(&context, handle)?;
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
        cancel.wait_until(Some(
            Instant::now()
                .checked_add(Duration::from_millis(interval_ms))
                .ok_or_else(|| {
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "DVR状態通知の期限が上限を超えました",
                    )
                })?,
        ));
    }
}

fn run_dvr_status_notifier_with_terminal_diagnostic(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
    cancel: maleicacid_tuner_hal2_service_runtime::WorkerContext,
) -> Result<(), HalError> {
    let terminal_error = match dvr_status_notifier_loop(Arc::clone(&context), handle, cancel) {
        Ok(()) => {
            record_dvr_status_notifier_lifecycle_outcome(
                &context,
                handle,
                DvrStatusNotifierCleanupDiagnosticRecord::WorkerTerminal {
                    object_id: handle.object_id(),
                    generation: handle.generation(),
                    terminal: ClassifiedWorkerTerminalResult::Normal(()),
                },
            );
            return Ok(());
        }
        Err(error) => error,
    };
    record_dvr_status_notifier_lifecycle_outcome(
        &context,
        handle,
        DvrStatusNotifierCleanupDiagnosticRecord::WorkerTerminal {
            object_id: handle.object_id(),
            generation: handle.generation(),
            terminal: WorkerFailureClassifier::classify_terminal(
                WorkerTerminalResult::RuntimeFailure(terminal_error.clone()),
                "DVR状態通知ワーカーがpanicしました",
            ),
        },
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
                supervisor.runtime.notify_worker();
            }
        },
    )
    .map_err(|error| HalError::Io {
        backend: "DVR状態通知",
        operation: "スレッド生成",
        path: None,
        errno: error.raw_os_error(),
        detail: HalErrorDetail::new(error.to_string()),
    })?;
    Ok(DvrStatusNotifier { worker })
}

fn dvr_notifier_owner_generation_is_fenced(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> bool {
    let runtime = context.runtime();
    let Ok(runtime) = maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::lock_shared(
        &runtime,
        "DVR通知所有者世代",
    ) else {
        return false;
    };
    runtime
        .dvr_status_metadata_snapshot_for_aidl_object(handle.object_id(), handle.generation())
        .is_err()
}

fn mark_dvr_notifier_service_critical(context: &SharedAidlServiceContext) {
    let runtime = context.runtime();
    maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::mark_shared_service_critical(
        &runtime,
    );
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
    if let Err(status) = crate::object_runtime::drop_leak_object(context, handle) {
        record_dvr_notifier_cleanup_control_failure(
            context,
            handle,
            HalError::cleanup_failed(
                "DVR notifier owner fencing",
                format!("Drop漏れ検出後の後片付けに失敗しました: {status:?}"),
            ),
        );
        return;
    }
    if !dvr_notifier_owner_generation_is_fenced(context, handle) {
        record_dvr_notifier_cleanup_control_failure(
            context,
            handle,
            HalError::cleanup_failed(
                "DVR notifier owner fencing",
                "Drop後の後片付け後もowner世代が有効です",
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
                        "DVR notifierの後片付け依存関係を解決できません",
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
    let terminal = join_worker_classified(job.notifier.worker);
    let cleanup_result = match &terminal {
        ClassifiedWorkerTerminalResult::Normal(())
        | ClassifiedWorkerTerminalResult::StopRequested => Ok(()),
        ClassifiedWorkerTerminalResult::Failure { error, .. } => Err(error.clone()),
    };
    let Some(context) = context else {
        return;
    };
    let record = if restart_requested {
        DvrStatusNotifierCleanupDiagnosticRecord::SupersedeCleanup {
            object_id: AidlObjectId(job.key.object_id),
            generation: AidlObjectGeneration(job.key.generation),
            terminal,
        }
    } else {
        match job.transfer_reason {
            DvrStatusNotifierTransferReason::Stop => {
                DvrStatusNotifierCleanupDiagnosticRecord::ReaperCompletion {
                    object_id: AidlObjectId(job.key.object_id),
                    generation: AidlObjectGeneration(job.key.generation),
                    terminal,
                }
            }
            DvrStatusNotifierTransferReason::Reset => {
                DvrStatusNotifierCleanupDiagnosticRecord::ResetNotifierCleanup {
                    object_id: AidlObjectId(job.key.object_id),
                    generation: AidlObjectGeneration(job.key.generation),
                    terminal,
                }
            }
            DvrStatusNotifierTransferReason::WorkerTerminal => {
                DvrStatusNotifierCleanupDiagnosticRecord::ReaperCompletion {
                    object_id: AidlObjectId(job.key.object_id),
                    generation: AidlObjectGeneration(job.key.generation),
                    terminal,
                }
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
        "設定されたワーカー回収期限までにワーカーが終了しませんでした",
    );
    record_dvr_status_notifier_lifecycle_outcome(
        &context,
        handle,
        DvrStatusNotifierCleanupDiagnosticRecord::ReaperDeadline {
            object_id: handle.object_id(),
            generation: handle.generation(),
            error: deadline_error,
        },
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
    let weak_supervisor = Arc::downgrade(&supervisor);
    let observer_context = context.clone();
    supervisor.runtime.start_worker(
        "tuner-hal2-dvr-notifier-reaper",
        move |control| {
            while !control.stop_requested() {
                let Some(supervisor) = weak_supervisor.upgrade() else {
                    return Ok(());
                };
                let (action, deadline) = supervisor.take_next_action()?;
                // 待機中に管理部の所有権を保持しない。所有者の消滅で停止・起床できる。
                drop(supervisor);
                match action {
                    Some(DvrStatusNotifierSupervisorAction::Completed(job)) => {
                        finish_reaped_dvr_status_notifier(context.upgrade(), job);
                    }
                    Some(DvrStatusNotifierSupervisorAction::Deadline(handle)) => {
                        handle_dvr_status_notifier_reaper_deadline(context.upgrade(), handle);
                    }
                    None => control.wait_until(deadline),
                }
            }
            Ok(())
        },
        move |terminal: &WorkerTerminalResult<()>| {
            if let ClassifiedWorkerTerminalResult::Failure { category, error } =
                WorkerFailureClassifier::classify_terminal(
                    terminal.clone(),
                    "DVR通知回収ワーカーがpanicしたか、終了待ちに失敗しました",
                )
            {
                log::error!(
                    "DVR notifier reaperに失敗しました: 分類={category:?} エラー={error:?}"
                );
                if let Some(context) = observer_context.upgrade() {
                    mark_dvr_notifier_service_critical(&context);
                }
            }
        },
    )
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
        DvrStatusNotifierStopDisposition::StartPending => Err(HalError::cleanup_failed(
            "DVR状態通知ワーカーの後片付け",
            "ワーカー開始処理の完了待ちです",
        )),
        DvrStatusNotifierStopDisposition::ReaperPending => Err(HalError::cleanup_failed(
            "DVR状態通知ワーカーの後片付け",
            "ワーカー所有権をDVR通知回収処理へ移管しました",
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
            DvrStatusNotifierCleanupDiagnosticRecord::ResetStoreRecoveredAfterPoison {
                error: error.clone(),
            },
        ) {
            Ok(()) => Err(error),
            Err(record_error) => Err(compose_primary_cleanup_failure(
                "DVR status notifier reset storeの復旧診断に失敗しました",
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
    use maleicacid_tuner_hal2_domain_request::{
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
        context
            .retain_dvr_callback_for_test(handle, &callback)
            .unwrap();
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
        context
            .retain_dvr_callback_for_test(handle, &callback)
            .unwrap();
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
