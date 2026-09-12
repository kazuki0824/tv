use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use maleicacid_tuner_hal2_descrambler::{DescramblerKeySlot, DescramblerKeyToken};

pub const DEFAULT_MAX_DESCRAMBLER_KEY_SLOTS: usize = 64;
pub const DEFAULT_UNPUBLISHED_RESERVATION_TTL: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DescramblerKeySlotId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyProvisioningIdentity {
    pub provider_id: u64,
    pub provider_generation: u64,
    pub key_epoch: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescramblerKeyLookupError {
    UnknownToken,
    ExpiredToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyProvisioningMutationError {
    UnknownToken,
    ExpiredToken,
    InvalidIdentity,
    IdentityMismatch,
    StaleEpoch,
    ResourceExhausted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescramblerKeySlotState {
    slot: DescramblerKeySlotId,
    identity: Option<KeyProvisioningIdentity>,
    key_slot: Option<DescramblerKeySlot>,
    refcount: usize,
    revoked: bool,
    reserved_at: Option<Instant>,
}

#[derive(Debug)]
pub struct DescramblerKeyTable {
    slots: BTreeMap<DescramblerKeyToken, DescramblerKeySlotState>,
    max_slots: usize,
}

impl Default for DescramblerKeyTable {
    fn default() -> Self {
        Self::with_max_slots(DEFAULT_MAX_DESCRAMBLER_KEY_SLOTS)
    }
}

impl DescramblerKeyTable {
    pub fn with_max_slots(max_slots: usize) -> Self {
        Self {
            slots: BTreeMap::new(),
            max_slots,
        }
    }

    pub fn has_token_resolution_state(&self) -> bool {
        !self.slots.is_empty()
    }

    pub fn live_slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn key_slot(&self, slot_id: DescramblerKeySlotId) -> Option<DescramblerKeySlot> {
        self.slots
            .values()
            .find(|state| state.slot == slot_id && !state.revoked)
            .and_then(|state| state.key_slot.clone())
    }

    pub fn reserve_key_slot(
        &mut self,
        token: DescramblerKeyToken,
        slot: DescramblerKeySlotId,
        provider_id: u64,
        provider_generation: u64,
    ) -> Result<(), KeyProvisioningMutationError> {
        if provider_id == 0 || provider_generation == 0 {
            return Err(KeyProvisioningMutationError::InvalidIdentity);
        }
        let now = Instant::now();
        self.reap_unpublished_reservations_at(now, DEFAULT_UNPUBLISHED_RESERVATION_TTL);
        if let Some(state) = self.slots.get_mut(&token) {
            let same_unpublished_reservation = state.identity.is_some_and(|identity| {
                identity.provider_id == provider_id
                    && identity.provider_generation == provider_generation
                    && identity.key_epoch == 0
            }) && state.key_slot.is_none()
                && state.refcount == 0
                && !state.revoked;
            if same_unpublished_reservation {
                state.reserved_at = Some(now);
                return Ok(());
            }
            return Err(KeyProvisioningMutationError::IdentityMismatch);
        }
        if self.slots.len() >= self.max_slots {
            return Err(KeyProvisioningMutationError::ResourceExhausted);
        }
        self.slots.insert(
            token,
            DescramblerKeySlotState {
                slot,
                identity: Some(KeyProvisioningIdentity {
                    provider_id,
                    provider_generation,
                    key_epoch: 0,
                }),
                key_slot: None,
                refcount: 0,
                revoked: false,
                reserved_at: Some(now),
            },
        );
        Ok(())
    }

    pub fn publish_key_slot(
        &mut self,
        token: DescramblerKeyToken,
        identity: KeyProvisioningIdentity,
        key_slot: DescramblerKeySlot,
    ) -> Result<DescramblerKeySlotId, KeyProvisioningMutationError> {
        if identity.provider_id == 0 || identity.provider_generation == 0 || identity.key_epoch == 0
        {
            return Err(KeyProvisioningMutationError::InvalidIdentity);
        }
        let state = self
            .slots
            .get_mut(&token)
            .ok_or(KeyProvisioningMutationError::UnknownToken)?;
        if state.revoked {
            return Err(KeyProvisioningMutationError::ExpiredToken);
        }
        let current = state
            .identity
            .ok_or(KeyProvisioningMutationError::IdentityMismatch)?;
        if current.provider_id != identity.provider_id
            || current.provider_generation != identity.provider_generation
        {
            return Err(KeyProvisioningMutationError::IdentityMismatch);
        }
        if identity.key_epoch <= current.key_epoch {
            return Err(KeyProvisioningMutationError::StaleEpoch);
        }
        state.identity = Some(identity);
        state.key_slot = Some(key_slot);
        state.reserved_at = None;
        Ok(state.slot)
    }

    pub fn revoke_key_slot(
        &mut self,
        token: &DescramblerKeyToken,
        provider_id: u64,
        provider_generation: u64,
    ) -> Result<(), KeyProvisioningMutationError> {
        if provider_id == 0 || provider_generation == 0 {
            return Err(KeyProvisioningMutationError::InvalidIdentity);
        }
        let remove = {
            let state = self
                .slots
                .get_mut(token)
                .ok_or(KeyProvisioningMutationError::UnknownToken)?;
            let identity = state
                .identity
                .ok_or(KeyProvisioningMutationError::IdentityMismatch)?;
            if identity.provider_id != provider_id
                || identity.provider_generation != provider_generation
            {
                return Err(KeyProvisioningMutationError::IdentityMismatch);
            }
            state.revoked = true;
            state.reserved_at = None;
            state.refcount == 0
        };
        if remove {
            self.slots.remove(token);
        }
        Ok(())
    }

    fn reap_unpublished_reservations_at(&mut self, now: Instant, max_age: Duration) -> usize {
        let before = self.slots.len();
        self.slots.retain(|_, state| {
            let stale_unpublished = state.refcount == 0
                && !state.revoked
                && state.key_slot.is_none()
                && state.reserved_at.is_some_and(|reserved_at| {
                    now.checked_duration_since(reserved_at)
                        .is_some_and(|age| age >= max_age)
                });
            !stale_unpublished
        });
        before.saturating_sub(self.slots.len())
    }

    pub fn reap_unpublished_reservations_older_than(&mut self, max_age: Duration) -> usize {
        self.reap_unpublished_reservations_at(Instant::now(), max_age)
    }

    pub fn acquire(
        &mut self,
        token: &DescramblerKeyToken,
    ) -> Result<DescramblerKeySlotId, DescramblerKeyLookupError> {
        let state = self
            .slots
            .get_mut(token)
            .ok_or(DescramblerKeyLookupError::UnknownToken)?;
        if state.revoked {
            return Err(DescramblerKeyLookupError::ExpiredToken);
        }
        if state.key_slot.is_none() {
            return Err(DescramblerKeyLookupError::UnknownToken);
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
            state.refcount == 0 && state.revoked
        };
        if remove {
            self.slots.remove(token);
        }
        Ok(())
    }
}

#[cfg(test)]
impl DescramblerKeyTable {
    pub(crate) fn insert_test_key_slot(
        &mut self,
        token: DescramblerKeyToken,
        slot: DescramblerKeySlotId,
        key_slot: DescramblerKeySlot,
    ) -> Result<(), KeyProvisioningMutationError> {
        let identity = KeyProvisioningIdentity {
            provider_id: u64::MAX,
            provider_generation: slot.0,
            key_epoch: 1,
        };
        self.reserve_key_slot(
            token.clone(),
            slot,
            identity.provider_id,
            identity.provider_generation,
        )?;
        self.publish_key_slot(token, identity, key_slot).map(|_| ())
    }

    pub(crate) fn refcount_for_test(&self, token: &DescramblerKeyToken) -> Option<usize> {
        self.slots.get(token).map(|state| state.refcount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maleicacid_tuner_hal2_descrambler::Multi2KeyMaterial;

    const PROVIDER: u64 = 0x4d43_4153_4b45_5901;
    const GENERATION: u64 = 7;

    fn token(value: u8) -> DescramblerKeyToken {
        DescramblerKeyToken::try_from_bytes(vec![value; 16]).unwrap()
    }

    fn identity(epoch: u64) -> KeyProvisioningIdentity {
        KeyProvisioningIdentity {
            provider_id: PROVIDER,
            provider_generation: GENERATION,
            key_epoch: epoch,
        }
    }

    fn keys(value: u8) -> DescramblerKeySlot {
        DescramblerKeySlot::empty()
            .try_with_even(Multi2KeyMaterial::new([value; 32], [value; 8], [value; 8]))
            .unwrap()
            .try_with_odd(Multi2KeyMaterial::new([value; 32], [value; 8], [value; 8]))
            .unwrap()
    }

    fn reserve(table: &mut DescramblerKeyTable, value: u8) {
        table
            .reserve_key_slot(
                token(value),
                DescramblerKeySlotId(u64::from(value)),
                PROVIDER,
                GENERATION,
            )
            .unwrap();
    }

    #[test]
    fn reservation_is_unresolved_until_complete_publication() {
        let mut table = DescramblerKeyTable::default();
        reserve(&mut table, 1);
        assert_eq!(
            table.acquire(&token(1)),
            Err(DescramblerKeyLookupError::UnknownToken)
        );
        assert_eq!(
            table.publish_key_slot(token(1), identity(1), keys(1)),
            Ok(DescramblerKeySlotId(1))
        );
        assert_eq!(table.acquire(&token(1)), Ok(DescramblerKeySlotId(1)));
        assert_eq!(table.key_slot(DescramblerKeySlotId(1)), Some(keys(1)));
    }

    #[test]
    fn retrying_reserve_keeps_original_slot_and_identity() {
        let mut table = DescramblerKeyTable::default();
        reserve(&mut table, 1);
        table
            .reserve_key_slot(token(1), DescramblerKeySlotId(99), PROVIDER, GENERATION)
            .unwrap();
        assert_eq!(
            table.publish_key_slot(token(1), identity(1), keys(1)),
            Ok(DescramblerKeySlotId(1))
        );
        assert_eq!(
            table.reserve_key_slot(token(1), DescramblerKeySlotId(99), PROVIDER, GENERATION),
            Err(KeyProvisioningMutationError::IdentityMismatch)
        );
    }

    #[test]
    fn colliding_provider_or_generation_cannot_replace_reservation() {
        let mut table = DescramblerKeyTable::default();
        reserve(&mut table, 1);
        for (provider, generation) in [(PROVIDER + 1, GENERATION), (PROVIDER, GENERATION + 1)] {
            assert_eq!(
                table.reserve_key_slot(token(1), DescramblerKeySlotId(2), provider, generation),
                Err(KeyProvisioningMutationError::IdentityMismatch)
            );
            let stale = KeyProvisioningIdentity {
                provider_id: provider,
                provider_generation: generation,
                key_epoch: 1,
            };
            assert_eq!(
                table.publish_key_slot(token(1), stale, keys(2)),
                Err(KeyProvisioningMutationError::IdentityMismatch)
            );
            assert_eq!(
                table.revoke_key_slot(&token(1), provider, generation),
                Err(KeyProvisioningMutationError::IdentityMismatch)
            );
        }
        assert_eq!(
            table.publish_key_slot(token(1), identity(1), keys(1)),
            Ok(DescramblerKeySlotId(1))
        );
    }

    #[test]
    fn stale_epoch_does_not_replace_current_keys() {
        let mut table = DescramblerKeyTable::default();
        reserve(&mut table, 1);
        table
            .publish_key_slot(token(1), identity(2), keys(2))
            .unwrap();
        for epoch in [1, 2] {
            assert_eq!(
                table.publish_key_slot(token(1), identity(epoch), keys(3)),
                Err(KeyProvisioningMutationError::StaleEpoch)
            );
            assert_eq!(table.key_slot(DescramblerKeySlotId(1)), Some(keys(2)));
        }
        table
            .publish_key_slot(token(1), identity(3), keys(3))
            .unwrap();
        assert_eq!(table.key_slot(DescramblerKeySlotId(1)), Some(keys(3)));
    }

    #[test]
    fn revoke_blocks_resolve_and_reuse_until_last_reference_releases() {
        let mut table = DescramblerKeyTable::default();
        reserve(&mut table, 1);
        table
            .publish_key_slot(token(1), identity(1), keys(1))
            .unwrap();
        table.acquire(&token(1)).unwrap();
        table.acquire(&token(1)).unwrap();
        table
            .revoke_key_slot(&token(1), PROVIDER, GENERATION)
            .unwrap();
        table
            .revoke_key_slot(&token(1), PROVIDER, GENERATION)
            .unwrap();
        assert_eq!(
            table.acquire(&token(1)),
            Err(DescramblerKeyLookupError::ExpiredToken)
        );
        assert_eq!(table.key_slot(DescramblerKeySlotId(1)), None);
        assert_eq!(
            table.publish_key_slot(token(1), identity(2), keys(2)),
            Err(KeyProvisioningMutationError::ExpiredToken)
        );
        assert_eq!(
            table.reserve_key_slot(token(1), DescramblerKeySlotId(2), PROVIDER, GENERATION + 1),
            Err(KeyProvisioningMutationError::IdentityMismatch)
        );
        table.release(&token(1)).unwrap();
        assert_eq!(table.live_slot_count(), 1);
        table.release(&token(1)).unwrap();
        assert_eq!(table.live_slot_count(), 0);
        assert_eq!(
            table.acquire(&token(1)),
            Err(DescramblerKeyLookupError::UnknownToken)
        );
    }

    #[test]
    fn unpublished_reservations_expire_but_published_keys_do_not() {
        let mut table = DescramblerKeyTable::with_max_slots(2);
        reserve(&mut table, 1);
        reserve(&mut table, 2);
        table
            .publish_key_slot(token(2), identity(1), keys(2))
            .unwrap();
        assert_eq!(
            table.reserve_key_slot(token(3), DescramblerKeySlotId(3), PROVIDER, GENERATION),
            Err(KeyProvisioningMutationError::ResourceExhausted)
        );
        assert_eq!(
            table.reap_unpublished_reservations_at(
                Instant::now() + DEFAULT_UNPUBLISHED_RESERVATION_TTL,
                DEFAULT_UNPUBLISHED_RESERVATION_TTL
            ),
            1
        );
        assert_eq!(
            table.acquire(&token(1)),
            Err(DescramblerKeyLookupError::UnknownToken)
        );
        assert_eq!(table.acquire(&token(2)), Ok(DescramblerKeySlotId(2)));
        reserve(&mut table, 3);
    }

    #[test]
    fn test_fixture_uses_production_reserve_publish_and_reference_accounting() {
        let mut table = DescramblerKeyTable::default();
        table
            .insert_test_key_slot(token(1), DescramblerKeySlotId(1), keys(1))
            .unwrap();
        assert_eq!(table.refcount_for_test(&token(1)), Some(0));
        table.acquire(&token(1)).unwrap();
        assert_eq!(table.refcount_for_test(&token(1)), Some(1));
        table.release(&token(1)).unwrap();
        assert_eq!(table.refcount_for_test(&token(1)), Some(0));
    }

    #[test]
    fn invalid_identity_and_unreserved_publish_are_rejected() {
        let mut table = DescramblerKeyTable::default();
        assert_eq!(
            table.reserve_key_slot(token(1), DescramblerKeySlotId(1), 0, GENERATION),
            Err(KeyProvisioningMutationError::InvalidIdentity)
        );
        assert_eq!(
            table.reserve_key_slot(token(1), DescramblerKeySlotId(1), PROVIDER, 0),
            Err(KeyProvisioningMutationError::InvalidIdentity)
        );
        assert_eq!(
            table.publish_key_slot(token(1), identity(1), keys(1)),
            Err(KeyProvisioningMutationError::UnknownToken)
        );
        reserve(&mut table, 1);
        assert_eq!(
            table.publish_key_slot(token(1), identity(0), keys(1)),
            Err(KeyProvisioningMutationError::InvalidIdentity)
        );
    }
}
