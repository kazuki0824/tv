use maleicacid_tuner_hal2_common::{
    compose_primary_cleanup_failure, HalError, HalInternalKind, HalInvalidArgumentKind,
    HalInvalidStateKind,
};
use maleicacid_tuner_hal2_domain_request::{
    LnbSetSatellitePositionRequest, LnbToneRequest, LnbVoltageRequest,
};
use maleicacid_tuner_hal2_lnb::{
    LnbBackendApplyOutcome, LnbBackendOps, LnbDiseqcMessage, LnbElectricalState,
    LnbFailureKind, LnbFailureRecord, LnbFailureStep, LnbLifecycleReason, LnbRuntime,
    LnbRuntimeState, LnbTone as RuntimeLnbTone, LnbVoltage as RuntimeLnbVoltage,
    PreparedLnbClose, PreparedLnbStateApply,
};

use super::TunerServiceRuntime;
use crate::lnb_backend_adapter::{
    ServiceRuntimeLnbBackendSnapshot, ServiceRuntimeLnbProfileAdapter,
};
use crate::registry::{
    FrontendRuntimeId, LnbPhysicalIoPermit, LnbRegistryProfile, LnbRuntimeId,
    PreparedLnbAssignmentLease,
};

pub(crate) enum PreparedFrontendLnbAssignment {
    Unchanged,
    Apply {
        prepared_lease: PreparedLnbAssignmentLease,
        runtime_apply: PreparedLnbStateApply,
        backend: ServiceRuntimeLnbBackendSnapshot,
    },
}

pub(crate) enum ExecutedFrontendLnbAssignment {
    Unchanged,
    Apply {
        prepared_lease: PreparedLnbAssignmentLease,
        runtime_apply: PreparedLnbStateApply,
        backend_result: LnbBackendApplyOutcome,
    },
}

impl PreparedFrontendLnbAssignment {
    pub(crate) fn execute(
        self,
        permit: &LnbPhysicalIoPermit<'_>,
    ) -> ExecutedFrontendLnbAssignment {
        match self {
            Self::Unchanged => ExecutedFrontendLnbAssignment::Unchanged,
            Self::Apply {
                prepared_lease,
                runtime_apply,
                backend,
            } => {
                let mut backend = ServiceRuntimeLnbProfileAdapter::new(backend, permit);
                let backend_result = backend.apply_lnb_state(
                    runtime_apply.lnb_id(),
                    runtime_apply.target_state(),
                );
                ExecutedFrontendLnbAssignment::Apply {
                    prepared_lease,
                    runtime_apply,
                    backend_result,
                }
            }
        }
    }
}

pub(crate) struct PreparedLnbDiseqc {
    lnb_key: LnbRuntimeId,
    expected_generation: u64,
    lnb_id: i32,
    message: LnbDiseqcMessage,
    backend: ServiceRuntimeLnbBackendSnapshot,
}

pub(crate) struct ExecutedLnbDiseqc {
    lnb_key: LnbRuntimeId,
    expected_generation: u64,
    outcome: LnbBackendApplyOutcome,
}

impl PreparedLnbDiseqc {
    pub(crate) fn execute(self, permit: &LnbPhysicalIoPermit<'_>) -> ExecutedLnbDiseqc {
        let mut backend = ServiceRuntimeLnbProfileAdapter::new(self.backend, permit);
        let outcome = backend.send_diseqc_message(self.lnb_id, &self.message);
        ExecutedLnbDiseqc {
            lnb_key: self.lnb_key,
            expected_generation: self.expected_generation,
            outcome,
        }
    }
}

pub(crate) struct PreparedLnbLifecycleClose {
    lnb_key: LnbRuntimeId,
    runtime_close: PreparedLnbClose,
    backend: ServiceRuntimeLnbBackendSnapshot,
}

pub(crate) struct ExecutedLnbLifecycleClose {
    lnb_key: LnbRuntimeId,
    runtime_close: PreparedLnbClose,
    backend_result: LnbBackendApplyOutcome,
}

impl PreparedLnbLifecycleClose {
    pub(crate) fn execute(
        self,
        permit: &LnbPhysicalIoPermit<'_>,
    ) -> ExecutedLnbLifecycleClose {
        let backend_result = if self.runtime_close.requires_backend_io() {
            let mut backend = ServiceRuntimeLnbProfileAdapter::new(self.backend, permit);
            backend.apply_lnb_state(self.runtime_close.lnb_id(), LnbElectricalState::safe())
        } else {
            LnbBackendApplyOutcome::Applied
        };
        ExecutedLnbLifecycleClose {
            lnb_key: self.lnb_key,
            runtime_close: self.runtime_close,
            backend_result,
        }
    }
}

