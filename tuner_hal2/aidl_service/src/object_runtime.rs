use std::sync::{Arc, Mutex};

use binder::Result as BinderResult;
use maleicacid_tuner_hal2_binder_adapter::{
    demux::DemuxCommand, dvr::DvrCommand, filter::FilterCommand, frontend::FrontendCommand,
    lnb::LnbCommand,
};
use maleicacid_tuner_hal2_binder_adapter::{
    AidlApi, AidlDomainRequest, AidlFailureSource, AidlMethodAdapter, AidlMethodCall,
    AidlMethodPlan, AidlObjectKind, AidlStatusMapper, DomainCommand, DomainProfileSupport,
    StatusPrecedenceStep,
};
use maleicacid_tuner_hal2_common::{HalError, HalInvalidStateKind};
use maleicacid_tuner_hal2_resource_ledger::{CleanupStep, LedgerId};
use maleicacid_tuner_hal2_service_runtime::{RuntimeObjectTableError, TunerServiceRuntime};

use crate::callback_store::clear_owner_callbacks;
use crate::error_bridge::{status_from_hal_error, status_from_hal_error_ref, status_unknown_error};
use crate::object_handle::AidlObjectHandle;

pub type SharedTunerRuntime = Arc<Mutex<TunerServiceRuntime>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropLeakDomainAction {
    None,
    RecordLnbDropLeak,
}

fn clear_runtime_callback_owner(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
) -> BinderResult<()> {
    let mut runtime = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    runtime
        .callback_registry_mut()
        .clear_owner(handle.object_id(), handle.generation());
    Ok(())
}

fn mark_runtime_callback_unhealthy(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    api: AidlApi,
) -> BinderResult<()> {
    let mut runtime = runtime
        .lock()
        .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
    runtime.callback_registry_mut().mark_unhealthy(
        handle.object_kind(),
        handle.object_id(),
        handle.generation(),
        api,
    );
    Ok(())
}

pub fn clear_owner_callback_registration(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    api: AidlApi,
    failure_message: &'static str,
) -> BinderResult<()> {
    match clear_owner_callbacks(handle) {
        Ok(()) => clear_runtime_callback_owner(runtime, handle),
        Err(_) => {
            mark_runtime_callback_unhealthy(runtime, handle, api)?;
            Err(status_unknown_error(failure_message))
        }
    }
}

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
        return Err(status_from_hal_error_ref(failure.error()));
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
        .map_err(|error| status_from_hal_error(object_table_hal_error(error)))
}

pub fn clear_live_lnb_callback_for_public_id(
    runtime: &SharedTunerRuntime,
    lnb_id: i32,
) -> BinderResult<()> {
    let handle = {
        let runtime = runtime
            .lock()
            .map_err(|_| status_unknown_error("service runtime lock poisoned"))?;
        let Some(entry) = runtime
            .object_table()
            .live_entry_for_runtime(AidlObjectKind::Lnb, LedgerId(i64::from(lnb_id)))
        else {
            return Ok(());
        };
        AidlObjectHandle::new(entry.object_kind, entry.object_id, entry.generation)
    };
    clear_owner_callback_registration(
        runtime,
        handle,
        AidlApi::LnbSetCallback,
        "callback store cleanup failed during LNB owner loss",
    )
}

fn unregister_public_runtime_entries(
    runtime: &mut TunerServiceRuntime,
    entries: &[maleicacid_tuner_hal2_service_runtime::RuntimeObjectEntry],
) {
    for entry in entries {
        runtime.unregister_public_runtime_for_closed_aidl_entry(entry);
    }
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
        .map_err(|error| status_from_hal_error(object_table_hal_error(error)))?;
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
            .map_err(|error| status_from_hal_error(object_table_hal_error(error)))?;
        return Err(status_unknown_error(
            "callback store cleanup failed during AIDL object close",
        ));
    }
    let closed_entries = runtime
        .object_table_mut()
        .commit_close_cascade(handle.object_id(), handle.generation())
        .map_err(|error| status_from_hal_error(object_table_hal_error(error)))?;
    unregister_public_runtime_entries(&mut runtime, &closed_entries);
    Ok(())
}

fn lnb_public_id_for_drop(
    runtime: &TunerServiceRuntime,
    handle: AidlObjectHandle,
) -> Option<i32> {
    if handle.object_kind() != AidlObjectKind::Lnb {
        return None;
    }
    runtime
        .object_table()
        .entry_for_kind(handle.object_id(), handle.generation(), AidlObjectKind::Lnb)
        .ok()
        .and_then(|entry| i32::try_from(entry.ledger_id.0).ok())
}

