use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;

use super::token::DescramblerKeyToken;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerKeySlotId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyLookupError {
    UnknownToken,
    ExpiredToken,
}

#[derive(Debug, Default)]
pub struct DescramblerKeyTable {
    slots: BTreeMap<DescramblerKeyToken, DescramblerKeySlotId>,
    #[cfg(test)]
    expired: BTreeSet<DescramblerKeyToken>,
}

impl DescramblerKeyTable {
    pub fn resolve(
        &self,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlotId, DescramblerKeyLookupError> {
        #[cfg(test)]
        if self.expired.contains(token) {
            return Err(DescramblerKeyLookupError::ExpiredToken);
        }
        match self.slots.get(token).copied() {
            Some(slot) => Ok(slot),
            None => Err(DescramblerKeyLookupError::UnknownToken),
        }
    }

    #[cfg(test)]
    pub(crate) fn insert_test_key(
        &mut self,
        token: DescramblerKeyToken,
        slot: DescramblerKeySlotId,
    ) {
        self.expired.remove(&token);
        self.slots.insert(token, slot);
    }

    #[cfg(test)]
    pub(crate) fn expire_test_key(&mut self, token: &DescramblerKeyToken) {
        if self.slots.remove(token).is_some() {
            self.expired.insert(token.clone());
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
}
