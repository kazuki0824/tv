use std::sync::Arc;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    FrontendScanMessage::FrontendScanMessage,
    FrontendScanMessageType::FrontendScanMessageType,
};
use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlObjectKind};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_service_runtime::frontend_worker_txn::FrontendScanEndNotifier;

use crate::callback_store::frontend_callback_for_owner;
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::SharedTunerRuntime;

fn mark_frontend_callback_unhealthy(runtime: &SharedTunerRuntime, handle: AidlObjectHandle) {
    if let Ok(mut guard) = runtime.lock() {
        guard.callback_registry_mut().mark_unhealthy(
            AidlObjectKind::Frontend,
            handle.object_id(),
            handle.generation(),
            AidlApi::FrontendSetCallback,
        );
    }
}

fn mark_scan_end_callback_failed(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    frontend_id: i32,
    generation: u64,
) -> Result<(), HalError> {
    mark_frontend_callback_unhealthy(runtime, handle);
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while marking scan callback failure",
        )
    })?;
    guard.mark_frontend_scan_session_callback_failed(frontend_id, generation)
}

fn deliver_scan_end_callback(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    frontend_id: i32,
    generation: u64,
) -> Result<(), HalError> {
    let callback = match frontend_callback_for_owner(handle) {
        Ok(Some(callback)) => callback,
        Ok(None) => {
            if let Err(mark_error) =
                mark_scan_end_callback_failed(runtime, handle, frontend_id, generation)
            {
                return Err(mark_error);
            }
            return Err(HalError::callback_failed(
                "IFrontendCallback.onScanMessage(END)",
                "frontend callback is not registered",
            ));
        }
        Err(_) => {
            if let Err(mark_error) =
                mark_scan_end_callback_failed(runtime, handle, frontend_id, generation)
            {
                return Err(mark_error);
            }
            return Err(HalError::callback_failed(
                "IFrontendCallback.onScanMessage(END)",
                "callback store lock poisoned",
            ));
        }
    };
    let message = FrontendScanMessage::IsEnd(true);
    if let Err(err) = callback.onScanMessage(FrontendScanMessageType::END, &message) {
        if let Err(mark_error) =
            mark_scan_end_callback_failed(runtime, handle, frontend_id, generation)
        {
            return Err(mark_error);
        }
        return Err(HalError::callback_failed(
            "IFrontendCallback.onScanMessage(END)",
            format!("binder failure: {err:?}"),
        ));
    }
    Ok(())
}

pub fn scan_end_notifier(
    runtime: SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> FrontendScanEndNotifier {
    Arc::new(move |frontend_id, generation| {
        deliver_scan_end_callback(&runtime, handle, frontend_id, generation)
    })
}
