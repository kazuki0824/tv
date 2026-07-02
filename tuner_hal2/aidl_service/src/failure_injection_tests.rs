use std::sync::{Arc, Mutex};

use maleicacid_tuner_hal2_binder_adapter::{
    AidlMethodCall, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
};
use maleicacid_tuner_hal2_resource_ledger::CleanupStep;
use maleicacid_tuner_hal2_service_runtime::{
    AidlObjectLifecycleSnapshot, RuntimeOwnerRelation, TunerServiceRuntime,
};

use crate::object_handle::AidlObjectHandle;
use crate::object_runtime::close_object_after_close_preflight;
use crate::service_context::{AidlServiceContext, SharedTunerRuntime};

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
        AidlObjectKind::Frontend,
        AidlObjectId(92_001),
        AidlObjectGeneration(1),
    );
    let runtime = shared_runtime_with_live_object(
        AidlObjectKind::Frontend,
        AidlObjectId(92_001),
        AidlObjectGeneration(1),
        92_001,
    );
    let context = AidlServiceContext::from_shared_runtime_for_test(runtime.clone());

    let result =
        close_object_after_close_preflight(&context, handle, AidlMethodCall::FrontendClose);

    assert!(result.is_err());
    assert_eq!(
        runtime
            .lock()
            .unwrap()
            .aidl_object_lifecycle(AidlObjectId(92_001))
            .unwrap(),
        AidlObjectLifecycleSnapshot::CleanupFailed {
            step: CleanupStep::ReleaseBackend
        }
    );
}
