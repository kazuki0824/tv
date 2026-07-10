use std::sync::Arc;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    FrontendScanMessage::FrontendScanMessage, FrontendScanMessageType::FrontendScanMessageType,
};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    FrontendCallbackDeliveryDiagnosticRecord, FrontendScanEndNotifier,
};

use crate::object_handle::AidlObjectHandle;
use crate::service_context::SharedAidlServiceContext;

fn frontend_scan_end_fallback_record(
    handle: AidlObjectHandle,
    frontend_id: i32,
    scan_generation: u64,
    phase: CallbackDeliveryFailurePhase,
    primary: HalError,
) -> FrontendCallbackDeliveryDiagnosticRecord {
    match phase {
        CallbackDeliveryFailurePhase::CallbackArtifactLookup
        | CallbackDeliveryFailurePhase::RuntimePolicySkip
        | CallbackDeliveryFailurePhase::NotifierCleanup
        | CallbackDeliveryFailurePhase::NotifierPreflight => {
            FrontendCallbackDeliveryDiagnosticRecord::callback_artifact_lookup(
                handle.object_id(),
                handle.generation(),
                primary,
            )
        }
        CallbackDeliveryFailurePhase::EventConversion
        | CallbackDeliveryFailurePhase::BinderDelivery
        | CallbackDeliveryFailurePhase::ScanEndDelivery
        | CallbackDeliveryFailurePhase::PostCommitNotification
        | CallbackDeliveryFailurePhase::NotifierTerminal => {
            FrontendCallbackDeliveryDiagnosticRecord::scan_end_delivery(
                handle.object_id(),
                handle.generation(),
                frontend_id,
                scan_generation,
                primary,
            )
        }
    }
}

fn finish_frontend_scan_end_delivery_failure(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    frontend_id: i32,
    scan_generation: u64,
    phase: CallbackDeliveryFailurePhase,
    primary: HalError,
) -> Result<(), HalError> {
    let runtime = context.runtime();
    match runtime.lock() {
        Ok(mut guard) => guard.finish_callback_delivery_failure_use_case(
            CallbackDeliveryFailureReport::frontend_scan_end(
                handle.object_id(),
                handle.generation(),
                frontend_id,
                scan_generation,
                phase,
                primary,
            ),
        ),
        Err(_) => {
            let record = frontend_scan_end_fallback_record(
                handle,
                frontend_id,
                scan_generation,
                phase,
                primary.clone(),
            );
            match context.record_frontend_callback_delivery_failure_fallback(record) {
                Ok(()) => Err(primary),
                Err(record_error) => Err(
                    maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                        "frontend callback delivery fallback diagnostic record failed",
                        primary,
                        record_error,
                    ),
                ),
            }
        }
    }
}

fn deliver_scan_end_callback(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    frontend_id: i32,
    generation: u64,
) -> Result<(), HalError> {
    let callback = match context.frontend_callback_for_owner(handle) {
        Ok(Some(callback)) => callback,
        Ok(None) => {
            let primary = HalError::callback_failed(
                "IFrontendCallback.onScanMessage(END)",
                "frontend callback is not registered",
            );
            return finish_frontend_scan_end_delivery_failure(
                context,
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
                context,
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
            context,
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
