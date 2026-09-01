use std::sync::{Arc, Mutex, MutexGuard};

use crate::lnb_control_txn::{LnbControlTxn, PreparedLnbControlTxn};
use crate::object_domain_cleanup::ObjectDomainCleanupCommand;
use crate::object_lifecycle::{
    aidl_public_runtime_id_for_close_cleanup, lnb_public_id_for_live_object_result,
};
use crate::object_method_use_case::ObjectMethodExecutionToken;
use crate::registry::{
    FrontendRuntimeId, LnbPhysicalIoAuthority, LnbRuntimeId, SatellitePowerTopology,
};
use maleicacid_tuner_hal2_common::{compose_primary_cleanup_failure, HalError, HalInternalKind};
use maleicacid_tuner_hal2_domain_request::{
    AidlObjectGeneration, AidlObjectId, AidlObjectKind, LnbSetSatellitePositionRequest,
    LnbToneRequest, LnbVoltageRequest,
};

use crate::boot::TunerServiceRuntime;

#[derive(Debug, Eq, PartialEq)]
#[must_use = "frontend fixed-power preparation must be completed or rolled back by value"]
pub(crate) struct FrontendFixedPowerPreparation {
    frontend_id: FrontendRuntimeId,
    newly_retained: bool,
}

impl FrontendFixedPowerPreparation {
    pub(crate) const fn frontend_id(&self) -> FrontendRuntimeId {
        self.frontend_id
    }

    pub(crate) const fn newly_retained(&self) -> bool {
        self.newly_retained
    }
}

impl TunerServiceRuntime {
    #[cfg(test)]
    pub(crate) fn set_frontend_lnb(
        &mut self,
        frontend_id: i32,
        lnb_id: i32,
    ) -> Result<(), HalError> {
        let authority = self
            .registry()
            .lnb_physical_io_authority(LnbRuntimeId(lnb_id))
            .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
        authority.execute(|permit| {
            let prepared = self
                .lnb_mutation_context()
                .prepare_frontend_lnb_assignment(frontend_id, lnb_id)?;
            let executed = prepared.execute(&permit);
            self.lnb_mutation_context()
                .commit_frontend_lnb_assignment(executed)
        })
    }

    #[cfg(test)]
    pub(crate) fn apply_lnb_voltage(
        &mut self,
        lnb_id: i32,
        request: LnbVoltageRequest,
    ) -> Result<(), HalError> {
        self.execute_lnb_control_for_test(lnb_id, |txn| txn.prepare_voltage(lnb_id, request))
    }

    #[cfg(test)]
    pub(crate) fn apply_lnb_tone(
        &mut self,
        lnb_id: i32,
        request: LnbToneRequest,
    ) -> Result<(), HalError> {
        self.execute_lnb_control_for_test(lnb_id, |txn| txn.prepare_tone(lnb_id, request))
    }

    #[cfg(test)]
    pub(crate) fn apply_lnb_satellite_position(
        &mut self,
        lnb_id: i32,
        request: LnbSetSatellitePositionRequest,
    ) -> Result<(), HalError> {
        self.execute_lnb_control_for_test(lnb_id, |txn| {
            txn.prepare_satellite_position(lnb_id, request)
        })
    }

    #[cfg(test)]
    pub(crate) fn send_lnb_diseqc(&mut self, lnb_id: i32, payload: &[u8]) -> Result<(), HalError> {
        let authority = self
            .registry()
            .lnb_physical_io_authority(LnbRuntimeId(lnb_id))
            .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
        authority.execute(|permit| {
            let prepared = self
                .lnb_mutation_context()
                .prepare_lnb_diseqc(lnb_id, payload)?;
            let executed = prepared.execute(&permit);
            self.lnb_mutation_context().finish_lnb_diseqc(executed)
        })
    }

    pub(crate) fn open_lnb_for_public_id(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_mutation_context().open_lnb_for_public_id(lnb_id)
    }

    pub(crate) fn commit_lnb_callback_registration(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_mutation_context()
            .commit_lnb_callback_registration(lnb_id)
    }

    pub(crate) fn clear_lnb_callback_registration(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_mutation_context()
            .clear_lnb_callback_registration(lnb_id)
    }

    pub(crate) fn record_lnb_drop_leak(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.lnb_mutation_context().record_lnb_drop_leak(lnb_id)
    }

