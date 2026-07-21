use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, PlaybackStatus::PlaybackStatus, RecordStatus::RecordStatus,
};
use binder::Strong;
use maleicacid_tuner_hal2_binder_adapter::{AidlObjectGeneration, AidlObjectId};
use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, FirstErrorCollector, HalError, HalInternalKind,
};
use maleicacid_tuner_hal2_demux::DvrStatusEvent;
use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    DvrPostCommitNotificationDiagnosticRecord, DvrPostCommitNotificationFailureKind,
    DvrPostCommitNotificationPhase, DvrStatusNotifierCleanupDiagnosticRecord,
    DvrStatusPollSnapshot,
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
    cancel: Arc<AtomicBool>,
    join: JoinHandle<Result<(), HalError>>,
}

fn stop_joined_dvr_status_notifier(notifier: DvrStatusNotifier) -> Result<(), HalError> {
    notifier.cancel.store(true, Ordering::Relaxed);
    notifier.join.thread().unpark();
    notifier.join.join().map_err(|_| {
        HalError::cleanup_failed(
            "DVR status notifier join",
            "DVR status notifier thread panicked",
        )
    })?
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
        guard.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::dvr(
            handle.object_id(),
            handle.generation(),
            phase,
            dvr_phase,
            primary.clone(),
        ))
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
    let initial_snapshot = poll_dvr_status_snapshot(&runtime, handle)?;
    let initial_preflight = dvr_status_notification_preflight(
        &context,
        handle,
        &initial_snapshot,
        DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure,
        "IDvrCallback.poll_status.initial",
    )?;
    if initial_preflight.should_skip_delivery() {
        return Err(HalError::callback_failed(
            "IDvrCallback.poll_status.initial",
            format!(
                "DVR status notifier terminated during initial preflight: {initial_preflight:?}"
            ),
        ));
    }
    let mut last_event = initial_snapshot.event;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let snapshot = poll_dvr_status_snapshot(&runtime, handle)?;
        let preflight = dvr_status_notification_preflight(
            &context,
            handle,
            &snapshot,
            DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure,
            "IDvrCallback.poll_status.terminal",
        )?;
        if preflight.should_skip_delivery() {
            return Err(HalError::callback_failed(
                "IDvrCallback.poll_status.terminal",
                format!("DVR status notifier terminated during preflight: {preflight:?}"),
            ));
        }
        if snapshot.event != last_event {
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
                        return Err(HalError::callback_failed(
                            "IDvrCallback.poll_status",
                            format!("DVR status notifier terminated after delivery outcome: {delivery_outcome:?}"),
                        ));
                    }
                }
            }
            last_event = snapshot.event;
        }
        thread::park_timeout(Duration::from_millis(snapshot.interval_ms.max(1)));
    }
}

fn run_dvr_status_notifier_with_terminal_diagnostic(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
    cancel: Arc<AtomicBool>,
) -> Result<(), HalError> {
    let context_for_loop = Arc::clone(&context);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        dvr_status_notifier_loop(context_for_loop, handle, cancel)
    }));
    let terminal_error = match outcome {
        Ok(Ok(())) => {
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
        Ok(Err(error)) => error,
        Err(_) => HalError::internal(
            HalInternalKind::InvariantViolation,
            "DVR status notifier thread panicked",
        ),
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
        terminal_error,
    );
    Ok(())
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

pub fn start_dvr_status_notifier(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let snapshot = poll_dvr_status_snapshot(&runtime, handle)?;
    let preflight = dvr_status_notification_preflight(
        context,
        handle,
        &snapshot,
        DvrPostCommitNotificationPhase::StatusNotifierStart,
        "IDvrCallback.notifier_start",
    )?;
    if preflight.should_skip_delivery() {
        return Ok(());
    }
    if dvr_callback_notifier_availability(context, handle)?
        != DvrCallbackNotifierAvailability::Available
    {
        return Ok(());
    }
    let old_notifier = {
        let mut store = context.dvr_status_notifiers_lock()?;
        let key = DvrStatusNotifierKey::new(handle);
        let mut old_notifier = store.remove(&key);
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = Arc::clone(&cancel);
        let thread_context = Arc::clone(context);
        let spawn_result = thread::Builder::new()
            .name(format!(
                "tuner-hal2-dvr-status-{}-{}",
                handle.object_id().0,
                handle.generation().0
            ))
            .spawn(move || {
                run_dvr_status_notifier_with_terminal_diagnostic(
                    thread_context,
                    handle,
                    thread_cancel,
                )
            });
        let join = match spawn_result {
            Ok(join) => join,
            Err(error) => {
                if let Some(old_notifier) = old_notifier.take() {
                    store.insert(key, old_notifier);
                }
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    format!("failed to spawn DVR status notifier: {error}"),
                ));
            }
        };
        if let Some(old_notifier) = old_notifier.as_ref() {
            old_notifier.cancel.store(true, Ordering::Relaxed);
            old_notifier.join.thread().unpark();
        }
        store.insert(key, DvrStatusNotifier { cancel, join });
        old_notifier
    };
    if let Some(notifier) = old_notifier {
        let cleanup_result = stop_joined_dvr_status_notifier(notifier);
        record_dvr_status_notifier_lifecycle_outcome(
            context,
            handle,
            DvrStatusNotifierCleanupDiagnosticRecord::supersede_cleanup(
                handle.object_id(),
                handle.generation(),
                cleanup_result.clone(),
            ),
        );
        if let Err(error) = cleanup_result {
            record_superseded_dvr_notifier_cleanup_failure(context, handle, error);
        }
    }
    Ok(())
}