fn record_domain_drop_leak(
    runtime: &mut TunerServiceRuntime,
    handle: AidlObjectHandle,
    action: DropLeakDomainAction,
) -> bool {
    match action {
        DropLeakDomainAction::None => true,
        DropLeakDomainAction::RecordLnbDropLeak => lnb_public_id_for_drop(runtime, handle)
            .map(|lnb_id| runtime.record_lnb_drop_leak(lnb_id).is_ok())
            .unwrap_or(false),
    }
}

pub fn drop_leak_object(
    runtime: &SharedTunerRuntime,
    handle: AidlObjectHandle,
    action: DropLeakDomainAction,
) {
    let Ok(mut runtime) = runtime.lock() else {
        return;
    };
    let domain_record_ok = record_domain_drop_leak(&mut runtime, handle, action);
    let callback_store_ok = clear_owner_callbacks(handle).is_ok();
    if callback_store_ok && domain_record_ok {
        runtime
            .callback_registry_mut()
            .clear_owner(handle.object_id(), handle.generation());
    } else {
        runtime
            .callback_registry_mut()
            .mark_owner_unhealthy(handle.object_id(), handle.generation());
    }
    let Ok(entries) = runtime
        .object_table_mut()
        .quarantine_cascade(handle.object_id(), handle.generation())
    else {
        return;
    };
    unregister_public_runtime_entries(&mut runtime, &entries);
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
        .map_err(|error| status_from_hal_error(object_table_hal_error(error)))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callback_store::{has_callback_for_owner, retain_test_callback_marker};
    use maleicacid_tuner_hal2_binder_adapter::{
        AidlApi, AidlObjectGeneration, AidlObjectId, AidlObjectKind,
    };
    use maleicacid_tuner_hal2_service_runtime::{
        CallbackHealthState, RuntimeObjectLifecycle, RuntimeOwnerRelation,
    };

    fn shared_runtime_with_live_object(
        kind: AidlObjectKind,
        object_id: AidlObjectId,
        generation: AidlObjectGeneration,
        public_runtime_id: i64,
    ) -> SharedTunerRuntime {
        let runtime = Arc::new(Mutex::new(TunerServiceRuntime::new()));
        runtime
            .lock()
            .unwrap()
            .register_aidl_object_for_runtime(
                kind,
                object_id,
                generation,
                public_runtime_id,
                RuntimeOwnerRelation::Root,
            )
            .unwrap();
        runtime
    }

    #[test]
    fn drop_leak_clears_runtime_callback_registration_and_quarantines_object() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Filter,
            AidlObjectId(91_001),
            AidlObjectGeneration(1),
        );
        clear_owner_callbacks(handle).unwrap();
        retain_test_callback_marker(handle, AidlApi::DemuxOpenFilter).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Filter,
            AidlObjectId(91_001),
            AidlObjectGeneration(1),
            91_001,
        );
        record_callback_registration(&runtime, handle, AidlApi::DemuxOpenFilter).unwrap();

        drop_leak_object(&runtime, handle, DropLeakDomainAction::None);

        let runtime = runtime.lock().unwrap();
        assert!(!has_callback_for_owner(handle, AidlApi::DemuxOpenFilter).unwrap());
        assert_eq!(runtime.callback_registry().registration_count(), 0);
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(91_001))
                .unwrap()
                .lifecycle,
            RuntimeObjectLifecycle::Quarantined
        );
    }

    #[test]
    fn drop_leak_marks_callback_unhealthy_when_domain_drop_record_fails() {
        let handle = AidlObjectHandle::new(
            AidlObjectKind::Lnb,
            AidlObjectId(91_002),
            AidlObjectGeneration(1),
        );
        clear_owner_callbacks(handle).unwrap();
        retain_test_callback_marker(handle, AidlApi::LnbSetCallback).unwrap();
        let runtime = shared_runtime_with_live_object(
            AidlObjectKind::Lnb,
            AidlObjectId(91_002),
            AidlObjectGeneration(1),
            91_002,
        );
        record_callback_registration(&runtime, handle, AidlApi::LnbSetCallback).unwrap();

        drop_leak_object(&runtime, handle, DropLeakDomainAction::RecordLnbDropLeak);

        let runtime = runtime.lock().unwrap();
        assert!(!has_callback_for_owner(handle, AidlApi::LnbSetCallback).unwrap());
        assert_eq!(
            runtime
                .callback_registry()
                .registration_for(
                    AidlObjectKind::Lnb,
                    AidlObjectId(91_002),
                    AidlObjectGeneration(1),
                    AidlApi::LnbSetCallback,
                )
                .unwrap()
                .health,
            CallbackHealthState::Unhealthy
        );
        assert_eq!(
            runtime
                .object_table()
                .entry(AidlObjectId(91_002))
                .unwrap()
                .lifecycle,
            RuntimeObjectLifecycle::Quarantined
        );
    }
}
