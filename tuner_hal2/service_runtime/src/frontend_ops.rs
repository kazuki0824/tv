use crate::boot::TunerServiceRuntime;
use crate::object_method_txn::ObjectMethodExecutionToken;
use maleicacid_tuner_hal2_common::HalError;
use maleicacid_tuner_hal2_domain_request::{AidlObjectGeneration, AidlObjectId, AidlObjectKind};

pub type SharedFrontendRuntime = std::sync::Arc<std::sync::Mutex<TunerServiceRuntime>>;

pub fn set_frontend_lnb_object_use_case(
    runtime: SharedFrontendRuntime,
    object_id: AidlObjectId,
    object_generation: AidlObjectGeneration,
    lnb_id: i32,
    dispatch: ObjectMethodExecutionToken,
) -> Result<(), HalError> {
    let mut guard = runtime.lock().map_err(|_| {
        HalError::internal(
            maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
            "service runtime lock poisoned",
        )
    })?;
    dispatch.consume_for_object(
        &mut guard,
        object_id,
        object_generation,
        AidlObjectKind::Frontend,
    )?;
    let frontend_entry = guard.frontend_entry_for_aidl_object(object_id, object_generation)?;
    let frontend_id = frontend_entry.id.0;
    let exported_lnb_id = guard
        .lnb_for_frontend_id(frontend_id)
        .ok_or(HalError::Unsupported("frontend has no exported LNB"))?
        .id
        .0;
    if exported_lnb_id != lnb_id {
        return Err(HalError::invalid_argument(
            maleicacid_tuner_hal2_common::HalInvalidArgumentKind::NumericRange,
            "LNB does not belong to this frontend",
        ));
    }
    guard.set_frontend_lnb(frontend_id, lnb_id)
}

impl TunerServiceRuntime {
    pub fn commit_frontend_callback_registration_for_object(
        &mut self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        self.public_runtime_id_for_object_method(
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        Ok(())
    }

    pub fn clear_frontend_callback_registration_for_object(
        &mut self,
        object_id: AidlObjectId,
        object_generation: AidlObjectGeneration,
        dispatch: ObjectMethodExecutionToken,
    ) -> Result<(), HalError> {
        dispatch.consume_for_object(
            self,
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        self.public_runtime_id_for_object_method(
            object_id,
            object_generation,
            AidlObjectKind::Frontend,
        )?;
        Ok(())
    }
}