    pub fn record_lnb_drop_leak_after_domain_cleanup_command(
        &mut self,
        command: ObjectDomainCleanupCommand,
    ) -> Result<(), HalError> {
        let lnb_id =
            lnb_public_id_for_live_object_result(self, command.object_id(), command.generation())?;
        self.record_lnb_drop_leak(lnb_id)
    }

    #[cfg(test)]
    fn execute_lnb_control_for_test<F>(&mut self, lnb_id: i32, prepare: F) -> Result<(), HalError>
    where
        F: FnOnce(&mut LnbControlTxn<'_>) -> Result<PreparedLnbControlTxn, HalError>,
    {
        let authority = self
            .registry()
            .lnb_physical_io_authority(LnbRuntimeId(lnb_id))
            .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
        authority.execute(|permit| {
            let prepared = prepare(&mut self.lnb_control_txn())?;
            let completed = prepared.execute(&permit);
            self.lnb_control_txn().finish(completed)
        })
    }
}

pub type SharedLnbRuntime = Arc<Mutex<TunerServiceRuntime>>;

fn lock_shared_lnb_runtime(
    runtime: &SharedLnbRuntime,
) -> Result<MutexGuard<'_, TunerServiceRuntime>, HalError> {
    runtime.lock().map_err(|_| {
        HalError::internal(
            HalInternalKind::InvariantViolation,
            "service runtime lock poisoned during LNB operation",
        )
    })
}

fn restore_fixed_power_lease_after_failure(
    runtime: &mut TunerServiceRuntime,
    frontend_id: FrontendRuntimeId,
    lnb_id: LnbRuntimeId,
    primary: HalError,
) -> HalError {
    match runtime
        .registry_mut()
        .retain_frontend_fixed_power_lease(frontend_id, lnb_id)
    {
        Ok(_) => primary,
        Err(cleanup) => compose_primary_cleanup_failure(
            "fixed LNB power failure and rail lease restoration both failed",
            primary,
            cleanup,
        ),
    }
}

fn rollback_new_fixed_power_lease(
    runtime: &mut TunerServiceRuntime,
    frontend_id: FrontendRuntimeId,
    newly_retained: bool,
    primary: HalError,
) -> HalError {
    if !newly_retained {
        return primary;
    }
    match runtime
        .registry_mut()
        .release_frontend_fixed_power_lease(frontend_id)
    {
        Ok(Some(_)) => primary,
        Ok(None) => compose_primary_cleanup_failure(
            "fixed LNB power preparation failed after its rail lease disappeared",
            primary,
            HalError::internal(
                HalInternalKind::InvariantViolation,
                "new fixed-power lease was missing during rollback",
            ),
        ),
        Err(cleanup) => compose_primary_cleanup_failure(
            "fixed LNB power preparation and rail lease rollback both failed",
            primary,
            cleanup,
        ),
    }
}