/// LNB registryのmutation primitiveへアクセスするcall-local context。
///
/// relationとcontrolの正規authorityは`FrontendLnbRelationTxn`と
/// `LnbControlTxn`が保持する。このcontextは状態も呼出しを跨ぐtransaction境界も
/// 所有しない。
pub(crate) struct LnbMutationContext<'a> {
    runtime: &'a mut TunerServiceRuntime,
}

impl TunerServiceRuntime {
    pub(crate) fn lnb_mutation_context(&mut self) -> LnbMutationContext<'_> {
        LnbMutationContext { runtime: self }
    }
}

impl<'a> LnbMutationContext<'a> {
    pub(crate) fn prepare_frontend_lnb_assignment(
        &mut self,
        frontend_id: i32,
        lnb_id: i32,
    ) -> Result<PreparedFrontendLnbAssignment, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().frontend(frontend_key).is_none() {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "frontend id is missing for LNB binding",
            ));
        }
        let Some(entry) = self.runtime.registry().lnb(lnb_key) else {
            return Err(missing_lnb_error());
        };
        if entry.owner_frontend_id != frontend_key {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "LNB does not belong to this frontend",
            ));
        }
        let target = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .map(|runtime| runtime.registry_state())
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(
            self.runtime
                .registry()
                .lnb_runtime(lnb_key)
                .ok_or_else(missing_lnb_error)?,
        )?;
        let Some(prepared_lease) = self
            .runtime
            .registry_mut()
            .prepare_lnb_assignment_lease(frontend_key, lnb_key)?
        else {
            return Ok(PreparedFrontendLnbAssignment::Unchanged);
        };
        let backend = match ServiceRuntimeLnbBackendSnapshot::new_with_pending_frontend(
            self.runtime.registry(),
            lnb_key,
            frontend_key,
        ) {
            Ok(backend) => backend,
            Err(error) => {
                if self
                    .runtime
                    .registry_mut()
                    .abort_prepared_lnb_assignment_lease(prepared_lease)
                {
                    return Err(map_lnb_failure(LnbFailureRecord {
                        lnb_id,
                        kind: error,
                        step: LnbFailureStep::ApplyBackend,
                    }));
                }
                return Err(compose_primary_cleanup_failure(
                    "LNB assignment backend prepare failed and prepared lease abort failed",
                    map_lnb_failure(LnbFailureRecord {
                        lnb_id,
                        kind: error,
                        step: LnbFailureStep::ApplyBackend,
                    }),
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "prepared LNB assignment lease disappeared before abort",
                    ),
                ));
            }
        };
        let runtime_apply = match self
            .runtime
            .registry_mut()
            .prepare_lnb_state_apply(lnb_key, target)
            .map_err(map_lnb_failure)
        {
            Ok(runtime_apply) => runtime_apply,
            Err(error) => {
                if self
                    .runtime
                    .registry_mut()
                    .abort_prepared_lnb_assignment_lease(prepared_lease)
                {
                    return Err(error);
                }
                return Err(compose_primary_cleanup_failure(
                    "LNB assignment runtime prepare failed and prepared lease abort failed",
                    error,
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "prepared LNB assignment lease disappeared before runtime prepare abort",
                    ),
                ));
            }
        };
        Ok(PreparedFrontendLnbAssignment::Apply {
            prepared_lease,
            runtime_apply,
            backend,
        })
    }

    pub(crate) fn commit_frontend_lnb_assignment(
        &mut self,
        executed: ExecutedFrontendLnbAssignment,
    ) -> Result<(), HalError> {
        let ExecutedFrontendLnbAssignment::Apply {
            prepared_lease,
            runtime_apply,
            backend_result,
        } = executed
        else {
            return Ok(());
        };
        let lnb_key = LnbRuntimeId(runtime_apply.lnb_id());
        let apply_result = self
            .runtime
            .registry_mut()
            .finish_lnb_state_apply(lnb_key, runtime_apply, backend_result)
            .map(|_| ())
            .map_err(map_lnb_failure);
        if let Err(error) = apply_result {
            if self
                .runtime
                .registry_mut()
                .abort_prepared_lnb_assignment_lease(prepared_lease)
            {
                return Err(error);
            }
            return Err(compose_primary_cleanup_failure(
                "LNB assignment backend apply failed and prepared lease abort failed",
                error,
                HalError::internal(
                    HalInternalKind::InvariantViolation,
                    "prepared LNB assignment lease disappeared after backend apply failure",
                ),
            ));
        }
        let cleanup = match self
            .runtime
            .registry_mut()
            .commit_prepared_lnb_assignment(prepared_lease)
        {
            Ok(cleanup) => cleanup,
            Err(error) => {
                if self
                    .runtime
                    .registry_mut()
                    .abort_prepared_lnb_assignment_lease(prepared_lease)
                {
                    return Err(error);
                }
                return Err(compose_primary_cleanup_failure(
                    "LNB assignment composite commit failed and prepared lease abort failed",
                    error,
                    HalError::internal(
                        HalInternalKind::InvariantViolation,
                        "prepared LNB assignment lease disappeared after commit failure",
                    ),
                ));
            }
        };
        if let Some(cleanup) = cleanup {
            self.runtime
                .registry_mut()
                .complete_lnb_assignment_cleanup(cleanup)?;
        }
        Ok(())
    }

    pub(crate) fn prepare_lnb_diseqc(
        &mut self,
        lnb_id: i32,
        payload: &[u8],
    ) -> Result<PreparedLnbDiseqc, HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let runtime = self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .ok_or_else(missing_lnb_error)?;
        ensure_lnb_open(runtime)?;
        if payload.is_empty() {
            return Err(HalError::invalid_argument(
                HalInvalidArgumentKind::NumericRange,
                "DiSEqC message must not be empty",
            ));
        }
        let backend = ServiceRuntimeLnbBackendSnapshot::new(self.runtime.registry(), lnb_key)
            .map_err(|kind| {
                map_lnb_failure(LnbFailureRecord {
                    lnb_id,
                    kind,
                    step: LnbFailureStep::SendDiseqc,
                })
            })?;
        if !backend.supports_diseqc() {
            return Err(HalError::Unsupported(
                "DiSEqC is unavailable for this LNB profile",
            ));
        }
        let message = LnbDiseqcMessage::new(lnb_id, payload).map_err(map_lnb_failure)?;
        Ok(PreparedLnbDiseqc {
            lnb_key,
            expected_generation: runtime.generation(),
            lnb_id,
            message,
            backend,
        })
    }

    pub(crate) fn finish_lnb_diseqc(
        &mut self,
        executed: ExecutedLnbDiseqc,
    ) -> Result<(), HalError> {
        self.runtime
            .registry_mut()
            .finish_lnb_diseqc(
                executed.lnb_key,
                executed.expected_generation,
                executed.outcome,
            )
            .map_err(map_lnb_failure)
    }

    pub(crate) fn open_lnb_for_public_id(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        self.runtime
            .registry_mut()
            .reopen_lnb(lnb_key)
            .map_err(map_lnb_failure)
    }

    pub(crate) fn commit_lnb_callback_registration(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        self.runtime
            .registry_mut()
            .set_lnb_callback_registered(lnb_key, true)
            .map_err(map_lnb_failure)
    }

    pub(crate) fn clear_lnb_callback_registration(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        self.runtime
            .registry_mut()
            .set_lnb_callback_registered(lnb_key, false)
            .map_err(map_lnb_failure)
    }

    pub(crate) fn owned_lnb_ids_for_frontend(
        &mut self,
        frontend_id: i32,
    ) -> Vec<i32> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        self
            .runtime
            .registry()
            .lnb_ids()
            .into_iter()
            .filter(|lnb_id| {
                self.runtime
                    .registry()
                    .lnb(*lnb_id)
                    .map(|entry| entry.owner_frontend_id == frontend_key)
                    .unwrap_or(false)
            })
            .map(|lnb_id| lnb_id.0)
            .collect()
    }

    pub(crate) fn prepare_lnb_lifecycle_close(
        &mut self,
        lnb_id: i32,
        _reason: LnbLifecycleReason,
    ) -> Result<PreparedLnbLifecycleClose, HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let backend = ServiceRuntimeLnbBackendSnapshot::new(self.runtime.registry(), lnb_key)
            .map_err(|kind| {
                map_lnb_failure(LnbFailureRecord {
                    lnb_id,
                    kind,
                    step: LnbFailureStep::ApplyBackend,
                })
            })?;
        let runtime_close = self
            .runtime
            .registry_mut()
            .prepare_lnb_close(lnb_key)
            .map_err(map_lnb_failure)?;
        Ok(PreparedLnbLifecycleClose {
            lnb_key,
            runtime_close,
            backend,
        })
    }

    pub(crate) fn finish_lnb_lifecycle_close(
        &mut self,
        executed: ExecutedLnbLifecycleClose,
    ) -> Result<(), HalError> {
        self.runtime
            .registry_mut()
            .finish_lnb_close(
                executed.lnb_key,
                executed.runtime_close,
                executed.backend_result,
            )
            .map_err(map_lnb_failure)
    }

    pub(crate) fn record_lnb_drop_leak(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.runtime.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        if self
            .runtime
            .registry()
            .lnb_runtime(lnb_key)
            .is_some_and(|runtime| runtime.state() == LnbRuntimeState::Closed)
        {
            return Ok(());
        }
        self.runtime
            .registry_mut()
            .record_lnb_drop_leak(lnb_key)
            .map_err(map_lnb_failure)
    }
}

