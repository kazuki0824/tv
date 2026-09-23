use maleicacid_tuner_hal2_common::{HalError, HalInvalidStateKind};
use maleicacid_tuner_hal2_domain_request::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
};
use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration, LedgerId};

use crate::object_close_txn::mark_object_close_cleanup_failed_cascade;
use crate::open_rollback::finish_open_rollback;
use crate::{
    CallbackArtifactCleanupResult, RuntimeObjectEntry, RuntimeObjectLifecycle,
    RuntimeOwnerRelation, TunerServiceRuntime,
};

fn primary_failure() -> HalError {
    HalError::invalid_state(
        HalInvalidStateKind::InvalidLifecycle,
        "failure injection primary",
    )
}

fn cleanup_failure() -> HalError {
    HalError::cleanup_failed("failure injection cleanup", "cleanup failed")
}

#[test]
fn runtime_poison_latches_critical_state_and_keeps_diagnostics_readable() {
    let runtime = std::sync::Mutex::new(TunerServiceRuntime::new());
    let failure_state = runtime.lock().unwrap().failure_state();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = runtime.lock().unwrap();
        panic!("service runtimeを汚染");
    }))
    .is_err());
    TunerServiceRuntime::mark_shared_service_critical(&runtime);
    assert_eq!(
        failure_state.snapshot(),
        crate::ServiceFailureSnapshot {
            service_critical: true,
            runtime_lock_poison_count: 1,
            diagnostic_counter_saturated: false,
        }
    );
    assert!(matches!(
        TunerServiceRuntime::lock_shared(&runtime, "retry"),
        Err(HalError::ServiceRuntimeLockPoisoned { operation: "retry" })
    ));
    assert!(runtime.is_poisoned());
    assert_eq!(failure_state.snapshot().runtime_lock_poison_count, 2);
}

#[test]
fn critical_service_cannot_be_reopened_by_boot_reset() {
    let mut runtime = TunerServiceRuntime::new();
    runtime.mark_service_critical();
    assert!(runtime
        .boot_from_probe_results_with_diagnostic_clear_result([])
        .1
        .is_err());
    assert_eq!(runtime.state(), crate::ServiceState::ServiceCritical);
    assert_eq!(
        runtime.failure_state().snapshot().runtime_lock_poison_count,
        0
    );
}

#[test]
fn filter_delivery_wake_preserves_runtime_poison_state() {
    let runtime = std::sync::Arc::new(std::sync::Mutex::new(TunerServiceRuntime::new()));
    let failure_state = runtime.lock().unwrap().failure_state();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = runtime.lock().unwrap();
        panic!("service runtimeを汚染");
    }))
    .is_err());
    assert!(matches!(
        crate::boot::notify_filter_delivery_change(&runtime),
        Err(HalError::ServiceRuntimeLockPoisoned { .. })
    ));
    assert!(failure_state.snapshot().service_critical);
    assert_eq!(failure_state.snapshot().runtime_lock_poison_count, 1);
}

#[test]
fn probe_io_failure_is_retained_without_advertising_a_frontend() {
    let mut runtime = TunerServiceRuntime::new();
    let error = HalError::Io {
        backend: "dvb",
        operation: "driver link読取り",
        path: Some("/sys/dvb/driver".into()),
        errno: Some(13),
        detail: maleicacid_tuner_hal2_common::HalErrorDetail::new("権限がありません"),
    };
    let outcome =
        runtime.boot_from_probe_results([crate::FrontendProbeOutcome::DeviceProbeFailed {
            backend: maleicacid_tuner_hal2_common::FrontendBackendKind::LinuxDvb,
            path: "/dev/dvb/adapter0/frontend0".into(),
            error: error.clone(),
        }]);
    assert_eq!(outcome, crate::ServiceBootOutcome::Degraded);
    assert!(runtime.query().frontend_ids().is_empty());
    assert!(runtime.startup_diagnostic_snapshot().records().iter().any(|record| matches!(record,
        crate::StartupDiagnosticRecord::DeviceProbeFailed { error: recorded, .. } if recorded == &error)));
}

