use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{drop_leak_object_from_drop, DropLeakDomainAction, SharedTunerRuntime};
use binder::Interface;

#[derive(Clone)]
pub struct DvrAidlObject {
    handle: AidlObjectHandle,
    runtime: SharedTunerRuntime,
}

impl Interface for DvrAidlObject {}

impl DvrAidlObject {
    pub fn new(
        handle: AidlObjectHandle,
        runtime: SharedTunerRuntime,
    ) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Dvr)?;
        Ok(Self { handle, runtime })
    }

    pub const fn handle(&self) -> AidlObjectHandle {
        self.handle
    }

    pub fn runtime(&self) -> SharedTunerRuntime {
        self.runtime.clone()
    }
}

impl Drop for DvrAidlObject {
    fn drop(&mut self) {
        drop_leak_object_from_drop(&self.runtime, self.handle, DropLeakDomainAction::None);
    }
}