pub(crate) fn ensure_frontend_fixed_power_for_object(
    runtime: &SharedLnbRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
) -> Result<FrontendFixedPowerPreparation, HalError> {
    let (frontend_id, lnb_id, authority) = {
        let guard = lock_shared_lnb_runtime(runtime)?;
        let frontend = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
        let frontend_id = frontend.id;
        if frontend.satellite_power_topology != SatellitePowerTopology::InternalFixed15V {
            return Ok(FrontendFixedPowerPreparation {
                frontend_id,
                newly_retained: false,
            });
        }
        let lnb_id = guard
            .registry()
            .lnb_for_frontend(frontend_id)
            .map(|entry| entry.id)
            .ok_or(HalError::Unsupported(
                "internal fixed-15V frontend has no registered LNB rail",
            ))?;
        let authority = guard
            .registry()
            .lnb_physical_io_authority(lnb_id)
            .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
        (frontend_id, lnb_id, authority)
    };

    authority.execute(|permit| {
        let (prepared, newly_retained) = {
            let mut guard = lock_shared_lnb_runtime(runtime)?;
            let current = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
            if current.id != frontend_id
                || current.satellite_power_topology != SatellitePowerTopology::InternalFixed15V
            {
                return Err(HalError::invalid_state(
                    maleicacid_tuner_hal2_common::HalInvalidStateKind::InvalidLifecycle,
                    "frontend fixed-power topology changed before rail preparation",
                ));
            }
            let newly_retained = guard
                .registry_mut()
                .retain_frontend_fixed_power_lease(frontend_id, lnb_id)?;
            let already_applied = guard.registry().lnb_runtime(lnb_id).is_some_and(|lnb| {
                lnb.state() == maleicacid_tuner_hal2_lnb::LnbRuntimeState::Open
                    && lnb.registry_state().voltage
                        == maleicacid_tuner_hal2_lnb::LnbVoltage::Voltage15V
            });
            if already_applied {
                return Ok(FrontendFixedPowerPreparation {
                    frontend_id,
                    newly_retained,
                });
            }
            if guard.registry().lnb_runtime(lnb_id).is_some_and(|lnb| {
                lnb.state() == maleicacid_tuner_hal2_lnb::LnbRuntimeState::Closed
            }) {
                if let Err(error) = guard
                    .registry_mut()
                    .reopen_lnb(lnb_id)
                    .map_err(crate::boot::lnb_txn::map_lnb_failure)
                {
                    return Err(rollback_new_fixed_power_lease(
                        &mut guard,
                        frontend_id,
                        newly_retained,
                        error,
                    ));
                }
            }
            let prepared = match guard.lnb_control_txn().prepare_internal_fixed_15v(lnb_id.0) {
                Ok(prepared) => prepared,
                Err(error) => {
                    return Err(rollback_new_fixed_power_lease(
                        &mut guard,
                        frontend_id,
                        newly_retained,
                        error,
                    ));
                }
            };
            (prepared, newly_retained)
        };

        let completed = prepared.execute(&permit);
        let backend_result = completed.backend_result();
        let finish_result = lock_shared_lnb_runtime(runtime)?
            .lnb_control_txn()
            .finish(completed);
        match finish_result {
            Ok(()) => Ok(FrontendFixedPowerPreparation {
                frontend_id,
                newly_retained,
            }),
            Err(error)
                if matches!(
                    backend_result,
                    maleicacid_tuner_hal2_lnb::LnbBackendApplyOutcome::Rejected(_)
                ) =>
            {
                let mut guard = lock_shared_lnb_runtime(runtime)?;
                Err(rollback_new_fixed_power_lease(
                    &mut guard,
                    frontend_id,
                    newly_retained,
                    error,
                ))
            }
            Err(error) => Err(error),
        }
    })
}

pub(crate) fn release_frontend_fixed_power_after_operation(
    runtime: &SharedLnbRuntime,
    frontend_id: FrontendRuntimeId,
) -> Result<(), HalError> {
    let (lnb_id, authority) = {
        let guard = lock_shared_lnb_runtime(runtime)?;
        let Some(lnb_id) = guard.registry().frontend_fixed_power_lnb(frontend_id) else {
            return Ok(());
        };
        let authority = guard
            .registry()
            .lnb_physical_io_authority(lnb_id)
            .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
        (lnb_id, authority)
    };

    authority.execute(|permit| {
        let prepared = {
            let mut guard = lock_shared_lnb_runtime(runtime)?;
            if guard.registry().frontend_fixed_power_lnb(frontend_id) != Some(lnb_id) {
                return Ok(());
            }
            let operation_is_terminal = guard
                .registry()
                .frontend_runtime(frontend_id)
                .map(|frontend| {
                    matches!(
                        frontend.snapshot().state,
                        maleicacid_tuner_hal2_device::FrontendRuntimeState::Idle
                            | maleicacid_tuner_hal2_device::FrontendRuntimeState::Closing
                            | maleicacid_tuner_hal2_device::FrontendRuntimeState::Failed
                    )
                })
                .unwrap_or(true);
            if !operation_is_terminal {
                return Ok(());
            }
            let state_is_safe = guard.registry().lnb_runtime(lnb_id).is_some_and(|lnb| {
                lnb.registry_state() == maleicacid_tuner_hal2_lnb::LnbElectricalState::safe()
            });
            let remaining = match guard
                .registry_mut()
                .release_frontend_fixed_power_lease(frontend_id)?
            {
                Some((released_lnb_id, remaining)) if released_lnb_id == lnb_id => remaining,
                Some(_) => {
                    return Err(HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "fixed-power release changed physical LNB identity",
                    ));
                }
                None => return Ok(()),
            };
            if remaining != 0 || state_is_safe {
                return Ok(());
            }
            match guard
                .lnb_control_txn()
                .prepare_voltage(lnb_id.0, LnbVoltageRequest::None)
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    return Err(restore_fixed_power_lease_after_failure(
                        &mut guard,
                        frontend_id,
                        lnb_id,
                        error,
                    ));
                }
            }
        };

        let completed = prepared.execute(&permit);
        match lock_shared_lnb_runtime(runtime)?
            .lnb_control_txn()
            .finish(completed)
        {
            Ok(()) => Ok(()),
            Err(error) => {
                let mut guard = lock_shared_lnb_runtime(runtime)?;
                Err(restore_fixed_power_lease_after_failure(
                    &mut guard,
                    frontend_id,
                    lnb_id,
                    error,
                ))
            }
        }
    })
}

