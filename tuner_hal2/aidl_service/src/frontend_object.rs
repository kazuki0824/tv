use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::IFrontendCallback::IFrontendCallback;
use binder::{Interface, Result as BinderResult, Strong};
use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlMethodCall, AidlMethodPlan};

use crate::callback_store::retain_frontend_callback;
use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::error_bridge::service_error;
use crate::object_runtime::{
    close_object, close_object_after_aidl_method_plan, drop_leak_object, ensure_object_live,
    plan_object_aidl_method, record_callback_registration, DropLeakDomainAction, SharedTunerRuntime,
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

    pub fn ensure_open(&self) -> BinderResult<()> {
        ensure_object_live(&self.runtime, self.handle)
    }

    pub fn plan_method(&self, method: AidlMethodCall) -> BinderResult<AidlMethodPlan> {
        plan_object_aidl_method(&self.runtime, self.handle, method)
    }

    pub fn close_object_after_plan(&self, method: AidlMethodCall) -> BinderResult<()> {
        close_object_after_aidl_method_plan(&self.runtime, self.handle, method)
    }

    pub fn close_object(&self) -> BinderResult<()> {
        close_object(&self.runtime, self.handle)
    }

    pub fn retain_callback(&self, callback: &Strong<dyn IFrontendCallback>) -> BinderResult<()> {
        retain_frontend_callback(self.handle, callback).map_err(|_| {
            service_error(
                android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::Result::Result::UNKNOWN_ERROR.0,
                "frontend callback store retain failed",
            )
        })?;
        record_callback_registration(&self.runtime, self.handle, AidlApi::FrontendSetCallback)
    }
}

impl Drop for FrontendAidlObject {
    fn drop(&mut self) {
        drop_leak_object(&self.runtime, self.handle, DropLeakDomainAction::None);
    }
}
