use crate::object_handle::{AidlObjectHandle, AidlObjectHandleError, AidlObjectKind};
use crate::object_runtime::{drop_leak_object_from_drop, DropLeakDomainAction};
use crate::service_context::{SharedAidlServiceContext, SharedTunerRuntime};
use binder::Interface;

#[derive(Clone)]
pub struct DemuxAidlObject {
    handle: AidlObjectHandle,
    context: SharedAidlServiceContext,
}

impl Interface for DemuxAidlObject {}

impl DemuxAidlObject {
    pub fn new(
        handle: AidlObjectHandle,
        context: SharedAidlServiceContext,
    ) -> Result<Self, AidlObjectHandleError> {
        handle.ensure_kind(AidlObjectKind::Demux)?;
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
}

impl Drop for DemuxAidlObject {
    fn drop(&mut self) {
        drop_leak_object_from_drop(&self.context, self.handle, DropLeakDomainAction::None);
    }
}
