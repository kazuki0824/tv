use maleicacid_tuner_hal_descrambler::DescramblerKeySlot;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const DESCRAMBLER_TOKEN_MAX_LEN: usize = 16;

fn is_legacy_placeholder_token(token: &[u8]) -> bool {
    token == b"placeholder"
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyRegistrationError {
    EmptySlot,
    RegistryUnavailable,
    CasBridgeUnconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyResolveError {
    EmptyToken,
    MalformedToken,
    UnknownToken,
    CasBridgeUnconnected,
    ExpiredKeySlot,
    RegistryUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerTokenOrigin {
    VtsOrUnitTest,
    CasBridge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDescramblerKeySlot {
    pub slot: DescramblerKeySlot,
    pub origin: DescramblerTokenOrigin,
}

#[derive(Debug, Default)]
pub struct DescramblerKeyTable {
    next_id: AtomicU64,
    slots: Mutex<BTreeMap<Vec<u8>, ResolvedDescramblerKeySlot>>,
}

impl DescramblerKeyTable {
    pub fn new() -> Self {
        Self { next_id: AtomicU64::new(1), slots: Mutex::new(BTreeMap::new()) }
    }

    pub fn resolve_with_diagnostic(&self, token: &[u8]) -> Result<ResolvedDescramblerKeySlot, DescramblerKeyResolveError> {
        if token.is_empty() {
            return Err(DescramblerKeyResolveError::EmptyToken);
        }
        if token.len() > DESCRAMBLER_TOKEN_MAX_LEN {
            return Err(DescramblerKeyResolveError::MalformedToken);
        }
        let slots = self.slots.lock().map_err(|_| DescramblerKeyResolveError::RegistryUnavailable)?;
        if let Some(slot) = slots.get(token).cloned() {
            return Ok(slot);
        }
        if is_legacy_placeholder_token(token) {
            Err(DescramblerKeyResolveError::CasBridgeUnconnected)
        } else {
            Err(DescramblerKeyResolveError::UnknownToken)
        }
    }

    #[cfg(test)]
    pub fn resolve_for_test(&self, token: &[u8]) -> Option<DescramblerKeySlot> {
        self.resolve_with_diagnostic(token).ok().map(|resolved| resolved.slot)
    }

    fn insert_slot(&self, slot: DescramblerKeySlot, origin: DescramblerTokenOrigin) -> Result<Vec<u8>, DescramblerKeyRegistrationError> {
        if !slot.has_any_key() {
            return Err(DescramblerKeyRegistrationError::EmptySlot);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let token = id.to_be_bytes().to_vec();
        self.slots
            .lock()
            .map_err(|_| DescramblerKeyRegistrationError::RegistryUnavailable)?
            .insert(token.clone(), ResolvedDescramblerKeySlot { slot, origin });
        Ok(token)
    }

    pub fn register_from_cas_bridge(
        &self,
        slot: DescramblerKeySlot,
        cas_bridge_connected: bool,
    ) -> Result<Vec<u8>, DescramblerKeyRegistrationError> {
        if !cas_bridge_connected {
            return Err(DescramblerKeyRegistrationError::CasBridgeUnconnected);
        }
        self.insert_slot(slot, DescramblerTokenOrigin::CasBridge)
    }

    #[cfg(test)]
    pub fn register_for_test(&self, slot: DescramblerKeySlot) -> Vec<u8> {
        self.insert_slot(slot, DescramblerTokenOrigin::VtsOrUnitTest).expect("テスト用の復号鍵スロットが不正です")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal_descrambler::Multi2KeyMaterial;

    fn key_slot() -> DescramblerKeySlot {
        DescramblerKeySlot::empty()
            .with_even(Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8]))
    }

    #[test]
    fn registered_tokens_are_short_opaque_binary_ids() {
        let table = DescramblerKeyTable::new();
        let token = table.register_for_test(key_slot());
        assert_eq!(token.len(), 8);
        assert!(table.resolve_with_diagnostic(&token).is_ok());
    }

    #[test]
    fn invalid_token_lengths_are_rejected_before_registry_resolution() {
        let table = DescramblerKeyTable::new();
        assert_eq!(
            table.resolve_with_diagnostic(&[]).unwrap_err(),
            DescramblerKeyResolveError::EmptyToken
        );
        assert_eq!(
            table.resolve_with_diagnostic(&[0x55; DESCRAMBLER_TOKEN_MAX_LEN + 1]).unwrap_err(),
            DescramblerKeyResolveError::MalformedToken
        );
    }

    #[test]
    fn unknown_length_valid_token_is_rejected() {
        let table = DescramblerKeyTable::new();
        assert_eq!(
            table.resolve_with_diagnostic(&[0x42; 8]).unwrap_err(),
            DescramblerKeyResolveError::UnknownToken
        );
    }
}
