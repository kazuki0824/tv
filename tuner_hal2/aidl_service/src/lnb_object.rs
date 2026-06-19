use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnbCallback::ILnbCallback;
use binder::{Interface, Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlMethodCall};

use crate::callback_store::retain_lnb_callback;
use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{
    clear_owner_callback_registration_hal,
    drop_leak_object_from_drop, execute_callback_registration_runtime_use_case,
    DropLeakDomainAction, SharedTunerRuntime,
};

pub struct LnbAidlObject {
    handle: AidlObjectHandle,
    runtime: SharedTunerRuntime,
}

impl Interface for LnbAidlObject {}

impl LnbAidlObject {
    pub fn new(
        handle: AidlObjectHandle,
        runtime: SharedTunerRuntime,
    ) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Lnb)?;
        Ok(Self { handle, runtime })
    }

    pub const fn handle(&self) -> AidlObjectHandle {
        self.handle
    }

    pub fn runtime(&self) -> SharedTunerRuntime {
        self.runtime.clone()
    }

    pub fn set_callback_transaction(&self, callback: &Strong<dyn ILnbCallback>) -> BinderResult<()> {
        execute_callback_registration_runtime_use_case(
            &self.runtime,
            self.handle,
            AidlMethodCall::LnbSetCallback,
            || retain_lnb_callback(self.handle, callback).map_err(|error| {
                error.into_hal_error("LNB callback store retain failed")
            }),
            || {
                clear_owner_callback_registration_hal(
                    &self.runtime,
                    self.handle,
                    Some(AidlApi::LnbSetCallback),
                    "LNB callback rollback failed",
                )
            },
            |runtime, handle, dispatch_preflight| {
                runtime.commit_lnb_callback_registration_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_preflight,
                )
            },
        )
    }
}

impl Drop for LnbAidlObject {
    fn drop(&mut self) {
        drop_leak_object_from_drop(
            &self.runtime,
            self.handle,
            DropLeakDomainAction::RecordLnbDropLeak,
        );
    }
}
