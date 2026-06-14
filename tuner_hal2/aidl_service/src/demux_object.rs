use binder::{Interface, Result as BinderResult};
use maleicacid_tuner_hal2_binder_adapter::{AidlMethodCall, AidlMethodPlan};

use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{
    close_object, close_object_after_aidl_method_plan, ensure_object_live, plan_object_aidl_method,
    quarantine_live_aidl_object_after_drop_leak,
    SharedTunerRuntime,
};

#[derive(Clone)]
pub struct DemuxAidlObject {
    handle: AidlObjectHandle,
    runtime: SharedTunerRuntime,
}

impl Interface for DemuxAidlObject {}

impl DemuxAidlObject {
    pub fn new(
        handle: AidlObjectHandle,
        runtime: SharedTunerRuntime,
    ) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Demux)?;
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
}

impl Drop for DemuxAidlObject {
    fn drop(&mut self) {
        quarantine_live_aidl_object_after_drop_leak(&self.runtime, self.handle);
    }
}
