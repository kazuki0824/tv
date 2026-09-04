use maleicacid_tuner_hal2_descrambler::{DescramblerKeySlot, Multi2KeyMaterial};
use maleicacid_tuner_hal2_key_provisioning_bridge::{
    KeyProvisioningCommand, KeyProvisioningStatus, Multi2KeyResource,
};

use crate::boot::TunerServiceRuntime;
use crate::descrambler_key_table::KeyProvisioningMutationError;
use crate::registry::KeyProvisioningRegistryError;

fn status_from_registry_error(error: KeyProvisioningRegistryError) -> KeyProvisioningStatus {
    match error {
        KeyProvisioningRegistryError::InvalidKeyToken(_) => KeyProvisioningStatus::InvalidToken,
        KeyProvisioningRegistryError::SlotIdExhausted => KeyProvisioningStatus::ResourceBusy,
        KeyProvisioningRegistryError::Registry(KeyProvisioningMutationError::ExpiredToken) => {
  KeyProvisioningStatus::Revoked
        }
        KeyProvisioningRegistryError::Registry(KeyProvisioningMutationError::StaleEpoch) => {
  KeyProvisioningStatus::StaleEpoch
        }
        KeyProvisioningRegistryError::Registry(KeyProvisioningMutationError::ResourceExhausted) => {
  KeyProvisioningStatus::ResourceBusy
        }
        KeyProvisioningRegistryError::Registry(
  KeyProvisioningMutationError::UnknownToken
  | KeyProvisioningMutationError::InvalidIdentity
  | KeyProvisioningMutationError::IdentityMismatch,
        ) => KeyProvisioningStatus::InvalidToken,
    }
}

fn prepare_key_slot(resource: &Multi2KeyResource) -> Result<DescramblerKeySlot, KeyProvisioningStatus> {
    let even = Multi2KeyMaterial::new(
        *resource.system_key(),
        *resource.cbc_initial_value(),
        *resource.even_ks(),
    );
    let odd = Multi2KeyMaterial::new(
        *resource.system_key(),
        *resource.cbc_initial_value(),
        *resource.odd_ks(),
    );
    DescramblerKeySlot::empty()
        .try_with_even(even)
        .and_then(|slot| slot.try_with_odd(odd))
        .map_err(|_| KeyProvisioningStatus::Internal)
}

impl TunerServiceRuntime {
    pub fn apply_key_provisioning_command(
        &mut self,
        command: KeyProvisioningCommand,
    ) -> KeyProvisioningStatus {
        match command {
  KeyProvisioningCommand::Ping => KeyProvisioningStatus::Ok,
  KeyProvisioningCommand::Reserve { key_token, identity } => self
      .registry_mut()
      .reserve_key_provisioning_resource(
key_token,
identity.provider_id(),
identity.provider_generation(),
      )
      .map(|_| KeyProvisioningStatus::Ok)
      .unwrap_or_else(status_from_registry_error),
  KeyProvisioningCommand::Publish { key_token, resource } => {
      let key_slot = match prepare_key_slot(&resource) {
Ok(key_slot) => key_slot,
Err(status) => return status,
      };
      let identity = resource.identity();
      self.registry_mut()
.publish_key_provisioning_resource(
    key_token,
    identity.provider_id(),
    identity.provider_generation(),
    resource.key_epoch(),
    key_slot,
)
.map(|_| KeyProvisioningStatus::Ok)
.unwrap_or_else(status_from_registry_error)
  }
  KeyProvisioningCommand::Revoke { key_token, identity } => self
      .registry_mut()
      .revoke_key_provisioning_resource(
key_token,
identity.provider_id(),
identity.provider_generation(),
      )
      .map(|_| KeyProvisioningStatus::Ok)
      .unwrap_or_else(status_from_registry_error),
        }
    }
}

#[cfg(test)]
mod tests {
    use maleicacid_tuner_hal2_key_provisioning_bridge::{
        KeyProvisioningCommand, KeyProvisioningStatus, Multi2KeyResource,
        ProvisioningIdentity,
    };

    use crate::boot::TunerServiceRuntime;

    const PROVIDER_ID: u64 = 41;

    fn resource(epoch: u64) -> Multi2KeyResource {
        Multi2KeyResource::try_new(
  PROVIDER_ID,
  7,
  epoch,
  [0x10; 32],
  [0x20; 8],
  [0x30; 8],
  [0x40; 8],
        )
        .unwrap()
    }

    fn identity(generation: u64) -> ProvisioningIdentity {
        ProvisioningIdentity::try_new(PROVIDER_ID, generation).unwrap()
    }

    #[test]
    fn publish_update_and_revoke_follow_epoch_and_stale_token_contract() {
        let mut runtime = TunerServiceRuntime::new();
        let token = vec![0x31, 0x32];
        assert_eq!(
  runtime.apply_key_provisioning_command(KeyProvisioningCommand::Reserve {
      key_token: token.clone(),
      identity: identity(7),
  }),
  KeyProvisioningStatus::Ok
        );
        assert_eq!(
  runtime.apply_key_provisioning_command(KeyProvisioningCommand::Publish {
      key_token: token.clone(),
      resource: resource(1),
  }),
  KeyProvisioningStatus::Ok
        );
        assert_eq!(
  runtime.apply_key_provisioning_command(KeyProvisioningCommand::Publish {
      key_token: token.clone(),
      resource: resource(1),
  }),
  KeyProvisioningStatus::StaleEpoch
        );
        assert_eq!(
  runtime.apply_key_provisioning_command(KeyProvisioningCommand::Revoke {
      key_token: token.clone(),
      identity: identity(8),
  }),
  KeyProvisioningStatus::InvalidToken
        );
        assert_eq!(
  runtime.apply_key_provisioning_command(KeyProvisioningCommand::Revoke {
      key_token: token,
      identity: identity(7),
  }),
  KeyProvisioningStatus::Ok
        );
    }
}
