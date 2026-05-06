use maleicacid_tuner_hal_descrambler::DescramblerKeySlot;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const DESCRAMBLER_TEST_TOKEN_PREFIX: &[u8] = b"maleicacid-test-desc-token-";
pub const DESCRAMBLER_CAS_TOKEN_PREFIX: &[u8] = b"maleicacid-cas-desc-token-";
pub const DESCRAMBLER_EXPIRED_TOKEN_PREFIX: &[u8] = b"maleicacid-expired-desc-token-";
pub const DESCRAMBLER_PLACEHOLDER_TOKEN_PREFIX: &[u8] = b"maleicacid-placeholder-desc-token";
pub const DESCRAMBLER_LEGACY_PLACEHOLDER_TOKEN_PREFIX: &[u8] = b"placeholder";
pub const DESCRAMBLER_LEGACY_TIS_PLACEHOLDER_TOKEN_PREFIX: &[u8] = b"maleicacid-kari-token-";

fn is_placeholder_or_unconnected_cas_token(token: &[u8]) -> bool {
    token.starts_with(DESCRAMBLER_PLACEHOLDER_TOKEN_PREFIX)
        || token.starts_with(DESCRAMBLER_LEGACY_PLACEHOLDER_TOKEN_PREFIX)
        || token.starts_with(DESCRAMBLER_LEGACY_TIS_PLACEHOLDER_TOKEN_PREFIX)
        || token.starts_with(DESCRAMBLER_CAS_TOKEN_PREFIX)
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
        let slots = self.slots.lock().map_err(|_| DescramblerKeyResolveError::RegistryUnavailable)?;
        if let Some(slot) = slots.get(token).cloned() {
            return Ok(slot);
        }
        if is_placeholder_or_unconnected_cas_token(token) {
            Err(DescramblerKeyResolveError::CasBridgeUnconnected)
        } else if token.starts_with(DESCRAMBLER_EXPIRED_TOKEN_PREFIX) {
            Err(DescramblerKeyResolveError::ExpiredKeySlot)
        } else if token.starts_with(DESCRAMBLER_TEST_TOKEN_PREFIX) {
            Err(DescramblerKeyResolveError::UnknownToken)
        } else {
            Err(DescramblerKeyResolveError::MalformedToken)
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
        let prefix = match origin {
            DescramblerTokenOrigin::VtsOrUnitTest => "maleicacid-test-desc-token",
            DescramblerTokenOrigin::CasBridge => "maleicacid-cas-desc-token",
        };
        let token = format!("{prefix}-{id:016x}").into_bytes();
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
