use std::sync::{Arc, Mutex};

use android_hardware_tv_tuner::aidl::android::hardware::tv::tuner::Result::Result as TunerResult;
use binder::{Result as BinderResult, Status};
use maleicacid_tuner_hal2_binder_adapter::{
    demux::DemuxCommand, dvr::DvrCommand, filter::FilterCommand, frontend::FrontendCommand,
    lnb::LnbCommand,
};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlDomainRequest, AidlFailureSource, AidlMethodAdapter, AidlMethodCall,
    AidlMethodPlan, AidlObjectKind, AidlStatusMapper, DomainCommand, DomainProfileSupport, StatusPrecedenceStep,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidStateKind};
use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerId};
use maleicacid_tuner_hal2_service_runtime::{RuntimeObjectTableError, TunerServiceRuntime};

use crate::callback_store::clear_owner_callbacks;
use crate::object_handle::AidlObjectHandle;

pub type SharedTunerRuntime = Arc<Mutex<TunerServiceRuntime>>;

#[derive(Clone, Debug)]
pub struct AidlMethodExecutionOutcome {
    pub plan: AidlMethodPlan,
    pub precedence: Vec<StatusPrecedenceStep>,
}

impl AidlMethodExecutionOutcome {
    pub fn plan(&self) -> &AidlMethodPlan {
        &self.plan
    }
    pub fn precedence(&self) -> &[StatusPrecedenceStep] {
        &self.precedence
    }
}

pub fn execute_object_aidl_method(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
) -> BinderResult<AidlMethodExecutionOutcome> {
    let method_plan = AidlMethodAdapter::plan(method);
    let mut failures = Vec::<AidlFailureSource>::new();

    let mut profile_unsupported_precedence = false;
    for request in command_domain_requests(&method_plan.command) {
        let executable_request = (*request).clone();
        match executable_request.profile_support() {
            DomainProfileSupport::Supported => {}
            DomainProfileSupport::UnsupportedRecordThenUnavailable => {
                profile_unsupported_precedence = true;
                failures.push(AidlFailureSource::ProfileUnsupported(
                    HalError::Unsupported(
                        "AIDL input variant is outside the TS-only tuner_hal2 profile",
                    ),
                ));
            }
        }
        if let Err(error) = executable_request.validate_supported_values() {
            failures.push(AidlFailureSource::InputValidation(error));
        }
    }

    let precedence = if profile_unsupported_precedence {
        AidlStatusMapper::unsupported_precedence_for_profile_api(method_plan.api)
            .steps
            .to_vec()
    } else {
        AidlStatusMapper::precedence_for_api(method_plan.api)
            .steps
            .to_vec()
    };

    if method_plan.command_plan.object != handle.object_kind() {
        failures.push(AidlFailureSource::ObjectLifetime(HalError::invalid_state(
            HalInvalidStateKind::InvalidLifecycle,
            "AIDL method/object kind mismatch",
        )));
    }

    for step in &precedence {
        match step {
            StatusPrecedenceStep::ObjectLifetime => {
                if failures
                    .iter()
                    .any(|failure| failure.step() == StatusPrecedenceStep::ObjectLifetime)
                {
                    continue;
                }
                match runtime.lock() {
                    Ok(runtime) => {
                        if let Err(error) = runtime.object_table().entry_for_kind(
                            handle.object_id(),
                            handle.generation(),
                            handle.object_kind(),
                        ) {
                            failures.push(AidlFailureSource::ObjectLifetime(
                                object_table_hal_error(error),
                            ));
                        }
                    }
                    Err(_) => failures.push(AidlFailureSource::RuntimeFailure(HalError::internal(
                        maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned",
                    ))),
                }
            }
            StatusPrecedenceStep::ProfileUnsupported | StatusPrecedenceStep::InputValidation => {
                // 共通executor slotをここで提供する。具体的validatorは後続stepで失敗をここへ渡す。
            }
            StatusPrecedenceStep::RuntimeDispatch => {
                if !failures.is_empty() {
                    continue;
                }
                match runtime.lock() {
                    Ok(mut runtime) => {
                        if let Err(err) = runtime.plan_command_dispatch(
                            method_plan.command_plan,
                            method_plan.command.runtime_executable_request(),
                        ) {
                            failures.push(AidlFailureSource::RuntimeDispatch(err.into_hal_error()));
                        }
                    }
                    Err(_) => failures.push(AidlFailureSource::RuntimeFailure(HalError::internal(
                        maleicacid_tuner_hal2_common::HalInternalKind::InvariantViolation,
                        "service runtime lock poisoned",
                    ))),
                }
            }
            StatusPrecedenceStep::RuntimeFailure | StatusPrecedenceStep::RollbackFailure => {
                // 後続runtime stepは具体的なexecution/rollback失敗をここへ渡す。
            }
        }
    }

    if let Some(failure) = AidlStatusMapper::resolve_failure_source_by_precedence(
        method_plan.api,
        &failures,
        profile_unsupported_precedence,
    ) {
        return Err(status_from_hal_error(failure.error()));
    }

    Ok(AidlMethodExecutionOutcome {
        plan: method_plan,
        precedence,
    })
}

