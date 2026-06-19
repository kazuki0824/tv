use crate::boot::TunerServiceRuntime;
use crate::method_dispatch::plan_object_method_dispatch;
use crate::object_method_txn::ObjectMethodDispatchPreflight;
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
        self.descrambler_txn()
            .set_descrambler_demux_source(descrambler_id, demux_id)
    }

    pub fn set_descrambler_key_token(
        &mut self,
        descrambler_id: i32,
        key_token: &[u8],
    ) -> Result<(), HalError> {
        self.descrambler_txn()
            .set_descrambler_key_token(descrambler_id, key_token)
    }

    pub fn add_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        self.descrambler_txn().add_descrambler_pid_non_null_source(
            descrambler_id,
            pid,
            source_filter_id,
        )
    }

    pub fn remove_descrambler_pid_non_null_source(
        &mut self,
        descrambler_id: i32,
        pid: u16,
        source_filter_id: i32,
    ) -> Result<(), HalError> {
        self.descrambler_txn()
            .remove_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
    }

    pub fn unregister_descrambler_runtime(
        &mut self,
        id: i32,
    ) -> Result<Option<DescramblerRegistryEntry>, HalError> {
        self.descrambler_txn().unregister_descrambler_runtime(id)
    }

    pub(crate) fn cleanup_descramblers_for_demux_owner_loss(
        &mut self,
        demux_id: i32,
    ) -> Result<(), HalError> {
        self.descrambler_txn()
            .cleanup_descramblers_for_demux_owner_loss(demux_id)
    }
}

impl TunerServiceRuntime {
    pub fn set_descrambler_demux_source_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        demux_id: i32,
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let descrambler_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Descrambler,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.set_descrambler_demux_source(descrambler_id, demux_id)
    }

    pub fn set_descrambler_key_token_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        key_token: &[u8],
        command_plan: maleicacid_tuner_hal2_domain_request::CommandPlan,
        executable_request: Option<maleicacid_tuner_hal2_domain_request::RuntimeExecutableRequest>,
    ) -> Result<(), HalError> {
        let descrambler_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Descrambler,
        )?;
        plan_object_method_dispatch(self, command_plan, executable_request)?;
        self.set_descrambler_key_token(descrambler_id, key_token)
    }

    pub fn add_descrambler_pid_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        pid: u16,
        source_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        source_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
        let descrambler_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Descrambler,
        )?;
        let source_filter_id = self.public_runtime_id_for_object_method(
            source_object_id,
            source_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        dispatch.plan(self)?;
        self.add_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
    }

    pub fn remove_descrambler_pid_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        pid: u16,
        source_object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        source_generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodDispatchPreflight,
    ) -> Result<(), HalError> {
        let descrambler_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Descrambler,
        )?;
        let source_filter_id = self.public_runtime_id_for_object_method(
            source_object_id,
            source_generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Filter,
        )?;
        dispatch.plan(self)?;
        self.remove_descrambler_pid_non_null_source(descrambler_id, pid, source_filter_id)
    }
}
