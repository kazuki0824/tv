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
    prepared_callbacks: BTreeMap<CallbackStoreKey, (PreparedCallbackArtifactToken, StoredCallback)>,
    next_prepared_token: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PreparedCallbackArtifactToken(u64);

impl CallbackStore {
    pub(crate) fn prepare_frontend_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IFrontendCallback>,
    ) -> Result<PreparedCallbackArtifactToken, AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, AidlApi::FrontendSetCallback);
        if self.prepared_callbacks.contains_key(&key) {
            return Err(AidlCallbackStoreError::PreparedArtifactInFlight);
        }
        let token = self.next_prepared_token()?;
        self.prepared_callbacks.insert(
            key,
            (token, StoredCallback::Frontend(callback.clone())),
        );
        Ok(token)
    }

    pub(crate) fn prepare_lnb_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn ILnbCallback>,
    ) -> Result<PreparedCallbackArtifactToken, AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, AidlApi::LnbSetCallback);
        if self.prepared_callbacks.contains_key(&key) {
            return Err(AidlCallbackStoreError::PreparedArtifactInFlight);
        }
        let token = self.next_prepared_token()?;
        self.prepared_callbacks.insert(
            key,
            (token, StoredCallback::Lnb(callback.clone())),
        );
        Ok(token)
    }

    fn next_prepared_token(
        &mut self,
    ) -> Result<PreparedCallbackArtifactToken, AidlCallbackStoreError> {
        let next = self
            .next_prepared_token
            .checked_add(1)
            .ok_or(AidlCallbackStoreError::PreparedTokenExhausted)?;
        self.next_prepared_token = next;
        Ok(PreparedCallbackArtifactToken(next))
    }

    pub(crate) fn commit_prepared_callback(
        &mut self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> bool {
        let key = CallbackStoreKey::new(handle, registration_api);
        if !self
            .prepared_callbacks
            .get(&key)
            .is_some_and(|(prepared, _)| *prepared == token)
        {
            return false;
        }
        let Some((_, callback)) = self.prepared_callbacks.remove(&key) else {
            return false;
        };
        self.callbacks.insert(key, callback);
        true
    }

    pub(crate) fn abort_prepared_callback(
        &mut self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> bool {
        let key = CallbackStoreKey::new(handle, registration_api);
        if !self
            .prepared_callbacks
            .get(&key)
            .is_some_and(|(prepared, _)| *prepared == token)
        {
            return false;
        }
        self.prepared_callbacks.remove(&key).is_some()
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
        let before = self.callbacks.len() + self.prepared_callbacks.len();
        self.callbacks.retain(|key, _| !key.matches_owner(handle));
        self.prepared_callbacks
            .retain(|key, _| !key.matches_owner(handle));
        before.saturating_sub(self.callbacks.len() + self.prepared_callbacks.len())
    }

    pub(crate) fn clear_all_callbacks(&mut self) -> usize {
        let before = self.callbacks.len() + self.prepared_callbacks.len();
        self.callbacks.clear();
        self.prepared_callbacks.clear();
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

    #[cfg(test)]
    fn prepare_test_callback_marker(
        &mut self,
        handle: AidlObjectHandle,
        api: AidlApi,
    ) -> PreparedCallbackArtifactToken {
        let token = self.next_prepared_token().unwrap();
        self.prepared_callbacks.insert(
            CallbackStoreKey::new(handle, api),
            (token, StoredCallback::TestMarker),
        );
        token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frontend_handle() -> AidlObjectHandle {
        AidlObjectHandle::new(
            AidlObjectKind::Frontend,
            AidlObjectId(41),
            AidlObjectGeneration(7),
        )
    }

    #[test]
    fn prepared_replacement_abort_preserves_current_callback() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        store.retain_test_callback_marker(handle, AidlApi::FrontendSetCallback);
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);

        assert!(store.abort_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            token
        ));
        assert!(store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.prepared_callbacks.is_empty());
    }

    #[test]
    fn prepared_replacement_becomes_current_only_at_commit() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);

        assert!(!store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.commit_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            token
        ));
        assert!(store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.prepared_callbacks.is_empty());
    }

    #[test]
    fn stale_prepared_token_cannot_commit_or_abort_another_artifact() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);
        let stale = PreparedCallbackArtifactToken(token.0 + 1);

        assert!(!store.commit_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            stale
        ));
        assert!(!store.abort_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            stale
        ));
        assert!(store.abort_prepared_callback(
            handle,
            AidlApi::FrontendSetCallback,
            token
        ));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AidlCallbackStoreError {
    Poisoned,
    PreparedArtifactInFlight,
    PreparedTokenExhausted,
}

impl AidlCallbackStoreError {
    pub(crate) fn into_hal_error(self, context: &'static str) -> HalError {
        match self {
            Self::Poisoned => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: callback store lock poisoned"),
            ),
            Self::PreparedArtifactInFlight => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: callback registration is already in flight"),
            ),
            Self::PreparedTokenExhausted => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: prepared callback artifact token exhausted"),
            ),
        }
    }
}
