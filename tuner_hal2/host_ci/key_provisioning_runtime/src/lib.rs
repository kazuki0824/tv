// 鍵provisioningの本番command/registry/tableを同時に型検査する。
// Binder起動だけを除いたhost境界であり、Soong/AIDL実体buildの代替ではない。
#![allow(dead_code)]

#[path = "../../../service_runtime/src/descrambler_key_table.rs"]
mod descrambler_key_table;
#[path = "../../../service_runtime/src/descrambler_session.rs"]
mod descrambler_session;
#[path = "../../../service_runtime/src/diagnostics.rs"]
mod diagnostics;
#[path = "../../../service_runtime/src/key_provisioning_ops.rs"]
mod key_provisioning_ops;
#[path = "../../../service_runtime/src/registry.rs"]
mod registry;

mod boot {
    use crate::registry::RuntimeRegistry;

    #[derive(Default)]
    pub struct TunerServiceRuntime {
        registry: RuntimeRegistry,
    }

    impl TunerServiceRuntime {
        pub fn new() -> Self {
            Self::default()
        }

        pub(crate) fn registry_mut(&mut self) -> &mut RuntimeRegistry {
            &mut self.registry
        }
    }
}
