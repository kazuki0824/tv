use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::ILnbCallback::ILnbCallback;
use binder::{Interface, Result as BinderResult, Status, Strong};
use maleicacid_tuner_hal2_binder_adapter::{AidlApi, AidlMethodCall, AidlMethodPlan};

use crate::callback_store::retain_lnb_callback;
use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{
    clear_owner_callback_registration, clear_owner_callback_registration_best_effort,
    close_object, close_object_after_aidl_method_plan, ensure_object_live, plan_object_aidl_method,
    record_callback_registration, CallbackCleanupRegistryAction, SharedTunerRuntime,
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

    pub fn rollback_callback_registration(&self) -> BinderResult<()> {
        clear_owner_callback_registration(
            &self.runtime,
            self.handle,
            AidlApi::LnbSetCallback,
            "LNB callback rollback failed",
        )
    }
}

impl LnbAidlObject {
    fn live_lnb_public_id_for_drop(&self) -> Option<i32> {
        match self.runtime.lock() {
            Ok(runtime) => runtime
                .object_table()
                .entry_for_kind(
                    self.handle.object_id(),
                    self.handle.generation(),
                    AidlObjectKind::Lnb,
                )
                .ok()
                .and_then(|entry| i32::try_from(entry.ledger_id.0).ok()),
            Err(_) => None,
        }
    }

    fn record_drop_leak_if_live(&self) {
        let Some(lnb_id) = self.live_lnb_public_id_for_drop() else {
            return;
        };
        let drop_record_ok = match self.runtime.lock() {
            Ok(mut runtime) => runtime.record_lnb_drop_leak(lnb_id).is_ok(),
            Err(_) => false,
        };
        let success_action = if drop_record_ok {
            CallbackCleanupRegistryAction::ClearOwner
        } else {
            CallbackCleanupRegistryAction::MarkUnhealthy
        };
        clear_owner_callback_registration_best_effort(
            &self.runtime,
            self.handle,
            AidlApi::LnbSetCallback,
            success_action,
        );
    }
}

impl Drop for LnbAidlObject {
    fn drop(&mut self) {
        self.record_drop_leak_if_live();
    }
}
