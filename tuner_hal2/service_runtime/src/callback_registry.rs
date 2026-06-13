use std::collections::BTreeMap;

use maleicacid_tuner_hal2_domain_request::{AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackHealthState {
    Registered,
    Unhealthy,
    Cleared,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeCallbackRegistration {
    pub owner_kind: AidlObjectKind,
    pub owner_id: AidlObjectId,
    pub owner_generation: AidlObjectGeneration,
    pub registration_api: AidlApi,
    pub health: CallbackHealthState,
}

#[derive(Debug, Default)]
pub struct RuntimeCallbackRegistry {
    registrations: BTreeMap<(AidlObjectKind, AidlObjectId, AidlObjectGeneration, AidlApi), RuntimeCallbackRegistration>,
}

impl RuntimeCallbackRegistry {
    pub fn record_registration(
        &mut self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) {
        let key = (owner_kind, owner_id, owner_generation, registration_api);
        self.registrations.insert(
            key,
            RuntimeCallbackRegistration { owner_kind, owner_id, owner_generation, registration_api, health: CallbackHealthState::Registered },
        );
    }

    pub fn mark_unhealthy(
        &mut self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) {
        let key = (owner_kind, owner_id, owner_generation, registration_api);
        if let Some(entry) = self.registrations.get_mut(&key) {
            entry.health = CallbackHealthState::Unhealthy;
        }
    }

    pub fn clear_owner(&mut self, owner_id: AidlObjectId, owner_generation: AidlObjectGeneration) {
        self.registrations.retain(|(_, id, generation, _), _| *id != owner_id || *generation != owner_generation);
    }

    pub fn registration_count(&self) -> usize { self.registrations.len() }

    pub fn registration_for(
        &self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) -> Option<&RuntimeCallbackRegistration> {
        self.registrations.get(&(owner_kind, owner_id, owner_generation, registration_api))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_registration_is_keyed_by_owner_generation_and_api() {
        let mut registry = RuntimeCallbackRegistry::default();
        registry.record_registration(AidlObjectKind::Lnb, AidlObjectId(10), AidlObjectGeneration(2), AidlApi::LnbSetCallback);
        assert_eq!(registry.registration_count(), 1);
        assert_eq!(registry.registration_for(AidlObjectKind::Lnb, AidlObjectId(10), AidlObjectGeneration(2), AidlApi::LnbSetCallback).unwrap().health, CallbackHealthState::Registered);
        registry.mark_unhealthy(AidlObjectKind::Lnb, AidlObjectId(10), AidlObjectGeneration(2), AidlApi::LnbSetCallback);
        assert_eq!(registry.registration_for(AidlObjectKind::Lnb, AidlObjectId(10), AidlObjectGeneration(2), AidlApi::LnbSetCallback).unwrap().health, CallbackHealthState::Unhealthy);
        registry.clear_owner(AidlObjectId(10), AidlObjectGeneration(2));
        assert_eq!(registry.registration_count(), 0);
    }
}
