use std::sync::Arc;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    FrontendEventType::FrontendEventType,
    FrontendScanMessage::FrontendScanMessage, FrontendScanMessageType::FrontendScanMessageType,
};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    FrontendCallbackDeliveryDiagnosticRecord, FrontendScanNotification, FrontendScanNotifier,
    FrontendTuneNotification, FrontendTuneNotifier,
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
    let result = match runtime.lock() {
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
    };
    result
}

fn deliver_scan_callback(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    frontend_id: i32,
    generation: u64,
    notification: FrontendScanNotification,
) -> Result<(), HalError> {
    let (message_type, message, method) = match notification {
        FrontendScanNotification::Locked => (
            FrontendScanMessageType::LOCKED,
            FrontendScanMessage::IsLocked(true),
            "IFrontendCallback.onScanMessage(LOCKED)",
        ),
        FrontendScanNotification::End => (
            FrontendScanMessageType::END,
            FrontendScanMessage::IsEnd(true),
            "IFrontendCallback.onScanMessage(END)",
        ),
    };
    let callback = match context.frontend_callback_for_owner(handle) {
        Ok(Some(callback)) => callback,
        Ok(None) => {
            let primary =
                HalError::callback_failed(method, "frontend callback is not registered");
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
            let primary = HalError::callback_failed(method, "callback store lock poisoned");
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
    if let Err(err) = callback.onScanMessage(message_type, &message) {
        let primary = HalError::callback_failed(method, format!("binder failure: {err:?}"));
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

pub fn scan_notifier(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> FrontendScanNotifier {
    Arc::new(move |frontend_id, generation, notification| {
        deliver_scan_callback(&context, handle, frontend_id, generation, notification)
    })
}

fn finish_frontend_event_delivery_failure(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    frontend_id: i32,
    frontend_generation: u64,
    artifact_lookup: bool,
    primary: HalError,
) -> Result<(), HalError> {
    let phase = if artifact_lookup {
        CallbackDeliveryFailurePhase::CallbackArtifactLookup
    } else {
        CallbackDeliveryFailurePhase::BinderDelivery
    };
    let runtime = context.runtime();
    match runtime.lock() {
        Ok(mut guard) => guard.finish_callback_delivery_failure_use_case(
            CallbackDeliveryFailureReport::frontend_event(
                handle.object_id(),
                handle.generation(),
                frontend_id,
                frontend_generation,
                phase,
                primary,
            ),
        ),
        Err(_) => {
            let record = if artifact_lookup {
                FrontendCallbackDeliveryDiagnosticRecord::callback_artifact_lookup(
                    handle.object_id(),
                    handle.generation(),
                    primary.clone(),
                )
            } else {
                FrontendCallbackDeliveryDiagnosticRecord::frontend_event_delivery(
                    handle.object_id(),
                    handle.generation(),
                    frontend_id,
                    frontend_generation,
                    primary.clone(),
                )
            };
            match context.record_frontend_callback_delivery_failure_fallback(record) {
                Ok(()) => Err(primary),
                Err(record_error) => Err(
                    maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                        "frontend event callback failure diagnostic record failed",
                        primary,
                        record_error,
                    ),
                ),
            }
        }
    }
}

fn deliver_tune_event_callback(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    frontend_id: i32,
    generation: u64,
    notification: FrontendTuneNotification,
) -> Result<(), HalError> {
    let (event, method) = match notification {
        FrontendTuneNotification::Locked => (
            FrontendEventType::LOCKED,
            "IFrontendCallback.onEvent(LOCKED)",
        ),
        FrontendTuneNotification::NoSignal => (
            FrontendEventType::NO_SIGNAL,
            "IFrontendCallback.onEvent(NO_SIGNAL)",
        ),
    };
    let callback = match context.frontend_callback_for_owner(handle) {
        Ok(Some(callback)) => callback,
        Ok(None) => {
            return finish_frontend_event_delivery_failure(
                context,
                handle,
                frontend_id,
                generation,
                true,
                HalError::callback_failed(method, "frontend callback is not registered"),
            );
        }
        Err(_) => {
            return finish_frontend_event_delivery_failure(
                context,
                handle,
                frontend_id,
                generation,
                true,
                HalError::callback_failed(method, "callback store lock poisoned"),
            );
        }
    };
    if let Err(error) = callback.onEvent(event) {
        return finish_frontend_event_delivery_failure(
            context,
            handle,
            frontend_id,
            generation,
            false,
            HalError::callback_failed(method, format!("binder failure: {error:?}")),
        );
    }
    Ok(())
}

pub fn tune_notifier(
    context: SharedAidlServiceContext,
    handle: AidlObjectHandle,
) -> FrontendTuneNotifier {
    Arc::new(move |frontend_id, generation, notification| {
        deliver_tune_event_callback(&context, handle, frontend_id, generation, notification)
    })
}
