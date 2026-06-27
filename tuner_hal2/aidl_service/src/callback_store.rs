use std::collections::BTreeMap;

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, IFilterCallback::IFilterCallback,
    IFrontendCallback::IFrontendCallback, ILnbCallback::ILnbCallback,
};
use binder::Strong;
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
};
use maleicacid_tuner_hal2_common::{HalError, HalInternalKind};

use crate::object_handle::AidlObjectHandle;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct CallbackStoreKey {
    owner_kind: AidlObjectKind,
    owner_id: AidlObjectId,
    owner_generation: AidlObjectGeneration,
    registration_api: AidlApi,
}

impl CallbackStoreKey {
    fn new(handle: AidlObjectHandle, registration_api: AidlApi) -> Self {
        Self {
            owner_kind: handle.object_kind(),
            owner_id: handle.object_id(),
            owner_generation: handle.generation(),
            registration_api,
        }
    }

    fn matches_owner(&self, handle: AidlObjectHandle) -> bool {
        self.owner_kind == handle.object_kind()
            && self.owner_id == handle.object_id()
            && self.owner_generation == handle.generation()
    }
}

#[derive(Clone)]
enum StoredCallback {
    Frontend(Strong<dyn IFrontendCallback>),
    Filter(Strong<dyn IFilterCallback>),
    Dvr(Strong<dyn IDvrCallback>),
    Lnb(Strong<dyn ILnbCallback>),
    #[cfg(test)]
    TestMarker,
}

#[derive(Default)]
pub(crate) struct CallbackStore {
    callbacks: BTreeMap<CallbackStoreKey, StoredCallback>,
}

impl CallbackStore {
    pub(crate) fn retain_frontend_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IFrontendCallback>,
    ) {
        self.callbacks.insert(
            CallbackStoreKey::new(handle, AidlApi::FrontendSetCallback),
            StoredCallback::Frontend(callback.clone()),
        );
    }

    pub(crate) fn retain_lnb_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn ILnbCallback>,
    ) {
        self.callbacks.insert(
            CallbackStoreKey::new(handle, AidlApi::LnbSetCallback),
            StoredCallback::Lnb(callback.clone()),
        );
    }

    pub(crate) fn retain_filter_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IFilterCallback>,
    ) {
        self.callbacks.insert(
            CallbackStoreKey::new(handle, AidlApi::DemuxOpenFilter),
            StoredCallback::Filter(callback.clone()),
        );
    }

    pub(crate) fn retain_dvr_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IDvrCallback>,
    ) {
        self.callbacks.insert(
            CallbackStoreKey::new(handle, AidlApi::DemuxOpenDvr),
            StoredCallback::Dvr(callback.clone()),
        );
    }

    pub(crate) fn clear_owner_callbacks(&mut self, handle: AidlObjectHandle) -> usize {
        let before = self.callbacks.len();
        self.callbacks.retain(|key, _| !key.matches_owner(handle));
        before.saturating_sub(self.callbacks.len())
    }

    pub(crate) fn clear_all_callbacks(&mut self) -> usize {
        let before = self.callbacks.len();
        self.callbacks.clear();
        before
    }

    pub(crate) fn frontend_callback_for_owner(
        &self,
        handle: AidlObjectHandle,
    ) -> Option<Strong<dyn IFrontendCallback>> {
        match self
            .callbacks
            .get(&CallbackStoreKey::new(handle, AidlApi::FrontendSetCallback))
        {
            Some(StoredCallback::Frontend(callback)) => Some(callback.clone()),
            _ => None,
        }
    }

    pub(crate) fn filter_callback_for_owner(
        &self,
        handle: AidlObjectHandle,
    ) -> Option<Strong<dyn IFilterCallback>> {
        match self
            .callbacks
            .get(&CallbackStoreKey::new(handle, AidlApi::DemuxOpenFilter))
        {
            Some(StoredCallback::Filter(callback)) => Some(callback.clone()),
            _ => None,
        }
    }

    pub(crate) fn dvr_callback_for_owner(
        &self,
        handle: AidlObjectHandle,
    ) -> Option<Strong<dyn IDvrCallback>> {
        match self
            .callbacks
            .get(&CallbackStoreKey::new(handle, AidlApi::DemuxOpenDvr))
        {
            Some(StoredCallback::Dvr(callback)) => Some(callback.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn has_callback_for_owner(&self, handle: AidlObjectHandle, api: AidlApi) -> bool {
        self.callbacks
            .contains_key(&CallbackStoreKey::new(handle, api))
    }

    #[cfg(test)]
    pub(crate) fn retain_test_callback_marker(&mut self, handle: AidlObjectHandle, api: AidlApi) {
        self.callbacks.insert(
            CallbackStoreKey {
                owner_kind: handle.object_kind(),
                owner_id: handle.object_id(),
                owner_generation: handle.generation(),
                registration_api: api,
            },
            StoredCallback::TestMarker,
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AidlCallbackStoreError {
    Poisoned,
}

impl AidlCallbackStoreError {
    pub(crate) fn into_hal_error(self, context: &'static str) -> HalError {
        match self {
            Self::Poisoned => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: callback store lock poisoned"),
            ),
        }
    }
}
