use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
    IDvrCallback::IDvrCallback, IFilterCallback::IFilterCallback,
    IFrontendCallback::IFrontendCallback, ILnbCallback::ILnbCallback,
};
#[cfg(test)]
use binder::Interface;
use binder::{DeathRecipient, IBinder, StatusCode, Strong};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrontendCallbackGeneration(u64);

impl FrontendCallbackGeneration {
    pub(crate) fn diagnostic_value(self) -> u64 {
        self.0
    }
}

pub(crate) struct FrontendCallbackRegistration {
    generation: FrontendCallbackGeneration,
    callback: Strong<dyn IFrontendCallback>,
    dead: Arc<AtomicBool>,
    death_recipient: Mutex<DeathLinkState>,
    death_gate: Arc<Mutex<()>>,
}

enum DeathLinkState {
    Pending,
    Linking,
    Linked(DeathRecipient),
    Unlinking,
    Closed,
}

// 死亡通知の線形化点。poison時も死亡だけは記録し、登録側はpoisonを失敗として扱う。
fn mark_callback_dead(dead: &AtomicBool, gate: &Mutex<()>) {
    let _guard = gate.lock();
    dead.store(true, Ordering::Release);
}

fn death_unlink_result(
    generation: FrontendCallbackGeneration,
    result: Result<(), StatusCode>,
) -> Result<(), AidlCallbackStoreError> {
    match result {
        Ok(()) | Err(StatusCode::NAME_NOT_FOUND | StatusCode::DEAD_OBJECT) => Ok(()),
        Err(status) => Err(AidlCallbackStoreError::DeathUnlink { generation, status }),
    }
}

impl FrontendCallbackRegistration {
    pub(crate) fn lock_death_gate(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, ()>, AidlCallbackStoreError> {
        self.death_gate
            .lock()
            .map_err(|_| AidlCallbackStoreError::Poisoned)
    }

    pub(crate) fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Acquire)
    }

    pub(crate) fn link_death(
        &self,
        on_death: impl Fn(FrontendCallbackGeneration) + Send + Sync + 'static,
    ) -> Result<(), AidlCallbackStoreError> {
        {
            let mut state = self
                .death_recipient
                .lock()
                .map_err(|_| AidlCallbackStoreError::Poisoned)?;
            if !matches!(*state, DeathLinkState::Pending) {
                return Err(AidlCallbackStoreError::RetirementPending);
            }
            *state = DeathLinkState::Linking;
        }
        let mut binder = self.callback.as_binder();
        if !binder.is_remote() {
            *self
                .death_recipient
                .lock()
                .map_err(|_| AidlCallbackStoreError::Poisoned)? = DeathLinkState::Closed;
            return Ok(());
        }
        let dead = Arc::clone(&self.dead);
        let generation = self.generation;
        let death_gate = Arc::clone(&self.death_gate);
        let mut recipient = DeathRecipient::new(move || {
            // 複合commitと死亡の確定順を同じlockで直列化する。
            // 死亡処理へ再入する前に解放し、runtime/storeとの逆順を作らない。
            mark_callback_dead(&dead, &death_gate);
            on_death(generation);
        });
        let result = binder
            .link_to_death(&mut recipient)
            .map_err(AidlCallbackStoreError::DeathLink);
        match self.death_recipient.lock() {
            Ok(mut state) => {
                if result.is_ok() {
                    *state = DeathLinkState::Linked(recipient);
                    return Ok(());
                }
                *state = DeathLinkState::Closed;
            }
            Err(_) => {
                self.dead.store(true, Ordering::Release);
                let primary = AidlCallbackStoreError::Poisoned;
                let cleanup = if result.is_ok() {
                    death_unlink_result(generation, binder.unlink_to_death(&mut recipient))
                } else {
                    result
                };
                // 汚染時は登録を公開せず、recipient破棄によるNDKの全link解除もlock外で行う。
                drop(recipient);
                return Err(match cleanup {
                    Ok(()) => primary,
                    Err(cleanup) => AidlCallbackStoreError::Composed {
                        primary: Box::new(primary),
                        cleanup: Box::new(cleanup),
                    },
                });
            }
        }
        drop(recipient);
        result
    }

    fn unlink_death(&self) -> Result<(), AidlCallbackStoreError> {
        let mut recipient = {
            let mut state = self
                .death_recipient
                .lock()
                .map_err(|_| AidlCallbackStoreError::Poisoned)?;
            match std::mem::replace(&mut *state, DeathLinkState::Unlinking) {
                DeathLinkState::Linked(recipient) => recipient,
                DeathLinkState::Pending | DeathLinkState::Closed => {
                    *state = DeathLinkState::Closed;
                    return Ok(());
                }
                DeathLinkState::Linking => {
                    *state = DeathLinkState::Linking;
                    return Err(AidlCallbackStoreError::RetirementPending);
                }
                DeathLinkState::Unlinking => return Err(AidlCallbackStoreError::RetirementPending),
            }
        };
        let result = death_unlink_result(
            self.generation,
            self.callback.as_binder().unlink_to_death(&mut recipient),
        );
        match self.death_recipient.lock() {
            Ok(mut state) => {
                if result.is_err() {
                    *state = DeathLinkState::Linked(recipient);
                    return result;
                }
                *state = DeathLinkState::Closed;
            }
            Err(_) => {
                self.dead.store(true, Ordering::Release);
                drop(recipient);
                return Err(match result {
                    Ok(()) => AidlCallbackStoreError::Poisoned,
                    Err(primary) => AidlCallbackStoreError::Composed {
                        primary: Box::new(primary),
                        cleanup: Box::new(AidlCallbackStoreError::Poisoned),
                    },
                });
            }
        }
        drop(recipient);
        Ok(())
    }
}

