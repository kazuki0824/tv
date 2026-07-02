use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IFrontendCallback::IFrontendCallback;
use binder::{Interface, Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::AidlMethodCall;

use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{
    drop_leak_object_from_drop, execute_callback_unregistration_runtime_use_case,
    execute_frontend_callback_registration_runtime_use_case,
};
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};

pub struct FrontendAidlObject {
    handle: AidlObjectHandle,
    context: SharedAidlServiceContext,
}

impl Interface for FrontendAidlObject {}

impl FrontendAidlObject {
    pub fn new(
        handle: AidlObjectHandle,
        context: SharedAidlServiceContext,
    ) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Frontend)?;
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

    pub(crate) fn set_callback_nullable_for_aidl(
        &self,
        callback: Option<&Strong<dyn IFrontendCallback>>,
    ) -> BinderResult<()> {
        match callback {
            Some(callback) => self.set_callback_transaction(callback),
            None => self.clear_callback_transaction(),
        }
    }

    pub(crate) fn clear_callback_transaction(&self) -> BinderResult<()> {
        execute_callback_unregistration_runtime_use_case(
            &self.context,
            self.handle,
            AidlMethodCall::FrontendSetCallback,
        )
    }

    pub(crate) fn set_callback_transaction(
        &self,
        callback: &Strong<dyn IFrontendCallback>,
    ) -> BinderResult<()> {
        execute_frontend_callback_registration_runtime_use_case(
            &self.context,
            self.handle,
            callback,
        )
    }
}

impl Drop for FrontendAidlObject {
    fn drop(&mut self) {
        drop_leak_object_from_drop(&self.context, self.handle);
    }
}