fn live_lnb_io_authority(
    runtime: &SharedLnbRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<(i32, LnbPhysicalIoAuthority), HalError> {
    let guard = lock_shared_lnb_runtime(runtime)?;
    let lnb_id =
        guard.public_runtime_id_for_object_method(object_id, generation, AidlObjectKind::Lnb)?;
    let authority = guard
        .registry()
        .lnb_physical_io_authority(LnbRuntimeId(lnb_id))
        .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
    Ok((lnb_id, authority))
}

fn execute_lnb_control_object_use_case<F>(
    runtime: SharedLnbRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    dispatch: ObjectMethodExecutionToken,
    prepare: F,
) -> Result<(), HalError>
where
    F: FnOnce(&mut LnbControlTxn<'_>, i32) -> Result<PreparedLnbControlTxn, HalError>,
{
    let (lnb_id, authority) = live_lnb_io_authority(&runtime, object_id, generation)?;
    authority.execute(|permit| {
        let prepared = {
            let mut guard = lock_shared_lnb_runtime(&runtime)?;
            dispatch.consume_for_object(&mut guard, object_id, generation, AidlObjectKind::Lnb)?;
            let current_lnb_id = guard.public_runtime_id_for_object_method(
                object_id,
                generation,
                AidlObjectKind::Lnb,
            )?;
            if current_lnb_id != lnb_id {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "LNB object changed physical endpoint before I/O preparation",
                ));
            }
            prepare(&mut guard.lnb_control_txn(), lnb_id)?
        };
        let completed = prepared.execute(&permit);
        lock_shared_lnb_runtime(&runtime)?
            .lnb_control_txn()
            .finish(completed)
    })
}

pub fn apply_lnb_voltage_object_use_case(
    runtime: SharedLnbRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    request: LnbVoltageRequest,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    execute_lnb_control_object_use_case(runtime, object_id, generation, dispatch, |txn, lnb_id| {
        txn.prepare_voltage(lnb_id, request)
    })
}

pub fn apply_lnb_tone_object_use_case(
    runtime: SharedLnbRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    request: LnbToneRequest,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    execute_lnb_control_object_use_case(runtime, object_id, generation, dispatch, |txn, lnb_id| {
        txn.prepare_tone(lnb_id, request)
    })
}

pub fn apply_lnb_satellite_position_object_use_case(
    runtime: SharedLnbRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    request: LnbSetSatellitePositionRequest,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    execute_lnb_control_object_use_case(runtime, object_id, generation, dispatch, |txn, lnb_id| {
        txn.prepare_satellite_position(lnb_id, request)
    })
}

pub fn send_lnb_diseqc_object_use_case(
    runtime: SharedLnbRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
    payload: Vec<u8>,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let (lnb_id, authority) = live_lnb_io_authority(&runtime, object_id, generation)?;
    authority.execute(|permit| {
        let prepared = {
            let mut guard = lock_shared_lnb_runtime(&runtime)?;
            dispatch.consume_for_object(&mut guard, object_id, generation, AidlObjectKind::Lnb)?;
            let current_lnb_id = guard.public_runtime_id_for_object_method(
                object_id,
                generation,
                AidlObjectKind::Lnb,
            )?;
            if current_lnb_id != lnb_id {
                return Err(HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "LNB object changed physical endpoint before DiSEqC I/O preparation",
                ));
            }
            guard
                .lnb_mutation_context()
                .prepare_lnb_diseqc(lnb_id, &payload)?
        };
        let executed = prepared.execute(&permit);
        lock_shared_lnb_runtime(&runtime)?
            .lnb_mutation_context()
            .finish_lnb_diseqc(executed)
    })
}

