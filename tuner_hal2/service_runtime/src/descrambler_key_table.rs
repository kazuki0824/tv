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
