//! frontend probe 能力、AIDL capability、runtime tune 許可を接続する骨格。
//!
//! r50dz25 WP-03 では Tuner HAL の frontend entry 生成と runtime 許可系の補助正本として使う。
//! declared type だけではなく probe が示す supported systems から能力を作る。

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum FrontendSystem {
    IsdbT,
    IsdbS,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct FrontendSystemSet {
    systems: BTreeSet<FrontendSystem>,
}

impl FrontendSystemSet {
    pub fn insert(&mut self, system: FrontendSystem) {
        self.systems.insert(system);
    }

    pub fn contains(&self, system: FrontendSystem) -> bool {
        self.systems.contains(&system)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct FrontendCapabilityModel {
    pub physical_frontend_key: i32,
    pub logical_frontend_id: i32,
    pub aidl_frontend_type: i32,
    pub supported_systems: FrontendSystemSet,
    pub runtime_allowed_systems: FrontendSystemSet,
    pub lnb_required: bool,
    pub resource_group_key: i32,
}

impl FrontendCapabilityModel {
    pub fn new(
        physical_frontend_key: i32,
        logical_frontend_id: i32,
        aidl_frontend_type: i32,
        lnb_required: bool,
        resource_group_key: i32,
    ) -> Self {
        Self {
            physical_frontend_key,
            logical_frontend_id,
            aidl_frontend_type,
            supported_systems: FrontendSystemSet::default(),
            runtime_allowed_systems: FrontendSystemSet::default(),
            lnb_required,
            resource_group_key,
        }
    }

    pub fn allow_system(mut self, system: FrontendSystem) -> Self {
        self.supported_systems.insert(system);
        self.runtime_allowed_systems.insert(system);
        self
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct FrontendRuntimePolicy {
    pub physical_frontend_key: i32,
    pub logical_frontend_id: i32,
    pub resource_group_key: i32,
    pub lnb_required: bool,
    pub allowed_systems: FrontendSystemSet,
}

impl FrontendRuntimePolicy {
    pub fn from_model(model: &FrontendCapabilityModel) -> Self {
        Self {
            physical_frontend_key: model.physical_frontend_key,
            logical_frontend_id: model.logical_frontend_id,
            resource_group_key: model.resource_group_key,
            lnb_required: model.lnb_required,
            allowed_systems: model.runtime_allowed_systems.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_system_set_can_represent_isdb_t_and_isdb_s() {
        let mut set = FrontendSystemSet::default();
        set.insert(FrontendSystem::IsdbT);
        set.insert(FrontendSystem::IsdbS);
        assert!(set.contains(FrontendSystem::IsdbT));
        assert!(set.contains(FrontendSystem::IsdbS));
    }

    #[test]
    fn runtime_policy_preserves_multiple_probe_systems() {
        let model = FrontendCapabilityModel::new(10, 20, 1, true, 30)
            .allow_system(FrontendSystem::IsdbT)
            .allow_system(FrontendSystem::IsdbS);
        let policy = FrontendRuntimePolicy::from_model(&model);
        assert!(policy.allowed_systems.contains(FrontendSystem::IsdbT));
        assert!(policy.allowed_systems.contains(FrontendSystem::IsdbS));
        assert!(policy.lnb_required);
        assert_eq!(policy.physical_frontend_key, 10);
        assert_eq!(policy.logical_frontend_id, 20);
        assert_eq!(policy.resource_group_key, 30);
    }
}
