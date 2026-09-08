use std::sync::Arc;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    FrontendEventType::FrontendEventType, FrontendScanMessage::FrontendScanMessage,
    FrontendScanMessageType::FrontendScanMessageType,
};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_service_runtime::{
    CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport,
    FrontendCallbackDeliveryDiagnosticRecord, FrontendScanNotification, FrontendScanNotifier,
    FrontendTuneNotification, FrontendTuneNotifier,
};

use crate::callback_store::FrontendCallbackGeneration;
use crate::object_handle::AidlObjectHandle;
use crate::service_context::SharedAidlServiceContext;

fn record_retired_callback_failure(
    context: &SharedAidlServiceContext,
    handle: AidlObjectHandle,
    generation: FrontendCallbackGeneration,
    primary: HalError,
) -> Result<(), HalError> {
    context.record_frontend_callback_delivery_failure_fallback(
        FrontendCallbackDeliveryDiagnosticRecord::RetiredRegistrationDelivery {
            object_id: handle.object_id(),
            generation: handle.generation(),
            callback_generation: generation.diagnostic_value(),
            error: primary,
        },
    )
}

fn frontend_scan_end_fallback_record(
    handle: AidlObjectHandle,
    frontend_id: i32,
    scan_generation: u64,
    phase: CallbackDeliveryFailurePhase,
    primary: HalError,
) -> FrontendCallbackDeliveryDiagnosticRecord {
    match phase {
        CallbackDeliveryFailurePhase::PostDeliveryCommit => {
            FrontendCallbackDeliveryDiagnosticRecord::scan_session_accounting(
                handle.object_id(),
                handle.generation(),
                frontend_id,
                scan_generation,
                primary,
            )
        }
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
    registration: Option<FrontendCallbackGeneration>,
) -> Result<(), HalError> {
    // 登録照合と失敗確定をruntime→store順で行い、外部診断への移行前には両方を解放する。
    let runtime = context.runtime();
    let runtime_lock = runtime.lock();
    let store = context.callback_store_lock().map_err(|error| {
        maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
            "callback結果の世代照合",
            primary.clone(),
            error.into_hal_error("callback store lock"),
        )
    })?;
    if let Some(registration) = registration {
        if !store.frontend_registration_matches(handle, registration) {
            drop(store);
            drop(runtime_lock);
            return record_retired_callback_failure(context, handle, registration, primary);
        }
    } else {
        drop(store);
        drop(runtime_lock);
        let record = FrontendCallbackDeliveryDiagnosticRecord::callback_artifact_lookup(
            handle.object_id(),
            handle.generation(),
            primary.clone(),
        );
        return match context.record_frontend_callback_delivery_failure_fallback(record) {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(
                maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                    "callback lookup診断",
                    primary,
                    cleanup,
                ),
            ),
        };
    }
    let result = match runtime_lock {
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
            drop(store);
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
        FrontendScanNotification::InputStreamIds(stream_ids) => (
            FrontendScanMessageType::INPUT_STREAM_IDS,
            FrontendScanMessage::InputStreamIds(stream_ids),
            "IFrontendCallback.onScanMessage(INPUT_STREAM_IDS)",
        ),
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
            let primary = HalError::callback_failed(method, "frontend callback is not registered");
            return finish_frontend_scan_end_delivery_failure(
                context,
                handle,
                frontend_id,
                generation,
                CallbackDeliveryFailurePhase::CallbackArtifactLookup,
                primary,
                None,
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
                None,
            );
        }
    };
    if let Err(err) = callback.callback().onScanMessage(message_type, &message) {
        let primary = HalError::callback_failed(method, format!("binder failure: {err:?}"));
        return finish_frontend_scan_end_delivery_failure(
            context,
            handle,
            frontend_id,
            generation,
            CallbackDeliveryFailurePhase::BinderDelivery,
            primary,
            Some(callback.generation()),
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
    registration: Option<FrontendCallbackGeneration>,
) -> Result<(), HalError> {
    let phase = if artifact_lookup {
        CallbackDeliveryFailurePhase::CallbackArtifactLookup
    } else {
        CallbackDeliveryFailurePhase::BinderDelivery
    };
    // 登録照合と失敗確定をruntime→store順で行い、外部診断への移行前には両方を解放する。
    let runtime = context.runtime();
    let runtime_lock = runtime.lock();
    let store = context.callback_store_lock().map_err(|error| {
        maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
            "callback結果の世代照合",
            primary.clone(),
            error.into_hal_error("callback store lock"),
        )
    })?;
    if let Some(registration) = registration {
        if !store.frontend_registration_matches(handle, registration) {
            drop(store);
            drop(runtime_lock);
            return record_retired_callback_failure(context, handle, registration, primary);
        }
    } else {
        drop(store);
        drop(runtime_lock);
        let record = FrontendCallbackDeliveryDiagnosticRecord::callback_artifact_lookup(
            handle.object_id(),
            handle.generation(),
            primary.clone(),
        );
        return match context.record_frontend_callback_delivery_failure_fallback(record) {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(
                maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(
                    "callback lookup診断",
                    primary,
                    cleanup,
                ),
            ),
        };
    }
    let result = match runtime_lock {
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
            drop(store);
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
    };
    result
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
        FrontendTuneNotification::LostLock => (
            FrontendEventType::LOST_LOCK,
            "IFrontendCallback.onEvent(LOST_LOCK)",
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
                None,
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
                None,
            );
        }
    };
    if let Err(error) = callback.callback().onEvent(event) {
        return finish_frontend_event_delivery_failure(
            context,
            handle,
            frontend_id,
            generation,
            false,
            HalError::callback_failed(method, format!("binder failure: {error:?}")),
            Some(callback.generation()),
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