pub fn stop_dvr_status_notifier(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let notifier = context
        .dvr_status_notifiers_lock()?
        .remove(&DvrStatusNotifierKey::new(handle));
    let Some(notifier) = notifier else {
        return Ok(());
    };
    stop_joined_dvr_status_notifier(notifier)
}

pub fn stop_all_dvr_status_notifiers(
    context: &crate::service_context::AidlServiceContext,
) -> Result<(), HalError> {
    let (notifiers, take_result) = context.take_dvr_status_notifiers_for_reset();
    let mut failures = FirstErrorCollector::new();
    if let Err(error) = take_result {
        if let Err(record_error) = context.record_dvr_status_notifier_cleanup_diagnostic(
            DvrStatusNotifierCleanupDiagnosticRecord::reset_store_recovered_after_poison(
                error.clone(),
            ),
        ) {
            failures.push_error(compose_primary_cleanup_failure(
                "DVR status notifier reset store recovery diagnostic failed",
                error.clone(),
                record_error,
            ));
        }
        failures.push_error(error);
    }
    for (key, notifier) in notifiers {
        let cleanup_result = stop_joined_dvr_status_notifier(notifier).map_err(|error| {
            HalError::cleanup_failed(
                "DVR status notifier reset",
                format!(
                    "object_id={} generation={} cleanup_error={error}",
                    key.object_id, key.generation
                ),
            )
        });
        if let Err(record_error) = context.record_dvr_status_notifier_cleanup_diagnostic(
            DvrStatusNotifierCleanupDiagnosticRecord::reset_notifier_cleanup(
                AidlObjectId(key.object_id),
                AidlObjectGeneration(key.generation),
                cleanup_result.clone(),
            ),
        ) {
            failures.push_error(match cleanup_result.clone() {
                Ok(()) => HalError::cleanup_failed(
                    "DVR status notifier reset cleanup diagnostic",
                    record_error.to_string(),
                ),
                Err(primary) => compose_primary_cleanup_failure(
                    "DVR status notifier reset cleanup diagnostic failed",
                    primary,
                    record_error,
                ),
            });
        }
        failures.push_result(cleanup_result);
    }
    failures.into_result()
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
        DvrOpenKind, OpenDvrRequest,
    };
    use maleicacid_tuner_hal2_service_runtime::execute_object_method_call_after_live;
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
                .open_demux_root_object(AidlMethodCall::PublicApi {
                    object: AidlObjectKind::Tuner,
                    api: AidlApi::TunerOpenDemux,
                })
                .unwrap()
        };
        let dvr_open = execute_object_method_call_after_live(
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
                runtime.open_dvr_child_runtime_for_demux_object(
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
        execute_object_method_call_after_live(
            &runtime,
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
            || -> Result<_, maleicacid_tuner_hal2_common::HalError> {
                let request = DvrConfigureRequest {
                    kind: DvrConfigureKind::Playback,
                    status_mask: i32::from(PlaybackStatus::SPACE_FULL.0),
                    low_threshold_bytes: 0,
                    high_threshold_bytes: 188,
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
        execute_object_method_call_after_live(
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
            vec![PlaybackStatus::SPACE_FULL]
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
