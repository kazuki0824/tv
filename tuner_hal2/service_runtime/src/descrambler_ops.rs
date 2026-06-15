use crate::boot::TunerServiceRuntime;
use crate::registry::{DescramblerRegistryEntry, RegistryCommitError};
use maleicacid_tuner_hal2_common::HalError;

impl TunerServiceRuntime {
    pub fn allocate_descrambler_runtime(
        &mut self,
    ) -> Result<DescramblerRegistryEntry, RegistryCommitError> {
        self.descrambler_txn().allocate_descrambler_runtime()
    }

    pub fn set_descrambler_demux_source(
        &mut self,
        descrambler_id: i32,
        demux_id: i32,
    ) -> Result<(), HalError> {
        self.descrambler_txn().set_descrambler_demux_source(descrambler_id, demux_id)
    }

    pub fn set_descrambler_key_token(
        &mut self,
        descrambler_id: i32,
        key_token: &[u8],
    ) -> Result<(), HalError> {
        self.descrambler_txn().set_descrambler_key_token(descrambler_id, key_token)
    }

    pub fn add_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        self.descrambler_txn().add_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
    }

    pub fn remove_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        self.descrambler_txn().remove_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
    }

    pub fn unregister_descrambler_runtime(
        &mut self,
        id: i32,
    ) -> Option<DescramblerRegistryEntry> {
        self.descrambler_txn().unregister_descrambler_runtime(id)
    }

    pub(crate) fn cleanup_descramblers_for_demux_owner_loss(&mut self, demux_id: i32) {
        self.descrambler_txn().cleanup_descramblers_for_demux_owner_loss(demux_id);
    }
}