/// storeで配送開始を受理した一回分の通知先。workerへ保存しない。
pub(crate) struct FrontendCallbackDelivery(Arc<FrontendCallbackRegistration>);

impl FrontendCallbackDelivery {
    pub(crate) fn generation(&self) -> FrontendCallbackGeneration {
        self.0.generation
    }
    pub(crate) fn callback(&self) -> &Strong<dyn IFrontendCallback> {
        &self.0.callback
    }
}

enum StoredCallback {
    Frontend(Arc<FrontendCallbackRegistration>),
    Filter(Strong<dyn IFilterCallback>),
    Dvr(Strong<dyn IDvrCallback>),
    Lnb {
        _retained_callback: Strong<dyn ILnbCallback>,
    },
    #[cfg(test)]
    TestMarker,
}

#[derive(Default)]
pub(crate) struct CallbackStore {
    callbacks: BTreeMap<CallbackStoreKey, StoredCallback>,
    prepared_callbacks: BTreeMap<CallbackStoreKey, (u64, StoredCallback)>,
    next_prepared_token: u64,
    retired_callbacks: Vec<StoredCallback>,
    retirement_batch: Option<Arc<Vec<StoredCallback>>>,
    retirement_running: bool,
}

#[must_use = "callbackの解放結果を所有者へ戻す必要があります"]
pub(crate) struct RetiredCallbacks(Arc<Vec<StoredCallback>>);

pub(crate) struct ReleasedCallbacks {
    _callbacks: Arc<Vec<StoredCallback>>,
}