fn lnb_io_authority_for_runtime_id(
    runtime: &SharedLnbRuntime,
    lnb_id: i32,
) -> Result<LnbPhysicalIoAuthority, HalError> {
    lock_shared_lnb_runtime(runtime)?
        .registry()
        .lnb_physical_io_authority(LnbRuntimeId(lnb_id))
        .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)
}

fn close_lnb_runtime_with_authority(
    runtime: &SharedLnbRuntime,
    lnb_id: i32,
    authority: LnbPhysicalIoAuthority,
    reason: maleicacid_tuner_hal2_lnb::LnbLifecycleReason,
) -> Result<(), HalError> {
    authority.execute(|permit| {
        let prepared = lock_shared_lnb_runtime(runtime)?
            .lnb_mutation_context()
            .prepare_lnb_lifecycle_close(lnb_id, reason)?;
        let executed = prepared.execute(&permit);
        lock_shared_lnb_runtime(runtime)?
            .lnb_mutation_context()
            .finish_lnb_lifecycle_close(executed)
    })
}

pub fn close_lnb_explicit_after_object_close_begin_use_case(
    runtime: SharedLnbRuntime,
    object_id: AidlObjectId,
    generation: AidlObjectGeneration,
) -> Result<(), HalError> {
    let (lnb_id, authority) = {
        let guard = lock_shared_lnb_runtime(&runtime)?;
        let lnb_id = aidl_public_runtime_id_for_close_cleanup(
            &guard,
            object_id,
            generation,
            AidlObjectKind::Lnb,
        )?;
        let authority = guard
            .registry()
            .lnb_physical_io_authority(LnbRuntimeId(lnb_id))
            .ok_or_else(crate::boot::lnb_txn::missing_lnb_error)?;
        (lnb_id, authority)
    };
    close_lnb_runtime_with_authority(
        &runtime,
        lnb_id,
        authority,
        maleicacid_tuner_hal2_lnb::LnbLifecycleReason::PublicClose,
    )?;
    let mut guard = lock_shared_lnb_runtime(&runtime)?;
    for frontend_id in guard
        .registry()
        .selected_frontends_for_lnb(LnbRuntimeId(lnb_id))
    {
        crate::frontend_ops::FrontendLnbRelationTxn::release(&mut guard, frontend_id.0)?;
    }
    Ok(())
}

pub fn close_lnb_after_root_open_rollback_use_case(
    runtime: SharedLnbRuntime,
    lnb_id: i32,
) -> Result<(), HalError> {
    let authority = lnb_io_authority_for_runtime_id(&runtime, lnb_id)?;
    close_lnb_runtime_with_authority(
        &runtime,
        lnb_id,
        authority,
        maleicacid_tuner_hal2_lnb::LnbLifecycleReason::PublicClose,
    )?;
    let mut guard = lock_shared_lnb_runtime(&runtime)?;
    for frontend_id in guard
        .registry()
        .selected_frontends_for_lnb(LnbRuntimeId(lnb_id))
    {
        crate::frontend_ops::FrontendLnbRelationTxn::release(&mut guard, frontend_id.0)?;
    }
    Ok(())
}

pub(crate) fn close_lnbs_from_frontend_owner_loss_report(
    runtime: SharedLnbRuntime,
    frontend_id: i32,
) -> Vec<(i32, Result<(), HalError>)> {
    let owned_lnb_ids = match lock_shared_lnb_runtime(&runtime) {
        Ok(mut guard) => guard
            .lnb_mutation_context()
            .owned_lnb_ids_for_frontend(frontend_id),
        Err(error) => return vec![(frontend_id, Err(error))],
    };
    let mut outcomes = Vec::with_capacity(owned_lnb_ids.len());
    for lnb_id in owned_lnb_ids {
        let result = lnb_io_authority_for_runtime_id(&runtime, lnb_id).and_then(|authority| {
            close_lnb_runtime_with_authority(
                &runtime,
                lnb_id,
                authority,
                maleicacid_tuner_hal2_lnb::LnbLifecycleReason::OwnerLoss,
            )
        });
        outcomes.push((lnb_id, result));
    }
    if outcomes.iter().all(|(_, result)| result.is_ok()) {
        if let Err(error) = lock_shared_lnb_runtime(&runtime).and_then(|mut guard| {
            crate::frontend_ops::FrontendLnbRelationTxn::release(&mut guard, frontend_id)
        }) {
            return vec![(frontend_id, Err(error))];
        }
    }
    outcomes
}

