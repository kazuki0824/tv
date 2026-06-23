use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, PlaybackStatus::PlaybackStatus, RecordStatus::RecordStatus,
};
use binder::Strong;
use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlObjectKind};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_demux::DvrStatusEvent;
use maleicacid_tuner_hal2_service_runtime::{CallbackRegistryUpdate, DvrStatusPollSnapshot};

use crate::callback_store::dvr_callback_for_owner;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::SharedTunerRuntime;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DvrStatusNotifierKey {
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

struct DvrStatusNotifier {
    cancel: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

static DVR_STATUS_NOTIFIERS: OnceLock<Mutex<BTreeMap<DvrStatusNotifierKey, DvrStatusNotifier>>> =
    OnceLock::new();

fn notifier_store() -> &'static Mutex<BTreeMap<DvrStatusNotifierKey, DvrStatusNotifier>> {
    DVR_STATUS_NOTIFIERS.get_or_init(|| Mutex::new(BTreeMap::new()))
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

fn dvr_callback_is_available(handle: AidlObjectHandle) -> Result<bool, HalError> {
    match dvr_callback_for_owner(handle) {
        Ok(Some(_)) => Ok(true),
        Ok(None) => Ok(false),
        Err(_) => Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "callback store lock poisoned while reading DVR callback",
        )),
    }
}

fn mark_dvr_callback_unhealthy(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while marking DVR callback unhealthy",
        )
    })?;
    match guard.callback_registry_mut().mark_unhealthy(
        AidlObjectKind::Dvr,
        handle.object_id(),
        handle.generation(),
        AidlApi::DemuxOpenDvr,
    ) {
        CallbackRegistryUpdate::Updated => {}
        CallbackRegistryUpdate::Missing => {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR callback registry entry missing while marking unhealthy",
            ));
        }
    }
    guard.mark_dvr_callback_unhealthy_for_object(handle.object_id(), handle.generation())
}

fn deliver_dvr_status_event(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    event: DvrStatusEvent,
    context: &'static str,
) -> Result<bool, HalError> {
    let callback = match dvr_callback_for_owner(handle) {
        Ok(Some(callback)) => callback,
        Ok(None) => return Ok(false),
        Err(_) => {
            return Err(HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: callback store lock poisoned"),
            ));
        }
    };
    if let Err(error) = dvr_status_event_to_hal_callback(&callback, event) {
        mark_dvr_callback_unhealthy(runtime, handle)?;
        return Err(HalError::callback_failed(
            context,
            format!("binder failure: {error:?}"),
        ));
    }
    Ok(true)
}

fn dvr_status_notifier_loop(
    runtime: SharedTunerRuntime,
    handle: AidlObjectHandle,
    cancel: Arc<AtomicBool>,
) {
    let mut last_event = match poll_dvr_status_snapshot(&runtime, handle) {
        Ok(snapshot)
            if snapshot.started
                && snapshot.callback_present
                && !snapshot.callback_unhealthy
                && snapshot.status_reporting_enabled =>
        {
            snapshot.event
        }
        _ => return,
    };

    loop {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let snapshot = match poll_dvr_status_snapshot(&runtime, handle) {
            Ok(snapshot) => snapshot,
            Err(_) => return,
        };
        if !snapshot.started
            || !snapshot.callback_present
            || snapshot.callback_unhealthy
            || !snapshot.status_reporting_enabled
        {
            return;
        }
        if snapshot.event != last_event {
            if let Some(event) = snapshot.event {
                if deliver_dvr_status_event(&runtime, handle, event, "IDvrCallback.poll_status")
                    .is_err()
                {
                    return;
                }
            }
            last_event = snapshot.event;
        }
        thread::park_timeout(Duration::from_millis(snapshot.interval_ms.max(1)));
    }
}

pub fn deliver_started_dvr_status(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let snapshot = poll_dvr_status_snapshot(runtime, handle)?;
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
    let _ = deliver_dvr_status_event(runtime, handle, event, "IDvrCallback.start_status")?;
    Ok(())
}

