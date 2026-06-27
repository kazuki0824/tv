use std::sync::Arc;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    FrontendScanMessage::FrontendScanMessage, FrontendScanMessageType::FrontendScanMessageType,
};
use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlObjectKind};
use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};
use maleicacid_tuner_hal2_service_runtime::{CallbackRegistryUpdate, FrontendScanEndNotifier};

use crate::object_handle::AidlObjectHandle;
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};

fn mark_frontend_callback_unhealthy(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> Result<(), HalError> {
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while marking frontend callback unhealthy",
        )
    })?;
    match guard.mark_callback_registration_unhealthy(
        AidlObjectKind::Frontend,
        handle.object_id(),
        handle.generation(),
        AidlApi::FrontendSetCallback,
    ) {
        CallbackRegistryUpdate::Updated => Ok(()),
        CallbackRegistryUpdate::Missing => Err(HalError::internal(
            HalInternalKind::InvariantViolation,
            "frontend callback registry entry missing while marking unhealthy",
        )),
    }
}

fn mark_scan_end_session_callback_failed(
    runtime: &SharedTunerRuntime,
    frontend_id: i32,
    generation: u64,
) -> Result<(), HalError> {
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while marking scan callback failure",
        )
    })?;
    guard.mark_frontend_scan_session_callback_failed(frontend_id, generation)
}

fn mark_scan_end_registered_callback_delivery_failed(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    frontend_id: i32,
    generation: u64,
) -> Result<(), HalError> {
    mark_scan_end_session_callback_failed(runtime, frontend_id, generation)?;
    mark_frontend_callback_unhealthy(runtime, handle)
}

fn deliver_scan_end_callback(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    frontend_id: i32,
    generation: u64,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    let callback = match context.frontend_callback_for_owner(handle) {
        Ok(Some(callback)) => callback,
        Ok(None) => {
            if let Err(mark_error) =
                mark_scan_end_session_callback_failed(&runtime, frontend_id, generation)
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
                mark_scan_end_session_callback_failed(&runtime, frontend_id, generation)
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
        let primary = HalError::callback_failed(
            "IFrontendCallback.onScanMessage(END)",
            format!("binder failure: {err:?}"),
        );
        return match mark_scan_end_registered_callback_delivery_failed(
            &runtime,
            handle,
            frontend_id,
            generation,
        ) {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(compose_primary_cleanup_failure(
                "frontend scan END callback delivery and unhealthy marking failed",
                primary,
                cleanup,
            )),
        };
    }
    Ok(())
}

pub fn scan_end_notifier(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> FrontendScanEndNotifier {
    Arc::new(move |frontend_id, generation| {
        deliver_scan_end_callback(&context, handle, frontend_id, generation)
    })
}