#[test]
fn fmq_failure_and_rollback_keep_the_primary_delivery_kind() {
    use maleicacid_tuner_hal2_common::FmqFailureKind;
    use maleicacid_tuner_hal2_demux::DemuxRuntimeError;
    for kind in [
        FmqFailureKind::WriteFailed,
        FmqFailureKind::ShortWrite,
        FmqFailureKind::EventFlagWakeFailed,
    ] {
        let expected = HalError::FmqDeliveryFailed {
            kind,
            object_id: Some(17),
        };
        assert_eq!(
            crate::boot::demux_runtime_error_to_hal(DemuxRuntimeError::fmq_delivery_failure(
                17, kind
            )),
            expected
        );
        let rollback = maleicacid_tuner_hal2_demux::QueueRuntimeError {
            kind: maleicacid_tuner_hal2_demux::QueueRuntimeErrorKind::DataPathFailure,
            detail: "transaction解放中にDVR queue epochロックが汚染されました",
        };
        let failure = DemuxRuntimeError::fmq_delivery_rollback_failed(17, kind, rollback);
        assert!(matches!(failure.kind,
            maleicacid_tuner_hal2_demux::DemuxRuntimeErrorKind::FmqDeliveryRollbackFailed {
                delivery, rollback: recorded,
            } if delivery == kind && recorded == rollback));
        let composed = crate::boot::demux_runtime_error_to_hal(failure);
        assert_eq!(composed.primary_error(), &expected);
        assert!(matches!(
            composed.cleanup_error(),
            Some(HalError::CleanupFailed { .. })
        ));
        assert!(composed
            .cleanup_error()
            .unwrap()
            .to_string()
            .contains(rollback.detail));
    }
}

#[test]
fn open_rollback_composes_object_and_runtime_cleanup_failure() {
    let result = finish_open_rollback(
        Err(primary_failure()),
        || Err(cleanup_failure()),
        "failure injection open rollback",
    );

    let Err(error) = result else {
        panic!("expected composed rollback failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::InvalidState { .. }
    ));
    assert!(matches!(
        error.cleanup_error(),
        Some(HalError::CleanupFailed { .. })
    ));
}

#[test]
fn close_cleanup_failed_marking_keeps_mark_failure_as_cleanup_detail() {
    let mut runtime = TunerServiceRuntime::new();

    let result = mark_object_close_cleanup_failed_cascade(
        &mut runtime,
        AidlObjectId(93_001),
        AidlObjectGeneration(1),
        CleanupStep::UnregisterRuntime,
        "failure injection cleanup failed marking",
    );

    let Err(error) = result else {
        panic!("expected cleanup-failed marking failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CleanupFailed { .. }
    ));
    assert!(error.cleanup_error().is_some());
}

#[test]
fn public_close_runtime_unregister_missing_target_is_cleanup_failure() {
    let mut runtime = TunerServiceRuntime::new();
    let entry = RuntimeObjectEntry {
        object_kind: AidlObjectKind::Demux,
        object_id: AidlObjectId(93_002),
        generation: AidlObjectGeneration(1),
        ledger_id: LedgerId(93_002),
        ledger_generation: LedgerGeneration(1),
        owner: RuntimeOwnerRelation::Root,
        lifecycle: RuntimeObjectLifecycle::Closed,
    };

    let result = runtime.unregister_public_runtime_for_closed_aidl_entry(&entry);

    let Err(error) = result else {
        panic!("expected missing runtime cleanup failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CleanupFailed { .. }
    ));
}

#[test]
fn filter_delivery_failure_finish_use_case_owns_diagnostic_and_composition() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};

    let mut runtime = TunerServiceRuntime::new();
    let primary = HalError::callback_failed("IFilterCallback.onFilterEvent", "binder failure");
    let result =
        runtime.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::filter(
            AidlObjectId(94_001),
            AidlObjectGeneration(1),
            CallbackDeliveryFailurePhase::BinderDelivery,
            primary,
        ));

    let Err(error) = result else {
        panic!("expected service_runtime to return callback delivery failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CallbackFailed { .. }
    ));
    assert!(error.cleanup_error().is_some());
    assert!(!runtime.filter_callback_delivery_diagnostics().is_empty());
}

