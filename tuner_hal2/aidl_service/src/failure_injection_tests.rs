use std::sync::{Arc, Mutex};

use binder::Result as BinderResult;
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlMethodCall, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
use maleicacid_tuner_hal2_service_runtime::{
    RuntimeObjectLifecycle, RuntimeOwnerRelation, TunerServiceRuntime,
};

use crate::callback_store::{
    clear_owner_callbacks, has_callback_for_owner, retain_test_callback_marker,
};
use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::{
    clear_owner_callback_registration_hal, close_object_with_domain_cleanup,
    execute_callback_registration_runtime_use_case, SharedTunerRuntime,
};

fn shared_runtime_with_live_object(
    kind: AidlObjectKind,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    public_runtime_id: i64,
) -> SharedTunerRuntime {
    let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
    runtime
        .lock()
        .unwrap()
        .register_aidl_object_for_runtime(
            kind,
            object_id,
            generation,
            public_runtime_id,
            RuntimeOwnerRelation::Root,
        )
        .unwrap();
    runtime
}

#[test]
fn close_domain_cleanup_failure_records_cleanup_failed_state() {
    let handle = AidlObjectHandle::new(
        AidlObjectKind::Filter,
        AidlObjectId(92_001),
        AidlObjectGeneration(1),
    );
    let runtime = shared_runtime_with_live_object(
        AidlObjectKind::Filter,
        AidlObjectId(92_001),
        AidlObjectGeneration(1),
        92_001,
    );

    let result = close_object_with_domain_cleanup(&runtime, handle, || {
        Err(HalError::cleanup_failed(
            "failure injection domain cleanup",
            "domain cleanup failed",
        ))
    });

    assert!(result.is_err());
    assert_eq!(
        runtime
            .lock()
            .unwrap()
            .object_table()
            .entry(AidlObjectId(92_001))
            .expect("object remains tracked")
            .lifecycle,
        RuntimeObjectLifecycle::CleanupFailed {
            step: CleanupStep::UnregisterRuntime
        }
    );
}

#[test]
fn callback_domain_failure_rolls_back_retained_callback_and_registry() {
    let handle = AidlObjectHandle::new(
        AidlObjectKind::Lnb,
        AidlObjectId(92_002),
        AidlObjectGeneration(1),
    );
    clear_owner_callbacks(handle).unwrap();
    let runtime = shared_runtime_with_live_object(
        AidlObjectKind::Lnb,
        AidlObjectId(92_002),
        AidlObjectGeneration(1),
        92_002,
    );

    let result: BinderResult<()> = execute_callback_registration_runtime_use_case(
        &runtime,
        handle,
        AidlMethodCall::LnbSetCallback,
        || {
            retain_test_callback_marker(handle, AidlApi::LnbSetCallback)
                .map_err(|error| error.into_hal_error("failure injection callback retain"))
        },
        || {
            clear_owner_callback_registration_hal(
                &runtime,
                handle,
                Some(AidlApi::LnbSetCallback),
                "failure injection callback rollback",
            )
        },
        |_runtime, _handle, _dispatch_preflight| {
            Err(HalError::Unsupported("failure injection domain commit"))
        },
    );

    assert!(result.is_err());
    assert!(!has_callback_for_owner(handle, AidlApi::LnbSetCallback).unwrap());
    assert_eq!(
        runtime
            .lock()
            .unwrap()
            .callback_registry()
            .registration_count(),
        0
    );
}
