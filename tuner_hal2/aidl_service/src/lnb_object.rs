use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnbCallback::ILnbCallback;
use binder::{Result as BinderResult, Status, Strong};
use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlMethodCall, AidlMethodPlan};

use crate::callback_store::retain_lnb_callback;
use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{close_object, close_object_after_aidl_method_plan, ensure_object_live, plan_object_aidl_method, record_callback_registration, SharedTunerRuntime};

#[derive(Clone)]
pub struct LnbAidlObject {
    handle: AidlObjectHandle,
    runtime: SharedTunerRuntime,
}

impl LnbAidlObject {
    pub fn new(handle: AidlObjectHandle, runtime: SharedTunerRuntime) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Lnb)?;
        Ok(Self { handle, runtime })
    }

    pub const fn handle(&self) -> AidlObjectHandle { self.handle }

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

    pub fn retain_callback(&self, callback: &Strong<dyn ILnbCallback>) -> BinderResult<()> {
        retain_lnb_callback(self.handle, callback).map_err(|_| Status::new_service_specific_error(android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::Result::Result::UNKNOWN_ERROR.0, None))?;
        record_callback_registration(&self.runtime, self.handle, AidlApi::LnbSetCallback)
    }
}
