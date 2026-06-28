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
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_demux::DvrStatusEvent;
use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport, DvrPostCommitNotificationPhase,
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

fn dvr_callback_is_available(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<bool, HalError> {
    match context.dvr_callback_for_owner(handle) {
        Ok(Some(_)) => Ok(true),
        Ok(None) => Ok(false),
        Err(_) => Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "callback store lock poisoned while reading DVR callback",
        )),
    }
}

fn finish_dvr_callback_delivery_failure(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    phase: CallbackDeliveryFailurePhase,
    dvr_phase: DvrPostCommitNotificationPhase,
    primary: HalError,
) -> Result<(), HalError> {
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
        primary,
    ))
}

fn deliver_dvr_status_event(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    event: DvrStatusEvent,
    delivery_context: &'static str,
    dvr_phase: DvrPostCommitNotificationPhase,
) -> Result<bool, HalError> {
    let callback = match context.dvr_callback_for_owner(handle) {
        Ok(Some(callback)) => callback,
        Ok(None) => return Ok(false),
        Err(_) => {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{delivery_context}: callback store lock poisoned"),
            ));
        }
    };
    if let Err(error) = dvr_status_event_to_hal_callback(&callback, event) {
        let primary =
            HalError::callback_failed(delivery_context, format!("binder failure: {error:?}"));
        finish_dvr_callback_delivery_failure(
            &context.runtime(),
            handle,
            CallbackDeliveryFailurePhase::BinderDelivery,
            dvr_phase,
            primary,
        )?;
        return Ok(false);
    }
    Ok(true)
}

fn dvr_status_notifier_loop(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
    cancel: Arc<AtomicBool>,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let initial_snapshot = poll_dvr_status_snapshot(&runtime, handle)?;
    if !initial_snapshot.started
        || !initial_snapshot.callback_present
        || initial_snapshot.callback_unhealthy
        || !initial_snapshot.status_reporting_enabled
    {
        return Ok(());
    }
    let mut last_event = initial_snapshot.event;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let snapshot = poll_dvr_status_snapshot(&runtime, handle)?;
        if !snapshot.started
            || !snapshot.callback_present
            || snapshot.callback_unhealthy
            || !snapshot.status_reporting_enabled
        {
            return Ok(());
        }
        if snapshot.event != last_event {
            if let Some(event) = snapshot.event {
                deliver_dvr_status_event(
                    &context,
                    handle,
                    event,
                    "IDvrCallback.poll_status",
                    DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure,
                )?;
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
        Ok(Ok(())) => return Ok(()),
        Ok(Err(error)) => error,
        Err(_) => HalError::internal(
            HalInternalKind::InvariantViolation,
            "DVR status notifier thread panicked",
        ),
    };
    record_dvr_post_commit_notification_outcome(
        &context.runtime(),
        handle,
        DvrPostCommitNotificationPhase::StatusNotifierRuntimeFailure,
        Err(terminal_error),
    )
}

pub fn record_dvr_post_commit_notification_outcome(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    phase: DvrPostCommitNotificationPhase,
    outcome: Result<(), HalError>,
) -> Result<(), HalError> {
    let Err(primary) = outcome else {
        return Ok(());
    };
    finish_dvr_callback_delivery_failure(
        runtime,
        handle,
        CallbackDeliveryFailurePhase::PostCommitNotification,
        phase,
        primary,
    )
}

pub fn deliver_started_dvr_status(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let snapshot = poll_dvr_status_snapshot(&runtime, handle)?;
    if !snapshot.started
        || !snapshot.callback_present
        || snapshot.callback_unhealthy
        || !snapshot.status_reporting_enabled
    {
        return Ok(());
    }
    let Some(event) = snapshot.event else {
        return Ok(());
    };
    deliver_dvr_status_event(
        context,
        handle,
        event,
        "IDvrCallback.start_status",
        DvrPostCommitNotificationPhase::InitialStatusDelivery,
    )?;
    Ok(())
}

pub fn start_dvr_status_notifier(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    stop_dvr_status_notifier(context, handle)?;
    let runtime = context.runtime();
    let snapshot = poll_dvr_status_snapshot(&runtime, handle)?;
    if !snapshot.started
        || !snapshot.callback_present
        || snapshot.callback_unhealthy
        || !snapshot.status_reporting_enabled
        || !dvr_callback_is_available(context, handle)?
    {
        return Ok(());
    }
    let mut store = context.dvr_status_notifiers_lock()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    let thread_context = Arc::clone(context);
    let join = thread::Builder::new()
        .name(format!(
            "tuner-hal2-dvr-status-{}-{}",
            handle.object_id().0,
            handle.generation().0
        ))
        .spawn(move || {
            run_dvr_status_notifier_with_terminal_diagnostic(thread_context, handle, thread_cancel)
        })
        .map_err(|error| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("failed to spawn DVR status notifier: {error}"),
            )
        })?;
    store.insert(
        DvrStatusNotifierKey::new(handle),
        DvrStatusNotifier { cancel, join },
    );
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
    notifier.cancel.store(true, Ordering::Relaxed);
    notifier.join.thread().unpark();
    let terminal_result = notifier.join.join().map_err(|_| {
        HalError::cleanup_failed(
            "DVR status notifier join",
            "DVR status notifier thread panicked",
        )
    })?;
    terminal_result
}

