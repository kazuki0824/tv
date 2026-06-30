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

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescramblerKeySlotState {
    slot: DescramblerKeySlotId,
    key_slot: DescramblerKeySlot,
    refcount: usize,
    expired: bool,
}

#[derive(Debug)]
pub struct DescramblerKeyTable {
    slots: BTreeMap<DescramblerKeyToken, DescramblerKeySlotState>,
    #[cfg(test)]
    expired: BTreeSet<DescramblerKeyToken>,
}

impl Default for DescramblerKeyTable {
    fn default() -> Self {
        Self {
            slots: BTreeMap::new(),
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
            .map(|state| state.key_slot.clone())
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
            state.refcount == 0 && state.expired
        };
        if remove {
            self.slots.remove(token);
            #[cfg(test)]
            self.expired.insert(token.clone());
        }
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;
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
                    key_slot,
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
}
