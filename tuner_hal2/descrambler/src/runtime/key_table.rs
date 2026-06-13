use std::collections::BTreeMap;

use super::token::DescramblerKeyToken;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerKeySlotId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyLookupError {
    UnknownToken,
    ExpiredToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DescramblerKeySlotState {
    Active(DescramblerKeySlotId),
    Expired(DescramblerKeySlotId),
}

#[derive(Debug, Default)]
pub struct DescramblerKeyTable {
    slots: BTreeMap<DescramblerKeyToken, DescramblerKeySlotState>,
}

impl DescramblerKeyTable {
    pub fn resolve(&self, token: &DescramblerKeyToken) -> Result<DescramblerKeySlotId, DescramblerKeyLookupError> {
        match self.slots.get(token).copied() {
            Some(DescramblerKeySlotState::Active(slot)) => Ok(slot),
            Some(DescramblerKeySlotState::Expired(_)) => Err(DescramblerKeyLookupError::ExpiredToken),
            None => Err(DescramblerKeyLookupError::UnknownToken),
        }
    }

    #[cfg(test)]
    pub(crate) fn insert_test_key(&mut self, token: DescramblerKeyToken, slot: DescramblerKeySlotId) {
        self.slots.insert(token, DescramblerKeySlotState::Active(slot));
    }

    #[cfg(test)]
    pub(crate) fn expire_test_key(&mut self, token: &DescramblerKeyToken) {
        if let Some(DescramblerKeySlotState::Active(slot)) = self.slots.get(token).copied() {
            self.slots.insert(token.clone(), DescramblerKeySlotState::Expired(slot));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_table_distinguishes_unknown_and_expired_tokens() {
        let token = DescramblerKeyToken::try_from_bytes(vec![1, 2, 3]).unwrap();
        let mut table = DescramblerKeyTable::default();
        assert_eq!(table.resolve(&token), Err(DescramblerKeyLookupError::UnknownToken));
        table.insert_test_key(token.clone(), DescramblerKeySlotId(7));
        assert_eq!(table.resolve(&token), Ok(DescramblerKeySlotId(7)));
        table.expire_test_key(&token);
        assert_eq!(table.resolve(&token), Err(DescramblerKeyLookupError::ExpiredToken));
    }
}