pub fn stop_all_dvr_status_notifiers(
    context: &crate::service_context::AidlServiceContext,
) -> Result<(), HalError> {
    let notifiers = {
        let mut store = context.dvr_status_notifiers_lock()?;
        std::mem::take(&mut *store)
    };
    let mut first_error = None;
    for (_, notifier) in notifiers {
        notifier.cancel.store(true, Ordering::Relaxed);
        notifier.join.thread().unpark();
        let result = notifier
            .join
            .join()
            .map_err(|_| {
                HalError::cleanup_failed(
                    "DVR status notifier join",
                    "DVR status notifier thread panicked",
                )
            })
            .and_then(|terminal| terminal);
        if first_error.is_none() {
            if let Err(error) = result {
                first_error = Some(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
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
        AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind, DvrConfigureKind,
        DvrConfigureRequest, DvrOpenKind, OpenDvrRequest,
    };
    use maleicacid_tuner_hal2_service_runtime::{CallbackHealthState, RuntimeOwnerRelation};
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
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Dvr,
            AidlObjectId(99_001),
            AidlObjectGeneration(1),
        );
        {
            let mut guard = runtime.lock().unwrap();
            let demux = guard.allocate_demux_runtime().unwrap();
            let dvr = guard.allocate_dvr_runtime(demux.id.0).unwrap();
            guard
                .register_demux_dvr_runtime(
                    demux.id.0,
                    dvr.id.0,
                    &OpenDvrRequest {
                        kind: DvrOpenKind::Playback,
                        buffer_size: 188,
                    },
                    true,
                )
                .unwrap();
            guard
                .register_aidl_object_for_runtime(
                    AidlObjectKind::Dvr,
                    handle.object_id(),
                    handle.generation(),
                    i64::from(dvr.id.0),
                    RuntimeOwnerRelation::Root,
                )
                .unwrap();
            guard
                .configure_dvr_runtime_request(
                    dvr.id.0,
                    DvrConfigureRequest {
                        kind: DvrConfigureKind::Playback,
                        status_mask: i32::from(PlaybackStatus::SPACE_FULL.0),
                        low_threshold_bytes: 0,
                        high_threshold_bytes: 188,
                    },
                )
                .unwrap();
            guard.start_dvr_runtime(dvr.id.0).unwrap();
        }
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
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .callback_registration_health(
                    AidlObjectKind::Dvr,
                    handle.object_id(),
                    handle.generation(),
                    AidlApi::DemuxOpenDvr,
                )
                .unwrap(),
            CallbackHealthState::Registered
        );
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
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .callback_registration_health(
                    AidlObjectKind::Dvr,
                    handle.object_id(),
                    handle.generation(),
                    AidlApi::DemuxOpenDvr,
                )
                .unwrap(),
            CallbackHealthState::Unhealthy
        );
        context.clear_owner_callbacks_for_test(handle).unwrap();
    }
}
