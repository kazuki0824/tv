use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, IFilterCallback::IFilterCallback,
    IFrontendCallback::IFrontendCallback, ILnbCallback::ILnbCallback,
};
use binder::Strong;
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
};

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
}

#[derive(Default)]
struct CallbackStore {
    callbacks: BTreeMap<CallbackStoreKey, StoredCallback>,
}

static CALLBACK_STORE: OnceLock<Mutex<CallbackStore>> = OnceLock::new();

fn store() -> &'static Mutex<CallbackStore> {
    CALLBACK_STORE.get_or_init(|| Mutex::new(CallbackStore::default()))
}

pub fn retain_frontend_callback(
    handle: AidlObjectHandle,
    callback: &Strong<dyn IFrontendCallback>,
) -> Result<(), AidlCallbackStoreError> {
    let mut store = store()
        .lock()
        .map_err(|_| AidlCallbackStoreError::Poisoned)?;
    store.callbacks.insert(
        CallbackStoreKey::new(handle, AidlApi::FrontendSetCallback),
        StoredCallback::Frontend(callback.clone()),
    );
    Ok(())
}

pub fn retain_lnb_callback(
    handle: AidlObjectHandle,
    callback: &Strong<dyn ILnbCallback>,
) -> Result<(), AidlCallbackStoreError> {
    let mut store = store()
        .lock()
        .map_err(|_| AidlCallbackStoreError::Poisoned)?;
    store.callbacks.insert(
        CallbackStoreKey::new(handle, AidlApi::LnbSetCallback),
        StoredCallback::Lnb(callback.clone()),
    );
    Ok(())
}

pub fn retain_filter_callback(
    handle: AidlObjectHandle,
    callback: &Strong<dyn IFilterCallback>,
) -> Result<(), AidlCallbackStoreError> {
    let mut store = store()
        .lock()
        .map_err(|_| AidlCallbackStoreError::Poisoned)?;
    store.callbacks.insert(
        CallbackStoreKey::new(handle, AidlApi::DemuxOpenFilter),
        StoredCallback::Filter(callback.clone()),
    );
    Ok(())
}

pub fn retain_dvr_callback(
    handle: AidlObjectHandle,
    callback: &Strong<dyn IDvrCallback>,
) -> Result<(), AidlCallbackStoreError> {
    let mut store = store()
        .lock()
        .map_err(|_| AidlCallbackStoreError::Poisoned)?;
    store.callbacks.insert(
        CallbackStoreKey::new(handle, AidlApi::DemuxOpenDvr),
        StoredCallback::Dvr(callback.clone()),
    );
    Ok(())
}

pub fn clear_owner_callbacks(handle: AidlObjectHandle) -> Result<(), AidlCallbackStoreError> {
    let mut store = store()
        .lock()
        .map_err(|_| AidlCallbackStoreError::Poisoned)?;
    store.callbacks.retain(|key, _| !key.matches_owner(handle));
    Ok(())
}

#[cfg(test)]
fn has_callback_for_owner(
    handle: AidlObjectHandle,
    api: AidlApi,
) -> Result<bool, AidlCallbackStoreError> {
    let store = store()
        .lock()
        .map_err(|_| AidlCallbackStoreError::Poisoned)?;
    Ok(store
        .callbacks
        .contains_key(&CallbackStoreKey::new(handle, api)))
}

pub fn frontend_callback_for_owner(
    handle: AidlObjectHandle,
) -> Result<Option<Strong<dyn IFrontendCallback>>, AidlCallbackStoreError> {
    let store = store()
        .lock()
        .map_err(|_| AidlCallbackStoreError::Poisoned)?;
    Ok(
        match store
            .callbacks
            .get(&CallbackStoreKey::new(handle, AidlApi::FrontendSetCallback))
        {
            Some(StoredCallback::Frontend(callback)) => Some(callback.clone()),
            _ => None,
        },
    )
}

pub fn filter_callback_for_owner(
    handle: AidlObjectHandle,
) -> Result<Option<Strong<dyn IFilterCallback>>, AidlCallbackStoreError> {
    let store = store()
        .lock()
        .map_err(|_| AidlCallbackStoreError::Poisoned)?;
    Ok(
        match store
            .callbacks
            .get(&CallbackStoreKey::new(handle, AidlApi::DemuxOpenFilter))
        {
            Some(StoredCallback::Filter(callback)) => Some(callback.clone()),
            _ => None,
        },
    )
}

pub fn dvr_callback_for_owner(
    handle: AidlObjectHandle,
) -> Result<Option<Strong<dyn IDvrCallback>>, AidlCallbackStoreError> {
    let store = store()
        .lock()
        .map_err(|_| AidlCallbackStoreError::Poisoned)?;
    Ok(
        match store
            .callbacks
            .get(&CallbackStoreKey::new(handle, AidlApi::DemuxOpenDvr))
        {
            Some(StoredCallback::Dvr(callback)) => Some(callback.clone()),
            _ => None,
        },
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AidlCallbackStoreError {
    Poisoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_owner_removes_all_callback_entries_for_generation() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Frontend,
            AidlObjectId(9001),
            AidlObjectGeneration(7),
        );
        {
            let mut store = store().lock().unwrap();
            store.callbacks.clear();
        }
        assert!(!has_callback_for_owner(handle, AidlApi::FrontendSetCallback).unwrap());
        clear_owner_callbacks(handle).unwrap();
        assert!(!has_callback_for_owner(handle, AidlApi::FrontendSetCallback).unwrap());
    }
}
