use binder::Interface;

use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{drop_leak_object_from_drop, DropLeakDomainAction, SharedTunerRuntime};

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
}

#[cfg(test)]
mod tests {
    use maleicacid_tuner_hal2_binder_adapter::{AidlStatusMapper, TunerStatusCode};
    use maleicacid_tuner_hal2_service_runtime::{
        configure_ip_cid_result, configure_monitor_event_result,
    };

    #[test]
    fn filter_profile_boundary_keeps_ip_cid_unavailable_before_value_validation() {
        for ip_cid in [-1, 0, 1, i32::MAX] {
            let error = configure_ip_cid_result(ip_cid)
                .expect_err("IP CID must remain unsupported by the TS-only profile");
            assert_eq!(
                AidlStatusMapper::map_error(&error),
                TunerStatusCode::Unavailable
            );
        }
    }

    #[test]
    fn filter_profile_boundary_keeps_monitor_event_zero_success_nonzero_unavailable() {
        assert!(configure_monitor_event_result(0).is_ok());
        for mask in [-1, 1, i32::MAX] {
            let error = configure_monitor_event_result(mask).expect_err(
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
        drop_leak_object_from_drop(&self.runtime, self.handle, DropLeakDomainAction::None);
    }
}
