use binder::Interface;
use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{
    drop_leak_object_from_drop,
    DropLeakDomainAction, SharedTunerRuntime,
};

#[derive(Clone)]
pub struct DescramblerAidlObject {
    handle: AidlObjectHandle,
    runtime: SharedTunerRuntime,
}

impl Interface for DescramblerAidlObject {}

impl DescramblerAidlObject {
    pub fn new(
        handle: AidlObjectHandle,
        runtime: SharedTunerRuntime,
    ) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Descrambler)?;
        Ok(Self { handle, runtime })
    }

    pub const fn handle(&self) -> AidlObjectHandle {
        self.handle
    }

    pub fn runtime(&self) -> SharedTunerRuntime {
        self.runtime.clone()
    }
}

impl Drop for DescramblerAidlObject {
    fn drop(&mut self) {
        drop_leak_object_from_drop(&self.runtime, self.handle, DropLeakDomainAction::None);
    }
}
