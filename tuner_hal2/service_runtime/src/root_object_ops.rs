use crate::boot::TunerServiceRuntime;
use crate::error_mapping::{object_table_error_to_hal, registry_commit_error_to_hal};
use crate::method_dispatch::plan_object_method_dispatch;
use crate::open_rollback::finish_open_rollback;
use crate::{RuntimeObjectEntry, RuntimeOwnerRelation};
use maleicacid_tuner_hal2_binder_adapter::{AidlMethodAdapter, AidlMethodCall};
use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError};
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

fn register_root_object(
    runtime: &mut TunerServiceRuntime,
    object_kind: AidlObjectKind,
    public_runtime_id: i64,
) -> Result<RuntimeObjectEntry, HalError> {
    runtime
        .register_aidl_object_for_runtime_auto_generation(
            object_kind,
            public_runtime_id,
            RuntimeOwnerRelation::Root,
        )
        .map_err(object_table_error_to_hal)
}

fn rollback_root_object_registration(
    runtime: &mut TunerServiceRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<(), HalError> {
    runtime
        .unregister_aidl_object_after_registration_failure(object_id, generation)
        .map(|_| ())
        .map_err(object_table_error_to_hal)
}

fn unregister_demux_runtime_for_open_rollback(
    runtime: &mut TunerServiceRuntime,
    demux_id: i32,
    context: &'static str,
) -> Result<(), HalError> {
    match runtime.unregister_demux_runtime(demux_id) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HalError::cleanup_failed(
            context,
            format!("demux runtime is missing during rollback: id={demux_id}"),
        )),
        Err(error) => Err(error),
    }
}

fn unregister_descrambler_runtime_for_open_rollback(
    runtime: &mut TunerServiceRuntime,
    descrambler_id: i32,
    context: &'static str,
) -> Result<(), HalError> {
    match runtime.unregister_descrambler_runtime(descrambler_id) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HalError::cleanup_failed(
            context,
            format!("descrambler runtime is missing during rollback: id={descrambler_id}"),
        )),
        Err(error) => Err(error),
    }
}

fn preflight_root_method_dispatch(
    runtime: &mut TunerServiceRuntime,
    method: AidlMethodCall,
) -> Result<(), HalError> {
    let method_plan = AidlMethodAdapter::plan(method)?;
    plan_object_method_dispatch(
        runtime,
        method_plan.command_plan(),
        method_plan.executable_request(),
    )
}

impl TunerServiceRuntime {
    pub fn open_frontend_root_object_for_id(
        &mut self,
        frontend_id: i32,
        method: AidlMethodCall,
    ) -> Result<RuntimeObjectEntry, HalError> {
        preflight_root_method_dispatch(self, method)?;
        if !self.has_frontend_id(frontend_id) {
            return Err(HalError::Unsupported("frontend id is not available"));
        }
        register_root_object(self, AidlObjectKind::Frontend, i64::from(frontend_id))
    }

