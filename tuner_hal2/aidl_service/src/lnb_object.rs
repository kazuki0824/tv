use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnbCallback::ILnbCallback;
use binder::{Interface, Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlMethodCall};

use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{
    clear_owner_callback_registration_hal, drop_leak_object_from_drop,
    execute_callback_registration_runtime_use_case, DropLeakDomainAction,
};
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};

pub struct LnbAidlObject {
    handle: AidlObjectHandle,
    context: SharedAidlServiceContext,
}

impl Interface for LnbAidlObject {}

impl LnbAidlObject {
    pub fn new(
        handle: AidlObjectHandle,
        context: SharedAidlServiceContext,
    ) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Lnb)?;
        Ok(Self { handle, context })
    }

    pub const fn handle(&self) -> AidlObjectHandle {
        self.handle
    }

    pub(crate) fn context(&self) -> SharedAidlServiceContext {
        self.context.clone()
    }

    pub(crate) fn runtime(&self) -> SharedTunerRuntime {
        self.context.runtime()
    }

    pub(crate) fn set_callback_transaction(
        &self,
        callback: &Strong<dyn ILnbCallback>,
    ) -> BinderResult<()> {
        execute_callback_registration_runtime_use_case(
            &self.context,
            self.handle,
            AidlMethodCall::LnbSetCallback,
            || {
                self.context
                    .retain_lnb_callback(self.handle, callback)
                    .map_err(|error| error.into_hal_error("LNB callback store retain failed"))
            },
            || {
                clear_owner_callback_registration_hal(
                    &self.context,
                    self.handle,
                    Some(AidlApi::LnbSetCallback),
                    "LNB callback rollback failed",
                )
            },
            |runtime, handle, dispatch_proof| {
                runtime.commit_lnb_callback_registration_for_object(
                    handle.object_id(),
                    handle.generation(),
                    dispatch_proof,
                )
            },
        )
    }
}

impl Drop for LnbAidlObject {
    fn drop(&mut self) {
        drop_leak_object_from_drop(
            &self.context,
            self.handle,
            DropLeakDomainAction::RecordLnbDropLeak,
        );
    }
}
