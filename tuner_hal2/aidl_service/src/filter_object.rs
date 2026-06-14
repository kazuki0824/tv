use binder::{Interface, Result as BinderResult};
use maleicacid_tuner_hal2_binder_adapter::{AidlMethodCall, AidlMethodPlan};
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_service_runtime::{
    configure_ip_cid_result, configure_monitor_event_result,
};

use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{
    close_object, close_object_after_aidl_method_plan, ensure_object_live, plan_object_aidl_method,
    quarantine_live_aidl_object_after_drop_leak,
    SharedTunerRuntime,
};

#[derive(Clone)]
pub struct FilterAidlObject {
    handle: AidlObjectHandle,
    runtime: SharedTunerRuntime,
}

impl Interface for FilterAidlObject {}

impl FilterAidlObject {
    pub fn new(
        handle: AidlObjectHandle,
        runtime: SharedTunerRuntime,
    ) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Filter)?;
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

    pub fn validate_configure_ip_cid_profile(ip_cid: i32) -> Result<(), HalError> {
        configure_ip_cid_result(ip_cid)
    }

    pub fn validate_configure_monitor_event_profile(
        monitor_event_types: i32,
    ) -> Result<(), HalError> {
        configure_monitor_event_result(monitor_event_types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_binder_adapter::{AidlStatusMapper, TunerStatusCode};

    #[test]
    fn filter_profile_boundary_keeps_ip_cid_unavailable_before_value_validation() {
        for ip_cid in [-1, 0, 1, i32::MAX] {
            let error = FilterAidlObject::validate_configure_ip_cid_profile(ip_cid)
                .expect_err("IP CID must remain unsupported by the TS-only profile");
            assert_eq!(
                AidlStatusMapper::map_error(&error),
                TunerStatusCode::Unavailable
            );
        }
    }

    #[test]
    fn filter_profile_boundary_keeps_monitor_event_zero_success_nonzero_unavailable() {
        assert!(FilterAidlObject::validate_configure_monitor_event_profile(0).is_ok());
        for mask in [-1, 1, i32::MAX] {
            let error = FilterAidlObject::validate_configure_monitor_event_profile(mask)
                .expect_err(
                    "non-zero monitor event mask must remain unavailable when profile disabled",
                );
            assert_eq!(
                AidlStatusMapper::map_error(&error),
                TunerStatusCode::Unavailable
            );
        }
    }
}

impl Drop for FilterAidlObject {
    fn drop(&mut self) {
        quarantine_live_aidl_object_after_drop_leak(&self.runtime, self.handle);
    }
}