pub(crate) fn missing_lnb_error() -> HalError {
    HalError::invalid_argument(
        HalInvalidArgumentKind::NumericRange,
        "LNB runtime id is missing",
    )
}

fn lnb_state_error() -> HalError {
    HalError::invalid_state(
        HalInvalidStateKind::InvalidLifecycle,
        "LNB runtime is not open",
    )
}

pub(crate) fn ensure_lnb_open(runtime: &LnbRuntime) -> Result<(), HalError> {
    if runtime.state() == LnbRuntimeState::Open {
        Ok(())
    } else {
        Err(lnb_state_error())
    }
}

pub(crate) fn map_lnb_failure(record: LnbFailureRecord) -> HalError {
    match record.kind {
        LnbFailureKind::InvalidState => lnb_state_error(),
        LnbFailureKind::GenerationOverflow => HalError::internal(
            HalInternalKind::InvariantViolation,
            "LNB generation overflow",
        ),
        LnbFailureKind::DiseqcInvalidMessage => HalError::invalid_argument(
            HalInvalidArgumentKind::NumericRange,
            "DiSEqC message length is invalid",
        ),
        LnbFailureKind::DiseqcUnsupported => {
            HalError::Unsupported("DiSEqC is unavailable for this LNB profile")
        }
        LnbFailureKind::BackendApplyFailed | LnbFailureKind::DropWithoutClose => HalError::internal(
            HalInternalKind::InvariantViolation,
            "LNB transaction failed",
        ),
    }
}

