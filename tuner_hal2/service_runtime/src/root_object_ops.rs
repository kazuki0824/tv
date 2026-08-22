use crate::boot::TunerServiceRuntime;
use crate::error_mapping::{object_table_error_to_hal, registry_commit_error_to_hal};
use crate::method_dispatch::plan_object_method_dispatch;
use crate::open_rollback::finish_open_rollback;
use crate::root_method_txn::{is_public_demux_id, published_demux_ids};
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
        method_plan.command_plan,
        method_plan.command.runtime_executable_request(),
    )
}

/// Call-local owner for every public root-object open transaction.
///
/// Persistent resource, runtime, and object-table state remains in their
/// canonical owners; this value only sequences one open or rollback call.
pub struct RootOpenTxn<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub fn root_open_txn(&mut self) -> RootOpenTxn<'_> {
        RootOpenTxn { runtime: self }
    }
}

impl RootOpenTxn<'_> {
    pub fn open_frontend_root_object_for_id(
        &mut self,
        frontend_id: i32,
        method: AidlMethodCall,
    ) -> Result<RuntimeObjectEntry, HalError> {
        preflight_root_method_dispatch(self.runtime, method)?;
        let Some(frontend) = self.runtime.frontend_entry(frontend_id) else {
            return Err(HalError::invalid_argument(
                maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
                "frontend id is not published by the capability snapshot",
            ));
        };
        if self.runtime.has_active_frontend_lease(frontend_id) {
            return Err(HalError::Unsupported(
                "frontend id is already leased by a live object",
            ));
        }
        if self.runtime.has_active_frontend_group_lease(frontend.capability.exclusive_group_id) {
            return Err(HalError::Unsupported(
                "frontend physical group is already leased by a live object",
            ));
        }
        if self.runtime.active_frontend_lease_count(frontend.system)
            >= self.runtime.current_max_number_of_frontends(frontend.system)
        {
            return Err(HalError::Unsupported(
                "frontend lease limit is reached for this frontend type",
            ));
        }
        register_root_object(
            self.runtime,
            AidlObjectKind::Frontend,
            i64::from(frontend_id),
        )
    }

    pub fn open_demux_root_object(
        &mut self,
        method: AidlMethodCall,
    ) -> Result<RuntimeObjectEntry, HalError> {
        preflight_root_method_dispatch(self.runtime, method)?;
        let demux_id = published_demux_ids(self.runtime.capability_snapshot())?
            .iter()
            .copied()
            .find(|demux_id| !self.runtime.has_demux_id(*demux_id))
            .ok_or(HalError::Unsupported(
                "no published demux lease is available",
            ))?;
        let entry = self
            .runtime
            .allocate_demux_runtime_for_public_id(demux_id)
            .map_err(|error| {
                registry_commit_error_to_hal(error, "demux runtime allocation failed")
            })?;
        match register_root_object(
            self.runtime,
            AidlObjectKind::Demux,
            i64::from(entry.id.0),
        ) {
            Ok(object_entry) => Ok(object_entry),
            Err(error) => {
                match unregister_demux_runtime_for_open_rollback(
                    self.runtime,
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
        preflight_root_method_dispatch(self.runtime, method)?;
        if !is_public_demux_id(self.runtime.capability_snapshot(), demux_id)? {
            return Err(HalError::invalid_argument(
                maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
                "demux id is not published by the capability snapshot",
            ));
        }
        if self.runtime.has_demux_id(demux_id) {
            return Err(HalError::Unsupported(
                "published demux id is already leased",
            ));
        }
        let entry = self
            .runtime
            .allocate_demux_runtime_for_public_id(demux_id)
            .map_err(|error| {
                registry_commit_error_to_hal(error, "demux runtime allocation failed")
            })?;
        match register_root_object(self.runtime, AidlObjectKind::Demux, i64::from(demux_id)) {
            Ok(object_entry) => Ok(object_entry),
            Err(error) => match unregister_demux_runtime_for_open_rollback(
                self.runtime,
                entry.id.0,
                "demux root object rollback after AIDL registration failure",
            ) {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(compose_primary_cleanup_failure(
                    "demux root object registration failure",
                    error,
                    cleanup_error,
                )),
            },
        }
    }

    pub fn open_descrambler_root_object(
        &mut self,
        method: AidlMethodCall,
    ) -> Result<RuntimeObjectEntry, HalError> {
        preflight_root_method_dispatch(self.runtime, method)?;
        let entry = self.runtime.allocate_descrambler_runtime().map_err(|error| {
            registry_commit_error_to_hal(error, "descrambler runtime allocation failed")
        })?;
        match register_root_object(
            self.runtime,
            AidlObjectKind::Descrambler,
            i64::from(entry.id.0),
        ) {
            Ok(object_entry) => Ok(object_entry),
            Err(error) => {
                match unregister_descrambler_runtime_for_open_rollback(
                    self.runtime,
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
        preflight_root_method_dispatch(self.runtime, method)?;
        let lnb_key = crate::registry::LnbRuntimeId(lnb_id);
        if !self.runtime.has_lnb_id(lnb_id) {
            return Err(HalError::invalid_argument(
                maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
                "LNB id is not published by the capability snapshot",
            ));
        }
        if self
            .runtime
            .object_table()
            .active_public_runtime_ids(AidlObjectKind::Lnb)
            .contains(&maleicacid_tuner_hal2_resource_ledger::LedgerId(i64::from(lnb_id)))
            || !self
                .runtime
                .registry()
                .lnb_runtime(lnb_key)
                .is_some_and(|runtime| {
                    matches!(
                        runtime.state(),
                        maleicacid_tuner_hal2_lnb::LnbRuntimeState::Open
                            | maleicacid_tuner_hal2_lnb::LnbRuntimeState::Closed
                    )
                })
        {
            return Err(HalError::Unsupported(
                "published LNB endpoint is not currently available",
            ));
        }
        let object_entry =
            register_root_object(self.runtime, AidlObjectKind::Lnb, i64::from(lnb_id))?;
        if let Err(error) = self.runtime.open_lnb_for_public_id(lnb_id) {
            let rollback = finish_open_rollback(
                rollback_root_object_registration(
                    self.runtime,
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
        preflight_root_method_dispatch(self.runtime, method)?;
        if lnb_name.is_empty() {
            return Err(HalError::invalid_argument(
                maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
                "LNB name must not be empty",
            ));
        }
        Err(HalError::Unsupported(
            "named external LNB endpoints are not available",
        ))
    }

    pub fn rollback_root_object_entry_after_aidl_failure(
        &mut self,
        entry: RuntimeObjectEntry,
        unregister_runtime: bool,
    ) -> Result<Option<i32>, HalError> {
        let lnb_cleanup_id = if entry.object_kind == AidlObjectKind::Lnb {
            Some(i32::try_from(entry.ledger_id.0).map_err(|_| {
                HalError::internal(
                    maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                    "LNB runtime id is outside i32 range during root object rollback",
                )
            })?)
        } else {
            None
        };
        let object_registration_rollback =
            rollback_root_object_registration(self.runtime, entry.object_id, entry.generation);
        finish_open_rollback(
            object_registration_rollback,
            || match entry.object_kind {
                AidlObjectKind::Lnb => Ok(()),
                AidlObjectKind::Demux if unregister_runtime => {
                    let public_runtime_id = i32::try_from(entry.ledger_id.0).map_err(|_| {
                        HalError::internal(
                            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                            "demux runtime id is outside i32 range during root object rollback",
                        )
                    })?;
                    unregister_demux_runtime_for_open_rollback(
                        self.runtime,
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
                        self.runtime,
                        public_runtime_id,
                        "descrambler root object open rollback",
                    )
                }
                _ => Ok(()),
            },
            "root object open rollback",
        )?;
        Ok(lnb_cleanup_id)
    }
}
