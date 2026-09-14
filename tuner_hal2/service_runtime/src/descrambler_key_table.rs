use std::collections::BTreeMap;
use std::sync::Arc;
use maleicacid_tuner_hal2_descrambler::CasKeyReference;
#[cfg(test)]
use std::collections::BTreeSet;

use maleicacid_tuner_hal2_descrambler::DescramblerKeySlot;

use maleicacid_tuner_hal2_descrambler::DescramblerKeyToken;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerKeySlotId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyLookupError {
    UnknownToken,
    ExpiredToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DescramblerKeyPublishError {
    SlotIdExhausted,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct DescramblerKeyRefreshRequest {
    token: DescramblerKeyToken,
    slot: DescramblerKeySlotId,
}

#[derive(Default)]
pub(crate) struct DescramblerPacketKeys {
    slots: BTreeMap<DescramblerKeySlotId, DescramblerKeySlot>,
}

impl DescramblerPacketKeys {
    pub(crate) fn key_slot(&self, slot: DescramblerKeySlotId) -> Option<DescramblerKeySlot> {
        self.slots.get(&slot).cloned()
    }
}

impl DescramblerKeyRefreshRequest {
    fn new(token: DescramblerKeyToken, slot: DescramblerKeySlotId) -> Self {
        Self { token, slot }
    }

    pub(crate) fn token(&self) -> &DescramblerKeyToken {
        &self.token
    }

    pub(crate) fn slot(&self) -> DescramblerKeySlotId {
        self.slot
    }
}

#[derive(Clone, Debug)]
struct DescramblerKeySlotState {
    reference: Arc<dyn CasKeyReference>,
    slot: DescramblerKeySlotId,
    refcount: usize,
    expired: bool,
}

#[derive(Debug)]
pub struct DescramblerKeyTable {
    slots: BTreeMap<DescramblerKeyToken, DescramblerKeySlotState>,
    next_slot: u64,
    #[cfg(test)]
    expired: BTreeSet<DescramblerKeyToken>,
}

impl Default for DescramblerKeyTable {
    fn default() -> Self {
        Self {
            slots: BTreeMap::new(),
            next_slot: 1,
            #[cfg(test)]
            expired: BTreeSet::new(),
        }
    }
}

impl DescramblerKeyTable {
    #[cfg(test)]
    pub fn has_token_resolution_state(&self) -> bool {
        !self.slots.is_empty() || !self.expired.is_empty()
    }

    pub(crate) fn publish(
        &mut self,
        token: DescramblerKeyToken,
        reference: Arc<dyn CasKeyReference>,
    ) -> Result<DescramblerKeySlotId, DescramblerKeyPublishError> {
        if let Some(state) = self.slots.get_mut(&token) {
            state.expired = false;
            return Ok(state.slot);
        }
        let slot = DescramblerKeySlotId(self.next_slot);
        self.next_slot = self
            .next_slot
            .checked_add(1)
            .ok_or(DescramblerKeyPublishError::SlotIdExhausted)?;
        self.slots.insert(
            token,
            DescramblerKeySlotState {
                reference,
                slot,
                refcount: 0,
                expired: false,
            },
        );
        Ok(slot)
    }

    pub(crate) fn begin_refresh(
        &self,
        token: &DescramblerKeyToken,
        slot: DescramblerKeySlotId,
    ) -> Option<DescramblerKeyRefreshRequest> {
        let state = self.slots.get(token)?;
        if state.slot != slot || state.refcount == 0 || state.expired {
            return None;
        }
        Some(DescramblerKeyRefreshRequest::new(token.clone(), slot))
    }

    fn is_live_request(&self, request: &DescramblerKeyRefreshRequest) -> bool {
        let Some(state) = self.slots.get(request.token()) else {
            return false;
        };
        // 結び付けの寿命だけを検証する。同じ参照先の並行取得は互いに失効させない。
        state.slot == request.slot() && state.refcount > 0 && !state.expired
    }

    pub(crate) fn snapshot_packet_keys(
        &self,
        requests: Vec<DescramblerKeyRefreshRequest>,
    ) -> DescramblerPacketKeys {
        let mut keys = DescramblerPacketKeys::default();
        for request in requests {
            if self.is_live_request(&request) {
                if let Some(state) = self.slots.get(request.token()) {
                    if let Ok(snapshot) = state.reference.snapshot() {
                        keys.slots.insert(request.slot(), snapshot);
                    }
                }
            }
        }
        keys
    }

    pub(crate) fn discard_if_unreferenced(
        &mut self,
        token: &DescramblerKeyToken,
        slot: DescramblerKeySlotId,
    ) {
        let remove = self
            .slots
            .get(token)
            .is_some_and(|state| state.slot == slot && state.refcount == 0);
        if remove {
            self.slots.remove(token);
        }
    }

    pub fn acquire(
        &mut self,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlotId, DescramblerKeyLookupError> {
        #[cfg(test)]
        if self.expired.contains(token) {
            return Err(DescramblerKeyLookupError::ExpiredToken);
        }
        let state = self
            .slots
            .get_mut(token)
            .ok_or(DescramblerKeyLookupError::UnknownToken)?;
        if state.expired {
            return Err(DescramblerKeyLookupError::ExpiredToken);
        }
        state.refcount = state
            .refcount
            .checked_add(1)
            .ok_or(DescramblerKeyLookupError::ExpiredToken)?;
        Ok(state.slot)
    }

    pub fn release(
        &mut self,
        token: &DescramblerKeyToken,
    ) -> Result<(), DescramblerKeyLookupError> {
        let remove = {
            let state = self
                .slots
                .get_mut(token)
                .ok_or(DescramblerKeyLookupError::UnknownToken)?;
            if state.refcount == 0 {
                return Err(DescramblerKeyLookupError::ExpiredToken);
            }
            state.refcount -= 1;
            state.refcount == 0
        };
        if remove {
            #[cfg(test)]
            let expired = self.slots.get(token).is_some_and(|state| state.expired);
            self.slots.remove(token);
            #[cfg(test)]
            if expired {
                self.expired.insert(token.clone());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use maleicacid_tuner_hal2_descrambler::CasKeyResolveError;

    #[derive(Debug, Default)]
    pub(crate) struct TestKeyReference(pub std::sync::Mutex<Option<DescramblerKeySlot>>);

    impl CasKeyReference for TestKeyReference {
        fn snapshot(&self) -> Result<DescramblerKeySlot, CasKeyResolveError> {
            self.0.lock().unwrap().clone().ok_or(CasKeyResolveError::UnknownToken)
        }
    }

    pub(crate) fn empty_reference() -> Arc<dyn CasKeyReference> {
        Arc::new(TestKeyReference::default())
    }

    use maleicacid_tuner_hal2_descrambler::Multi2KeyMaterial;

    fn key_slot(byte: u8) -> DescramblerKeySlot {
        DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new(
                [byte; 32],
                [byte.wrapping_add(1); 8],
                [byte.wrapping_add(2); 8],
            ))
            .unwrap()
    }

    impl DescramblerKeyTable {
        pub(crate) fn insert_test_key(
            &mut self,
            token: DescramblerKeyToken,
            slot: DescramblerKeySlotId,
        ) {
            self.expired.remove(&token);
            self.slots.insert(
                token,
                DescramblerKeySlotState {
                    reference: empty_reference(),
                    slot,
                    refcount: 0,
                    expired: false,
                },
            );
        }

        pub(crate) fn expire_test_key(&mut self, token: &DescramblerKeyToken) {
            if let Some(state) = self.slots.get_mut(token) {
                if state.refcount == 0 {
                    self.slots.remove(token);
                    self.expired.insert(token.clone());
                } else {
                    state.expired = true;
                }
            } else {
                self.expired.insert(token.clone());
            }
        }

        pub(crate) fn refcount_for_test(&self, token: &DescramblerKeyToken) -> Option<usize> {
            self.slots.get(token).map(|state| state.refcount)
        }
    }

    #[test]
    fn key_table_distinguishes_unknown_and_expired_tokens() {
        let token = DescramblerKeyToken::try_from_bytes(vec![1; 8]).unwrap();
        let mut table = DescramblerKeyTable::default();
        assert_eq!(
            table.acquire(&token),
            Err(DescramblerKeyLookupError::UnknownToken)
        );
        assert!(!table.has_token_resolution_state());
        table.insert_test_key(token.clone(), DescramblerKeySlotId(7));
        assert!(table.has_token_resolution_state());
        assert_eq!(table.acquire(&token), Ok(DescramblerKeySlotId(7)));
        assert_eq!(table.release(&token), Ok(()));
        table.expire_test_key(&token);
        assert!(table.has_token_resolution_state());
        assert_eq!(
            table.acquire(&token),
            Err(DescramblerKeyLookupError::ExpiredToken)
        );
    }

    #[test]
    fn acquired_expired_token_is_removed_after_last_release() {
        let token = DescramblerKeyToken::try_from_bytes(vec![2; 8]).unwrap();
        let mut table = DescramblerKeyTable::default();
        table.insert_test_key(token.clone(), DescramblerKeySlotId(9));
        assert_eq!(table.acquire(&token), Ok(DescramblerKeySlotId(9)));
        assert_eq!(table.refcount_for_test(&token), Some(1));
        table.expire_test_key(&token);
        assert_eq!(
            table.acquire(&token),
            Err(DescramblerKeyLookupError::ExpiredToken)
        );
        assert_eq!(table.refcount_for_test(&token), Some(1));
        assert_eq!(table.release(&token), Ok(()));
        assert_eq!(table.refcount_for_test(&token), None);
        assert_eq!(
            table.acquire(&token),
            Err(DescramblerKeyLookupError::ExpiredToken)
        );
    }

    #[test]
    fn shared_slot_updates_revoke_and_recover_without_rebinding() {
        let token = DescramblerKeyToken::try_from_bytes(vec![3; 16]).unwrap();
        let mut table = DescramblerKeyTable::default();
        let reference = Arc::new(TestKeyReference(std::sync::Mutex::new(Some(key_slot(1)))));
        let slot = table.publish(token.clone(), reference.clone()).unwrap();
        assert_eq!(table.acquire(&token), Ok(slot));
        let request = table.begin_refresh(&token, slot).unwrap();
        let first = table.snapshot_packet_keys(vec![request]);
        for _ in 0..1000 {
            let request = table.begin_refresh(&token, slot).unwrap();
            assert_eq!(table.snapshot_packet_keys(vec![request]).key_slot(slot), Some(key_slot(1)));
        }
        *reference.0.lock().unwrap() = Some(key_slot(9));
        let request = table.begin_refresh(&token, slot).unwrap();
        assert_eq!(table.snapshot_packet_keys(vec![request]).key_slot(slot), Some(key_slot(9)));
        *reference.0.lock().unwrap() = None;
        let request = table.begin_refresh(&token, slot).unwrap();
        assert!(table.snapshot_packet_keys(vec![request]).key_slot(slot).is_none());
        assert_eq!(first.key_slot(slot), Some(key_slot(1)));
        *reference.0.lock().unwrap() = Some(key_slot(7));
        let request = table.begin_refresh(&token, slot).unwrap();
        assert_eq!(table.snapshot_packet_keys(vec![request]).key_slot(slot), Some(key_slot(7)));
        assert_eq!(table.refcount_for_test(&token), Some(1));
    }

    #[test]
    fn released_or_expired_binding_rejects_pending_packet_request() {
        for expire in [false, true] {
            let token = DescramblerKeyToken::try_from_bytes(vec![4; 16]).unwrap();
            let mut table = DescramblerKeyTable::default();
            let reference = Arc::new(TestKeyReference(std::sync::Mutex::new(Some(key_slot(1)))));
            let slot = table.publish(token.clone(), reference).unwrap();
            table.acquire(&token).unwrap();
            let stale = table.begin_refresh(&token, slot).unwrap();
            if expire { table.expire_test_key(&token); } else { table.release(&token).unwrap(); }
            assert!(table.snapshot_packet_keys(vec![stale]).key_slot(slot).is_none());
            if !expire {
                let new_slot = table.publish(token.clone(), empty_reference()).unwrap();
                assert_ne!(new_slot, slot);
                table.acquire(&token).unwrap();
                assert!(table.begin_refresh(&token, slot).is_none());
            }
        }
    }

    #[test]
    fn another_consumer_keeps_the_existing_shared_reference_and_refcount() {
        let token = DescramblerKeyToken::try_from_bytes(vec![6; 16]).unwrap();
        let mut table = DescramblerKeyTable::default();
        let reference = Arc::new(TestKeyReference(std::sync::Mutex::new(Some(key_slot(1)))));
        let slot = table.publish(token.clone(), reference.clone()).unwrap();
        table.acquire(&token).unwrap();
        let first = table.begin_refresh(&token, slot).unwrap();
        assert_eq!(table.publish(token.clone(), empty_reference()), Ok(slot));
        table.acquire(&token).unwrap();
        *reference.0.lock().unwrap() = Some(key_slot(8));
        assert_eq!(table.snapshot_packet_keys(vec![first]).key_slot(slot), Some(key_slot(8)));
        assert_eq!(table.refcount_for_test(&token), Some(2));
        table.release(&token).unwrap();
        let request = table.begin_refresh(&token, slot).unwrap();
        assert_eq!(table.snapshot_packet_keys(vec![request]).key_slot(slot), Some(key_slot(8)));
    }

    #[test]
    fn failed_binding_can_discard_an_unreferenced_publication() {
        let token = DescramblerKeyToken::try_from_bytes(vec![5; 16]).unwrap();
        let mut table = DescramblerKeyTable::default();
        let slot = table.publish(token.clone(), empty_reference()).unwrap();
        table.discard_if_unreferenced(&token, slot);
        assert_eq!(table.acquire(&token), Err(DescramblerKeyLookupError::UnknownToken));
    }
}
