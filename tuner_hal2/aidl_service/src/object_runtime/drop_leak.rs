use super::{
    callback_store_error_to_hal, lnb_public_id_for_live_object_result, quarantine_object_cascade,
    status_from_hal_error, status_unknown_error, unregister_quarantined_public_runtime_entries,
    AidlObjectHandle, BinderResult, CallbackRegistryUpdate, FirstErrorCollector, HalError,
    TunerServiceRuntime,
};
use crate::callback_store::AidlCallbackStoreError;
use crate::dvr_callback_delivery::stop_dvr_status_notifier;
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};
use maleicacid_tuner_hal2_binder_adapter::AidlObjectKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropLeakDomainAction {
    None,
    RecordLnbDropLeak,
}

fn lnb_public_id_for_drop(
    runtime: &TunerServiceRuntime,
    handle: AidlObjectHandle,
) -> Result<Option<i32>, HalError> {
    if handle.object_kind() != AidlObjectKind::Lnb {
        return Ok(None);
    }
    lnb_public_id_for_live_object_result(runtime, handle.object_id(), handle.generation()).map(Some)
}

fn record_domain_drop_leak(
    runtime: &mut TunerServiceRuntime,
    handle: AidlObjectHandle,
    action: DropLeakDomainAction,
) -> Result<(), HalError> {
    match action {
        DropLeakDomainAction::None => Ok(()),
        DropLeakDomainAction::RecordLnbDropLeak => {
            let Some(lnb_id) = lnb_public_id_for_drop(runtime, handle)? else {
                return Err(HalError::cleanup_failed(
                    "drop leak LNB domain record",
                    "drop leak target is not an LNB object",
                ));
            };
            runtime.record_lnb_drop_leak(lnb_id)
        }
    }
}

struct DropLeakOwnerArtifactCleanup {
    handle: AidlObjectHandle,
    callback_store_clear: Result<usize, AidlCallbackStoreError>,
    dvr_notifier_stop: Option<Result<(), HalError>>,
}

fn drop_leak_target_handles(
    root_handle: AidlObjectHandle,
    entries: &[maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry],
) -> Vec<AidlObjectHandle> {
    let mut handles = entries
        .iter()
        .map(|entry| AidlObjectHandle::new(entry.object_kind, entry.object_id, entry.generation))
        .collect::<Vec<_>>();
    if !handles.iter().any(|handle| {
        handle.object_id() == root_handle.object_id()
            && handle.generation() == root_handle.generation()
    }) {
        handles.push(root_handle);
    }
    handles
}

fn clear_drop_leak_owner_artifacts(
    context: &SharedAidlServiceContext,
    root_handle: AidlObjectHandle,
    entries: &[maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry],
) -> Vec<DropLeakOwnerArtifactCleanup> {
    drop_leak_target_handles(root_handle, entries)
        .into_iter()
        .map(|handle| {
            let callback_store_clear = context.clear_owner_callbacks(handle);
            let dvr_notifier_stop = (handle.object_kind() == AidlObjectKind::Dvr)
                .then(|| stop_dvr_status_notifier(context, handle));
            DropLeakOwnerArtifactCleanup {
                handle,
                callback_store_clear,
                dvr_notifier_stop,
            }
        })
        .collect()
}

fn record_drop_leak_owner_artifact_cleanup(
    runtime: &mut TunerServiceRuntime,
    cleanup: DropLeakOwnerArtifactCleanup,
    error_collector: &mut FirstErrorCollector<HalError>,
) {
    match cleanup.callback_store_clear {
        Ok(removed) => {
            match runtime.clear_callback_registration_owner(
                cleanup.handle.object_id(),
                cleanup.handle.generation(),
            ) {
                CallbackRegistryUpdate::Updated => {}
                CallbackRegistryUpdate::Missing if removed == 0 => {}
                CallbackRegistryUpdate::Missing => {
                    error_collector.push_error(HalError::cleanup_failed(
                        "drop leak callback registry clear",
                        "callback registry owner missing during clear",
                    ));
                }
            }
        }
        Err(error) => {
            error_collector.push_error(callback_store_error_to_hal(
                error,
                "drop leak callback store clear failed",
            ));
            match runtime.mark_callback_registration_owner_unhealthy(
                cleanup.handle.object_id(),
                cleanup.handle.generation(),
            ) {
                CallbackRegistryUpdate::Updated => {}
                CallbackRegistryUpdate::Missing => {
                    error_collector.push_error(HalError::cleanup_failed(
                        "drop leak callback registry unhealthy marking",
                        "callback registry owner missing while marking unhealthy",
                    ));
                }
            }
        }
    }

    if let Some(Err(error)) = cleanup.dvr_notifier_stop {
        error_collector.push_error(error);
    }
}

pub fn drop_leak_object(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    action: DropLeakDomainAction,
) -> BinderResult<()> {
    let runtime_handle = context.runtime();
    let (domain_record, quarantine_result) = {
        let mut runtime = runtime_handle.lock().map_err(|_| {
            status_unknown_error("service runtime lock poisoned during drop leak quarantine")
        })?;
        let domain_record = record_domain_drop_leak(&mut runtime, handle, action);
        let quarantine_result =
            quarantine_object_cascade(&mut runtime, handle.object_id(), handle.generation());
        (domain_record, quarantine_result)
    };

    let mut error_collector = FirstErrorCollector::new();
    if let Err(error) = domain_record {
        error_collector.push_error(error);
    }

    let quarantine_entries = match quarantine_result {
        Ok(entries) => entries,
        Err(error) => {
            error_collector.push_error(error);
            Vec::new()
        }
    };

    let owner_artifact_cleanup =
        clear_drop_leak_owner_artifacts(context, handle, &quarantine_entries);

    let mut runtime = runtime_handle.lock().map_err(|_| {
        status_unknown_error("service runtime lock poisoned during drop leak terminalization")
    })?;
    for cleanup in owner_artifact_cleanup {
        record_drop_leak_owner_artifact_cleanup(&mut runtime, cleanup, &mut error_collector);
    }
    if !quarantine_entries.is_empty() {
        error_collector.push_result(unregister_quarantined_public_runtime_entries(
            &mut runtime,
            &quarantine_entries,
        ));
    }

    match error_collector.into_result() {
        Err(error) => Err(status_from_hal_error(error)),
        Ok(()) => Ok(()),
    }
}

pub fn drop_leak_object_from_drop(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    action: DropLeakDomainAction,
) {
    if let Err(status) = drop_leak_object(context, handle, action) {
        context.record_drop_leak_error(handle, &status);
    }
}