    pub fn open_demux_root_object(
        &mut self,
        method: AidlMethodCall,
    ) -> Result<RuntimeObjectEntry, HalError> {
        preflight_root_method_dispatch(self, method)?;
        let entry = self.allocate_demux_runtime().map_err(|error| {
            registry_commit_error_to_hal(error, "demux runtime allocation failed")
        })?;
        match register_root_object(self, AidlObjectKind::Demux, i64::from(entry.id.0)) {
            Ok(object_entry) => Ok(object_entry),
            Err(error) => {
                match unregister_demux_runtime_for_open_rollback(
                    self,
                    entry.id.0,
                    "demux root object rollback after AIDL registration failure",
                ) {
                    Ok(()) => Err(error),
                    Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                        "demux root object registration failure",
                        error,
                        cleanup_error,
                    )),
                }
            }
        }
    }

    pub fn open_demux_root_object_by_id(
        &mut self,
        demux_id: i32,
        method: AidlMethodCall,
    ) -> Result<RuntimeObjectEntry, HalError> {
        preflight_root_method_dispatch(self, method)?;
        if !self.has_demux_id(demux_id) {
            return Err(HalError::Unsupported("demux id is not available"));
        }
        register_root_object(self, AidlObjectKind::Demux, i64::from(demux_id))
    }

    pub fn open_descrambler_root_object(
        &mut self,
        method: AidlMethodCall,
    ) -> Result<RuntimeObjectEntry, HalError> {
        preflight_root_method_dispatch(self, method)?;
        let entry = self.allocate_descrambler_runtime().map_err(|error| {
            registry_commit_error_to_hal(error, "descrambler runtime allocation failed")
        })?;
        match register_root_object(self, AidlObjectKind::Descrambler, i64::from(entry.id.0)) {
            Ok(object_entry) => Ok(object_entry),
            Err(error) => {
                match unregister_descrambler_runtime_for_open_rollback(
                    self,
                    entry.id.0,
                    "descrambler root object rollback after AIDL registration failure",
                ) {
                    Ok(()) => Err(error),
                    Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                        "descrambler root object registration failure",
                        error,
                        cleanup_error,
                    )),
                }
            }
        }
    }

    pub fn open_lnb_root_object_for_id(
        &mut self,
        lnb_id: i32,
        method: AidlMethodCall,
    ) -> Result<RuntimeObjectEntry, HalError> {
        preflight_root_method_dispatch(self, method)?;
        if !self.has_lnb_id(lnb_id) {
            return Err(HalError::Unsupported("LNB id is not available"));
        }
        let object_entry = register_root_object(self, AidlObjectKind::Lnb, i64::from(lnb_id))?;
        if let Err(error) = self.open_lnb_for_public_id(lnb_id) {
            let rollback = finish_open_rollback(
                rollback_root_object_registration(
                    self,
                    object_entry.object_id,
                    object_entry.generation,
                ),
                || Ok(()),
                "LNB root object open rollback after runtime open failure",
            );
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(compose_primary_cleanup_failure(
                    "LNB root object open failure",
                    error,
                    rollback_error,
                )),
            };
        }
        Ok(object_entry)
    }

    pub fn open_lnb_root_object_by_name(
        &mut self,
        lnb_name: &str,
        method: AidlMethodCall,
    ) -> Result<(i32, RuntimeObjectEntry), HalError> {
        preflight_root_method_dispatch(self, method)?;
        let Some(lnb_id) = self.lnb_id_by_name(lnb_name) else {
            return Err(HalError::Unsupported("LNB name is not available"));
        };
        let object_entry = register_root_object(self, AidlObjectKind::Lnb, i64::from(lnb_id))?;
        if let Err(error) = self.open_lnb_for_public_id(lnb_id) {
            let rollback = finish_open_rollback(
                rollback_root_object_registration(
                    self,
                    object_entry.object_id,
                    object_entry.generation,
                ),
                || Ok(()),
                "LNB root object open rollback after runtime open failure",
            );
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(compose_primary_cleanup_failure(
                    "LNB root object open failure",
                    error,
                    rollback_error,
                )),
            };
        }
        Ok((lnb_id, object_entry))
    }

    pub fn rollback_root_object_entry_after_aidl_failure(
        &mut self,
        entry: RuntimeObjectEntry,
        unregister_runtime: bool,
    ) -> Result<(), HalError> {
        let object_registration_rollback =
            rollback_root_object_registration(self, entry.object_id, entry.generation);
        finish_open_rollback(
            object_registration_rollback,
            || match entry.object_kind {
                AidlObjectKind::Lnb => {
                    let public_runtime_id = i32::try_from(entry.ledger_id.0).map_err(|_| {
                        HalError::internal(
                            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                            "LNB runtime id is outside i32 range during root object rollback",
                        )
                    })?;
                    self.close_lnb_explicit(public_runtime_id)
                }
                AidlObjectKind::Demux if unregister_runtime => {
                    let public_runtime_id = i32::try_from(entry.ledger_id.0).map_err(|_| {
                        HalError::internal(
                            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                            "demux runtime id is outside i32 range during root object rollback",
                        )
                    })?;
                    unregister_demux_runtime_for_open_rollback(
                        self,
                        public_runtime_id,
                        "demux root object open rollback",
                    )
                }
                AidlObjectKind::Descrambler if unregister_runtime => {
                    let public_runtime_id = i32::try_from(entry.ledger_id.0).map_err(|_| {
                        HalError::internal(
                            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                            "descrambler runtime id is outside i32 range during root object rollback",
                        )
                    })?;
                    unregister_descrambler_runtime_for_open_rollback(
                        self,
                        public_runtime_id,
                        "descrambler root object open rollback",
                    )
                }
                _ => Ok(()),
            },
            "root object open rollback",
        )
    }
}
