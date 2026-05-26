use crate::hal_sync::lock_mutex_status;
use maleicacid_tuner_hal_descrambler::DescramblerKeySlot;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const DESCRAMBLER_TOKEN_MAX_LEN: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyRegistrationError {
    EmptySlot,
    IncompleteKeyPair,
    RegistryUnavailable,
    CasBridgeUnconnected,
    TokenExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyResolveError {
    EmptyToken,
    MalformedToken,
    UnknownToken,
    ExpiredKeySlot,
    RegistryUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerTokenOrigin {
    UnitTestOnly,
    CasBridge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDescramblerKeySlot {
    pub slot: DescramblerKeySlot,
    pub origin: DescramblerTokenOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DescramblerKeySlotState {
    Active(ResolvedDescramblerKeySlot),
    Expired {
        origin: DescramblerTokenOrigin,
    },
}

#[derive(Debug, Default)]
pub struct DescramblerKeyTable {
    next_id: AtomicU64,
    slots: Mutex<BTreeMap<Vec<u8>, DescramblerKeySlotState>>,
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
        let slots = lock_mutex_status(&self.slots, "descrambler_key_table_slots").map_err(|_| DescramblerKeyResolveError::RegistryUnavailable)?;
        match slots.get(token) {
            Some(DescramblerKeySlotState::Active(slot)) => Ok(slot.clone()),
            Some(DescramblerKeySlotState::Expired { .. }) => Err(DescramblerKeyResolveError::ExpiredKeySlot),
            None => Err(DescramblerKeyResolveError::UnknownToken),
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
        let mut slots = lock_mutex_status(&self.slots, "descrambler_key_table_slots")
            .map_err(|_| DescramblerKeyRegistrationError::RegistryUnavailable)?;
        for _ in 0..1024 {
            let id = self.next_id.load(Ordering::SeqCst);
            if id == 0 {
                return Err(DescramblerKeyRegistrationError::TokenExhausted);
            }
            let next = id.checked_add(1).unwrap_or(0);
            self.next_id.store(next, Ordering::SeqCst);
            let token = id.to_be_bytes().to_vec();
            if slots.contains_key(&token) {
                continue;
            }
            slots.insert(
                token.clone(),
                DescramblerKeySlotState::Active(ResolvedDescramblerKeySlot { slot, origin }),
            );
            return Ok(token);
        }
        Err(DescramblerKeyRegistrationError::TokenExhausted)
    }

    pub fn expire_token(&self, token: &[u8]) -> Result<(), DescramblerKeyResolveError> {
        if token.is_empty() {
            return Err(DescramblerKeyResolveError::EmptyToken);
        }
        if token.len() > DESCRAMBLER_TOKEN_MAX_LEN {
            return Err(DescramblerKeyResolveError::MalformedToken);
        }
        let mut slots = lock_mutex_status(&self.slots, "descrambler_key_table_slots").map_err(|_| DescramblerKeyResolveError::RegistryUnavailable)?;
        match slots.get_mut(token) {
            Some(state) => match state {
                DescramblerKeySlotState::Active(resolved) => {
                    let origin = resolved.origin;
                    *state = DescramblerKeySlotState::Expired { origin };
                    Ok(())
                }
                DescramblerKeySlotState::Expired { .. } => Ok(()),
            },
            None => Err(DescramblerKeyResolveError::UnknownToken),
        }
    }

    pub fn expire_all_by_origin(
        &self,
        target_origin: DescramblerTokenOrigin,
    ) -> Result<usize, DescramblerKeyResolveError> {
        let mut expired = 0usize;
        let mut slots = lock_mutex_status(&self.slots, "descrambler_key_table_slots").map_err(|_| DescramblerKeyResolveError::RegistryUnavailable)?;
        for state in slots.values_mut() {
            match state {
                DescramblerKeySlotState::Active(resolved) if resolved.origin == target_origin => {
                    let origin = resolved.origin;
                    *state = DescramblerKeySlotState::Expired { origin };
                    expired = expired.saturating_add(1);
                }
                _ => {}
            }
        }
        Ok(expired)
    }

    pub fn expire_all(&self) -> Result<usize, DescramblerKeyResolveError> {
        let mut expired = 0usize;
        let mut slots = lock_mutex_status(&self.slots, "descrambler_key_table_slots").map_err(|_| DescramblerKeyResolveError::RegistryUnavailable)?;
        for state in slots.values_mut() {
            if let DescramblerKeySlotState::Active(resolved) = state {
                let origin = resolved.origin;
                *state = DescramblerKeySlotState::Expired { origin };
                expired = expired.saturating_add(1);
            }
        }
        Ok(expired)
    }

    pub fn register_from_cas_bridge(
        &self,
        slot: DescramblerKeySlot,
        cas_bridge_connected: bool,
    ) -> Result<Vec<u8>, DescramblerKeyRegistrationError> {
        if !cas_bridge_connected {
            return Err(DescramblerKeyRegistrationError::CasBridgeUnconnected);
        }
        if !slot.has_even_and_odd_keys() {
            return Err(DescramblerKeyRegistrationError::IncompleteKeyPair);
        }
        self.insert_slot(slot, DescramblerTokenOrigin::CasBridge)
    }

    #[cfg(test)]
    pub fn register_for_test(&self, slot: DescramblerKeySlot) -> Vec<u8> {
        self.insert_slot(slot, DescramblerTokenOrigin::UnitTestOnly).expect("テスト用の復号鍵スロットが不正です")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal_descrambler::{Multi2KeyMaterial, Multi2PrepareError};

    fn even_key_slot() -> DescramblerKeySlot {
        DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8])).unwrap()
    }

    fn odd_key_slot() -> DescramblerKeySlot {
        DescramblerKeySlot::empty()
            .try_with_odd(Multi2KeyMaterial::new([0x11; 32], [0x21; 8], [0x31; 8])).unwrap()
    }

    fn paired_key_slot() -> DescramblerKeySlot {
        even_key_slot()
            .try_with_odd(Multi2KeyMaterial::new([0x12; 32], [0x22; 8], [0x32; 8])).unwrap()
    }

    #[test]
    fn cas_bridge_registration_fails_if_even_prepare_returns_invalid_rounds_zero() {
        let mut invalid_even = Multi2KeyMaterial::new([0x10; 32], [0x20; 8], [0x30; 8]);
        invalid_even.rounds = 0;
        assert_eq!(
            DescramblerKeySlot::empty().try_with_even(invalid_even),
            Err(Multi2PrepareError::InvalidRoundsZero)
        );
    }

    #[test]
    fn cas_bridge_registration_fails_if_odd_prepare_returns_invalid_rounds_zero() {
        let mut invalid_odd = Multi2KeyMaterial::new([0x11; 32], [0x21; 8], [0x31; 8]);
        invalid_odd.rounds = 0;
        assert_eq!(
            DescramblerKeySlot::empty().try_with_odd(invalid_odd),
            Err(Multi2PrepareError::InvalidRoundsZero)
        );
    }

    #[test]
    fn registered_tokens_are_short_opaque_binary_ids() {
        let table = DescramblerKeyTable::new();
        let token = table.register_for_test(even_key_slot());
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
        assert_eq!(
            table.resolve_with_diagnostic(b"placeholder").unwrap_err(),
            DescramblerKeyResolveError::UnknownToken
        );
    }

    #[test]
    fn cas_bridge_requires_even_and_odd_key_pair() {
        let table = DescramblerKeyTable::new();
        assert_eq!(
            table.register_from_cas_bridge(even_key_slot(), false).unwrap_err(),
            DescramblerKeyRegistrationError::CasBridgeUnconnected
        );
        assert_eq!(
            table.register_from_cas_bridge(odd_key_slot(), false).unwrap_err(),
            DescramblerKeyRegistrationError::CasBridgeUnconnected
        );
        assert_eq!(
            table.register_from_cas_bridge(paired_key_slot(), false).unwrap_err(),
            DescramblerKeyRegistrationError::CasBridgeUnconnected
        );
        assert_eq!(
            table.register_from_cas_bridge(even_key_slot(), true).unwrap_err(),
            DescramblerKeyRegistrationError::IncompleteKeyPair
        );
        assert_eq!(
            table.register_from_cas_bridge(odd_key_slot(), true).unwrap_err(),
            DescramblerKeyRegistrationError::IncompleteKeyPair
        );
        let token = table.register_from_cas_bridge(paired_key_slot(), true).unwrap();
        assert!(table.resolve_with_diagnostic(&token).is_ok());
        assert!(table.register_for_test(even_key_slot()).len() == 8);
        assert!(table.register_for_test(odd_key_slot()).len() == 8);
    }

    #[test]
    fn expired_token_resolves_as_expired_not_unknown() {
        let table = DescramblerKeyTable::new();
        let token = table.register_for_test(even_key_slot());
        assert!(table.resolve_with_diagnostic(&token).is_ok());
        table.expire_token(&token).unwrap();
        assert_eq!(
            table.resolve_with_diagnostic(&token).unwrap_err(),
            DescramblerKeyResolveError::ExpiredKeySlot
        );
        assert_eq!(
            table.resolve_with_diagnostic(&[0x42; 8]).unwrap_err(),
            DescramblerKeyResolveError::UnknownToken
        );
    }

    #[test]
    fn expire_all_by_origin_keeps_origin_specific_boundaries() {
        let table = DescramblerKeyTable::new();
        let unit_token = table.register_for_test(even_key_slot());
        let cas_token = table.register_from_cas_bridge(paired_key_slot(), true).unwrap();
        assert_eq!(table.expire_all_by_origin(DescramblerTokenOrigin::CasBridge).unwrap(), 1);
        assert!(table.resolve_with_diagnostic(&unit_token).is_ok());
        assert_eq!(
            table.resolve_with_diagnostic(&cas_token).unwrap_err(),
            DescramblerKeyResolveError::ExpiredKeySlot
        );
    }

    #[test]
    fn expire_all_marks_remaining_active_tokens_expired() {
        let table = DescramblerKeyTable::new();
        let first = table.register_for_test(even_key_slot());
        let second = table.register_from_cas_bridge(paired_key_slot(), true).unwrap();
        assert_eq!(table.expire_all().unwrap(), 2);
        assert_eq!(
            table.resolve_with_diagnostic(&first).unwrap_err(),
            DescramblerKeyResolveError::ExpiredKeySlot
        );
        assert_eq!(
            table.resolve_with_diagnostic(&second).unwrap_err(),
            DescramblerKeyResolveError::ExpiredKeySlot
        );
    }
}

#[cfg(test)]
mod r50dz52_g3_05_tests {
    use super::*;
    use maleicacid_tuner_hal_descrambler::Multi2KeyMaterial;
    use std::sync::atomic::Ordering;

    fn even_slot_for_wrap_test() -> DescramblerKeySlot {
        DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([0x41; 32], [0x42; 8], [0x43; 8]))
            .unwrap()
    }

    fn odd_slot_for_wrap_test() -> DescramblerKeySlot {
        DescramblerKeySlot::empty()
            .try_with_odd(Multi2KeyMaterial::new([0x51; 32], [0x52; 8], [0x53; 8]))
            .unwrap()
    }

    fn paired_slot_for_wrap_test() -> DescramblerKeySlot {
        even_slot_for_wrap_test()
            .try_with_odd(Multi2KeyMaterial::new([0x61; 32], [0x62; 8], [0x63; 8]))
            .unwrap()
    }

    #[test]
    fn token_allocator_wrap_does_not_overwrite_existing_key_table_entry() {
        let table = DescramblerKeyTable::new();
        let first = table.register_for_test(even_slot_for_wrap_test());
        assert!(table.resolve_with_diagnostic(&first).is_ok());

        table.next_id.store(u64::MAX, Ordering::SeqCst);
        let max_token = table.register_for_test(odd_slot_for_wrap_test());
        assert_eq!(max_token, u64::MAX.to_be_bytes().to_vec());

        assert_eq!(
            table.register_from_cas_bridge(paired_slot_for_wrap_test(), true),
            Err(DescramblerKeyRegistrationError::TokenExhausted)
        );
        assert!(table.resolve_with_diagnostic(&first).is_ok());
        assert!(table.resolve_with_diagnostic(&max_token).is_ok());
    }
}