fn runtime_voltage(request: LnbVoltageRequest) -> RuntimeLnbVoltage {
    match request {
        LnbVoltageRequest::None => RuntimeLnbVoltage::None,
        LnbVoltageRequest::Voltage11V => RuntimeLnbVoltage::Voltage11V,
        LnbVoltageRequest::Voltage15V => RuntimeLnbVoltage::Voltage15V,
    }
}

pub(crate) fn validate_voltage_for_profile(
    profile: LnbRegistryProfile,
    request: LnbVoltageRequest,
) -> Result<RuntimeLnbVoltage, HalError> {
    match (profile, request) {
        (_, LnbVoltageRequest::None)
        | (LnbRegistryProfile::EarthPt1FixedLnb, LnbVoltageRequest::Voltage11V)
        | (LnbRegistryProfile::EarthPt1FixedLnb, LnbVoltageRequest::Voltage15V)
        | (LnbRegistryProfile::Px4Device15VOnly, LnbVoltageRequest::Voltage15V) => {
            Ok(runtime_voltage(request))
        }
        (LnbRegistryProfile::Px4Device15VOnly, LnbVoltageRequest::Voltage11V)
        | (LnbRegistryProfile::NoPower, LnbVoltageRequest::Voltage11V)
        | (LnbRegistryProfile::NoPower, LnbVoltageRequest::Voltage15V) => Err(
            HalError::Unsupported("LNB voltage is unavailable for this fixed profile"),
        ),
    }
}

pub(crate) fn validate_tone_for_profile(
    request: LnbToneRequest,
) -> Result<RuntimeLnbTone, HalError> {
    match request {
        LnbToneRequest::None => Ok(RuntimeLnbTone::Off),
        LnbToneRequest::Continuous => Err(HalError::Unsupported(
            "LNB continuous tone is unavailable for this fixed profile",
        )),
    }
}

pub(crate) fn validate_position_for_profile(
    request: LnbSetSatellitePositionRequest,
) -> Result<Option<i32>, HalError> {
    if request.position == 0 {
        Ok(None)
    } else {
        Err(HalError::Unsupported(
            "LNB satellite position is unavailable for this fixed profile",
        ))
    }
}