#[cfg(test)]
mod wp_r11_lnb_apply_tests {
    use crate::boot::TunerServiceRuntime;
    use crate::registry::{
        FrontendCapabilitySnapshot, FrontendRegistryEntry, FrontendRuntimeId,
        FrontendScalarCapability, LnbRegistryEntry, LnbRegistryProfile, LnbRuntimeId,
        SatellitePowerTopology,
    };
    use maleicacid_tuner_hal2_common::{FrontendBackendKind, FrontendSystem, HalError};
    use maleicacid_tuner_hal2_domain_request::LnbVoltageRequest;
    use maleicacid_tuner_hal2_lnb::{LnbElectricalState, LnbRuntimeState};

    fn runtime_with_lnb(profile: LnbRegistryProfile) -> TunerServiceRuntime {
        let mut runtime = TunerServiceRuntime::new();
        runtime
            .registry_mut_for_test()
            .register_frontend(FrontendRegistryEntry {
                id: FrontendRuntimeId(1),
                backend: FrontendBackendKind::Px4CharDevice,
                system: FrontendSystem::IsdbS,
                device_path: "/dev/null".into(),
                lnb_profile: Some(profile),
                satellite_power_topology: match profile {
                    LnbRegistryProfile::Px4Device15VOnly | LnbRegistryProfile::EarthPt1FixedLnb => {
                        SatellitePowerTopology::InternalFixed15V
                    }
                    LnbRegistryProfile::NoPower => SatellitePowerTopology::ExternalOrShared,
                },
                capability: FrontendCapabilitySnapshot {
                    scalar: FrontendScalarCapability {
                        min_frequency_hz: 1_049_480_000,
                        max_frequency_hz: 2_053_000_000,
                        min_symbol_rate: 28_860_000,
                        max_symbol_rate: 28_860_000,
                        acquire_range_hz: 0,
                    },
                    exclusive_group_id: 0x1000_0001,
                    isdbt_segment: None,
                },
            })
            .unwrap();
        runtime
            .registry_mut_for_test()
            .register_lnb(LnbRegistryEntry {
                id: LnbRuntimeId(10001),
                name: Some("test-lnb".to_string()),
                owner_frontend_id: FrontendRuntimeId(1),
                profile,
            })
            .unwrap();
        runtime
    }

