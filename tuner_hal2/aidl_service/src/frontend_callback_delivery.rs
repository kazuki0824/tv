use std::sync::Arc;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    FrontendScanMessage::FrontendScanMessage, FrontendScanMessageType::FrontendScanMessageType,
};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};
use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport, FrontendScanEndNotifier,
};

use crate::object_handle::AidlObjectHandle;
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};

fn finish_frontend_scan_end_delivery_failure(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    frontend_id: i32,
    scan_generation: u64,
    phase: CallbackDeliveryFailurePhase,
    primary: HalError,
) -> Result<(), HalError> {
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned while finishing frontend callback delivery failure",
        )
    })?;
    guard.finish_callback_delivery_failure_use_case(
        CallbackDeliveryFailureReport::frontend_scan_end(
            handle.object_id(),
            handle.generation(),
            frontend_id,
            scan_generation,
            phase,
            primary,
        ),
    )
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
            let primary = HalError::callback_failed(
                "IFrontendCallback.onScanMessage(END)",
                "frontend callback is not registered",
            );
            return finish_frontend_scan_end_delivery_failure(
                &runtime,
                handle,
                frontend_id,
                generation,
                CallbackDeliveryFailurePhase::CallbackArtifactLookup,
                primary,
            );
        }
        Err(_) => {
            let primary = HalError::callback_failed(
                "IFrontendCallback.onScanMessage(END)",
                "callback store lock poisoned",
            );
            return finish_frontend_scan_end_delivery_failure(
                &runtime,
                handle,
                frontend_id,
                generation,
                CallbackDeliveryFailurePhase::CallbackArtifactLookup,
                primary,
            );
        }
    };
    let message = FrontendScanMessage::IsEnd(true);
    if let Err(err) = callback.onScanMessage(FrontendScanMessageType::END, &message) {
        let primary = HalError::callback_failed(
            "IFrontendCallback.onScanMessage(END)",
            format!("binder failure: {err:?}"),
        );
        return finish_frontend_scan_end_delivery_failure(
            &runtime,
            handle,
            frontend_id,
            generation,
            CallbackDeliveryFailurePhase::BinderDelivery,
            primary,
        );
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
