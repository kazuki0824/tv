use maleicacid_tuner_hal2_common::{
    HalError, HalInternalKind, HalInvalidArgumentKind, HalInvalidStateKind,
};
use maleicacid_tuner_hal2_lnb::{
    LnbFailureKind, LnbFailureRecord, LnbLifecycleReason, LnbLifecycleTxn, LnbRuntime,
    LnbRuntimeState,
};

use crate::boot::TunerServiceRuntime;
use crate::lnb_backend_adapter::{store_lnb_runtime, ServiceRuntimeLnbBackend};
use crate::registry::{FrontendRuntimeId, LnbRuntimeId};

fn missing_lnb_error() -> HalError {
    HalError::invalid_argument(HalInvalidArgumentKind::NumericRange, "LNB runtime id is missing")
}

fn lnb_state_error() -> HalError {
    HalError::invalid_state(
        HalInvalidStateKind::InvalidLifecycle,
        "LNB runtime is not open",
    )
}

fn ensure_lnb_open(runtime: &LnbRuntime) -> Result<(), HalError> {
    if runtime.state() == LnbRuntimeState::Open {
        Ok(())
    } else {
        Err(lnb_state_error())
    }
}

fn map_lnb_failure(record: LnbFailureRecord) -> HalError {
    match record.kind {
        LnbFailureKind::InvalidState => lnb_state_error(),
        LnbFailureKind::BackendApplyFailed
        | LnbFailureKind::RegistryCommitFailed
        | LnbFailureKind::CallbackClearFailed
        | LnbFailureKind::OperationAlreadyActive
        | LnbFailureKind::OperationLockFailed
        | LnbFailureKind::GenerationOverflow
        | LnbFailureKind::DropWithoutClose => HalError::internal(
            HalInternalKind::InvariantViolation,
            "LNB lifecycle transaction failed",
        ),
    }
}

impl TunerServiceRuntime {
    pub fn open_lnb_for_public_id(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let mut runtime = self
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        runtime.reopen_after_public_open().map_err(map_lnb_failure)?;
        store_lnb_runtime(self, lnb_key, runtime)
    }

    pub fn mark_lnb_callback_registered(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let Some(runtime) = self.registry_mut().lnb_runtime_mut(lnb_key) else {
            return Err(missing_lnb_error());
        };
        ensure_lnb_open(runtime)?;
        runtime.set_callback_registered(true);
        Ok(())
    }

    pub fn close_lnb_explicit(&mut self, lnb_id: i32) -> Result<(), HalError> {
        self.close_lnb_with_reason(LnbRuntimeId(lnb_id), LnbLifecycleReason::PublicClose)
    }

    pub fn close_lnb_from_frontend_owner_loss(
        &mut self,
        frontend_id: i32,
    ) -> Result<Vec<i32>, HalError> {
        let frontend_key = FrontendRuntimeId(frontend_id);
        let owned_lnb_ids: Vec<LnbRuntimeId> = self
            .registry()
            .lnb_ids()
            .into_iter()
            .filter(|lnb_id| {
                self.registry()
                    .lnb(*lnb_id)
                    .map(|entry| entry.owner_frontend_id == frontend_key)
                    .unwrap_or(false)
            })
            .collect();
        let mut closed = Vec::with_capacity(owned_lnb_ids.len());
        for lnb_key in owned_lnb_ids {
            self.close_lnb_with_reason(lnb_key, LnbLifecycleReason::OwnerLoss)?;
            closed.push(lnb_key.0);
        }
        Ok(closed)
    }

    pub fn record_lnb_drop_leak(&mut self, lnb_id: i32) -> Result<(), HalError> {
        let lnb_key = LnbRuntimeId(lnb_id);
        if self.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let mut runtime = self
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        if runtime.state() == LnbRuntimeState::Closed {
            return Ok(());
        }
        let outcome = {
            let mut backend = ServiceRuntimeLnbBackend::new(self.registry(), lnb_key);
            LnbLifecycleTxn::new().close(&mut runtime, &mut backend, LnbLifecycleReason::DropLeak)
        };
        store_lnb_runtime(self, lnb_key, runtime)?;
        match outcome.result {
            Ok(()) => Ok(()),
            Err(record) if record.kind == LnbFailureKind::DropWithoutClose => Ok(()),
            Err(record) => Err(map_lnb_failure(record)),
        }
    }

    fn close_lnb_with_reason(
        &mut self,
        lnb_key: LnbRuntimeId,
        reason: LnbLifecycleReason,
    ) -> Result<(), HalError> {
        if self.registry().lnb(lnb_key).is_none() {
            return Err(missing_lnb_error());
        }
        let mut runtime = self
            .registry()
            .lnb_runtime(lnb_key)
            .cloned()
            .ok_or_else(missing_lnb_error)?;
        let outcome = {
            let mut backend = ServiceRuntimeLnbBackend::new(self.registry(), lnb_key);
            LnbLifecycleTxn::new().close(&mut runtime, &mut backend, reason)
        };
        store_lnb_runtime(self, lnb_key, runtime)?;
        outcome.result.map_err(map_lnb_failure)
    }
}