#[test]
fn dvr_delivery_failure_finish_use_case_records_diagnostic_and_composes_marking_failure() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
    use crate::diagnostics::DvrPostCommitNotificationPhase;

    let mut runtime = TunerServiceRuntime::new();
    let primary = HalError::callback_failed("IDvrCallback.onRecordStatus", "binder failure");
    let result =
        runtime.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::dvr(
            AidlObjectId(94_002),
            AidlObjectGeneration(1),
            CallbackDeliveryFailurePhase::BinderDelivery,
            DvrPostCommitNotificationPhase::InitialStatusDelivery,
            primary,
        ));

    let Err(error) = result else {
        panic!("expected service_runtime to return DVR callback delivery failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CallbackFailed { .. }
    ));
    assert!(error.cleanup_error().is_some());
    assert!(!runtime
        .dvr_post_commit_notification_diagnostics()
        .expect("DVR post-commit diagnostics snapshot should be available")
        .records()
        .is_empty());
}

fn record_runtime_callback_registration(
    runtime: &mut TunerServiceRuntime,
    owner_kind: AidlObjectKind,
    owner_id: AidlObjectId,
    owner_generation: AidlObjectGeneration,
    api: AidlApi,
) {
    runtime
        .register_aidl_object_for_runtime(
            owner_kind,
            owner_id,
            owner_generation,
            owner_id.0,
            RuntimeOwnerRelation::Root,
        )
        .expect("test owner object should be registered before callback registration");
    let outcome = runtime.record_callback_artifact_after_owner_ready_use_case(
        owner_kind,
        owner_id,
        owner_generation,
        api,
        Ok(()),
    );
    runtime
        .finish_callback_registration_after_artifact_result_use_case(outcome, None)
        .expect("runtime callback registration should be recorded in test");
}

#[test]
fn frontend_callback_death_clears_only_the_live_owner_registration() {
    let mut runtime = TunerServiceRuntime::new();
    let owner_id = AidlObjectId(94_030);
    let generation = AidlObjectGeneration(1);
    record_runtime_callback_registration(
        &mut runtime,
        AidlObjectKind::Frontend,
        owner_id,
        generation,
        AidlApi::FrontendSetCallback,
    );
    assert!(runtime.frontend_callback_delivery_ready(owner_id, generation));
    assert!(!runtime.frontend_callback_delivery_ready(owner_id, AidlObjectGeneration(2)));
    assert!(runtime
        .begin_frontend_callback_death_use_case(owner_id, AidlObjectGeneration(2))
        .is_err());
    assert!(runtime.frontend_callback_delivery_ready(owner_id, generation));
    let outcome = runtime
        .begin_frontend_callback_death_use_case(owner_id, generation)
        .unwrap();
    runtime
        .finish_owner_callback_cleanup_outcome(outcome, Ok(CallbackArtifactCleanupResult::Cleared))
        .unwrap();
    assert!(!runtime.frontend_callback_delivery_ready(owner_id, generation));
}

#[test]
fn owner_callback_cleanup_registry_missing_is_runtime_failure() {
    use crate::diagnostics::CallbackArtifactRuntimeSplitOutcome;

    let mut runtime = TunerServiceRuntime::new();
    let owner_id = AidlObjectId(94_020);
    let owner_generation = AidlObjectGeneration(1);
    let command = runtime.plan_owner_callback_cleanup_artifact_command(
        AidlObjectKind::Frontend,
        owner_id,
        owner_generation,
        Some(AidlApi::FrontendSetCallback),
        "failure injection owner callback cleanup",
    );

    let result = runtime.finish_owner_callback_cleanup_use_case(
        command,
        Ok(()),
        Ok(CallbackArtifactCleanupResult::Cleared),
    );

    let Err(error) = result else {
        panic!("expected runtime registry missing to fail cleanup finish");
    };
    assert!(matches!(error.primary_error(), HalError::Internal { .. }));
    assert!(runtime
        .callback_artifact_runtime_split_diagnostics()
        .expect("callback artifact runtime split diagnostics snapshot should be available")
        .records()
        .iter()
        .any(
            |record| record.outcome == CallbackArtifactRuntimeSplitOutcome::RuntimeRegistryMissing
        ));
}

