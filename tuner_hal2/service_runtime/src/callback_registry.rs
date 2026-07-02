use std::collections::BTreeMap;

use maleicacid_tuner_hal2_domain_request::{
    AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackHealthState {
    Registered,
    Unhealthy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeCallbackRegistration {
    health: CallbackHealthState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallbackRegistryUpdate {
    Updated,
    Missing,
}

#[derive(Debug, Default)]
pub(crate) struct RuntimeCallbackRegistry {
    registrations: BTreeMap<
        (AidlObjectKind, AidlObjectId, AidlObjectGeneration, AidlApi),
        RuntimeCallbackRegistration,
    >,
}

impl RuntimeCallbackRegistry {
    pub(crate) fn record_registration(
        &mut self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) {
        let key = (owner_kind, owner_id, owner_generation, registration_api);
        self.registrations.insert(
            key,
            RuntimeCallbackRegistration {
                health: CallbackHealthState::Registered,
            },
        );
    }

    pub(crate) fn mark_unhealthy(
        &mut self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) -> CallbackRegistryUpdate {
        let key = (owner_kind, owner_id, owner_generation, registration_api);
        if let Some(entry) = self.registrations.get_mut(&key) {
            entry.health = CallbackHealthState::Unhealthy;
            CallbackRegistryUpdate::Updated
        } else {
            CallbackRegistryUpdate::Missing
        }
    }

    pub(crate) fn mark_owner_unhealthy(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
    ) -> CallbackRegistryUpdate {
        let mut updated = false;
        for ((_, id, generation, _), entry) in self.registrations.iter_mut() {
            if *id == owner_id && *generation == owner_generation {
                entry.health = CallbackHealthState::Unhealthy;
                updated = true;
            }
        }
        if updated {
            CallbackRegistryUpdate::Updated
        } else {
            CallbackRegistryUpdate::Missing
        }
    }

    pub(crate) fn clear_owner(
        &mut self,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
    ) -> CallbackRegistryUpdate {
        let before = self.registrations.len();
        self.registrations
            .retain(|(_, id, generation, _), _| *id != owner_id || *generation != owner_generation);
        if self.registrations.len() < before {
            CallbackRegistryUpdate::Updated
        } else {
            CallbackRegistryUpdate::Missing
        }
    }

    pub(crate) fn registration_count(&self) -> usize {
        self.registrations.len()
    }

    pub(crate) fn registration_for(
        &self,
        owner_kind: AidlObjectKind,
        owner_id: AidlObjectId,
        owner_generation: AidlObjectGeneration,
        registration_api: AidlApi,
    ) -> Option<&RuntimeCallbackRegistration> {
        self.registrations
            .get(&(owner_kind, owner_id, owner_generation, registration_api))
    }
}

impl RuntimeCallbackRegistration {
    pub(crate) const fn health(&self) -> CallbackHealthState {
        self.health
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_registration_is_keyed_by_owner_generation_and_api() {
        let mut registry = RuntimeCallbackRegistry::default();
        registry.record_registration(
            AidlObjectKind::Lnb,
            AidlObjectId(10),
            AidlObjectGeneration(2),
            AidlApi::LnbSetCallback,
        );
        assert_eq!(registry.registration_count(), 1);
        assert_eq!(
            registry
                .registration_for(
                    AidlObjectKind::Lnb,
                    AidlObjectId(10),
                    AidlObjectGeneration(2),
                    AidlApi::LnbSetCallback
                )
                .unwrap()
                .health(),
            CallbackHealthState::Registered
        );
        assert_eq!(
            registry.mark_unhealthy(
                AidlObjectKind::Lnb,
                AidlObjectId(10),
                AidlObjectGeneration(2),
                AidlApi::LnbSetCallback,
            ),
            CallbackRegistryUpdate::Updated
        );
        assert_eq!(
            registry
                .registration_for(
                    AidlObjectKind::Lnb,
                    AidlObjectId(10),
                    AidlObjectGeneration(2),
                    AidlApi::LnbSetCallback
                )
                .unwrap()
                .health(),
            CallbackHealthState::Unhealthy
        );
        assert_eq!(
            registry.clear_owner(AidlObjectId(10), AidlObjectGeneration(2)),
            CallbackRegistryUpdate::Updated
        );
        assert_eq!(registry.registration_count(), 0);
        assert_eq!(
            registry.clear_owner(AidlObjectId(10), AidlObjectGeneration(2)),
            CallbackRegistryUpdate::Missing
        );
    }

    #[test]
    fn mark_owner_unhealthy_marks_all_owner_registrations() {
        let mut registry = RuntimeCallbackRegistry::default();
        registry.record_registration(
            AidlObjectKind::Frontend,
            AidlObjectId(20),
            AidlObjectGeneration(3),
            AidlApi::FrontendSetCallback,
        );
        registry.record_registration(
            AidlObjectKind::Lnb,
            AidlObjectId(20),
            AidlObjectGeneration(3),
            AidlApi::LnbSetCallback,
        );
        registry.record_registration(
            AidlObjectKind::Lnb,
            AidlObjectId(20),
            AidlObjectGeneration(4),
            AidlApi::LnbSetCallback,
        );
        assert_eq!(
            registry.mark_owner_unhealthy(AidlObjectId(20), AidlObjectGeneration(3)),
            CallbackRegistryUpdate::Updated
        );
        assert_eq!(
            registry
                .registration_for(
                    AidlObjectKind::Frontend,
                    AidlObjectId(20),
                    AidlObjectGeneration(3),
                    AidlApi::FrontendSetCallback
                )
                .unwrap()
                .health(),
            CallbackHealthState::Unhealthy
        );
        assert_eq!(
            registry
                .registration_for(
                    AidlObjectKind::Lnb,
                    AidlObjectId(20),
                    AidlObjectGeneration(3),
                    AidlApi::LnbSetCallback
                )
                .unwrap()
                .health(),
            CallbackHealthState::Unhealthy
        );
        assert_eq!(
            registry
                .registration_for(
                    AidlObjectKind::Lnb,
                    AidlObjectId(20),
                    AidlObjectGeneration(4),
                    AidlApi::LnbSetCallback
                )
                .unwrap()
                .health(),
            CallbackHealthState::Registered
        );
    }

    #[test]
    fn mark_unhealthy_reports_missing_registration() {
        let mut registry = RuntimeCallbackRegistry::default();
        assert_eq!(
            registry.mark_unhealthy(
                AidlObjectKind::Frontend,
                AidlObjectId(99),
                AidlObjectGeneration(1),
                AidlApi::FrontendSetCallback,
            ),
            CallbackRegistryUpdate::Missing
        );
        assert_eq!(
            registry.mark_owner_unhealthy(AidlObjectId(99), AidlObjectGeneration(1)),
            CallbackRegistryUpdate::Missing
        );
    }
}