fn command_domain_requests(command: &DomainCommand) -> Vec<&AidlDomainRequest> {
    match command {
        DomainCommand::Frontend(FrontendCommand::SetCallback(request)) => vec![request],
        DomainCommand::Demux(
            DemuxCommand::SetFrontendDataSource(request)
            | DemuxCommand::OpenFilter(request)
            | DemuxCommand::OpenDvr(request),
        ) => vec![request],
        DomainCommand::Filter(
            FilterCommand::Configure(request)
            | FilterCommand::ConfigureAvStreamType(request)
            | FilterCommand::ReleaseAvHandle(request)
            | FilterCommand::SetDataSource(request)
            | FilterCommand::SetDelayHint(request),
        ) => vec![request],
        DomainCommand::Dvr(
            DvrCommand::Configure(request)
            | DvrCommand::AttachFilter(request)
            | DvrCommand::DetachFilter(request),
        ) => vec![request],
        DomainCommand::Lnb(
            LnbCommand::SetCallback(request)
            | LnbCommand::SetVoltage(request)
            | LnbCommand::SetTone(request)
            | LnbCommand::SetSatellitePosition(request),
        ) => vec![request],
        _ => Vec::new(),
    }
}

pub fn ensure_object_live(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    let runtime = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    runtime
        .object_table()
        .entry_for_kind(
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
        )
        .map(|_| ())
        .map_err(status_from_object_table_error)
}

pub fn clear_live_lnb_callback_for_public_id(
    runtime: &SharedTunerRuntime,
    lnb_id: i32,
) -> BinderResult<()> {
    let handle = {
        let mut runtime = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
        let Some(entry) = runtime.object_table().live_entry_for_runtime(
            AidlObjectKind::Lnb,
            LedgerId(i64::from(lnb_id)),
        ) else {
            return Ok(());
        };
        runtime
            .callback_registry_mut()
            .clear_owner(entry.object_id, entry.generation);
        AidlObjectHandle::new(entry.object_kind, entry.object_id, entry.generation)
    };
    clear_owner_callbacks(handle).map_err(|_| {
        status_unknown_error("callback store cleanup failed during LNB owner loss")
    })
}

pub fn close_object(runtime: &SharedTunerRuntime, handle: AidlObjectHandle) -> BinderResult<()> {
    let mut runtime = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    runtime
        .object_table_mut()
        .begin_close_cascade(
            handle.object_id(),
            handle.generation(),
            CleanupStep::StopWorker,
        )
        .map_err(status_from_object_table_error)?;
    runtime
        .callback_registry_mut()
        .clear_owner(handle.object_id(), handle.generation());
    if clear_owner_callbacks(handle).is_err() {
        runtime
            .object_table_mut()
            .mark_cleanup_failed_cascade(
                handle.object_id(),
                handle.generation(),
                CleanupStep::UnregisterRuntime,
            )
            .map_err(status_from_object_table_error)?;
        return Err(status_unknown_error(
            "callback store cleanup failed during AIDL object close",
        ));
    }
    let closed_entries = runtime
        .object_table_mut()
        .commit_close_cascade(handle.object_id(), handle.generation())
        .map_err(status_from_object_table_error)?;
    for entry in &closed_entries {
        runtime.unregister_public_runtime_for_closed_aidl_entry(entry);
    }
    Ok(())
}

pub fn record_callback_registration(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    api: AidlApi,
) -> BinderResult<()> {
    let mut runtime = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    runtime
        .object_table()
        .entry_for_kind(
            handle.object_id(),
            handle.generation(),
            handle.object_kind(),
        )
        .map_err(status_from_object_table_error)?;
    runtime.callback_registry_mut().record_registration(
        handle.object_kind(),
        handle.object_id(),
        handle.generation(),
        api,
    );
    Ok(())
}

pub fn plan_object_aidl_method(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
) -> BinderResult<AidlMethodPlan> {
    execute_object_aidl_method(runtime, handle, method).map(|outcome| outcome.plan)
}