#[test]
fn owner_callback_cleanup_marking_missing_is_composed_cleanup_failure() {
    use crate::diagnostics::CallbackArtifactRuntimeSplitOutcome;

    let mut runtime = TunerServiceRuntime::new();
    let owner_id = AidlObjectId(94_021);
    let owner_generation = AidlObjectGeneration(1);
    let command = runtime.plan_owner_callback_cleanup_artifact_command(
        AidlObjectKind::Frontend,
        owner_id,
        owner_generation,
        Some(AidlApi::FrontendSetCallback),
        "failure injection owner callback cleanup",
    );

    let result =
        runtime.finish_owner_callback_cleanup_use_case(command, Ok(()), Err(cleanup_failure()));

    let Err(error) = result else {
        panic!("expected unhealthy marking failure to be composed");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CleanupFailed { .. }
    ));
    assert!(error.cleanup_error().is_some());
    assert!(runtime
        .callback_artifact_runtime_split_diagnostics()
        .expect("callback artifact runtime split diagnostics snapshot should be available")
        .records()
        .iter()
        .any(
            |record| record.outcome == CallbackArtifactRuntimeSplitOutcome::RuntimeRegistryMissing
        ));
}

#[test]
fn filter_callback_artifact_lookup_failure_records_diagnostic_without_cleanup_composition() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};

    let mut runtime = TunerServiceRuntime::new();
    let owner_id = AidlObjectId(94_003);
    let owner_generation = AidlObjectGeneration(1);
    record_runtime_callback_registration(
        &mut runtime,
        AidlObjectKind::Filter,
        owner_id,
        owner_generation,
        AidlApi::DemuxOpenFilter,
    );

    let primary = HalError::callback_failed("IFilterCallback.lookup", "callback artifact missing");
    let result =
        runtime.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::filter(
            owner_id,
            owner_generation,
            CallbackDeliveryFailurePhase::CallbackArtifactLookup,
            primary,
        ));

    let Err(error) = result else {
        panic!("expected callback artifact lookup failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CallbackFailed { .. }
    ));
    assert!(error.cleanup_error().is_none());
    assert!(!runtime.filter_callback_delivery_diagnostics().is_empty());
}

#[test]
fn filter_binder_delivery_failure_composes_runtime_callback_accounting() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};

    let mut runtime = TunerServiceRuntime::new();
    let owner_id = AidlObjectId(94_004);
    let owner_generation = AidlObjectGeneration(1);
    record_runtime_callback_registration(
        &mut runtime,
        AidlObjectKind::Filter,
        owner_id,
        owner_generation,
        AidlApi::DemuxOpenFilter,
    );

    let primary = HalError::callback_failed("IFilterCallback.onFilterEvent", "binder failure");
    let result =
        runtime.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::filter(
            owner_id,
            owner_generation,
            CallbackDeliveryFailurePhase::BinderDelivery,
            primary,
        ));

    let Err(error) = result else {
        panic!("expected binder delivery failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CallbackFailed { .. }
    ));
    assert!(error.cleanup_error().is_some());
    assert!(!runtime.filter_callback_delivery_diagnostics().is_empty());
}

#[test]
fn dvr_callback_artifact_lookup_failure_records_diagnostic_without_cleanup_composition() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
    use crate::diagnostics::DvrPostCommitNotificationPhase;
    use maleicacid_tuner_hal2_domain_request::AidlApi;

    let mut runtime = TunerServiceRuntime::new();
    let owner_id = AidlObjectId(94_005);
    let owner_generation = AidlObjectGeneration(1);
    record_runtime_callback_registration(
        &mut runtime,
        AidlObjectKind::Dvr,
        owner_id,
        owner_generation,
        AidlApi::DemuxOpenDvr,
    );

    let primary = HalError::callback_failed("IDvrCallback.lookup", "callback artifact missing");
    let result =
        runtime.finish_callback_delivery_failure_use_case(CallbackDeliveryFailureReport::dvr(
            owner_id,
            owner_generation,
            CallbackDeliveryFailurePhase::CallbackArtifactLookup,
            DvrPostCommitNotificationPhase::InitialStatusDelivery,
            primary,
        ));

    let Err(error) = result else {
        panic!("expected DVR callback artifact lookup failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CallbackFailed { .. }
    ));
    assert!(error.cleanup_error().is_none());
    assert!(!runtime
        .dvr_post_commit_notification_diagnostics()
        .expect("DVR post-commit diagnostics snapshot should be available")
        .records()
        .is_empty());
}

