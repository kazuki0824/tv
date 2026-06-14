use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;

use crate::core::DescramblerKeySlot;

use super::token::DescramblerKeyToken;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerKeySlotId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyLookupError {
    UnknownToken,
    ExpiredToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyRegistrationError {
    EmptySlot,
    DuplicateToken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescramblerKeySlotState {
    slot: DescramblerKeySlotId,
    key_slot: DescramblerKeySlot,
    refcount: usize,
    expired: bool,
}

#[derive(Debug, Default)]
pub struct DescramblerKeyTable {
    slots: BTreeMap<DescramblerKeyToken, DescramblerKeySlotState>,
    #[cfg(test)]
    expired: BTreeSet<DescramblerKeyToken>,
}

impl DescramblerKeyTable {
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn resolve(
        &self,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlotId, DescramblerKeyLookupError> {
        #[cfg(test)]
        if self.expired.contains(token) {
            return Err(DescramblerKeyLookupError::ExpiredToken);
        }
        match self.slots.get(token).cloned() {
            Some(state) if state.expired => Err(DescramblerKeyLookupError::ExpiredToken),
            Some(state) => Ok(state.slot),
            None => Err(DescramblerKeyLookupError::UnknownToken),
        }
    }

    pub fn key_slot(&self, slot_id: DescramblerKeySlotId) -> Option<DescramblerKeySlot> {
        self.slots
            .values()
            .find(|state| state.slot == slot_id && !state.expired)
            .map(|state| state.key_slot.clone())
    }

    pub fn register_key_slot(
        &mut self,
        token: DescramblerKeyToken,
        key_slot: DescramblerKeySlot,
    ) -> Result<DescramblerKeySlotId, DescramblerKeyRegistrationError> {
        if !key_slot.has_any_key() {
            return Err(DescramblerKeyRegistrationError::EmptySlot);
        }
        if self.slots.contains_key(&token) {
            return Err(DescramblerKeyRegistrationError::DuplicateToken);
        }
        let slot_id = DescramblerKeySlotId(token.stable_slot_id());
        self.slots.insert(
            token,
            DescramblerKeySlotState {
                slot: slot_id,
                key_slot,
                refcount: 0,
                expired: false,
            },
        );
        Ok(slot_id)
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

    pub(crate) fn expire_for_test_support(&mut self, token: &DescramblerKeyToken) {
        #[cfg(test)]
        {
            self.expire_test_key(token);
        }
        #[cfg(not(test))]
        {
            if let Some(state) = self.slots.get_mut(token) {
                if state.refcount == 0 {
                    self.slots.remove(token);
                } else {
                    state.expired = true;
                }
            }
        }
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
            table.resolve(&token),
            Err(DescramblerKeyLookupError::UnknownToken)
        );
        table.insert_test_key(token.clone(), DescramblerKeySlotId(7));
        assert_eq!(table.resolve(&token), Ok(DescramblerKeySlotId(7)));
        table.expire_test_key(&token);
        assert_eq!(
            table.resolve(&token),
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
            table.resolve(&token),
            Err(DescramblerKeyLookupError::ExpiredToken)
        );
    }
}
