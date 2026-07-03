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
        .iter()
        .any(
            |record| record.outcome == CallbackArtifactRuntimeSplitOutcome::RuntimeRegistryMissing
        ));
}

#[test]
fn filter_callback_artifact_lookup_failure_records_diagnostic_without_unhealthy_marking() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
    use crate::CallbackHealthState;

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
    assert_eq!(
        runtime.callback_registration_health(
            AidlObjectKind::Filter,
            owner_id,
            owner_generation,
            AidlApi::DemuxOpenFilter,
        ),
        Some(CallbackHealthState::Registered)
    );
    assert!(!runtime.filter_callback_delivery_diagnostics().is_empty());
}

#[test]
fn filter_binder_delivery_failure_marks_runtime_callback_unhealthy_when_registry_exists() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
    use crate::CallbackHealthState;

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
    assert_eq!(
        runtime.callback_registration_health(
            AidlObjectKind::Filter,
            owner_id,
            owner_generation,
            AidlApi::DemuxOpenFilter,
        ),
        Some(CallbackHealthState::Unhealthy)
    );
    assert!(!runtime.filter_callback_delivery_diagnostics().is_empty());
}

#[test]
fn dvr_callback_artifact_lookup_failure_does_not_mark_registry_unhealthy() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
    use crate::diagnostics::DvrPostCommitNotificationPhase;
    use crate::CallbackHealthState;
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
    assert_eq!(
        runtime.callback_registration_health(
            AidlObjectKind::Dvr,
            owner_id,
            owner_generation,
            AidlApi::DemuxOpenDvr,
        ),
        Some(CallbackHealthState::Registered)
    );
    assert!(!runtime
        .dvr_post_commit_notification_diagnostics()
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
        diagnostics[0].phase,
        crate::FrontendCallbackDeliveryDiagnosticPhase::ScanEndDelivery
    );
    assert_eq!(
        diagnostics[1].phase,
        crate::FrontendCallbackDeliveryDiagnosticPhase::ScanSessionAccounting
    );
    assert_eq!(
        diagnostics[2].phase,
        crate::FrontendCallbackDeliveryDiagnosticPhase::CallbackRegistryAccounting
    );
}

#[test]
fn frontend_scan_end_artifact_lookup_failure_does_not_mark_runtime_callback_unhealthy() {
    use crate::boot::{CallbackDeliveryFailurePhase, CallbackDeliveryFailureReport};
    use crate::CallbackHealthState;
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
        diagnostics[0].phase,
        crate::FrontendCallbackDeliveryDiagnosticPhase::CallbackArtifactLookup
    );
    assert_eq!(
        runtime.callback_registration_health(
            AidlObjectKind::Frontend,
            owner_id,
            owner_generation,
            AidlApi::FrontendSetCallback,
        ),
        Some(CallbackHealthState::Registered)
    );
}
