use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IFrontendCallback::IFrontendCallback;
use binder::{Interface, Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlMethodCall};

use crate::callback_store::retain_frontend_callback;
use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{
    clear_owner_callback_registration_hal,
    drop_leak_object_from_drop, execute_callback_registration_runtime_use_case,
    DropLeakDomainAction, SharedTunerRuntime,
};

pub struct FrontendAidlObject {
    handle: AidlObjectHandle,
    runtime: SharedTunerRuntime,
}

impl Interface for FrontendAidlObject {}

impl FrontendAidlObject {
    pub fn new(
        handle: AidlObjectHandle,
        runtime: SharedTunerRuntime,
    ) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Frontend)?;
        Ok(Self { handle, runtime })
    }

    pub const fn handle(&self) -> AidlObjectHandle {
        self.handle
    }

    pub fn runtime(&self) -> SharedTunerRuntime {
        self.runtime.clone()
    }

    pub fn set_callback_transaction(&self, callback: &Strong<dyn IFrontendCallback>) -> BinderResult<()> {
        execute_callback_registration_runtime_use_case(
            &self.runtime,
            self.handle,
            AidlMethodCall::FrontendSetCallback,
            || retain_frontend_callback(self.handle, callback).map_err(|error| {
                error.into_hal_error("frontend callback store retain failed")
            }),
            || {
                clear_owner_callback_registration_hal(
                    &self.runtime,
                    self.handle,
                    Some(AidlApi::FrontendSetCallback),
                    "frontend callback rollback failed",
                )
            },
            |runtime, handle, dispatch_preflight| {
                runtime.commit_frontend_callback_registration_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_preflight,
                )
            },
        )
    }
}

impl Drop for FrontendAidlObject {
    fn drop(&mut self) {
        drop_leak_object_from_drop(&self.runtime, self.handle, DropLeakDomainAction::None);
    }
}