    #[test]
    fn diseqc_empty_payload_is_invalid_argument() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        let err = runtime.send_lnb_diseqc(10001, &[]).unwrap_err();
        assert!(matches!(err, HalError::InvalidArgument { .. }));
    }

    #[test]
    fn diseqc_valid_payload_is_profile_unsupported_not_success() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        let err = runtime
            .send_lnb_diseqc(10001, &[0xe0, 0x10, 0x5a])
            .unwrap_err();
        assert_eq!(
            err,
            HalError::Unsupported("DiSEqC is unavailable for this LNB profile")
        );
    }

    #[test]
    fn diseqc_oversized_payload_is_still_profile_unsupported() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        let payload = vec![0_u8; maleicacid_tuner_hal2_lnb::LnbDiseqcMessage::MAX_LEN + 1];
        let err = runtime.send_lnb_diseqc(10001, &payload).unwrap_err();
        assert_eq!(
            err,
            HalError::Unsupported("DiSEqC is unavailable for this LNB profile")
        );
    }

    #[test]
    fn px4_lnb_profile_rejects_11v_before_registry_commit() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        let err = runtime
            .apply_lnb_voltage(10001, LnbVoltageRequest::Voltage11V)
            .unwrap_err();
        assert_eq!(
            err,
            HalError::Unsupported("LNB voltage is unavailable for this fixed profile")
        );
        let lnb = runtime.registry().lnb_runtime(LnbRuntimeId(10001)).unwrap();
        assert_eq!(lnb.registry_state(), LnbElectricalState::safe());
    }

    #[test]
    fn selected_lnb_backend_failure_keeps_registry_state() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        runtime
            .registry_mut_for_test()
            .bind_lnb_to_frontend(FrontendRuntimeId(1), LnbRuntimeId(10001))
            .unwrap();

        let err = runtime
            .apply_lnb_voltage(10001, LnbVoltageRequest::Voltage15V)
            .unwrap_err();

        assert!(matches!(err, HalError::Internal { .. }));
        let lnb = runtime.registry().lnb_runtime(LnbRuntimeId(10001)).unwrap();
        assert_eq!(lnb.registry_state(), LnbElectricalState::safe());
        assert_eq!(lnb.state(), LnbRuntimeState::Quarantined);
    }

    #[test]
    fn set_frontend_lnb_backend_failure_does_not_commit_binding() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);

        let err = runtime.set_frontend_lnb(1, 10001).unwrap_err();

        assert!(matches!(err, HalError::Internal { .. }));
        assert_eq!(
            runtime
                .registry()
                .selected_lnb_for_frontend(FrontendRuntimeId(1)),
            None
        );
        let lnb = runtime.registry().lnb_runtime(LnbRuntimeId(10001)).unwrap();
        assert_eq!(lnb.registry_state(), LnbElectricalState::safe());
        assert_eq!(lnb.state(), LnbRuntimeState::Quarantined);
    }

    #[test]
    fn frontend_lnb_relation_commits_and_releases_with_assignment_lease() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::NoPower);

        runtime.set_frontend_lnb(1, 10001).unwrap();
        runtime.set_frontend_lnb(1, 10001).unwrap();

        assert_eq!(
            runtime
                .registry()
                .selected_lnb_for_frontend(FrontendRuntimeId(1)),
            Some(LnbRuntimeId(10001))
        );
        assert_eq!(
            runtime
                .registry()
                .lnb_registry()
                .rail_reference_count(LnbRuntimeId(10001)),
            Some(1)
        );
        crate::frontend_ops::FrontendLnbRelationTxn::release(&mut runtime, 1).unwrap();
        assert_eq!(
            runtime
                .registry()
                .selected_lnb_for_frontend(FrontendRuntimeId(1)),
            None
        );
        assert_eq!(
            runtime
                .registry()
                .lnb_registry()
                .rail_reference_count(LnbRuntimeId(10001)),
            Some(0)
        );
    }

    #[test]
    fn fixed_power_lease_is_idempotent_and_blocks_incompatible_voltage() {
        let mut runtime = runtime_with_lnb(LnbRegistryProfile::Px4Device15VOnly);
        let frontend_id = FrontendRuntimeId(1);
        let lnb_id = LnbRuntimeId(10001);

        assert!(runtime
            .registry_mut_for_test()
            .retain_frontend_fixed_power_lease(frontend_id, lnb_id)
            .unwrap());
        assert!(!runtime
            .registry_mut_for_test()
            .retain_frontend_fixed_power_lease(frontend_id, lnb_id)
            .unwrap());
        assert_eq!(
            runtime
                .registry()
                .lnb_registry()
                .rail_reference_count(lnb_id),
            Some(1)
        );
        assert!(runtime
            .registry_mut_for_test()
            .prepare_lnb_state_apply(lnb_id, LnbElectricalState::safe())
            .is_err());

        assert_eq!(
            runtime
                .registry_mut_for_test()
                .release_frontend_fixed_power_lease(frontend_id)
                .unwrap(),
            Some((lnb_id, 0))
        );
    }
}

impl TunerServiceRuntime {
    pub fn commit_lnb_callback_registration_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;

        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        self.commit_lnb_callback_registration(lnb_id)
    }

    pub fn clear_lnb_callback_registration_for_object(
        &mut self,
        object_id: maleicacid_tuner_hal2_domain_request::AidlObjectId,
        generation: maleicacid_tuner_hal2_domain_request::AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;

        let lnb_id = self.public_runtime_id_for_object_method(
            object_id,
            generation,
            maleicacid_tuner_hal2_domain_request::AidlObjectKind::Lnb,
        )?;
        self.clear_lnb_callback_registration(lnb_id)
    }
}