pub fn close_object_after_aidl_method_plan(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    method: AidlMethodCall,
) -> BinderResult<()> {
    plan_object_aidl_method(runtime, handle, method)?;
    close_object(runtime, handle)
}

fn object_table_hal_error(error: RuntimeObjectTableError) -> HalError {
    let message = match error {
        RuntimeObjectTableError::MissingObject { .. } => "AIDL object is closed or missing",
        RuntimeObjectTableError::ObjectKindMismatch { .. } => "AIDL object kind mismatch",
        RuntimeObjectTableError::GenerationMismatch { .. } => "AIDL object generation mismatch",
        RuntimeObjectTableError::InvalidOwner { .. } => "AIDL object owner mismatch",
        RuntimeObjectTableError::MissingOwner { .. } => "AIDL object owner is missing",
        RuntimeObjectTableError::OwnerGenerationMismatch { .. } => {
            "AIDL object owner generation mismatch"
        }
        RuntimeObjectTableError::OwnerKindMismatch { .. } => "AIDL object owner kind mismatch",
        RuntimeObjectTableError::OwnerNotLive { .. } => "AIDL object owner is not live",
        RuntimeObjectTableError::InvalidLifecycle { .. } => "AIDL object is not live",
        RuntimeObjectTableError::DuplicateObjectId { .. } => "AIDL object id already registered",
        RuntimeObjectTableError::DuplicateRuntimeBinding { .. } => {
            "AIDL public runtime object is already opened"
        }
        RuntimeObjectTableError::UnsupportedObjectKind { .. } => "AIDL object kind is unsupported",
        RuntimeObjectTableError::GenerationOverflow => "AIDL object generation overflow",
    };
    HalError::invalid_state(HalInvalidStateKind::InvalidLifecycle, message)
}

fn status_from_hal_error(error: &HalError) -> Status {
    let code = match AidlStatusMapper::map_error(error) {
        maleicacid_tuner_hal2_binder_adapter::TunerStatusCode::Ok => TunerResult::UNKNOWN_ERROR.0,
        maleicacid_tuner_hal2_binder_adapter::TunerStatusCode::InvalidArgument => {
            TunerResult::INVALID_ARGUMENT.0
        }
        maleicacid_tuner_hal2_binder_adapter::TunerStatusCode::InvalidState => {
            TunerResult::INVALID_STATE.0
        }
        maleicacid_tuner_hal2_binder_adapter::TunerStatusCode::Unavailable => {
            TunerResult::UNAVAILABLE.0
        }
        maleicacid_tuner_hal2_binder_adapter::TunerStatusCode::UnknownError => {
            TunerResult::UNKNOWN_ERROR.0
        }
    };
    service_error(code, &error.to_string())
}

fn status_from_object_table_error(error: RuntimeObjectTableError) -> Status {
    let message = match error {
        RuntimeObjectTableError::MissingObject { .. } => "AIDL object is closed or missing",
        RuntimeObjectTableError::ObjectKindMismatch { .. } => "AIDL object kind mismatch",
        RuntimeObjectTableError::GenerationMismatch { .. } => "AIDL object generation mismatch",
        RuntimeObjectTableError::InvalidOwner { .. } => "AIDL object owner mismatch",
        RuntimeObjectTableError::MissingOwner { .. } => "AIDL object owner is missing",
        RuntimeObjectTableError::OwnerGenerationMismatch { .. } => {
            "AIDL object owner generation mismatch"
        }
        RuntimeObjectTableError::OwnerKindMismatch { .. } => "AIDL object owner kind mismatch",
        RuntimeObjectTableError::OwnerNotLive { .. } => "AIDL object owner is not live",
        RuntimeObjectTableError::InvalidLifecycle { .. } => "AIDL object is not live",
        RuntimeObjectTableError::DuplicateObjectId { .. } => "AIDL object id already registered",
        RuntimeObjectTableError::DuplicateRuntimeBinding { .. } => {
            "AIDL public runtime object is already opened"
        }
        RuntimeObjectTableError::UnsupportedObjectKind { .. } => "AIDL object kind is unsupported",
        RuntimeObjectTableError::GenerationOverflow => "AIDL object generation overflow",
    };
    service_error(TunerResult::INVALID_STATE.0, message)
}

fn status_unknown_error(message: &str) -> Status {
    service_error(TunerResult::UNKNOWN_ERROR.0, message)
}

fn service_error(code: i32, message: &str) -> Status {
    match std::ffi::CString::new(message) {
        Ok(detail) => Status::new_service_specific_error(code, Some(detail.as_c_str())),
        Err(_) => Status::new_service_specific_error(code, None),
    }
}