pub fn start_dvr_status_notifier(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    stop_dvr_status_notifier(handle)?;
    let snapshot = poll_dvr_status_snapshot(runtime, handle)?;
    if !snapshot.started
        || !snapshot.callback_present
        || snapshot.callback_unhealthy
        || !snapshot.status_reporting_enabled
        || !dvr_callback_is_available(handle)?
    {
        return Ok(());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    let thread_runtime = Arc::clone(runtime);
    let join = thread::Builder::new()
        .name(format!(
            "tuner-hal2-dvr-status-{}-{}",
            handle.object_id().0,
            handle.generation().0
        ))
        .spawn(move || dvr_status_notifier_loop(thread_runtime, handle, thread_cancel))
        .map_err(|error| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("failed to spawn DVR status notifier: {error}"),
            )
        })?;
    notifier_store()
        .lock()
        .map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier store lock poisoned while starting worker",
            )
        })?
        .insert(
            DvrStatusNotifierKey::new(handle),
            DvrStatusNotifier { cancel, join },
        );
    Ok(())
}

pub fn stop_dvr_status_notifier(handle: AidlObjectHandle) -> Result<(), HalError> {
    let notifier = notifier_store()
        .lock()
        .map_err(|_| {
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "DVR status notifier store lock poisoned while stopping worker",
            )
        })?
        .remove(&DvrStatusNotifierKey::new(handle));
    let Some(notifier) = notifier else {
        return Ok(());
    };
    notifier.cancel.store(true, Ordering::Relaxed);
    notifier.join.thread().unpark();
    notifier.join.join().map_err(|_| {
        HalError::cleanup_failed(
            "DVR status notifier join",
            "DVR status notifier thread panicked",
        )
    })?;
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
        AidlApi, AidlObjectGeneration, AidlObjectId, DvrConfigureKind, DvrConfigureRequest,
        DvrOpenKind, OpenDvrRequest,
    };
    use maleicacid_tuner_hal2_service_runtime::{CallbackHealthState, RuntimeOwnerRelation};
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::callback_store::{clear_owner_callbacks, retain_dvr_callback};

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

    fn build_started_playback_dvr_runtime() -> (SharedTunerRuntime, AidlObjectHandle) {
        let runtime = Arc::new(Mutex::new(
            maleicacid_tuner_hal2_service_runtime::TunerServiceRuntime::new(),
        ));
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
        (runtime, handle)
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
        let (runtime, handle) = build_started_playback_dvr_runtime();
        let state = Arc::new(CallbackState::default());
        let callback = new_test_callback(Arc::clone(&state));
        clear_owner_callbacks(handle).unwrap();
        retain_dvr_callback(handle, &callback).unwrap();
        runtime
            .lock()
            .unwrap()
            .callback_registry_mut()
            .record_registration(
                AidlObjectKind::Dvr,
                handle.object_id(),
                handle.generation(),
                AidlApi::DemuxOpenDvr,
            );

        deliver_started_dvr_status(&runtime, handle).unwrap();

        assert_eq!(
            *state.playback_statuses.lock().unwrap(),
            vec![PlaybackStatus::SPACE_FULL]
        );
        assert_eq!(
            runtime
                .lock()
                .unwrap()
                .callback_registry()
                .registration_for(
                    AidlObjectKind::Dvr,
                    handle.object_id(),
                    handle.generation(),
                    AidlApi::DemuxOpenDvr,
                )
                .unwrap()
                .health,
            CallbackHealthState::Registered
        );
        clear_owner_callbacks(handle).unwrap();
    }

    #[test]
    fn deliver_started_dvr_status_marks_unhealthy_on_binder_failure() {
        let (runtime, handle) = build_started_playback_dvr_runtime();
        let state = Arc::new(CallbackState::default());
        state.fail_delivery.store(true, Ordering::Relaxed);
        let callback = new_test_callback(Arc::clone(&state));
        clear_owner_callbacks(handle).unwrap();
        retain_dvr_callback(handle, &callback).unwrap();
        runtime
            .lock()
            .unwrap()
            .callback_registry_mut()
            .record_registration(
                AidlObjectKind::Dvr,
                handle.object_id(),
                handle.generation(),
                AidlApi::DemuxOpenDvr,
            );

        assert!(deliver_started_dvr_status(&runtime, handle).is_err());

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
                .callback_registry()
                .registration_for(
                    AidlObjectKind::Dvr,
                    handle.object_id(),
                    handle.generation(),
                    AidlApi::DemuxOpenDvr,
                )
                .unwrap()
                .health,
            CallbackHealthState::Unhealthy
        );
        clear_owner_callbacks(handle).unwrap();
    }
}