impl RetiredCallbacks {
    pub(crate) fn release(&self) -> Result<(), AidlCallbackStoreError> {
        let mut first_error = None;
        for callback in self.0.iter() {
            let result = match callback {
                StoredCallback::Frontend(registration) => registration.unlink_death(),
                _ => Ok(()),
            };
            if let Err(error) = result {
                first_error = Some(match first_error {
                    Some(primary) => AidlCallbackStoreError::Composed {
                        primary: Box::new(primary),
                        cleanup: Box::new(error),
                    },
                    None => error,
                });
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
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
        let registration = FrontendCallbackRegistration {
            generation: FrontendCallbackGeneration(token.0),
            callback: callback.clone(),
            dead: Arc::new(AtomicBool::new(false)),
            death_recipient: Mutex::new(DeathLinkState::Pending),
            death_gate: Arc::new(Mutex::new(())),
        };
        self.prepared_callbacks.insert(
            key,
            (token.0, StoredCallback::Frontend(Arc::new(registration))),
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
            (
                token.0,
                StoredCallback::Lnb {
                    _retained_callback: callback.clone(),
                },
            ),
        );
        Ok(token)
    }

    pub(crate) fn prepare_filter_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IFilterCallback>,
    ) -> Result<PreparedCallbackArtifactToken, AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, AidlApi::DemuxOpenFilter);
        if self.prepared_callbacks.contains_key(&key) {
            return Err(AidlCallbackStoreError::PreparedArtifactInFlight);
        }
        let token = self.next_prepared_token()?;
        self.prepared_callbacks
            .insert(key, (token.0, StoredCallback::Filter(callback.clone())));
        Ok(token)
    }

    pub(crate) fn prepare_dvr_callback(
        &mut self,
        handle: AidlObjectHandle,
        callback: &Strong<dyn IDvrCallback>,
    ) -> Result<PreparedCallbackArtifactToken, AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, AidlApi::DemuxOpenDvr);
        if self.prepared_callbacks.contains_key(&key) {
            return Err(AidlCallbackStoreError::PreparedArtifactInFlight);
        }
        let token = self.next_prepared_token()?;
        self.prepared_callbacks
            .insert(key, (token.0, StoredCallback::Dvr(callback.clone())));
        Ok(token)
    }

    fn next_prepared_token(
        &mut self,
    ) -> Result<PreparedCallbackArtifactToken, AidlCallbackStoreError> {
        if self.retirement_batch.is_some() || !self.retired_callbacks.is_empty() {
            return Err(AidlCallbackStoreError::RetirementPending);
        }
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
    ) -> Result<(), AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, registration_api);
        if !self
            .prepared_callbacks
            .get(&key)
            .is_some_and(|(prepared, _)| *prepared == token.0)
        {
            return Err(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch);
        }
        let (_, callback) = self
            .prepared_callbacks
            .remove(&key)
            .ok_or(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch)?;
        if let Some(previous) = self.callbacks.insert(key, callback) {
            self.retired_callbacks.push(previous);
        }
        Ok(())
    }

    pub(crate) fn abort_prepared_callback(
        &mut self,
        handle: AidlObjectHandle,
        registration_api: AidlApi,
        token: PreparedCallbackArtifactToken,
    ) -> Result<(), AidlCallbackStoreError> {
        let key = CallbackStoreKey::new(handle, registration_api);
        if !self
            .prepared_callbacks
            .get(&key)
            .is_some_and(|(prepared, _)| *prepared == token.0)
        {
            return Err(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch);
        }
        let (_, callback) = self
            .prepared_callbacks
            .remove(&key)
            .ok_or(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch)?;
        self.retired_callbacks.push(callback);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn retain_dvr_callback_for_test(
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
        let keys: Vec<_> = self
            .callbacks
            .keys()
            .filter(|key| key.matches_owner(handle))
            .cloned()
            .collect();
        for key in keys {
            if let Some(callback) = self.callbacks.remove(&key) {
                self.retired_callbacks.push(callback);
            }
        }
        let keys: Vec<_> = self
            .prepared_callbacks
            .keys()
            .filter(|key| key.matches_owner(handle))
            .cloned()
            .collect();
        for key in keys {
            if let Some((_, callback)) = self.prepared_callbacks.remove(&key) {
                self.retired_callbacks.push(callback);
            }
        }
        before.saturating_sub(self.callbacks.len() + self.prepared_callbacks.len())
    }

    pub(crate) fn clear_all_callbacks(&mut self) -> usize {
        let before = self.callbacks.len() + self.prepared_callbacks.len();
        self.retired_callbacks
            .extend(std::mem::take(&mut self.callbacks).into_values());
        self.retired_callbacks.extend(
            std::mem::take(&mut self.prepared_callbacks)
                .into_values()
                .map(|(_, callback)| callback),
        );
        before
    }

    pub(crate) fn take_retired_callbacks(
        &mut self,
    ) -> Result<Option<RetiredCallbacks>, AidlCallbackStoreError> {
        if self.retirement_running {
            return Err(AidlCallbackStoreError::RetirementPending);
        }
        if self.retirement_batch.is_none() && !self.retired_callbacks.is_empty() {
            self.retirement_batch = Some(Arc::new(std::mem::take(&mut self.retired_callbacks)));
        }
        let Some(batch) = self.retirement_batch.as_ref() else {
            return Ok(None);
        };
        self.retirement_running = true;
        Ok(Some(RetiredCallbacks(Arc::clone(batch))))
    }

    pub(crate) fn finish_retired_callbacks(
        &mut self,
        retired: RetiredCallbacks,
        success: bool,
    ) -> (ReleasedCallbacks, Result<(), AidlCallbackStoreError>) {
        let matches = self.retirement_running
            && self
                .retirement_batch
                .as_ref()
                .is_some_and(|batch| Arc::ptr_eq(batch, &retired.0));
        let released = ReleasedCallbacks {
            _callbacks: retired.0,
        };
        if !matches {
            return (
                released,
                Err(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch),
            );
        }
        if success {
            self.retirement_batch = None;
        }
        self.retirement_running = false;
        (released, Ok(()))
    }

    pub(crate) fn prepared_frontend_registration(
        &self,
        handle: AidlObjectHandle,
        token: &PreparedCallbackArtifactToken,
    ) -> Option<Arc<FrontendCallbackRegistration>> {
        match self
            .prepared_callbacks
            .get(&CallbackStoreKey::new(handle, AidlApi::FrontendSetCallback))
        {
            Some((id, StoredCallback::Frontend(registration))) if *id == token.0 => {
                Some(Arc::clone(registration))
            }
            _ => None,
        }
    }

    pub(crate) fn frontend_registration_matches(
        &self,
        handle: AidlObjectHandle,
        generation: FrontendCallbackGeneration,
    ) -> bool {
        matches!(self.callbacks.get(&CallbackStoreKey::new(handle, AidlApi::FrontendSetCallback)),
            Some(StoredCallback::Frontend(registration)) if registration.generation == generation)
    }

    pub(crate) fn retire_frontend_registration(
        &mut self,
        handle: AidlObjectHandle,
        generation: FrontendCallbackGeneration,
    ) -> bool {
        if !self.frontend_registration_matches(handle, generation) {
            return false;
        }
        if let Some(callback) = self
            .callbacks
            .remove(&CallbackStoreKey::new(handle, AidlApi::FrontendSetCallback))
        {
            self.retired_callbacks.push(callback);
            true
        } else {
            false
        }
    }

    pub(crate) fn frontend_callback_for_owner(
        &self,
        handle: AidlObjectHandle,
    ) -> Option<FrontendCallbackDelivery> {
        match self
            .callbacks
            .get(&CallbackStoreKey::new(handle, AidlApi::FrontendSetCallback))
        {
            Some(StoredCallback::Frontend(registration))
                if !registration.dead.load(Ordering::Acquire) =>
            {
                Some(FrontendCallbackDelivery(Arc::clone(registration)))
            }
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
            (token.0, StoredCallback::TestMarker),
        );
        token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::{
        FrontendEventType::FrontendEventType, FrontendScanMessage::FrontendScanMessage,
        FrontendScanMessageType::FrontendScanMessageType, IFrontendCallback::BnFrontendCallback,
    };

    struct TestFrontendCallback;
    impl Interface for TestFrontendCallback {}
    impl IFrontendCallback for TestFrontendCallback {
        fn onEvent(&self, _event: FrontendEventType) -> binder::Result<()> {
            Ok(())
        }
        fn onScanMessage(
            &self,
            _kind: FrontendScanMessageType,
            _message: &FrontendScanMessage,
        ) -> binder::Result<()> {
            Ok(())
        }
    }

    fn frontend_callback() -> Strong<dyn IFrontendCallback> {
        BnFrontendCallback::new_binder(TestFrontendCallback, binder::BinderFeatures::default())
    }

    struct CallbackDropProbe {
        owner: std::sync::Weak<Mutex<CallbackStore>>,
        dropped_outside_lock: Arc<AtomicBool>,
    }
    impl Interface for CallbackDropProbe {}
    impl IFrontendCallback for CallbackDropProbe {
        fn onEvent(&self, _event: FrontendEventType) -> binder::Result<()> {
            Ok(())
        }
        fn onScanMessage(
            &self,
            _kind: FrontendScanMessageType,
            _message: &FrontendScanMessage,
        ) -> binder::Result<()> {
            Ok(())
        }
    }
    impl Drop for CallbackDropProbe {
        fn drop(&mut self) {
            if let Some(owner) = self.owner.upgrade() {
                self.dropped_outside_lock
                    .store(owner.try_lock().is_ok(), Ordering::Release);
            }
        }
    }

    #[test]
    fn death_waits_for_registration_commit_gate_and_is_seen_by_the_next_commit() {
        let mut store = CallbackStore::default();
        let handle = frontend_handle();
        let callback = frontend_callback();
        let token = store.prepare_frontend_callback(handle, &callback).unwrap();
        let registration = store
            .prepared_frontend_registration(handle, &token)
            .unwrap();
        let guard = registration.lock_death_gate().unwrap();
        let dying = Arc::clone(&registration);
        let (started, observed) = std::sync::mpsc::channel();
        let death = std::thread::spawn(move || {
            started.send(()).unwrap();
            mark_callback_dead(&dying.dead, &dying.death_gate);
        });
        observed.recv().unwrap();
        assert!(!registration.is_dead());
        store
            .commit_prepared_callback(handle, AidlApi::FrontendSetCallback, token)
            .unwrap();
        drop(guard);
        death.join().unwrap();
        let _next_commit = registration.lock_death_gate().unwrap();
        assert!(registration.is_dead());
        assert!(store.retire_frontend_registration(handle, registration.generation()));
    }

    #[test]
    fn already_unlinked_statuses_do_not_depend_on_death_notification_delivery() {
        let generation = FrontendCallbackGeneration(1);
        assert_eq!(death_unlink_result(generation, Ok(())), Ok(()));
        assert_eq!(
            death_unlink_result(generation, Err(StatusCode::NAME_NOT_FOUND)),
            Ok(())
        );
        assert_eq!(
            death_unlink_result(generation, Err(StatusCode::DEAD_OBJECT)),
            Ok(())
        );
        assert!(matches!(
            death_unlink_result(generation, Err(StatusCode::INVALID_OPERATION)),
            Err(AidlCallbackStoreError::DeathUnlink { .. })
        ));
    }

    #[test]
    fn final_callback_reference_is_released_after_owner_lock() {
        let owner = Arc::new(Mutex::new(CallbackStore::default()));
        let dropped = Arc::new(AtomicBool::new(false));
        let callback = BnFrontendCallback::new_binder(
            CallbackDropProbe {
                owner: Arc::downgrade(&owner),
                dropped_outside_lock: Arc::clone(&dropped),
            },
            binder::BinderFeatures::default(),
        );
        let handle = frontend_handle();
        let token = owner
            .lock()
            .unwrap()
            .prepare_frontend_callback(handle, &callback)
            .unwrap();
        drop(callback);
        owner
            .lock()
            .unwrap()
            .abort_prepared_callback(handle, AidlApi::FrontendSetCallback, token)
            .unwrap();
        let batch = owner
            .lock()
            .unwrap()
            .take_retired_callbacks()
            .unwrap()
            .unwrap();
        assert!(batch.release().is_ok());
        let (released, finish) = owner.lock().unwrap().finish_retired_callbacks(batch, true);
        assert!(finish.is_ok());
        assert!(!dropped.load(Ordering::Acquire));
        drop(released);
        assert!(dropped.load(Ordering::Acquire));
    }

    fn release_retired(store: &mut CallbackStore) {
        while let Some(batch) = store.take_retired_callbacks().unwrap() {
            assert!(batch.release().is_ok());
            let (released, finished) = store.finish_retired_callbacks(batch, true);
            assert!(finished.is_ok());
            drop(released);
        }
    }

    #[test]
    fn replaced_callback_death_cannot_retire_current_registration() {
        let mut store = CallbackStore::default();
        let handle = frontend_handle();
        let callback = frontend_callback();
        let token = store.prepare_frontend_callback(handle, &callback).unwrap();
        store
            .commit_prepared_callback(handle, AidlApi::FrontendSetCallback, token)
            .unwrap();
        let old = store.frontend_callback_for_owner(handle).unwrap();
        // 同じBinder identityの再登録にも新しい登録世代を発行する。
        let token = store.prepare_frontend_callback(handle, &callback).unwrap();
        store
            .commit_prepared_callback(handle, AidlApi::FrontendSetCallback, token)
            .unwrap();
        let current = store.frontend_callback_for_owner(handle).unwrap();
        assert_ne!(old.generation(), current.generation());
        assert!(!store.retire_frontend_registration(handle, old.generation()));
        assert_eq!(
            store
                .frontend_callback_for_owner(handle)
                .unwrap()
                .generation(),
            current.generation()
        );
        assert!(store.retire_frontend_registration(handle, current.generation()));
        assert!(store.frontend_callback_for_owner(handle).is_none());
        release_retired(&mut store);
    }

    #[test]
    fn retired_cleanup_failure_keeps_committed_replacement_current() {
        let mut store = CallbackStore::default();
        let handle = frontend_handle();
        let callback = frontend_callback();
        let old = store.prepare_frontend_callback(handle, &callback).unwrap();
        store
            .commit_prepared_callback(handle, AidlApi::FrontendSetCallback, old)
            .unwrap();
        let old_generation = store
            .frontend_callback_for_owner(handle)
            .unwrap()
            .generation();
        let new = store.prepare_frontend_callback(handle, &callback).unwrap();
        store
            .commit_prepared_callback(handle, AidlApi::FrontendSetCallback, new)
            .unwrap();
        let current_generation = store
            .frontend_callback_for_owner(handle)
            .unwrap()
            .generation();
        assert_ne!(old_generation, current_generation);
        let batch = store.take_retired_callbacks().unwrap().unwrap();
        let (released, result) = store.finish_retired_callbacks(batch, false);
        assert!(result.is_ok());
        drop(released);
        assert_eq!(
            store.frontend_callback_for_owner(handle).unwrap().generation(),
            current_generation
        );
        assert_eq!(
            store.prepare_frontend_callback(handle, &callback),
            Err(AidlCallbackStoreError::RetirementPending)
        );
        release_retired(&mut store);
        assert_eq!(
            store.frontend_callback_for_owner(handle).unwrap().generation(),
            current_generation
        );
    }

    #[test]
    fn retired_artifact_is_retained_until_explicit_success_and_can_retry() {
        let mut store = CallbackStore::default();
        let handle = frontend_handle();
        let callback = frontend_callback();
        let token = store.prepare_frontend_callback(handle, &callback).unwrap();
        let registration = store
            .prepared_frontend_registration(handle, &token)
            .unwrap();
        let retained = Arc::downgrade(&registration);
        drop(registration);
        store
            .abort_prepared_callback(handle, AidlApi::FrontendSetCallback, token)
            .unwrap();
        let batch = store.take_retired_callbacks().unwrap().unwrap();
        assert_eq!(
            store.prepare_frontend_callback(handle, &callback),
            Err(AidlCallbackStoreError::RetirementPending)
        );
        let (released, finished) = store.finish_retired_callbacks(batch, false);
        assert!(finished.is_ok());
        drop(released);
        assert!(retained.upgrade().is_some());
        release_retired(&mut store);
        assert!(retained.upgrade().is_none());
        assert!(store.prepare_frontend_callback(handle, &callback).is_ok());
    }

    #[test]
    fn checked_callback_generation_exhaustion_preserves_current_registration() {
        let mut store = CallbackStore::default();
        let handle = frontend_handle();
        let callback = frontend_callback();
        let token = store.prepare_frontend_callback(handle, &callback).unwrap();
        store
            .commit_prepared_callback(handle, AidlApi::FrontendSetCallback, token)
            .unwrap();
        let generation = store
            .frontend_callback_for_owner(handle)
            .unwrap()
            .generation();
        store.next_prepared_token = u64::MAX;
        assert_eq!(
            store.prepare_frontend_callback(handle, &callback),
            Err(AidlCallbackStoreError::PreparedTokenExhausted)
        );
        assert_eq!(
            store
                .frontend_callback_for_owner(handle)
                .unwrap()
                .generation(),
            generation
        );
    }

    #[test]
    fn prepared_callback_artifact_token_is_single_use() {
        static_assertions::assert_not_impl_any!(PreparedCallbackArtifactToken: Clone, Copy);
        static_assertions::assert_not_impl_any!(RetiredCallbacks: Clone, Copy);
        static_assertions::assert_impl_all!(FrontendCallbackRegistration: Send, Sync);
    }

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

        assert_eq!(
            store.abort_prepared_callback(handle, AidlApi::FrontendSetCallback, token),
            Ok(())
        );
        assert!(store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.prepared_callbacks.is_empty());
    }

    #[test]
    fn prepared_replacement_becomes_current_only_at_commit() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);

        assert!(!store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert_eq!(
            store.commit_prepared_callback(handle, AidlApi::FrontendSetCallback, token),
            Ok(())
        );
        assert!(store.has_callback_for_owner(handle, AidlApi::FrontendSetCallback));
        assert!(store.prepared_callbacks.is_empty());
    }

    #[test]
    fn stale_prepared_token_cannot_commit_or_abort_another_artifact() {
        let handle = frontend_handle();
        let mut store = CallbackStore::default();
        let token = store.prepare_test_callback_marker(handle, AidlApi::FrontendSetCallback);
        let stale_id = token.0 + 1;

        assert_eq!(
            store.commit_prepared_callback(
                handle,
                AidlApi::FrontendSetCallback,
                PreparedCallbackArtifactToken(stale_id),
            ),
            Err(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch)
        );
        assert_eq!(
            store.abort_prepared_callback(
                handle,
                AidlApi::FrontendSetCallback,
                PreparedCallbackArtifactToken(stale_id),
            ),
            Err(AidlCallbackStoreError::PreparedArtifactAuthorityMismatch)
        );
        assert_eq!(
            store.abort_prepared_callback(handle, AidlApi::FrontendSetCallback, token),
            Ok(())
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AidlCallbackStoreError {
    Poisoned,
    PreparedArtifactInFlight,
    PreparedArtifactAuthorityMismatch,
    PreparedTokenExhausted,
    RetirementPending,
    DeathLink(StatusCode),
    DeathUnlink {
        generation: FrontendCallbackGeneration,
        status: StatusCode,
    },
    Composed {
        primary: Box<Self>,
        cleanup: Box<Self>,
    },
}

impl AidlCallbackStoreError {
    pub(crate) fn into_hal_error(self, context: &'static str) -> HalError {
        match self {
            Self::RetirementPending => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: 古いcallbackの解放が未完了です"),
            ),
            Self::DeathLink(error) => HalError::callback_failed(
                "linkToDeath",
                format!("{context}: Binder死亡通知の登録に失敗しました: {error:?}"),
            ),
            Self::Composed { primary, cleanup } => maleicacid_tuner_hal2_common::compose_primary_cleanup_failure(context, primary.into_hal_error(context), cleanup.into_hal_error(context)),
            Self::DeathUnlink { generation, status } => HalError::callback_failed(
                "unlinkToDeath",
                format!("{context}: Binder死亡通知の解除に失敗しました generation={generation:?}: {status:?}"),
            ),
            Self::Poisoned => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: callback store lock poisoned"),
            ),
            Self::PreparedArtifactInFlight => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: callback registration is already in flight"),
            ),
            Self::PreparedArtifactAuthorityMismatch => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: prepared callback artifact authority is stale or missing"),
            ),
            Self::PreparedTokenExhausted => HalError::internal(
                HalInternalKind::InvariantViolation,
                format!("{context}: prepared callback artifact token exhausted"),
            ),
        }
    }
}
