use std::collections::BTreeMap;
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
    RefreshGenerationExhausted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DescramblerKeyRefreshRequest {
    token: DescramblerKeyToken,
    slot: DescramblerKeySlotId,
    generation: u64,
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
    fn new(token: DescramblerKeyToken, slot: DescramblerKeySlotId, generation: u64) -> Self {
        Self {
            token,
            slot,
            generation,
        }
    }

    pub(crate) fn token(&self) -> &DescramblerKeyToken {
        &self.token
    }

    pub(crate) fn slot(&self) -> DescramblerKeySlotId {
        self.slot
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescramblerKeySlotState {
    slot: DescramblerKeySlotId,
    key_slot: Option<DescramblerKeySlot>,
    refresh_generation: u64,
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
    pub fn has_token_resolution_state(&self) -> bool {
        if !self.slots.is_empty() {
            return true;
        }
        #[cfg(test)]
        {
            !self.expired.is_empty()
        }
        #[cfg(not(test))]
        {
            false
        }
    }

    pub fn key_slot(&self, slot_id: DescramblerKeySlotId) -> Option<DescramblerKeySlot> {
        self.slots
            .values()
            .find(|state| state.slot == slot_id && !state.expired)
            .and_then(|state| state.key_slot.clone())
    }

    pub(crate) fn publish(
        &mut self,
        token: DescramblerKeyToken,
        key_slot: DescramblerKeySlot,
    ) -> Result<DescramblerKeySlotId, DescramblerKeyPublishError> {
        if let Some(state) = self.slots.get_mut(&token) {
            state.refresh_generation = state
                .refresh_generation
                .checked_add(1)
                .ok_or(DescramblerKeyPublishError::RefreshGenerationExhausted)?;
            state.key_slot = Some(key_slot);
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
                slot,
                key_slot: Some(key_slot),
                refresh_generation: 1,
                refcount: 0,
                expired: false,
            },
        );
        Ok(slot)
    }

    pub(crate) fn begin_refresh(
        &mut self,
        token: &DescramblerKeyToken,
        slot: DescramblerKeySlotId,
    ) -> Option<DescramblerKeyRefreshRequest> {
        let state = self.slots.get_mut(token)?;
        if state.slot != slot || state.refcount == 0 || state.expired {
            return None;
        }
        let Some(generation) = state.refresh_generation.checked_add(1) else {
            state.key_slot = None;
            return None;
        };
        state.refresh_generation = generation;
        state.key_slot = None;
        Some(DescramblerKeyRefreshRequest::new(
            token.clone(),
            slot,
            generation,
        ))
    }

    pub(crate) fn apply_refresh(
        &mut self,
        request: &DescramblerKeyRefreshRequest,
        key_slot: Option<DescramblerKeySlot>,
    ) -> bool {
        let Some(state) = self.slots.get_mut(request.token()) else {
            return false;
        };
        if state.slot != request.slot()
            || state.refresh_generation != request.generation
            || state.refcount == 0
            || state.expired
        {
            return false;
        }
        state.key_slot = key_slot;
        let Some(next_generation) = state.refresh_generation.checked_add(1) else {
            state.key_slot = None;
            return false;
        };
        state.refresh_generation = next_generation;
        true
    }

    pub(crate) fn resolve_packet_keys(
        &mut self,
        refreshes: Vec<(DescramblerKeyRefreshRequest, Option<DescramblerKeySlot>)>,
    ) -> DescramblerPacketKeys {
        let mut keys = DescramblerPacketKeys::default();
        for (request, key_slot) in refreshes {
            if self.apply_refresh(&request, key_slot.clone()) {
                if let Some(key_slot) = key_slot {
                    keys.slots.insert(request.slot(), key_slot);
                }
            }
        }
        keys
    }

    #[cfg(test)]
    pub(crate) fn packet_keys_for_test(&self) -> DescramblerPacketKeys {
        DescramblerPacketKeys {
            slots: self.slots.values().filter(|state| !state.expired)
                .filter_map(|state| state.key_slot.clone().map(|key| (state.slot, key))).collect(),
        }
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
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_descrambler::{KeyParity, Multi2KeyMaterial};

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
            self.insert_test_key_slot(token, slot, DescramblerKeySlot::empty());
        }

        pub(crate) fn insert_test_key_slot(
            &mut self,
            token: DescramblerKeyToken,
            slot: DescramblerKeySlotId,
            key_slot: DescramblerKeySlot,
        ) {
            self.expired.remove(&token);
            self.slots.insert(
                token,
                DescramblerKeySlotState {
                    slot,
                    key_slot: Some(key_slot),
                    refresh_generation: 1,
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
    fn refresh_replaces_or_invalidates_material_for_the_stable_slot() {
        let token = DescramblerKeyToken::try_from_bytes(vec![3; 16]).unwrap();
        let mut table = DescramblerKeyTable::default();
        let slot = table.publish(token.clone(), key_slot(1)).unwrap();
        assert_eq!(table.acquire(&token), Ok(slot));
        let request = table.begin_refresh(&token, slot).unwrap();

        table.apply_refresh(&request, Some(key_slot(9)));
        assert!(table
            .key_slot(slot)
            .and_then(|keys| keys.key_for(KeyParity::Even).cloned())
            .is_some());
        let invalidation = table.begin_refresh(&token, slot).unwrap();
        table.apply_refresh(&invalidation, None);
        assert_eq!(table.key_slot(slot), None);
    }

    #[test]
    fn stale_refresh_cannot_modify_a_republished_token() {
        let token = DescramblerKeyToken::try_from_bytes(vec![4; 16]).unwrap();
        let mut table = DescramblerKeyTable::default();
        let old_slot = table.publish(token.clone(), key_slot(2)).unwrap();
        assert_eq!(table.acquire(&token), Ok(old_slot));
        let stale_request = table.begin_refresh(&token, old_slot).unwrap();
        assert_eq!(table.release(&token), Ok(()));

        let new_slot = table.publish(token.clone(), key_slot(7)).unwrap();
        assert_ne!(new_slot, old_slot);
        assert_eq!(table.acquire(&token), Ok(new_slot));
        table.apply_refresh(&stale_request, None);
        assert!(table.key_slot(new_slot).is_some());
    }

    #[test]
    fn newer_publication_fences_an_in_flight_refresh_for_the_same_slot() {
        let token = DescramblerKeyToken::try_from_bytes(vec![6; 16]).unwrap();
        let mut table = DescramblerKeyTable::default();
        let slot = table.publish(token.clone(), key_slot(1)).unwrap();
        assert_eq!(table.acquire(&token), Ok(slot));
        let stale_request = table.begin_refresh(&token, slot).unwrap();

        assert_eq!(table.publish(token.clone(), key_slot(8)), Ok(slot));
        table.apply_refresh(&stale_request, None);
        assert!(table.key_slot(slot).is_some());
    }

    #[test]
    fn overlapping_refresh_failure_has_no_packet_key_while_newer_query_is_pending() {
        let token = DescramblerKeyToken::try_from_bytes(vec![0x21; 16]).unwrap();
        let mut table = DescramblerKeyTable::default();
        let slot = table.publish(token.clone(), key_slot(1)).unwrap();
        table.acquire(&token).unwrap();
        let first = table.begin_refresh(&token, slot).unwrap();
        let second = table.begin_refresh(&token, slot).unwrap();
        let failed = table.resolve_packet_keys(vec![(first, None)]);
        assert!(failed.key_slot(slot).is_none());
        assert!(table.key_slot(slot).is_none());
        let current = table.resolve_packet_keys(vec![(second, Some(key_slot(2)))]);
        assert_eq!(current.key_slot(slot), Some(key_slot(2)));
        assert!(failed.key_slot(slot).is_none());
    }

    #[test]
    fn overtaken_refresh_never_uses_material_published_by_another_call() {
        for response in [None, Some(key_slot(1))] {
            let token = DescramblerKeyToken::try_from_bytes(vec![0x22; 16]).unwrap();
            let mut table = DescramblerKeyTable::default();
            let slot = table.publish(token.clone(), key_slot(1)).unwrap();
            table.acquire(&token).unwrap();
            let stale = table.begin_refresh(&token, slot).unwrap();
            table.publish(token, key_slot(9)).unwrap();
            let packet = table.resolve_packet_keys(vec![(stale, response)]);
            assert!(packet.key_slot(slot).is_none());
            assert_eq!(table.key_slot(slot), Some(key_slot(9)));
        }
    }

    #[test]
    fn released_slot_rejects_delayed_packet_material() {
        let token = DescramblerKeyToken::try_from_bytes(vec![0x23; 16]).unwrap();
        let mut table = DescramblerKeyTable::default();
        let slot = table.publish(token.clone(), key_slot(1)).unwrap();
        table.acquire(&token).unwrap();
        let stale = table.begin_refresh(&token, slot).unwrap();
        table.release(&token).unwrap();
        let packet = table.resolve_packet_keys(vec![(stale, Some(key_slot(1)))]);
        assert!(packet.key_slot(slot).is_none());
    }

    #[test]
    fn failed_binding_can_discard_an_unreferenced_publication() {
        let token = DescramblerKeyToken::try_from_bytes(vec![5; 16]).unwrap();
        let mut table = DescramblerKeyTable::default();
        let slot = table.publish(token.clone(), key_slot(3)).unwrap();
        table.discard_if_unreferenced(&token, slot);
        assert_eq!(
            table.acquire(&token),
            Err(DescramblerKeyLookupError::UnknownToken)
        );
    }
}
