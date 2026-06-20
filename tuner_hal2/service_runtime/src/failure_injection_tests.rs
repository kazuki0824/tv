use maleicacid_tuner_hal2_common::{HalError, HalInvalidStateKind};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};
use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerGeneration, LedgerId};

use crate::object_close_txn::mark_object_close_cleanup_failed_cascade;
use crate::open_rollback::finish_open_rollback;
use crate::{
    RuntimeObjectEntry, RuntimeObjectLifecycle, RuntimeOwnerRelation, TunerServiceRuntime,
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