#[test]
fn frontend_scan_end_delivery_failure_composition_is_owned_by_service_runtime() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};

    let mut runtime = TunerServiceRuntime::new();
    let primary =
        HalError::callback_failed("IFrontendCallback.onScanMessage(END)", "binder failure");
    let result = runtime.finish_callback_delivery_failure_use_case(
        CallbackDeliveryFailureReport::frontend_scan_end(
            AidlObjectId(94_006),
            AidlObjectGeneration(1),
            94_006,
            1,
            CallbackDeliveryFailurePhase::BinderDelivery,
            primary,
        ),
    );

    let Err(error) = result else {
        panic!("expected frontend scan end delivery failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CallbackFailed { .. }
    ));
    assert!(error.cleanup_error().is_some());
    let diagnostics = runtime.frontend_callback_delivery_diagnostics();
    assert_eq!(diagnostics.len(), 3);
    assert_eq!(
        diagnostics[0].phase(),
        crate::FrontendCallbackDeliveryDiagnosticPhase::ScanEndDelivery
    );
    assert_eq!(
        diagnostics[1].phase(),
        crate::FrontendCallbackDeliveryDiagnosticPhase::ScanSessionAccounting
    );
    assert_eq!(
        diagnostics[2].phase(),
        crate::FrontendCallbackDeliveryDiagnosticPhase::CallbackRegistryAccounting
    );
}

#[test]
fn frontend_event_delivery_failure_marks_registered_callback_unhealthy() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
    use maleicacid_tuner_hal2_domain_request::AidlApi;

    let mut runtime = TunerServiceRuntime::new();
    let owner_id = AidlObjectId(94_008);
    let owner_generation = AidlObjectGeneration(1);
    record_runtime_callback_registration(
        &mut runtime,
        AidlObjectKind::Frontend,
        owner_id,
        owner_generation,
        AidlApi::FrontendSetCallback,
    );
    let primary = HalError::callback_failed("IFrontendCallback.onEvent", "binder failure");
    let result = runtime.finish_callback_delivery_failure_use_case(
        CallbackDeliveryFailureReport::frontend_event(
            owner_id,
            owner_generation,
            94_008,
            7,
            CallbackDeliveryFailurePhase::BinderDelivery,
            primary,
        ),
    );

    let Err(error) = result else {
        panic!("expected frontend event delivery failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CallbackFailed { .. }
    ));
    assert!(error.cleanup_error().is_none());
    let diagnostics = runtime.frontend_callback_delivery_diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].phase(),
        crate::FrontendCallbackDeliveryDiagnosticPhase::FrontendEventDelivery
    );
}

#[test]
fn frontend_scan_end_artifact_lookup_failure_records_lookup_diagnostic_only() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
    use maleicacid_tuner_hal2_domain_request::AidlApi;

    let mut runtime = TunerServiceRuntime::new();
    let owner_id = AidlObjectId(94_007);
    let owner_generation = AidlObjectGeneration(1);
    record_runtime_callback_registration(
        &mut runtime,
        AidlObjectKind::Frontend,
        owner_id,
        owner_generation,
        AidlApi::FrontendSetCallback,
    );

    let primary = HalError::callback_failed(
        "IFrontendCallback.lookup",
        "frontend scan-end callback artifact missing",
    );
    let result = runtime.finish_callback_delivery_failure_use_case(
        CallbackDeliveryFailureReport::frontend_scan_end(
            owner_id,
            owner_generation,
            94_007,
            1,
            CallbackDeliveryFailurePhase::CallbackArtifactLookup,
            primary,
        ),
    );

    let Err(error) = result else {
        panic!("expected frontend artifact lookup failure");
    };
    assert!(matches!(
        error.primary_error(),
        HalError::CallbackFailed { .. }
    ));
    assert!(error.cleanup_error().is_none());
    let diagnostics = runtime.frontend_callback_delivery_diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].phase(),
        crate::FrontendCallbackDeliveryDiagnosticPhase::CallbackArtifactLookup
    );
}
